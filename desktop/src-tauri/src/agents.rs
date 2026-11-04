//! Agent tasks: a prompt with its own branch and folder (`spor::tasks`),
//! and the agents that work on it (`spor::agent`). The rules for picking the
//! agent and wrapping follow-ups match the GPUI app's `src/gui/agents.rs`.

use crate::undo::Undo;
use crate::{first_line, App};
use serde::{Deserialize, Serialize};
use spor::agent::{self, AgentKind, Entry, Mode, Permissions, Run, RunState, Runs};
use spor::tasks::{LandMode, Task, TaskState, TaskStats, Tasks};
use tauri::State;

impl App {
    fn tasks(&self) -> Result<Tasks, String> {
        Tasks::open(&self.repo()?)
    }

    fn task(&self, id: &str) -> Result<(Tasks, Task), String> {
        let tasks = self.tasks()?;
        let task = tasks.get(id).ok_or_else(|| format!("No task {id}"))?;
        Ok((tasks, task))
    }
}

// ---- What the UI sees ----------------------------------------------------

#[derive(Serialize)]
pub(crate) struct Stats {
    commits: usize,
    uncommitted: usize,
    files: usize,
    additions: usize,
    deletions: usize,
    base_moved: bool,
    last_commit: i64,
    missing: bool,
}

impl From<TaskStats> for Stats {
    fn from(s: TaskStats) -> Self {
        Self {
            commits: s.commits,
            uncommitted: s.uncommitted,
            files: s.files,
            additions: s.additions,
            deletions: s.deletions,
            base_moved: s.base_moved,
            last_commit: s.last_commit,
            missing: s.missing,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct RunInfo {
    index: usize,
    agent: &'static str,
    label: String,
    /// What it was asked: the task's prompt, or a follow-up.
    prompt: String,
    state: &'static str,
    exit_code: Option<i32>,
    started: i64,
    ended: Option<i64>,
    cost_usd: Option<f64>,
    resumable: bool,
    /// Taken back: its changes are gone and follow-ups resume from before it.
    undone: bool,
}

impl From<Run> for RunInfo {
    fn from(r: Run) -> Self {
        let (state, exit_code) = match r.state {
            RunState::Running => ("running", None),
            RunState::Finished => ("finished", Some(0)),
            RunState::Failed(c) => ("failed", Some(c)),
            RunState::Stopped => ("stopped", None),
        };
        Self {
            index: r.index,
            agent: match r.kind {
                AgentKind::Claude => "claude",
                AgentKind::Command(_) => "command",
            },
            label: r.kind.label().to_string(),
            prompt: r.prompt.clone(),
            state,
            exit_code,
            started: r.started,
            ended: r.ended,
            cost_usd: r.cost_usd,
            resumable: r.session.is_some(),
            undone: r.undone,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TaskSummary {
    id: String,
    title: String,
    prompt: String,
    base: String,
    branch: String,
    worktree: String,
    created: i64,
    landed: Option<String>,
    stats: Stats,
    run: Option<RunInfo>,
    runs: usize,
    command: String,
    /// Questions the agent is waiting on: "Needs you".
    asks: usize,
    /// The permission mode of its latest run.
    mode: Option<&'static str>,
}

fn summary(tasks: &Tasks, task: &Task) -> TaskSummary {
    let runs = Runs::of(tasks, task);
    let list = runs.list();
    TaskSummary {
        id: task.id.clone(),
        title: task.title(),
        prompt: task.prompt.clone(),
        base: task.base.clone(),
        branch: task.branch.clone(),
        worktree: task.worktree.display().to_string(),
        created: task.created,
        landed: match &task.state {
            TaskState::Landed(sha) => Some(sha.clone()),
            TaskState::Open => None,
        },
        stats: tasks.stats(task).into(),
        mode: list.last().and_then(|r| runs.mode(r.index)).map(mode_name),
        runs: list.len(),
        run: list.into_iter().last().map(RunInfo::from),
        command: task.agent_command(),
        asks: runs.asks().len(),
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Item {
    Prompt {
        text: String,
    },
    Text {
        text: String,
    },
    Tool {
        name: String,
        detail: String,
    },
    ToolError {
        text: String,
    },
    Done {
        ok: bool,
        summary: String,
        cost_usd: Option<f64>,
        turns: Option<u32>,
        seconds: Option<f64>,
    },
    Output {
        text: String,
    },
    Ask {
        id: String,
        tool: String,
        detail: String,
        /// What "Allow for this session" covers, if offered (#40).
        session: Option<String>,
    },
    Answer {
        id: String,
        allow: bool,
        message: String,
        always: bool,
    },
    Steer {
        text: String,
    },
    Setup {
        ok: Option<bool>,
        code: Option<i32>,
    },
}

impl Item {
    /// Paths inside the task's folder read better relative to it.
    fn from(entry: Entry, folder: &str) -> Self {
        match entry {
            Entry::Prompt(text) => Item::Prompt { text },
            Entry::Text(text) => Item::Text { text },
            Entry::Tool { name, detail } => Item::Tool {
                name,
                detail: detail.replace(folder, ""),
            },
            Entry::ToolError(text) => Item::ToolError { text },
            Entry::Done {
                ok,
                summary,
                cost_usd,
                turns,
                seconds,
            } => Item::Done {
                ok,
                summary,
                cost_usd,
                turns,
                seconds,
            },
            Entry::Output(text) => Item::Output { text },
            Entry::Ask {
                id,
                tool,
                detail,
                session,
            } => Item::Ask {
                id,
                tool,
                detail: detail.replace(folder, ""),
                session,
            },
            Entry::Answer {
                id,
                allow,
                message,
                always,
            } => Item::Answer {
                id,
                allow,
                message,
                always,
            },
            Entry::Steer(text) => Item::Steer { text },
            Entry::Setup { ok, code } => Item::Setup { ok, code },
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TaskDetail {
    task: TaskSummary,
    transcript: Vec<Item>,
    runs: Vec<RunInfo>,
    /// Files that would conflict if the task landed on its base now.
    conflicts: Vec<String>,
    /// The run "Undo this run" would take back, if any.
    undoable: Option<usize>,
    /// Questions the agent is waiting on.
    asks: Vec<AskInfo>,
    /// The running agent can be answered, steered and interrupted.
    listening: bool,
}

#[derive(Serialize)]
pub(crate) struct AskInfo {
    id: String,
    tool: String,
    detail: String,
    /// What "Allow for this session" would cover; None: only once.
    session: Option<String>,
}

/// The UI's names for permission modes.
fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Supervised => "supervised",
        Mode::AcceptEdits => "acceptEdits",
        Mode::Auto => "auto",
        Mode::Full => "full",
    }
}

fn mode_of(name: &str) -> Result<Mode, String> {
    Mode::ALL
        .into_iter()
        .find(|m| mode_name(*m) == name)
        .ok_or_else(|| format!("Unknown permission mode {name}"))
}

/// Which agent to run, as the UI names it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AgentChoice {
    Claude,
    Command { template: String },
    None,
}

impl AgentChoice {
    fn kind(&self) -> Option<AgentKind> {
        match self {
            AgentChoice::Claude => Some(AgentKind::Claude),
            AgentChoice::Command { template } => Some(AgentKind::Command(template.clone())),
            AgentChoice::None => None,
        }
    }
}

fn missing(kind: &AgentKind) -> Option<String> {
    match kind {
        AgentKind::Claude if !agent::available("claude") => {
            Some("Claude Code isn't installed, or isn't on the PATH".into())
        }
        AgentKind::Command(c) if c.trim().is_empty() => Some("The agent command is empty".into()),
        _ => None,
    }
}

// ---- Commands ------------------------------------------------------------

#[tauri::command(async)]
pub(crate) fn list_tasks(app: State<App>) -> Result<Vec<TaskSummary>, String> {
    let tasks = app.tasks()?;
    Ok(tasks.list().iter().map(|t| summary(&tasks, t)).collect())
}

#[tauri::command(async)]
pub(crate) fn task_detail(app: State<App>, id: String) -> Result<TaskDetail, String> {
    let (tasks, task) = app.task(&id)?;
    let runs = Runs::of(&tasks, &task);
    let folder = format!("{}/", task.worktree.display());
    let conflicts = if task.state == TaskState::Open && task.worktree.exists() {
        task.repo()
            .merge_conflicts(&task.base, "HEAD")
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(TaskDetail {
        task: summary(&tasks, &task),
        transcript: runs
            .transcript()
            .into_iter()
            .map(|e| Item::from(e, &folder))
            .collect(),
        undoable: runs.undoable().map(|r| r.index),
        asks: runs
            .asks()
            .into_iter()
            .map(|a| AskInfo {
                id: a.id,
                tool: a.tool,
                detail: a.detail.replace(&folder, ""),
                session: a.session,
            })
            .collect(),
        listening: runs.listening(),
        runs: runs.list().into_iter().map(RunInfo::from).collect(),
        conflicts,
    })
}

#[tauri::command(async)]
pub(crate) fn task_patch(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if task.state != TaskState::Open {
        return Ok(String::new());
    }
    tasks.patch(&task)
}

#[tauri::command(async)]
pub(crate) fn create_task(
    app: State<App>,
    prompt: String,
    base: String,
    agent: AgentChoice,
    mode: String,
) -> Result<TaskSummary, String> {
    let mode = mode_of(&mode)?;
    let kind = agent.kind();
    if let Some(e) = kind.as_ref().and_then(missing) {
        return Err(e);
    }
    let tasks = app.tasks()?;
    let task = tasks
        .create(&prompt, &base)
        .map_err(|e| format!("Couldn't create the task: {}", first_line(&e)))?;
    if let Some(kind) = kind {
        Runs::of(&tasks, &task).start(&task, &kind, &task.prompt, None, Permissions { mode })?;
    }
    Ok(summary(&tasks, &task))
}

/// Start an agent on a task: with its prompt, or with a follow-up message
/// that continues the agent's session when it has one.
#[tauri::command(async)]
pub(crate) fn send_to_agent(
    app: State<App>,
    id: String,
    message: Option<String>,
    agent: AgentChoice,
    mode: String,
) -> Result<(), String> {
    let mode = mode_of(&mode)?;
    let (tasks, task) = app.task(&id)?;
    let runs = Runs::of(&tasks, &task);
    let follow_up = message.is_some();
    // Follow-ups go to the agent that did the work, whatever's picked now.
    let previous = runs.latest().map(|r| r.kind);
    let kind = match (follow_up, previous, agent.kind()) {
        (true, Some(k), _) | (_, _, Some(k)) => k,
        _ => return Err("Pick an agent first".into()),
    };
    if let Some(e) = missing(&kind) {
        return Err(e);
    }
    let session = if follow_up { runs.session() } else { None };
    let prompt = match message {
        // A custom command, or Claude without a session to resume, needs the
        // whole task again, not just the feedback.
        Some(m) if session.is_none() => format!(
            "{}\n\nFeedback on the work so far:\n{}",
            task.prompt.trim(),
            m.trim()
        ),
        Some(m) => m.trim().to_string(),
        None => task.prompt.clone(),
    };
    runs.start(
        &task,
        &kind,
        &prompt,
        session.as_deref(),
        Permissions { mode },
    )
    .map(|_| ())
}

#[tauri::command(async)]
pub(crate) fn stop_agent(app: State<App>, id: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    // An interrupt when it listens (it keeps its session); else, or if that
    // doesn't land, the old kill.
    Runs::of(&tasks, &task).finish(std::time::Duration::from_secs(4))
}

/// A message into the turn the agent is on (steering, #22).
#[tauri::command(async)]
pub(crate) fn steer(app: State<App>, id: String, text: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    let text = text.trim();
    if text.is_empty() {
        return Err("Write something to send".into());
    }
    Runs::of(&tasks, &task)
        .send(text)
        .map_err(|e| first_line(&e))
}

#[tauri::command(async)]
pub(crate) fn land_task(
    app: State<App>,
    id: String,
    merge: bool,
    commit_uncommitted: bool,
) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    let mode = if merge {
        LandMode::Merge
    } else {
        LandMode::Squash
    };
    let undo = tasks
        .land(&task, mode, commit_uncommitted)
        .map_err(|e| first_line(&e))?;
    let message = format!("Landed {} on {}", task.id, task.base);
    app.undo.push(message.clone(), Undo::Task(undo));
    Ok(message)
}

#[tauri::command(async)]
pub(crate) fn discard_task(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if Runs::of(&tasks, &task)
        .latest()
        .is_some_and(|r| r.state == RunState::Running)
    {
        return Err("Stop the agent first".into());
    }
    let undo = tasks.discard(&task).map_err(|e| first_line(&e))?;
    let message = format!("Discarded {}", task.id);
    app.undo.push(message.clone(), Undo::Task(undo));
    Ok(message)
}

#[tauri::command(async)]
pub(crate) fn open_folder(app: State<App>, id: String) -> Result<(), String> {
    let (_, task) = app.task(&id)?;
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&task.worktree)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open the folder: {e}"))
}

#[derive(Serialize)]
pub(crate) struct RunPatch {
    patch: String,
    from_base: Vec<String>,
}

/// One run's changes, from its checkpoint before to its checkpoint after.
#[tauri::command(async)]
pub(crate) fn run_patch(app: State<App>, id: String, index: usize) -> Result<RunPatch, String> {
    let (tasks, task) = app.task(&id)?;
    let p = Runs::of(&tasks, &task).patch(index)?;
    Ok(RunPatch {
        patch: p.patch,
        from_base: p.from_base,
    })
}

/// Take back the task's last run: its folder as it was before, and the
/// conversation resumed from before it. ⌘Z brings it back.
#[tauri::command(async)]
pub(crate) fn undo_run(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    let index = Runs::of(&tasks, &task)
        .undo_last()
        .map_err(|e| first_line(&e))?;
    let label = format!("Undo run {} of {}", index + 1, task.id);
    app.undo.push(
        label.clone(),
        Undo::RunUndone {
            task: task.id.clone(),
            index,
        },
    );
    Ok(format!(
        "Undid run {} — its changes are gone, and follow-ups start from before it",
        index + 1
    ))
}

/// Answer a question the agent asked: allow, or deny (with what to do
/// instead, if `message` says). `always`: allow it for the rest of the
/// session (#40).
#[tauri::command(async)]
pub(crate) fn answer_ask(
    app: State<App>,
    id: String,
    request_id: String,
    allow: bool,
    message: String,
    always: bool,
) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    let runs = Runs::of(&tasks, &task);
    if allow && always {
        runs.allow_for_session(&request_id)
    } else {
        runs.answer(&request_id, allow, message.trim())
    }
    .map_err(|e| first_line(&e))
}

/// Change the running agent's permission mode.
#[tauri::command(async)]
pub(crate) fn set_mode(app: State<App>, id: String, mode: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    Runs::of(&tasks, &task)
        .set_mode(mode_of(&mode)?)
        .map_err(|e| first_line(&e))
}

/// Start the next agent without the folder's setup script (#23).
#[tauri::command(async)]
pub(crate) fn skip_setup(app: State<App>, id: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    Runs::of(&tasks, &task).skip_setup()
}
