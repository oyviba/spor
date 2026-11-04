//! The Tauri prototype's backend: spor's core (`spor::tasks`, `spor::agent`,
//! `spor::git`) exposed as commands for the web UI. No logic of its own
//! beyond what the GPUI app does in `src/gui/agents.rs`: pick the agent,
//! wrap follow-ups, keep one undo.

use serde::{Deserialize, Serialize};
use spor::agent::{self, AgentKind, Entry, Permissions, Run, RunState, Runs};
use spor::git::Repo;
use spor::tasks::{LandMode, Task, TaskState, TaskStats, Tasks, Undoable};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
struct App {
    repo: Mutex<Option<Repo>>,
    /// The last land or discard, for Undo.
    undo: Mutex<Option<(String, Undoable)>>,
}

impl App {
    fn repo(&self) -> Result<Repo, String> {
        self.repo
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Open a repository first".to_string())
    }

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
struct RepoInfo {
    root: String,
    name: String,
    branch: Option<String>,
    branches: Vec<String>,
    default_base: Option<String>,
    claude: bool,
}

#[derive(Serialize)]
struct Stats {
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
struct RunInfo {
    index: usize,
    agent: &'static str,
    label: String,
    state: &'static str,
    exit_code: Option<i32>,
    started: i64,
    ended: Option<i64>,
    cost_usd: Option<f64>,
    resumable: bool,
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
            state,
            exit_code,
            started: r.started,
            ended: r.ended,
            cost_usd: r.cost_usd,
            resumable: r.session.is_some(),
        }
    }
}

#[derive(Serialize)]
struct TaskSummary {
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
        runs: list.len(),
        run: list.into_iter().last().map(RunInfo::from),
        command: task.agent_command(),
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Item {
    Prompt { text: String },
    Text { text: String },
    Tool { name: String, detail: String },
    ToolError { text: String },
    Done {
        ok: bool,
        summary: String,
        cost_usd: Option<f64>,
        turns: Option<u32>,
        seconds: Option<f64>,
    },
    Output { text: String },
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
        }
    }
}

#[derive(Serialize)]
struct TaskDetail {
    task: TaskSummary,
    transcript: Vec<Item>,
    runs: Vec<RunInfo>,
    /// Files that would conflict if the task landed on its base now.
    conflicts: Vec<String>,
}

/// Which agent to run, as the UI names it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AgentChoice {
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

fn repo_info(repo: &Repo) -> RepoInfo {
    let mut branches: Vec<String> = repo
        .list_branches()
        .unwrap_or_default()
        .into_iter()
        .filter(|b| !b.is_remote && !b.name.starts_with("agent/"))
        .map(|b| b.name)
        .collect();
    branches.sort();
    RepoInfo {
        root: repo.root().display().to_string(),
        name: repo
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        branch: repo.tracking().ok().and_then(|t| t.branch),
        branches,
        default_base: repo.default_base_branch(),
        claude: agent::available("claude"),
    }
}

#[tauri::command(async)]
fn current_repo(app: State<App>) -> Option<RepoInfo> {
    app.repo.lock().unwrap().as_ref().map(repo_info)
}

#[tauri::command(async)]
fn open_repo(app: State<App>, path: String) -> Result<RepoInfo, String> {
    let path = expand_home(path.trim());
    let repo = Repo::discover(&path).map_err(|e| first_line(&e))?;
    let info = repo_info(&repo);
    *app.repo.lock().unwrap() = Some(repo);
    *app.undo.lock().unwrap() = None;
    Ok(info)
}

#[tauri::command(async)]
fn list_tasks(app: State<App>) -> Result<Vec<TaskSummary>, String> {
    let tasks = app.tasks()?;
    Ok(tasks.list().iter().map(|t| summary(&tasks, t)).collect())
}

#[tauri::command(async)]
fn task_detail(app: State<App>, id: String) -> Result<TaskDetail, String> {
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
        runs: runs.list().into_iter().map(RunInfo::from).collect(),
        conflicts,
    })
}

#[tauri::command(async)]
fn task_patch(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if task.state != TaskState::Open {
        return Ok(String::new());
    }
    tasks.patch(&task)
}

#[tauri::command(async)]
fn create_task(
    app: State<App>,
    prompt: String,
    base: String,
    agent: AgentChoice,
    allow_commands: bool,
) -> Result<TaskSummary, String> {
    let kind = agent.kind();
    if let Some(e) = kind.as_ref().and_then(missing) {
        return Err(e);
    }
    let tasks = app.tasks()?;
    let task = tasks
        .create(&prompt, &base)
        .map_err(|e| format!("Couldn't create the task: {}", first_line(&e)))?;
    if let Some(kind) = kind {
        Runs::of(&tasks, &task).start(
            &task,
            &kind,
            &task.prompt,
            None,
            Permissions {
                commands: allow_commands,
            },
        )?;
    }
    Ok(summary(&tasks, &task))
}

/// Start an agent on a task: with its prompt, or with a follow-up message
/// that continues the agent's session when it has one.
#[tauri::command(async)]
fn send_to_agent(
    app: State<App>,
    id: String,
    message: Option<String>,
    agent: AgentChoice,
    allow_commands: bool,
) -> Result<(), String> {
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
        Permissions {
            commands: allow_commands,
        },
    )
    .map(|_| ())
}

#[tauri::command(async)]
fn stop_agent(app: State<App>, id: String) -> Result<(), String> {
    let (tasks, task) = app.task(&id)?;
    Runs::of(&tasks, &task).stop()
}

#[tauri::command(async)]
fn land_task(
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
    *app.undo.lock().unwrap() = Some((message.clone(), undo));
    Ok(message)
}

#[tauri::command(async)]
fn discard_task(app: State<App>, id: String) -> Result<String, String> {
    let (tasks, task) = app.task(&id)?;
    if Runs::of(&tasks, &task)
        .latest()
        .is_some_and(|r| r.state == RunState::Running)
    {
        return Err("Stop the agent first".into());
    }
    let undo = tasks.discard(&task).map_err(|e| first_line(&e))?;
    let message = format!("Discarded {}", task.id);
    *app.undo.lock().unwrap() = Some((message.clone(), undo));
    Ok(message)
}

#[tauri::command(async)]
fn undo(app: State<App>) -> Result<String, String> {
    let Some((what, undo)) = app.undo.lock().unwrap().take() else {
        return Err("Nothing to undo".into());
    };
    app.tasks()?.restore(&undo).map_err(|e| first_line(&e))?;
    Ok(format!("Undid: {what}"))
}

#[tauri::command(async)]
fn open_folder(app: State<App>, id: String) -> Result<(), String> {
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

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    }
}

pub fn run() {
    let app = App::default();
    // The repository to open: the first argument, else $SPOR_REPO, else the
    // current directory if it's in one.
    let start = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("SPOR_REPO").ok())
        .map(|p| expand_home(&p))
        .or_else(|| std::env::current_dir().ok());
    if let Some(repo) = start.and_then(|p| Repo::discover(&p).ok()) {
        *app.repo.lock().unwrap() = Some(repo);
    }
    tauri::Builder::default()
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            current_repo,
            open_repo,
            list_tasks,
            task_detail,
            task_patch,
            create_task,
            send_to_agent,
            stop_agent,
            land_task,
            discard_task,
            undo,
            open_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Spor desktop prototype");
}
