//! Agent tasks: a prompt with its own branch and folder (`spor::tasks`),
//! and the agents that work on it (`spor::agent`, which has the rules for
//! picking the agent and wrapping follow-ups).

use crate::undo::Undo;
use crate::{first_line, App};
use serde::{Deserialize, Serialize};
use spor::agent::{
    self, AgentKind, Effort, Entry, Mode, Model, Permissions, Run, RunState, Runs, Setup,
    SetupState,
};
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
    unpushed: usize,
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
            unpushed: s.unpushed,
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
            label: match r.model.describe() {
                Some(model) => format!("{} · {model}", r.kind.label()),
                None => r.kind.label().to_string(),
            },
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
    /// Where it started when that isn't `base`: `origin/main`.
    from: Option<String>,
    branch: String,
    worktree: String,
    created: i64,
    landed: Option<String>,
    /// Its pull request was closed without merging, and it was cleaned up.
    closed: bool,
    stats: Stats,
    run: Option<RunInfo>,
    runs: usize,
    command: String,
    /// Questions the agent is waiting on: "Needs you".
    asks: usize,
    /// The permission mode of its latest run.
    mode: Option<&'static str>,
    /// The GitHub issue it was started from.
    issue: Option<u64>,
    /// The pull request it was published as.
    pull: Option<u64>,
    /// Follow-ups waiting for the agent to finish (#22), kept with the task.
    queued: Vec<String>,
}

fn summary(tasks: &Tasks, task: &Task) -> TaskSummary {
    let runs = Runs::of(tasks, task);
    let list = runs.list();
    TaskSummary {
        id: task.id.clone(),
        title: task.title(),
        prompt: task.prompt.clone(),
        base: task.base.clone(),
        from: task.from.clone(),
        branch: task.branch.clone(),
        worktree: task.worktree.display().to_string(),
        created: task.created,
        landed: match &task.state {
            TaskState::Landed(sha) => Some(sha.clone()),
            TaskState::Open | TaskState::Closed => None,
        },
        closed: task.state == TaskState::Closed,
        stats: tasks.stats(task).into(),
        mode: list.last().and_then(|r| runs.mode(r.index)).map(mode_name),
        runs: list.len(),
        run: list.into_iter().last().map(RunInfo::from),
        command: task.agent_command(),
        asks: runs.asks().len(),
        issue: task.issue,
        pull: task.pull,
        queued: runs.queued(),
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
    /// Work that came to the base after the task started and landing it
    /// would take back out, per file (`Tasks::undoes`).
    undoes: Vec<Undone>,
    /// The run "Undo this run" would take back, if any.
    undoable: Option<usize>,
    /// The runs "Undo from here" can start at.
    undoable_from: Vec<usize>,
    /// The folder's setup script and how it went (#23).
    setup: Option<SetupInfo>,
    /// Questions the agent is waiting on.
    asks: Vec<AskInfo>,
    /// The running agent can be answered, steered and interrupted.
    listening: bool,
}

#[derive(Serialize)]
pub(crate) struct Undone {
    path: String,
    lines: usize,
}

#[derive(Serialize)]
pub(crate) struct SetupInfo {
    script: String,
    /// "running", "worked", "failed", "skipped", "due"; None: not run yet.
    state: Option<&'static str>,
    code: Option<i32>,
}

fn setup_info(setup: Setup) -> Option<SetupInfo> {
    let script = setup.script?.display().to_string();
    let (state, code) = match setup.state {
        None => (None, None),
        Some(SetupState::Running) => (Some("running"), None),
        Some(SetupState::Worked) => (Some("worked"), None),
        Some(SetupState::Failed(code)) => (Some("failed"), code),
        Some(SetupState::Skipped) => (Some("skipped"), None),
        Some(SetupState::Due) => (Some("due"), None),
    };
    Some(SetupInfo {
        script,
        state,
        code,
    })
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

/// Which agent to run, as the UI names it. Claude's model and effort are
/// left to Claude Code when they're missing or empty.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AgentChoice {
    Claude {
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<String>,
    },
    Command {
        template: String,
    },
    None,
}

impl AgentChoice {
    fn kind(&self) -> Option<AgentKind> {
        match self {
            AgentChoice::Claude { .. } => Some(AgentKind::Claude),
            AgentChoice::Command { template } => Some(AgentKind::Command(template.clone())),
            AgentChoice::None => None,
        }
    }

    /// The agent and model picked now, for `Runs::follow_up`. A follow-up
    /// goes to the agent that did the work, so then the pick isn't needed
    /// (and isn't checked).
    fn chosen(&self, runs: &Runs, follow_up: bool) -> Result<Option<(AgentKind, Model)>, String> {
        if follow_up && runs.latest().is_some() {
            return Ok(None);
        }
        self.kind().map(|k| Ok((k, self.model()?))).transpose()
    }

    fn model(&self) -> Result<Model, String> {
        match self {
            AgentChoice::Claude { model, effort } => {
                let effort = match effort.as_deref().filter(|e| !e.is_empty()) {
                    Some(e) => {
                        Some(Effort::from_flag(e).ok_or_else(|| format!("Unknown effort {e}"))?)
                    }
                    None => None,
                };
                Model::new(model.as_deref(), effort)
            }
            _ => Ok(Model::default()),
        }
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
            .merge_conflicts(task.start(), "HEAD")
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
        undoable_from: runs.undoable_from(),
        setup: setup_info(runs.setup(&task)),
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
        undoes: if conflicts.is_empty() {
            tasks
                .undoes(&task)
                .into_iter()
                .map(|(path, lines)| Undone { path, lines })
                .collect()
        } else {
            Vec::new()
        },
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

/// Start a task on a GitHub issue: its prompt is the issue (title, text,
/// comments), and it remembers the issue so its PR closes it.
#[tauri::command(async)]
pub(crate) fn create_issue_task(
    app: State<App>,
    number: u64,
    prompt: String,
    base: String,
    agent: AgentChoice,
    mode: String,
) -> Result<TaskSummary, String> {
    let mode = mode_of(&mode)?;
    let model = agent.model()?;
    let kind = agent.kind();
    if let Some(e) = kind.as_ref().and_then(agent::missing) {
        return Err(e);
    }
    refresh_start(&app, &base);
    let tasks = app.tasks()?;
    if let Some(open) = tasks
        .list()
        .into_iter()
        .find(|t| t.issue == Some(number) && t.state == TaskState::Open)
    {
        return Err(format!("#{number} already has a task: {}", open.id));
    }
    let title = prompt.lines().next().unwrap_or("").trim();
    let title = title
        .strip_suffix(&format!("(#{number})"))
        .unwrap_or(title)
        .trim();
    let task = tasks
        .create_for_issue(&prompt, &base, number, title)
        .map_err(|e| format!("Couldn't create the task: {}", first_line(&e)))?;
    if let Some(kind) = kind {
        Runs::of(&tasks, &task).start_with(
            &task,
            &kind,
            &task.prompt,
            None,
            Permissions { mode },
            &model,
        )?;
    }
    Ok(summary(&tasks, &task))
}

/// A task starting from a remote's branch (`origin/main`) starts from what
/// the remote has now: fetch that branch first. Quietly, and only as good as
/// it can be: offline, it starts from the last fetch.
fn refresh_start(app: &App, base: &str) {
    let Ok(repo) = app.repo() else { return };
    if let (Some(branch), Some((remote, _))) = (repo.remote_branch(base), base.split_once('/')) {
        let _ = spor::askpass::git_network_quiet(repo.root(), &["fetch", remote, &branch]);
    }
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
    let model = agent.model()?;
    let kind = agent.kind();
    if let Some(e) = kind.as_ref().and_then(agent::missing) {
        return Err(e);
    }
    refresh_start(&app, &base);
    let tasks = app.tasks()?;
    let task = tasks
        .create(&prompt, &base)
        .map_err(|e| format!("Couldn't create the task: {}", first_line(&e)))?;
    if let Some(kind) = kind {
        Runs::of(&tasks, &task).start_with(
            &task,
            &kind,
            &task.prompt,
            None,
            Permissions { mode },
            &model,
        )?;
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
    let chosen = agent.chosen(&runs, message.is_some())?;
    runs.follow_up(
        &task,
        message.as_deref(),
        chosen.as_ref().map(|(k, m)| (k, m)),
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

/// Keep an image pasted or dropped into a prompt, and say where it is: the
/// prompt names it by that path. The image is the request's raw body.
#[tauri::command(async)]
pub(crate) fn attach_image(
    app: State<App>,
    request: tauri::ipc::Request,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("No image was sent".into());
    };
    let path = app.tasks()?.attach(data)?;
    Ok(path.display().to_string())
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

/// Take back the task's last run, or with `from`, that run and every run
/// after it: the folder as it was before, and the conversation resumed
/// from before it. ⌘Z brings it back.
#[tauri::command(async)]
pub(crate) fn undo_run(app: State<App>, id: String, from: Option<usize>) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    let runs = Runs::of(&tasks, &task);
    let (index, taken) = match from {
        Some(index) => (
            index,
            runs.undo_from(index).map_err(|e| first_line(&e))?.len(),
        ),
        None => (runs.undo_last().map_err(|e| first_line(&e))?, 1),
    };
    let label = if taken == 1 {
        format!("Undo run {} of {}", index + 1, task.id)
    } else {
        format!("Undo runs {}–{} of {}", index + 1, index + taken, task.id)
    };
    app.undo.push(
        label.clone(),
        Undo::RunUndone {
            task: task.id.clone(),
            index,
        },
    );
    Ok(if taken == 1 {
        format!(
            "Undid run {} — its changes are gone, and follow-ups start from before it",
            index + 1
        )
    } else {
        format!(
            "Undid {taken} runs — their changes are gone, and follow-ups start from before run {}",
            index + 1
        )
    })
}

/// Queue a follow-up for when the agent finishes (#22). Kept with the
/// task, so it isn't lost with the window.
#[tauri::command(async)]
pub(crate) fn queue_follow_up(app: State<App>, id: String, text: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    Runs::of(&tasks, &task).queue(text.trim())
}

#[tauri::command(async)]
pub(crate) fn unqueue(app: State<App>, id: String, index: usize) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    Runs::of(&tasks, &task).unqueue(index)
}

/// Send the task's oldest queued follow-up if its agent is idle. True if
/// one went.
#[tauri::command(async)]
pub(crate) fn send_queued(
    app: State<App>,
    id: String,
    agent: AgentChoice,
    mode: String,
) -> Result<bool, String> {
    let mode = mode_of(&mode)?;
    let (tasks, task) = app.task(&id)?;
    let runs = Runs::of(&tasks, &task);
    let chosen = agent.chosen(&runs, true)?;
    runs.send_queued(
        &task,
        chosen.as_ref().map(|(k, m)| (k, m)),
        Permissions { mode },
    )
}

/// Run the folder's setup script again, before the next agent (#23).
#[tauri::command(async)]
pub(crate) fn rerun_setup(app: State<App>, id: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    Runs::of(&tasks, &task).rerun_setup()
}

/// Delete the run checkpoints of tasks that are done, and discarded
/// tasks' transcripts. Undo can't bring them back afterwards.
#[tauri::command(async)]
pub(crate) fn clean_up_tasks(app: State<App>) -> Result<String, String> {
    let n = app.tasks()?.clean_up();
    Ok(match n {
        0 => "Nothing to clean up".into(),
        1 => "Cleaned up after 1 old task".into(),
        n => format!("Cleaned up after {n} old tasks"),
    })
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

// ---- Pull requests -------------------------------------------------------

/// Push the task's branch and open a pull request for it into its base.
#[tauri::command(async)]
pub(crate) fn publish_task(
    app: State<App>,
    id: String,
    draft: bool,
    commit_uncommitted: bool,
) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if Runs::of(&tasks, &task)
        .latest()
        .is_some_and(|r| r.state == RunState::Running)
    {
        return Err("The agent is still working".into());
    }
    let remote = remote(&app)?;
    let (number, _) = tasks
        .publish(
            &task,
            &remote,
            draft,
            commit_uncommitted,
            agent::available("claude"),
        )
        .map_err(|e| first_line(&e))?;
    Ok(format!(
        "Opened {}pull request #{number}",
        if draft { "draft " } else { "" }
    ))
}

/// `origin`, else the repository's first remote.
fn remote(app: &App) -> Result<String, String> {
    let remotes = app.repo()?.remotes();
    remotes
        .iter()
        .find(|r| *r == "origin")
        .or(remotes.first())
        .cloned()
        .ok_or_else(|| "This repository has no remote".to_string())
}

/// Push the agent's new commits to the task's pull request.
#[tauri::command(async)]
pub(crate) fn push_task(
    app: State<App>,
    id: String,
    commit_uncommitted: Option<bool>,
) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    let stats = tasks.stats(&task);
    let commit = commit_uncommitted.unwrap_or(false) && stats.uncommitted > 0;
    let n = stats.unpushed + usize::from(commit);
    tasks
        .push(&task, &remote(&app)?, commit)
        .map_err(|e| first_line(&e))?;
    Ok(match (n, task.pull) {
        (0, _) => format!("{} is up to date", task.branch),
        (n, Some(pr)) => format!(
            "Pushed {n} {} to #{pr}",
            if n == 1 { "commit" } else { "commits" }
        ),
        (n, None) => format!("Pushed {n} {}", if n == 1 { "commit" } else { "commits" }),
    })
}

#[derive(Serialize)]
pub(crate) struct TaskPull {
    task: String,
    pull: crate::github::Pull,
    /// The commit it was merged as, once merged.
    merge_commit: Option<String>,
}

/// The pull requests of every open task: the one it was published as, or
/// one opened from its branch some other way (which it then remembers).
/// One `gh` call for all of them.
#[tauri::command(async)]
pub(crate) fn task_pulls(app: State<App>) -> Result<Vec<TaskPull>, String> {
    let tasks = app.tasks()?;
    let open: Vec<Task> = tasks
        .list()
        .into_iter()
        .filter(|t| t.state == TaskState::Open)
        .collect();
    if open.is_empty() {
        return Ok(Vec::new());
    }
    let pulls =
        spor::github::pulls_for_branches(tasks.root(), "agent/").map_err(|e| first_line(&e))?;
    let mut out = Vec::new();
    for task in open {
        let found = match task.pull {
            Some(n) => pulls.iter().find(|p| p.pull.number == n),
            // Newest first; an open one beats an old closed one, and a PR
            // older than the task belongs to an earlier branch of that name.
            None => pulls
                .iter()
                .filter(|p| p.pull.head == task.branch && p.pull.updated >= task.created)
                .min_by_key(|p| {
                    matches!(
                        p.pull.state,
                        spor::remote::PrState::Closed | spor::remote::PrState::Merged
                    )
                }),
        };
        let Some(found) = found else { continue };
        if task.pull.is_none() {
            let _ = tasks.link_pull(&task, found.pull.number);
        }
        out.push(TaskPull {
            task: task.id.clone(),
            pull: found.pull.clone().into(),
            merge_commit: found.merge_commit.clone(),
        });
    }
    Ok(out)
}

/// What reviewers said on the task's pull request since its agent last
/// started, as a follow-up for the agent.
#[tauri::command(async)]
pub(crate) fn review_feedback(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    let number = task
        .pull
        .ok_or_else(|| format!("{} has no pull request", task.id))?;
    let dir = tasks.root().to_path_buf();
    let detail = spor::github::pull_detail(&dir, number).map_err(|e| first_line(&e))?;
    let notes = spor::github::line_notes(&dir, number).map_err(|e| first_line(&e))?;
    let since = Runs::of(&tasks, &task)
        .latest()
        .map_or(task.created, |r| r.started);
    spor::github::review_feedback(&detail, &notes, since)
        .ok_or_else(|| format!("Nothing new on #{number} since the agent last worked on it"))
}

/// The failed Actions jobs on the task's pull request (the failing step
/// and the end of each log), as a follow-up for its agent.
#[tauri::command(async)]
pub(crate) fn check_feedback(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    let number = task
        .pull
        .ok_or_else(|| format!("{} has no pull request", task.id))?;
    let dir = tasks.root().to_path_buf();
    let detail = spor::github::pull_detail(&dir, number).map_err(|e| first_line(&e))?;
    let failures = spor::github::pull_failures(&dir, &detail).map_err(|e| first_line(&e))?;
    spor::github::failure_feedback(&failures, Some(&detail.pull))
        .ok_or_else(|| format!("No Actions job has failed on #{number}"))
}

/// The task's pull request was merged: mark it landed and remove its folder
/// and branch. ⌘Z brings them back.
#[tauri::command(async)]
pub(crate) fn settle_task(app: State<App>, id: String, commit: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if Runs::of(&tasks, &task)
        .latest()
        .is_some_and(|r| r.state == RunState::Running)
    {
        return Err("Stop the agent first".into());
    }
    let undo = tasks.settle(&task, &commit).map_err(|e| first_line(&e))?;
    let message = format!("Cleaned up {} — its pull request was merged", task.id);
    app.undo.push(message.clone(), Undo::Task(undo));
    Ok(message)
}

/// The task's pull request was closed without merging: mark it closed and
/// remove its folder and branch. ⌘Z brings them back.
#[tauri::command(async)]
pub(crate) fn close_task(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if Runs::of(&tasks, &task)
        .latest()
        .is_some_and(|r| r.state == RunState::Running)
    {
        return Err("Stop the agent first".into());
    }
    let undo = tasks.close(&task).map_err(|e| first_line(&e))?;
    let message = format!("Closed {} — its pull request was closed", task.id);
    app.undo.push(message.clone(), Undo::Task(undo));
    Ok(message)
}
