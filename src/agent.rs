//! Running agents in task folders, and reading back what they did.
//!
//! A task can have several runs: the first with its prompt, later ones with
//! feedback. Each run's files sit in the task's run directory
//! (`.git/spor/tasks/<id>.d/`): `run-N.meta` (what was asked, how, when),
//! `run-N.log` (everything it printed) and `run-N.exit` (its exit code, once
//! it ends). The agent runs under a small `sh` wrapper that writes the exit
//! file, so a run's state survives spor quitting and starting again, and the
//! agent keeps working if spor quits.
//!
//! Each run is bracketed by checkpoints (`crate::checkpoint`): the task's
//! folder as it was before (`refs/spor/tasks/<id>/run-N/before`) and after
//! (`…/after`, taken by the wrapper once the agent exits). They give each
//! run its own diff, and let the last run be undone: its folder goes back to
//! `before`, and the next follow-up resumes the conversation from before it
//! (follow-ups fork the agent's session, so an earlier run's session is a
//! snapshot of the conversation up to it).

use crate::checkpoint;
use crate::host;
use crate::json::Json;
use crate::tasks::{Task, Tasks};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which agent a run uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKind {
    /// Claude Code, headless, with its structured event stream.
    Claude,
    /// Any command; `{prompt}` is replaced with the prompt, shell-quoted,
    /// and `$SPOR_PROMPT` holds it too. Its output is shown as is.
    Command(String),
}

impl AgentKind {
    pub fn label(&self) -> &str {
        match self {
            AgentKind::Claude => "Claude Code",
            AgentKind::Command(_) => "Custom command",
        }
    }
}

/// How much the agent may do without asking (#21). Under the host (see
/// `host`) Claude Code asks about the rest and spor shows the question;
/// without it nobody can answer, and what it would ask about is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Asks before commands and file edits.
    Supervised,
    /// Edits freely, asks before commands.
    AcceptEdits,
    /// Claude Code's own review approves routine actions and asks about
    /// the rest.
    Auto,
    /// Never asks.
    Full,
    /// Reads and plans, changes nothing, and asks for the plan to be
    /// approved before it starts (#101): its `ExitPlanMode` is a question.
    Plan,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Supervised,
        Mode::AcceptEdits,
        Mode::Auto,
        Mode::Full,
        Mode::Plan,
    ];

    /// Claude Code's `--permission-mode`.
    pub fn flag(self) -> &'static str {
        match self {
            Mode::Supervised => "manual",
            Mode::AcceptEdits => "acceptEdits",
            Mode::Auto => "auto",
            Mode::Full => "bypassPermissions",
            Mode::Plan => "plan",
        }
    }

    pub fn from_flag(flag: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.flag() == flag)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Supervised => "Supervised",
            Mode::AcceptEdits => "Auto-accept edits",
            Mode::Auto => "Auto",
            Mode::Full => "Full access",
            Mode::Plan => "Plan first",
        }
    }
}

/// How hard Claude Code thinks: its `--effort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];

    pub fn flag(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }

    pub fn from_flag(flag: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.flag() == flag)
    }
}

/// Which model Claude Code runs, and how hard it thinks. `None` leaves it
/// to Claude Code (its settings, or its own default).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    /// An alias (`opus`, `sonnet`, …) or a model's full name.
    pub name: Option<String>,
    pub effort: Option<Effort>,
}

impl Model {
    /// A model from what someone picked: an empty name is the default, and
    /// a name has to look like one (it goes on Claude's command line).
    pub fn new(name: Option<&str>, effort: Option<Effort>) -> Result<Model, String> {
        let name = name.map(str::trim).filter(|n| !n.is_empty());
        if let Some(n) = name {
            let ok = !n.starts_with('-')
                && n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._[]:@/".contains(c));
            if !ok {
                return Err(format!("{n} isn't a model name"));
            }
        }
        Ok(Model {
            name: name.map(String::from),
            effort,
        })
    }

    /// In words, for the run's label: "Opus · high effort".
    pub fn describe(&self) -> Option<String> {
        let name = self.name.as_deref().map(|n| {
            let mut c = n.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        });
        let effort = self.effort.map(|e| format!("{} effort", e.flag()));
        match (name, effort) {
            (None, None) => None,
            (Some(n), None) => Some(n),
            (None, Some(e)) => Some(e),
            (Some(n), Some(e)) => Some(format!("{n} · {e}")),
        }
    }
}

/// How much the agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub mode: Mode,
}

impl Permissions {
    /// Edits only, or edits and commands (a yes/no switch).
    pub fn commands(allowed: bool) -> Self {
        Self {
            mode: if allowed {
                Mode::Full
            } else {
                Mode::AcceptEdits
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Running,
    /// Exited successfully.
    Finished,
    /// Exited with this code.
    Failed(i32),
    /// Stopped from spor, or gone without a trace (the machine restarted).
    Stopped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub index: usize,
    pub kind: AgentKind,
    pub prompt: String,
    pub state: RunState,
    pub started: i64,
    /// When it ended, if it has.
    pub ended: Option<i64>,
    /// The agent's session, for sending follow-ups (Claude Code).
    pub session: Option<String>,
    /// The model it ran (Claude Code).
    pub model: Model,
    pub cost_usd: Option<f64>,
    /// Tokens spent and how full the context is (Claude Code).
    pub usage: Usage,
    /// What Claude Code reported when it started: the model that actually
    /// ran, and its MCP servers (Claude Code).
    pub init: Option<Init>,
    /// Undone: its changes were taken back, and follow-ups resume from
    /// before it.
    pub undone: bool,
}

/// Tokens a Claude Code run used, and how full its context window was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_created: u64,
    /// What the last turn sent as context: input plus cached tokens. Zero
    /// until the first assistant message.
    pub context: u64,
    /// The model's context window, when the run reported it; else a guess
    /// from the model's name and how much has been sent (`window()`).
    pub window: Option<u64>,
}

impl Usage {
    /// Everything the run sent and received.
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_created
    }

    /// The context window: as reported, else 1M for a `[1m]` model or when
    /// more than 200k has already fit, else 200k.
    pub fn window(&self, model: Option<&str>) -> u64 {
        self.window.unwrap_or_else(|| {
            if self.context > 200_000 || model.is_some_and(|m| m.contains("[1m]")) {
                1_000_000
            } else {
                200_000
            }
        })
    }

    /// How full the context is, 0 to 100 and over. None before any turn.
    pub fn fill(&self, model: Option<&str>) -> Option<u32> {
        (self.context > 0)
            .then(|| (self.context as f64 * 100.0 / self.window(model) as f64).round() as u32)
    }
}

/// Claude Code's `system/init`: the model that ran and its MCP servers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Init {
    pub model: String,
    /// Each MCP server's name and status (`connected`, `failed`, …).
    pub servers: Vec<(String, String)>,
}

impl Init {
    /// The servers that didn't connect.
    pub fn failed(&self) -> Vec<&str> {
        self.servers
            .iter()
            .filter(|(_, status)| status != "connected")
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// One run's changes, from its `before` checkpoint to its `after` (or to
/// the folder now, while it runs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPatch {
    pub patch: String,
    /// Files left out because they came from the base (the run rebased or
    /// merged it in) rather than from the agent.
    pub from_base: Vec<String>,
}

/// One line of a task's transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// What the agent was asked (the prompt, or feedback).
    Prompt(String),
    /// The agent talking.
    Text(String),
    /// A tool call: its name, and the gist of what it did (a command, a file).
    Tool { name: String, detail: String },
    /// A tool call that failed or was refused, with the reason.
    ToolError(String),
    /// The end of a run.
    Done {
        ok: bool,
        summary: String,
        cost_usd: Option<f64>,
        turns: Option<u32>,
        seconds: Option<f64>,
    },
    /// Raw output: a custom command's, or anything that wasn't an event.
    Output(String),
    /// The agent asks to use a tool its permission mode doesn't allow
    /// (through the host, #20): the question's id, the tool, the gist.
    Ask {
        id: String,
        tool: String,
        detail: String,
        /// What "Allow for this session" would cover, from the rules Claude
        /// Code suggests (#40): "`npm test` commands". None: it suggests
        /// none, so only this once.
        session: Option<String>,
        /// A permission, a question with options, or a plan.
        kind: AskKind,
    },
    /// How a question was answered (`message`: what the user said instead;
    /// `always`: allowed for the rest of the session).
    Answer {
        id: String,
        allow: bool,
        message: String,
        always: bool,
    },
    /// A message sent to the agent while it worked (steering, #22).
    Steer(String),
    /// Claude Code started: the model that actually ran and its MCP servers.
    Init(Init),
    /// Claude Code compacted the conversation here (#98).
    Compacted,
    /// The folder's setup script (#23): started (`ok` None), or done. Its
    /// output is the `Output` lines between.
    Setup { ok: Option<bool>, code: Option<i32> },
    /// The agent's plan as it now stands (`TodoWrite`, #96). Each one
    /// replaces the last.
    Todos(Vec<Todo>),
    /// What a command printed (#97), paired with its call by `name` and
    /// `detail`. `ok` is false when the tool reported an error (a command
    /// that exited non-zero). `output` keeps the end of it; `truncated`
    /// says the log holds more.
    Result {
        name: String,
        detail: String,
        ok: bool,
        output: String,
        truncated: bool,
    },
    /// The agent thinking, before the text or call it led to (#99).
    Thinking(String),
    /// A subagent started by the `Task`/`Agent` call `id` (#99).
    Subagent {
        id: String,
        /// Its kind ("Explore"), if the call named one.
        agent: String,
        description: String,
        prompt: String,
    },
    /// What a subagent did: an entry of the call `parent` started.
    Nested { parent: String, entry: Box<Entry> },
}

/// The most of a command's output kept in the transcript: its end, where
/// a failure usually is. The full text stays in the run's log.
const MAX_RESULT_CHARS: usize = 8000;

/// What kind of check a command is, if it is one (#97).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    Tests,
    Build,
}

/// Whether a shell command runs a project's tests or builds it. Looks at
/// each command in a `&&`/`;`/`|` chain, so `cd app && pnpm test` counts.
pub fn check_kind(command: &str) -> Option<CheckKind> {
    const TESTS: [&str; 14] = [
        "cargo test",
        "cargo nextest",
        "pnpm test",
        "pnpm run test",
        "npm test",
        "npm run test",
        "yarn test",
        "bun test",
        "pytest",
        "python -m pytest",
        "go test",
        "mvn test",
        "gradle test",
        "dotnet test",
    ];
    const BUILDS: [&str; 11] = [
        "cargo build",
        "cargo check",
        "pnpm build",
        "pnpm run build",
        "npm run build",
        "yarn build",
        "tsc",
        "go build",
        "make",
        "mvn package",
        "gradle build",
    ];
    for part in command.split(['&', ';', '|', '\n']) {
        let part = part.trim();
        let starts = |list: &[&str]| {
            list.iter().any(|c| {
                part.strip_prefix(c)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
            })
        };
        if starts(&TESTS) {
            return Some(CheckKind::Tests);
        }
        if starts(&BUILDS) {
            return Some(CheckKind::Build);
        }
    }
    None
}

/// How the last test run and the last build in a transcript went: `Some(ok)`
/// for the last one that ran, `None` if none did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Checks {
    pub tests: Option<bool>,
    pub build: Option<bool>,
}

/// The outcome of the last test run and build in `entries` (#97).
pub fn checks_of(entries: &[Entry]) -> Checks {
    let mut checks = Checks::default();
    for entry in entries {
        let mut entry = entry;
        while let Entry::Nested { entry: inner, .. } = entry {
            entry = inner;
        }
        if let Entry::Result {
            name, detail, ok, ..
        } = entry
        {
            if name != "Bash" {
                continue;
            }
            match check_kind(detail) {
                Some(CheckKind::Tests) => checks.tests = Some(*ok),
                Some(CheckKind::Build) => checks.build = Some(*ok),
                None => {}
            }
        }
    }
    checks
}

/// One step of the agent's plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    /// The step: "Migrate the tests".
    pub content: String,
    /// The step while it's being done: "Migrating the tests".
    pub active: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// What a running agent is doing (#96): how far through its plan, and the
/// tool call it made last.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    /// Steps done, and how many there are (0 when it has no plan).
    pub done: usize,
    pub total: usize,
    /// The step in progress, as it says it ("Migrating the tests").
    pub step: Option<String>,
    /// Its latest tool call: the tool and its gist.
    pub tool: Option<(String, String)>,
}

/// `TodoWrite`'s list.
fn todos(input: &Json) -> Vec<Todo> {
    input
        .get("todos")
        .array()
        .iter()
        .map(|t| {
            let content = t.get("content").str().unwrap_or_default().to_string();
            Todo {
                active: t
                    .get("activeForm")
                    .str()
                    .filter(|a| !a.trim().is_empty())
                    .unwrap_or(&content)
                    .to_string(),
                content,
                status: match t.get("status").str() {
                    Some("completed") => TodoStatus::Completed,
                    Some("in_progress") => TodoStatus::InProgress,
                    _ => TodoStatus::Pending,
                },
            }
        })
        .collect()
}

/// Progress through a plan: done, total and the step in progress.
pub fn plan_progress(list: &[Todo]) -> (usize, usize, Option<String>) {
    let done = list
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    let step = list
        .iter()
        .find(|t| t.status == TodoStatus::InProgress)
        .map(|t| t.active.clone());
    (done, list.len(), step)
}

/// A question the agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub id: String,
    pub tool: String,
    pub detail: String,
    /// What "Allow for this session" would cover, if it's offered.
    pub session: Option<String>,
    pub kind: AskKind,
}

impl Ask {
    /// The question in a few words, for a list row or a notification:
    /// "asks: Keep the old endpoint working?", "has a plan for you to
    /// review: Rename greet", "wants to run `make test`".
    pub fn gist(&self) -> String {
        let with = |s: &str| {
            if self.detail.is_empty() {
                s.to_string()
            } else {
                format!("{s}: {}", self.detail)
            }
        };
        match &self.kind {
            AskKind::Questions(_) => with("asks"),
            AskKind::Plan(_) => with("has a plan for you to review"),
            AskKind::Permission => {
                let verb = match self.tool.as_str() {
                    "Bash" => "run",
                    "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => "edit",
                    "WebFetch" => "fetch",
                    "WebSearch" => "search the web for",
                    tool => return with(&format!("wants to use {tool}")),
                };
                if self.detail.is_empty() {
                    format!("wants to {verb}")
                } else {
                    format!("wants to {verb} `{}`", self.detail)
                }
            }
        }
    }
}

/// What the agent is asking for. Claude Code asks all three through the
/// same `can_use_tool` request; the tool says which it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskKind {
    /// To use a tool its permission mode doesn't allow: allow or deny.
    Permission,
    /// `AskUserQuestion` (#102): questions, each with options to pick from.
    /// The answer goes back as the tool's input, with `answers` added.
    Questions(Vec<Question>),
    /// `ExitPlanMode` (#101): the plan, in markdown. Approving lets it
    /// start; denying with comments sends it back to planning.
    Plan(String),
}

/// One of `AskUserQuestion`'s questions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub question: String,
    /// A short label for it ("Approach").
    pub header: String,
    pub options: Vec<Choice>,
    /// More than one option can be picked.
    pub multi: bool,
}

/// An option of a question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub description: String,
}

impl AskKind {
    /// Which kind a tool's request is, from its name and input.
    pub fn of(tool: &str, input: &Json) -> AskKind {
        match tool {
            "AskUserQuestion" => AskKind::Questions(
                input
                    .get("questions")
                    .array()
                    .iter()
                    .map(|q| Question {
                        question: q.get("question").str().unwrap_or_default().to_string(),
                        header: q.get("header").str().unwrap_or_default().to_string(),
                        options: q
                            .get("options")
                            .array()
                            .iter()
                            .map(|o| Choice {
                                label: o.get("label").str().unwrap_or_default().to_string(),
                                description: o
                                    .get("description")
                                    .str()
                                    .unwrap_or_default()
                                    .to_string(),
                            })
                            .collect(),
                        multi: q.get("multiSelect").bool() == Some(true),
                    })
                    .collect(),
            ),
            // spor's own `ask` tool (#106): one question, options optional.
            crate::mcp::ASK_TOOL => AskKind::Questions(vec![Question {
                question: input.get("question").str().unwrap_or_default().to_string(),
                header: String::new(),
                options: input
                    .get("options")
                    .array()
                    .iter()
                    .map(|o| Choice {
                        label: o
                            .str()
                            .or(o.get("label").str())
                            .unwrap_or_default()
                            .to_string(),
                        description: o.get("description").str().unwrap_or_default().to_string(),
                    })
                    .collect(),
                multi: false,
            }]),
            "ExitPlanMode" => {
                AskKind::Plan(input.get("plan").str().unwrap_or_default().to_string())
            }
            _ => AskKind::Permission,
        }
    }
}

/// The gist of a question for one line (the Inbox, a notification): the
/// first question, or the plan's first line.
fn ask_detail(kind: &AskKind, input: &Json) -> String {
    match kind {
        AskKind::Permission => tool_detail(input),
        AskKind::Questions(qs) => qs.first().map(|q| q.question.clone()).unwrap_or_default(),
        AskKind::Plan(plan) => plan
            .lines()
            .map(|l| l.trim().trim_start_matches('#').trim())
            .find(|l| !l.is_empty())
            .unwrap_or_default()
            .to_string(),
    }
}

/// The input that answers `AskUserQuestion`: what it asked, with
/// `answers` keyed by each question's text. Several picks are joined with
/// ", ", as Claude Code expects.
pub fn answered_input(input: &Json, answers: &[Vec<String>]) -> String {
    let mut out = match input {
        Json::Object(o) => o.clone(),
        _ => Default::default(),
    };
    let mut map = std::collections::BTreeMap::new();
    for (q, picked) in input.get("questions").array().iter().zip(answers) {
        if let Some(text) = q.get("question").str() {
            map.insert(text.to_string(), Json::String(picked.join(", ")));
        }
    }
    out.insert("answers".into(), Json::Object(map));
    // `ask` has one question, and carries its answer in `answer`.
    if input.get("question").str().is_some() {
        let picked = answers.first().map(|a| a.join(", ")).unwrap_or_default();
        out.insert("answer".into(), Json::String(picked));
    }
    Json::Object(out).to_json()
}

/// A task's setup script (#23), for its details card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    /// The script, relative to the task's folder when it's in it.
    pub script: Option<PathBuf>,
    /// How it went; None if it hasn't run.
    pub state: Option<SetupState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupState {
    Running,
    Worked,
    /// Its exit code, if it got as far as one.
    Failed(Option<i32>),
    Skipped,
    /// It worked, and runs again before the next agent.
    Due,
}

/// The runs of one task.
#[derive(Debug, Clone)]
pub struct Runs {
    dir: PathBuf,
    /// Where the task's checkpoints live: `refs/spor/tasks/<id>`.
    refs: String,
    worktree: PathBuf,
    /// For giving a pending task its branch when an agent first writes.
    tasks: Tasks,
    base: String,
    /// The main checkout, for setup scripts that copy from it.
    repo_root: PathBuf,
    /// Images attached to prompts, which Claude may read.
    attachments: PathBuf,
}

impl Runs {
    pub fn of(tasks: &Tasks, task: &Task) -> Self {
        Self {
            dir: tasks.run_dir(&task.id),
            refs: checkpoint_prefix(&task.id),
            worktree: task.worktree.clone(),
            tasks: tasks.clone(),
            base: task.start().to_string(),
            repo_root: tasks.root().to_path_buf(),
            attachments: tasks.attachments_dir(),
        }
    }

    /// The setup script a new task's folder still needs (#23): the
    /// repository's own `.git/spor/setup` if there is one, else the
    /// committed `.spor/setup`; none once setup worked (or was skipped).
    pub fn setup_script(&self, task: &Task) -> Option<PathBuf> {
        if self.dir.join("setup.ok").exists() {
            return None;
        }
        self.find_setup_script(task)
    }

    /// The task's setup script, whether or not it still has to run.
    fn find_setup_script(&self, task: &Task) -> Option<PathBuf> {
        let private = crate::git::Repo::new(&self.repo_root)
            .common_dir()
            .ok()
            .map(|d| d.join("spor").join("setup"));
        private
            .into_iter()
            .chain([task.worktree.join(".spor").join("setup")])
            .find(|p| p.is_file())
    }

    /// Don't run the setup script: start the next agent without it.
    pub fn skip_setup(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        std::fs::write(self.dir.join("setup.ok"), "skipped\n").map_err(|e| e.to_string())
    }

    /// Run the setup script again, before the next agent.
    pub fn rerun_setup(&self) -> Result<(), String> {
        match std::fs::remove_file(self.dir.join("setup.ok")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    /// The task's setup script and how it went, for its details card.
    pub fn setup(&self, task: &Task) -> Setup {
        let script = self.find_setup_script(task).map(|p| {
            p.strip_prefix(&task.worktree)
                .map(Path::to_path_buf)
                .unwrap_or(p)
        });
        let ok = std::fs::read_to_string(self.dir.join("setup.ok")).ok();
        if ok.as_deref() == Some("skipped\n") {
            return Setup {
                script,
                state: Some(SetupState::Skipped),
            };
        }
        // The latest run that ran it says how it went.
        let mut state = None;
        for run in self.list() {
            let log = std::fs::read_to_string(self.file(run.index, "log")).unwrap_or_default();
            for line in log
                .lines()
                .filter(|l| l.starts_with(r#"{"type":"spor_setup""#))
            {
                if let Some(Entry::Setup { ok, code }) = parse_claude(line).pop() {
                    state = Some(match ok {
                        None if run.state == RunState::Running => SetupState::Running,
                        None => SetupState::Failed(None),
                        Some(true) => SetupState::Worked,
                        Some(false) => SetupState::Failed(code),
                    });
                }
            }
        }
        // Run again (rerun_setup) once it worked: due before the next agent.
        if state == Some(SetupState::Worked) && ok.is_none() {
            state = Some(SetupState::Due);
        }
        Setup { script, state }
    }

    /// Follow-ups waiting for the agent to finish (#22), oldest first.
    /// Kept with the task, so every window sees them, and they outlive the
    /// app.
    pub fn queued(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("queue"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(unescape)
            .collect()
    }

    /// Add a follow-up to send when the agent finishes.
    pub fn queue(&self, message: &str) -> Result<(), String> {
        let mut queued = self.queued();
        queued.push(message.to_string());
        self.write_queue(&queued)
    }

    /// Drop queued follow-up `index`.
    pub fn unqueue(&self, index: usize) -> Result<(), String> {
        let mut queued = self.queued();
        if index >= queued.len() {
            return Err("That follow-up was already sent".into());
        }
        queued.remove(index);
        self.write_queue(&queued)
    }

    /// Send the oldest queued follow-up, if the agent is idle: as
    /// [`Runs::follow_up`] does. It leaves the queue only once it's sent.
    pub fn send_queued(
        &self,
        task: &Task,
        chosen: Option<(&AgentKind, &Model)>,
        permissions: Permissions,
    ) -> Result<bool, String> {
        if self.latest().is_some_and(|r| r.state == RunState::Running) {
            return Ok(false);
        }
        let Some(first) = self.queued().into_iter().next() else {
            return Ok(false);
        };
        self.follow_up(task, Some(&first), chosen, permissions)?;
        // Someone may have changed the queue meanwhile: drop this message,
        // wherever it is now.
        let mut queued = self.queued();
        if let Some(i) = queued.iter().position(|m| *m == first) {
            queued.remove(i);
            self.write_queue(&queued)?;
        }
        Ok(true)
    }

    fn write_queue(&self, queued: &[String]) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.dir.join("queue");
        if queued.is_empty() {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        let text: String = queued.iter().map(|m| escape(m) + "\n").collect();
        // Written whole, then moved into place: another window reading it
        // never sees half a queue.
        let tmp = self.dir.join("queue.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    /// The ref of one of run `index`'s checkpoints: before, after, undone.
    pub fn checkpoint_ref(&self, index: usize, which: &str) -> String {
        format!("{}/run-{index}/{which}", self.refs)
    }

    /// Start an agent in the task's folder. With `resume`, Claude Code
    /// continues that session with `prompt` as the next message.
    pub fn start(
        &self,
        task: &Task,
        kind: &AgentKind,
        prompt: &str,
        resume: Option<&str>,
        permissions: Permissions,
    ) -> Result<Run, String> {
        self.start_with(task, kind, prompt, resume, permissions, &Model::default())
    }

    /// What the agent is told about where it is (#105): its folder, branch
    /// and base, that spor publishes, the issue, the attachments, and the
    /// other active tasks with the files they have changed so far. A
    /// repository adds to it with a committed `.spor/context.md`.
    pub fn context(&self, task: &Task) -> String {
        let mut out = format!(
            "You are working in spor, which runs coding agents in parallel, one folder per task.\n\
             - Your folder is {} (a git worktree beside the repository), on branch {}, started from {}.\n\
             - Don't push or open pull requests: spor publishes when the person decides.\n",
            task.worktree.display(),
            task.branch,
            task.start()
        );
        if let Some(number) = task.issue {
            out.push_str(&format!(
                "- This task is for issue #{number}; its pull request will say `Closes #{number}`.\n"
            ));
        }
        if self.worktree.join(".spor").join("checks").exists() {
            out.push_str("- The repository has checks: run `.spor/checks` before you finish.\n");
        }
        out.push_str(&format!(
            "- Images attached to prompts are kept in {}.\n",
            self.attachments.display()
        ));
        if let Ok(tasks) = Tasks::open(&crate::git::Repo::new(&self.repo_root)) {
            let others = crate::mcp::others_touching(&tasks, task);
            if !others.is_empty() {
                out.push_str(
                    "\nOther agents are working in this repository right now. Avoid changing the files \
                     they have changed unless your task needs it, and say so when you do:\n",
                );
                for (id, prompt, files) in others {
                    let first = prompt.lines().next().unwrap_or_default().trim();
                    let first: String = first.chars().take(100).collect();
                    out.push_str(&format!("- {id}: {first}\n"));
                    if !files.is_empty() {
                        let shown: Vec<&str> = files.iter().take(15).map(String::as_str).collect();
                        let more = files.len() - shown.len();
                        out.push_str(&format!(
                            "  changed so far: {}{}\n",
                            shown.join(", "),
                            if more > 0 {
                                format!(" and {more} more")
                            } else {
                                String::new()
                            }
                        ));
                    }
                }
                out.push_str("The `other_tasks` tool of the spor MCP server tells you what they touch now.\n");
            }
        }
        if let Ok(extra) = std::fs::read_to_string(task.worktree.join(".spor").join("context.md")) {
            let extra = extra.trim();
            if !extra.is_empty() {
                out.push_str(&format!("\n{extra}\n"));
            }
        }
        out
    }

    /// The arguments that give Claude Code spor's MCP server (#106): a
    /// config file in the run directory that starts the app's executable
    /// as the server, and the tools it may use without asking. `ask` isn't
    /// among them: it is a question for the person.
    fn mcp_args(&self, program: &Path, task: &Task, index: usize, log: &Path) -> Vec<String> {
        let file = self.file(index, "mcp.json");
        let args = [
            crate::mcp::ARG.to_string(),
            self.repo_root.display().to_string(),
            task.id.clone(),
            log.display().to_string(),
        ];
        let config = format!(
            r#"{{"mcpServers":{{"spor":{{"command":{},"args":[{}]}}}}}}"#,
            host::quote(&program.to_string_lossy()),
            args.iter()
                .map(|a| host::quote(a))
                .collect::<Vec<_>>()
                .join(",")
        );
        if std::fs::write(&file, config).is_err() {
            return Vec::new();
        }
        vec![
            "--mcp-config".into(),
            file.display().to_string(),
            "--allowedTools".into(),
            crate::mcp::READ_ONLY_TOOLS.join(","),
        ]
    }

    /// Whether this task's runs check the repository before finishing
    /// (#103). On unless turned off for the task.
    pub fn checks_enabled(&self) -> bool {
        !self.dir.join("no-checks").exists()
    }

    /// Turn the Stop hook on or off for the task's later runs.
    pub fn set_checks(&self, on: bool) -> Result<(), String> {
        let flag = self.dir.join("no-checks");
        if on {
            match std::fs::remove_file(&flag) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            }
        } else {
            std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
            std::fs::write(flag, "").map_err(|e| e.to_string())
        }
    }

    /// Format edited files with `.spor/format` (opt-in per task, #103).
    pub fn format_enabled(&self) -> bool {
        self.dir.join("format-on").exists()
    }

    pub fn set_format(&self, on: bool) -> Result<(), String> {
        let flag = self.dir.join("format-on");
        if on {
            std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
            std::fs::write(flag, "").map_err(|e| e.to_string())
        } else {
            match std::fs::remove_file(&flag) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            }
        }
    }

    /// The arguments that pass spor's hooks to Claude Code (#103): a
    /// settings file in the run directory, so nothing lands in the
    /// repository or the user's own settings.
    fn hook_args(&self, program: &Path, task: &Task, index: usize, log: &Path) -> Vec<String> {
        let checks = self.checks_enabled() && task.worktree.join(".spor").join("checks").is_file();
        let format = self.format_enabled() && task.worktree.join(".spor").join("format").is_file();
        let Some(json) = crate::hooks::settings(
            program,
            &task.worktree,
            log,
            &self.file(index, "tries"),
            checks,
            format,
            crate::hooks::DEFAULT_TRIES,
        ) else {
            return Vec::new();
        };
        let file = self.file(index, "settings.json");
        if std::fs::write(&file, json).is_err() {
            return Vec::new();
        }
        vec!["--settings".into(), file.display().to_string()]
    }

    /// What the running agent last reported with `report_progress` (#106):
    /// the step, how many there are, and its note.
    pub fn reported(&self) -> Option<(usize, usize, String)> {
        let run = self.latest()?;
        let log = std::fs::read_to_string(self.file(run.index, "log")).ok()?;
        log.lines()
            .rev()
            .filter(|l| l.contains("spor_progress"))
            .filter_map(Json::parse)
            .find(|e| e.get("type").str() == Some("spor_progress"))
            .map(|e| {
                (
                    e.get("step").num().unwrap_or(0.0) as usize,
                    e.get("of").num().unwrap_or(0.0) as usize,
                    e.get("note").str().unwrap_or_default().to_string(),
                )
            })
    }

    /// `start`, with Claude Code on `model`.
    pub fn start_with(
        &self,
        task: &Task,
        kind: &AgentKind,
        prompt: &str,
        resume: Option<&str>,
        permissions: Permissions,
        model: &Model,
    ) -> Result<Run, String> {
        if self.latest().is_some_and(|r| r.state == RunState::Running) {
            return Err(format!("An agent is already working on {}", task.id));
        }
        // A task has no branch until an agent that can change things runs
        // (#110). Planning only reads, so it looks at the main checkout and
        // leaves nothing behind; anything else gets its branch now.
        let started;
        let (task, folder) = if task.pending && permissions.mode == Mode::Plan {
            (task, self.repo_root.clone())
        } else {
            started = self.tasks.start_branch(task)?;
            (&started, started.worktree.clone())
        };
        if !folder.exists() {
            return Err(format!("{}'s folder is gone", task.id));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let index = self.count();
        // Made up front, so an image attached while it works can be read.
        let _ = std::fs::create_dir_all(&self.attachments);
        let (program, mut args) =
            command_for(kind, prompt, resume, permissions, model, &self.attachments);
        // What the agent is told, kept beside the run so it's clear afterwards.
        let context = self.context(task);
        let _ = std::fs::write(self.file(index, "context"), &context);
        if *kind == AgentKind::Claude {
            // Before `-p <prompt>`, which stays last.
            let at = args.iter().position(|a| a == "-p").unwrap_or(args.len());
            args.splice(
                at..at,
                ["--append-system-prompt".to_string(), context.clone()],
            );
        }
        let log = self.file(index, "log");
        let exit = self.file(index, "exit");
        // What the folder was like before, for this run's diff and undo.
        // Best effort: a run without checkpoints still runs.
        let _ = checkpoint::capture(&folder, &self.checkpoint_ref(index, "before"));
        // "$@" is the agent; its exit code lands in the exit file however
        // it ends, short of the machine going down. The after checkpoint is
        // taken first, so a finished run always has one.
        let script = r#""$@" >> "$SPOR_LOG" 2>&1; code=$?
SPOR_REF="$SPOR_AFTER" sh -c "$SPOR_SNAPSHOT" > /dev/null 2>&1
echo $code > "$SPOR_EXIT""#;
        let hosted = *kind == AgentKind::Claude && host::program().is_some();
        // The agent's command line: Claude under the host (it can ask, and
        // be steered, #20), or anything under the plain wrapper.
        let agent: Vec<std::ffi::OsString> = match host::program().filter(|_| hosted) {
            Some(host_program) => [
                host_program.as_os_str().to_owned(),
                host::ARG.into(),
                log.clone().into(),
                exit.clone().into(),
                self.file(index, "sock").into(),
                self.checkpoint_ref(index, "after").into(),
                "--".into(),
                program.clone().into(),
            ]
            .into_iter()
            .chain(hosted_args(&args).into_iter().map(Into::into))
            .chain(
                self.mcp_args(host_program, task, index, &log)
                    .into_iter()
                    .chain(self.hook_args(host_program, task, index, &log))
                    .map(Into::into),
            )
            .collect(),
            None => ["sh", "-c", script, "spor-agent", &program]
                .into_iter()
                .map(Into::into)
                .chain(args.iter().map(Into::into))
                .collect(),
        };
        // A new folder's setup script runs first, in the same detached
        // process, and the agent only if it worked (#23).
        // (Not for a plan that reads the main checkout.)
        let setup = if folder == task.worktree {
            self.setup_script(task)
        } else {
            None
        };
        let mut cmd = match &setup {
            Some(_) => {
                let mut cmd = Command::new("sh");
                cmd.arg("-c")
                    .arg(SETUP_THEN_AGENT)
                    .arg("spor-setup")
                    .args(&agent);
                cmd
            }
            None => {
                let mut cmd = Command::new(&agent[0]);
                cmd.args(&agent[1..]);
                cmd
            }
        };
        if let Some(path) = &setup {
            cmd.env("SPOR_SETUP", path)
                .env("SPOR_SETUP_OK", self.dir.join("setup.ok"))
                .env("SPOR_REPO", &self.repo_root)
                .env("SPOR_BASE", &task.base);
        }
        cmd.current_dir(&folder)
            .env("SPOR_LOG", &log)
            .env("SPOR_EXIT", &exit)
            .env("SPOR_AFTER", self.checkpoint_ref(index, "after"))
            .env("SPOR_SNAPSHOT", checkpoint::SNAPSHOT_SH)
            .env("SPOR_PROMPT", prompt)
            .env("SPOR_CONTEXT", &context)
            .env("SPOR_TASK", &task.id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            // Its own process group, so Stop reaches whatever it started.
            use std::os::unix::process::CommandExt as _;
            cmd.process_group(0);
        }
        let started = now();
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Couldn't start {}: {e}", kind.label()))?;
        let pid = child.id();
        let meta = [
            ("kind", kind_name(kind)),
            ("hosted", hosted.to_string()),
            ("mode", permissions.mode.flag().to_string()),
            ("model", model.name.clone().unwrap_or_default()),
            (
                "effort",
                model.effort.map(Effort::flag).unwrap_or_default().into(),
            ),
            ("prompt", prompt.to_string()),
            ("started", started.to_string()),
            ("pid", pid.to_string()),
        ];
        let text: String = meta
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}\t{}\n", escape(v)))
            .collect();
        std::fs::write(self.file(index, "meta"), text).map_err(|e| e.to_string())?;
        // Reap it when it ends; the exit file is what spor reads.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(Run {
            index,
            kind: kind.clone(),
            prompt: prompt.to_string(),
            state: RunState::Running,
            started,
            ended: None,
            session: None,
            model: model.clone(),
            cost_usd: None,
            usage: Usage::default(),
            init: None,
            undone: false,
        })
    }

    /// End the running agent's turn: interrupt it when it's listening (it
    /// finishes cleanly and keeps its session, #22), and kill it only if
    /// that doesn't land within `grace` or it can't be talked to.
    pub fn finish(&self, grace: std::time::Duration) -> Result<(), String> {
        if self.listening() && self.interrupt().is_ok() {
            let until = std::time::Instant::now() + grace;
            while std::time::Instant::now() < until {
                if self.latest().is_none_or(|r| r.state != RunState::Running) {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        self.stop()
    }

    /// Stop the running agent, and everything it started.
    pub fn stop(&self) -> Result<(), String> {
        let Some(run) = self.latest().filter(|r| r.state == RunState::Running) else {
            return Ok(());
        };
        let pid = self.meta(run.index, "pid").unwrap_or_default();
        std::fs::write(self.file(run.index, "stopped"), "").map_err(|e| e.to_string())?;
        let group = format!("-{pid}");
        let ok = Command::new("kill")
            .args(["-TERM", "--", &group])
            .status()
            .is_ok_and(|s| s.success())
            || Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .is_ok_and(|s| s.success());
        if ok {
            // The wrapper went down with the agent, so its after checkpoint
            // never happened: take it here, once the agent has let go.
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = checkpoint::capture(&self.worktree, &self.checkpoint_ref(run.index, "after"));
            Ok(())
        } else {
            Err("Couldn't stop the agent".into())
        }
    }

    /// Every run, oldest first.
    pub fn list(&self) -> Vec<Run> {
        (0..self.count()).filter_map(|i| self.run(i)).collect()
    }

    pub fn latest(&self) -> Option<Run> {
        self.count().checked_sub(1).and_then(|i| self.run(i))
    }

    /// The session to continue with feedback: the latest one a run that
    /// wasn't undone reported.
    pub fn session(&self) -> Option<String> {
        self.list()
            .into_iter()
            .rev()
            .filter(|r| !r.undone)
            .find_map(|r| r.session)
    }

    /// Run `index`'s changes. While it runs, up to the folder as it is now.
    /// When the run moved onto a new base (it rebased, or merged the base
    /// in), files that now match the base are left out: they came with the
    /// base, not from the agent.
    pub fn patch(&self, index: usize) -> Result<RunPatch, String> {
        let run = self.run(index).ok_or("No such run")?;
        let wt = &self.worktree;
        let before = checkpoint::resolve(wt, &self.checkpoint_ref(index, "before"))
            .ok_or("This run has no checkpoint (it ran before spor took them)")?;
        let after_ref = self.checkpoint_ref(index, "after");
        let after = match checkpoint::resolve(wt, &after_ref) {
            Some(a) if run.state != RunState::Running => a,
            // Running: the folder now. Ended without one (stopped while spor
            // was closed): take it now, as close as we can get.
            _ => {
                let live = if run.state == RunState::Running {
                    self.checkpoint_ref(index, "live")
                } else {
                    after_ref
                };
                checkpoint::capture(wt, &live)?
            }
        };
        let moved = |snapshot: &str| {
            checkpoint::parent(wt, snapshot).and_then(|head| {
                let repo = crate::git::Repo::new(wt);
                repo.merge_base(&head, &self.base).ok()
            })
        };
        let changed = checkpoint::changed(wt, &before, &after)?;
        let mut from_base = Vec::new();
        let paths = if moved(&before) != moved(&after) {
            let differs: std::collections::HashSet<String> =
                checkpoint::changed(wt, &self.base, &after)?
                    .into_iter()
                    .collect();
            let (mine, theirs): (Vec<String>, Vec<String>) =
                changed.into_iter().partition(|p| differs.contains(p));
            from_base = theirs;
            Some(mine)
        } else {
            None
        };
        Ok(RunPatch {
            patch: checkpoint::diff(wt, &before, &after, paths.as_deref())?,
            from_base,
        })
    }

    /// Whether the running agent can be talked to: asked questions answered,
    /// steered, interrupted (it runs under the host).
    pub fn listening(&self) -> bool {
        self.latest()
            .is_some_and(|r| r.state == RunState::Running && self.file(r.index, "sock").exists())
    }

    fn command(&self, line: &str) -> Result<(), String> {
        let run = self
            .latest()
            .filter(|r| r.state == RunState::Running)
            .ok_or("No agent is working")?;
        host::request(&self.file(run.index, "sock"), line)
    }

    /// Send a message to the agent while it works: it takes it into the
    /// turn it's on (steering).
    pub fn send(&self, text: &str) -> Result<(), String> {
        self.command(&format!(r#"{{"message":{}}}"#, host::quote(text)))
    }

    /// Ask the agent to stop the turn it's on. It ends the run cleanly,
    /// keeping the session, unlike [`Runs::stop`].
    pub fn interrupt(&self) -> Result<(), String> {
        self.command(r#"{"interrupt":true}"#)
    }

    /// Answer a question the agent asked: allow the tool, or not (with what
    /// to do instead, if `message` says).
    pub fn answer(&self, id: &str, allow: bool, message: &str) -> Result<(), String> {
        self.command(&format!(
            r#"{{"answer":{{"request_id":{},"allow":{allow},"message":{}}}}}"#,
            host::quote(id),
            host::quote(message)
        ))
    }

    /// Allow the tool, and don't ask again about this kind of action while
    /// the session lasts: the rules Claude Code suggested go back with the
    /// answer (`Ask::session` says what they cover).
    pub fn allow_for_session(&self, id: &str) -> Result<(), String> {
        self.command(&format!(
            r#"{{"answer":{{"request_id":{},"allow":true,"always":true,"message":""}}}}"#,
            host::quote(id)
        ))
    }

    /// Answer `AskUserQuestion` (#102): one list of picked labels (or
    /// words of your own) per question, in order. The agent carries on in
    /// the same turn with them.
    pub fn answer_questions(&self, id: &str, answers: &[Vec<String>]) -> Result<(), String> {
        let input = self
            .ask_input(id)
            .ok_or("That question was already answered")?;
        if answers
            .iter()
            .all(|a| a.iter().all(|s| s.trim().is_empty()))
        {
            return Err("Pick an answer first".into());
        }
        // The picks are the answer's message too, so the transcript says
        // what was answered.
        let said: Vec<String> = answers
            .iter()
            .map(|a| a.join(", "))
            .filter(|a| !a.trim().is_empty())
            .collect();
        self.command(&format!(
            r#"{{"answer":{{"request_id":{},"allow":true,"message":{},"input":{}}}}}"#,
            host::quote(id),
            host::quote(&said.join(" · ")),
            answered_input(&input, answers)
        ))
    }

    /// Approve the agent's plan (#101): it leaves plan mode and starts, in
    /// `mode` from here on.
    pub fn approve_plan(&self, id: &str, mode: Mode) -> Result<(), String> {
        if mode == Mode::Plan {
            return Err("Pick how it should work once the plan is approved".into());
        }
        self.answer(id, true, "")?;
        self.set_mode(mode)
    }

    /// The input of a question the running agent asked, from its log.
    fn ask_input(&self, id: &str) -> Option<Json> {
        let run = self.latest().filter(|r| r.state == RunState::Running)?;
        let log = std::fs::read_to_string(self.file(run.index, "log")).ok()?;
        log.lines()
            .filter(|l| l.contains("can_use_tool") && l.contains(id))
            .filter_map(Json::parse)
            .find(|e| e.get("request_id").str() == Some(id))
            .map(|e| e.get("request").get("input").clone())
    }

    /// Change the running agent's permission mode (Claude Code under the
    /// host). Takes effect for what it does next.
    pub fn set_mode(&self, mode: Mode) -> Result<(), String> {
        self.command(&format!(r#"{{"mode":{}}}"#, host::quote(mode.flag())))?;
        if let Some(run) = self.latest() {
            self.set_meta(run.index, "mode", mode.flag())?;
        }
        Ok(())
    }

    /// The permission mode run `index` was started with (or changed to).
    pub fn mode(&self, index: usize) -> Option<Mode> {
        self.meta(index, "mode").and_then(|m| Mode::from_flag(&m))
    }

    fn set_meta(&self, index: usize, key: &str, value: &str) -> Result<(), String> {
        let path = self.file(index, "meta");
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.starts_with(&format!("{key}\t")))
            .map(String::from)
            .collect();
        lines.push(format!("{key}\t{}", escape(value)));
        std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
    }

    /// What the running agent is doing (#96): its plan's progress and its
    /// latest tool call. Reads only the lines that carry them, so a list
    /// of tasks can ask each one. None when nothing is running.
    pub fn progress(&self) -> Option<Progress> {
        let run = self.latest().filter(|r| r.state == RunState::Running)?;
        let log = std::fs::read_to_string(self.file(run.index, "log")).ok()?;
        let mut out = Progress::default();
        let mut plan = false;
        for line in log.lines().rev() {
            if out.tool.is_some() && plan {
                break;
            }
            if !line.contains("\"tool_use\"") {
                continue;
            }
            let Some(event) = Json::parse(line) else {
                continue;
            };
            if event.get("type").str() != Some("assistant") {
                continue;
            }
            for part in event.get("message").get("content").array().iter().rev() {
                if part.get("type").str() != Some("tool_use") {
                    continue;
                }
                let name = part.get("name").str().unwrap_or("Tool");
                if !plan && name == "TodoWrite" {
                    (out.done, out.total, out.step) = plan_progress(&todos(part.get("input")));
                    plan = true;
                }
                if out.tool.is_none() && name != "TodoWrite" {
                    out.tool = Some((name.to_string(), tool_detail(part.get("input"))));
                }
            }
        }
        Some(out)
    }

    /// The questions the running agent is waiting on.
    pub fn asks(&self) -> Vec<Ask> {
        let Some(run) = self.latest().filter(|r| r.state == RunState::Running) else {
            return Vec::new();
        };
        let log = std::fs::read_to_string(self.file(run.index, "log")).unwrap_or_default();
        let entries = parse_claude(&log);
        let answered: std::collections::HashSet<&str> = entries
            .iter()
            .filter_map(|e| match e {
                Entry::Answer { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Ask {
                    id,
                    tool,
                    detail,
                    session,
                    kind,
                } if !answered.contains(id.as_str()) => Some(Ask {
                    id: id.clone(),
                    tool: tool.clone(),
                    detail: detail.clone(),
                    session: session.clone(),
                    kind: kind.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The last run that can be undone: it ended, wasn't undone, and has a
    /// before checkpoint.
    pub fn undoable(&self) -> Option<Run> {
        self.list()
            .into_iter()
            .rev()
            .find(|r| !r.undone)
            .filter(|r| r.state != RunState::Running)
            .filter(|r| {
                checkpoint::resolve(&self.worktree, &self.checkpoint_ref(r.index, "before"))
                    .is_some()
            })
    }

    /// Undo the last run: its folder goes back to how it was before it ran
    /// (the branch included), and later follow-ups resume the conversation
    /// from before it. The folder as it was is kept, for [`Runs::redo`].
    pub fn undo_last(&self) -> Result<usize, String> {
        let run = self
            .undoable()
            .ok_or("There's no run to undo (or an agent is still working)")?;
        self.undo_from(run.index)?;
        Ok(run.index)
    }

    /// The runs "Undo from here" can start at: none while an agent works;
    /// otherwise each run not yet undone that has a before checkpoint.
    pub fn undoable_from(&self) -> Vec<usize> {
        let list = self.list();
        if list.iter().any(|r| r.state == RunState::Running) {
            return Vec::new();
        }
        list.iter()
            .filter(|r| !r.undone)
            .filter(|r| {
                checkpoint::resolve(&self.worktree, &self.checkpoint_ref(r.index, "before"))
                    .is_some()
            })
            .map(|r| r.index)
            .collect()
    }

    /// Undo run `index` and every run after it: the folder as it was
    /// before `index` ran, and follow-ups resume from before it. Returns
    /// the runs it took back. [`Runs::redo`] with `index` brings them all
    /// back.
    pub fn undo_from(&self, index: usize) -> Result<Vec<usize>, String> {
        if !self.undoable_from().contains(&index) {
            return Err("That run can't be undone (or an agent is still working)".into());
        }
        let wt = &self.worktree;
        let before = checkpoint::resolve(wt, &self.checkpoint_ref(index, "before"))
            .ok_or("This run has no checkpoint")?;
        checkpoint::capture(wt, &self.checkpoint_ref(index, "undone"))?;
        checkpoint::restore(wt, &before)?;
        // Each marker names the run the undo started at, so a redo brings
        // back this batch and not runs undone on their own before it.
        let mut taken = Vec::new();
        for run in self
            .list()
            .into_iter()
            .filter(|r| r.index >= index && !r.undone)
        {
            std::fs::write(self.file(run.index, "undone"), index.to_string())
                .map_err(|e| e.to_string())?;
            taken.push(run.index);
        }
        Ok(taken)
    }

    /// Take back [`Runs::undo_last`] or [`Runs::undo_from`]: the folder as
    /// it was, and the runs count again.
    pub fn redo(&self, index: usize) -> Result<(), String> {
        let wt = &self.worktree;
        let undone = checkpoint::resolve(wt, &self.checkpoint_ref(index, "undone"))
            .ok_or("Nothing to redo")?;
        if self.latest().is_some_and(|r| r.state == RunState::Running) {
            return Err("An agent is working; stop it first".into());
        }
        checkpoint::restore(wt, &undone)?;
        for i in index..self.count() {
            let marker = self.file(i, "undone");
            let Ok(from) = std::fs::read_to_string(&marker) else {
                continue;
            };
            // Older markers are empty: those were one run each.
            let batch = match from.trim() {
                "" => i,
                n => n.parse().unwrap_or(i),
            };
            if batch == index {
                std::fs::remove_file(&marker).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// What was asked and what the agents did, run after run.
    pub fn transcript(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for run in self.list() {
            out.push(Entry::Prompt(run.prompt.clone()));
            let log = std::fs::read_to_string(self.file(run.index, "log")).unwrap_or_default();
            match run.kind {
                AgentKind::Claude => out.extend(parse_claude(&log)),
                AgentKind::Command(_) => {
                    let lines: Vec<&str> = log.lines().collect();
                    let skip = lines.len().saturating_sub(MAX_OUTPUT_LINES);
                    if skip > 0 {
                        out.push(Entry::Output(format!("… {skip} earlier lines")));
                    }
                    // spor's own marker lines (setup) are events; the rest
                    // is the command's output, as it printed it.
                    out.extend(lines[skip..].iter().map(|l| {
                        if l.starts_with(r#"{"type":"spor_"#) {
                            if let Some(e) = parse_claude(l).pop() {
                                return e;
                            }
                        }
                        Entry::Output(l.to_string())
                    }));
                }
            }
            match &run.state {
                RunState::Stopped => out.push(Entry::Done {
                    ok: false,
                    summary: "Stopped".into(),
                    cost_usd: None,
                    turns: None,
                    seconds: run.ended.map(|e| (e - run.started) as f64),
                }),
                RunState::Failed(code)
                    if !out
                        .iter()
                        .rev()
                        .take(1)
                        .any(|e| matches!(e, Entry::Done { .. })) =>
                {
                    out.push(Entry::Done {
                        ok: false,
                        summary: format!("Exited with code {code}"),
                        cost_usd: None,
                        turns: None,
                        seconds: run.ended.map(|e| (e - run.started) as f64),
                    })
                }
                RunState::Finished if run.kind != AgentKind::Claude => out.push(Entry::Done {
                    ok: true,
                    summary: "Finished".into(),
                    cost_usd: None,
                    turns: None,
                    seconds: run.ended.map(|e| (e - run.started) as f64),
                }),
                _ => {}
            }
        }
        out
    }

    pub fn run(&self, index: usize) -> Option<Run> {
        let kind = match self.meta(index, "kind")?.as_str() {
            "claude" => AgentKind::Claude,
            other => AgentKind::Command(other.strip_prefix("command ").unwrap_or(other).into()),
        };
        let started = self.meta(index, "started")?.parse().unwrap_or(0);
        let exit_file = self.file(index, "exit");
        let stopped = self.file(index, "stopped").exists();
        let read_exit = || {
            std::fs::read_to_string(&exit_file)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
        };
        let mut exit = read_exit();
        let running = exit.is_none() && alive(self.meta(index, "pid").as_deref());
        if exit.is_none() && !running {
            // It may have written its exit code and gone between the two
            // looks.
            exit = read_exit();
        }
        let ended = std::fs::metadata(&exit_file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let state = match exit {
            _ if stopped && exit.is_none_or(|c| c != 0) => RunState::Stopped,
            Some(0) => RunState::Finished,
            Some(code) => RunState::Failed(code),
            None if running => RunState::Running,
            None => RunState::Stopped,
        };
        let stats = match kind {
            AgentKind::Claude => {
                let log = std::fs::read_to_string(self.file(index, "log")).unwrap_or_default();
                claude_log(&log)
            }
            AgentKind::Command(_) => LogStats::default(),
        };
        let (session, cost_usd) = (stats.session, stats.cost);
        Some(Run {
            index,
            kind,
            prompt: self.meta(index, "prompt").unwrap_or_default(),
            state,
            started,
            ended,
            session,
            model: Model {
                name: self.meta(index, "model"),
                effort: self
                    .meta(index, "effort")
                    .and_then(|e| Effort::from_flag(&e)),
            },
            cost_usd,
            usage: stats.usage,
            init: stats.init,
            undone: self.file(index, "undone").exists(),
        })
    }

    fn count(&self) -> usize {
        (0..).take_while(|&i| self.file(i, "meta").exists()).count()
    }

    fn meta(&self, index: usize, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.file(index, "meta")).ok()?;
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('\t'))
            .map(unescape)
    }

    pub(crate) fn file(&self, index: usize, ext: &str) -> PathBuf {
        self.dir.join(format!("run-{index}.{ext}"))
    }
}

const MAX_OUTPUT_LINES: usize = 2000;

/// The program and arguments for one run.
fn command_for(
    kind: &AgentKind,
    prompt: &str,
    resume: Option<&str>,
    permissions: Permissions,
    model: &Model,
    attachments: &Path,
) -> (String, Vec<String>) {
    match kind {
        AgentKind::Claude => {
            let mut args: Vec<String> = [
                "--output-format",
                "stream-json",
                "--verbose",
                "--permission-mode",
                permissions.mode.flag(),
            ]
            .map(String::from)
            .to_vec();
            if let Some(name) = &model.name {
                args.extend(["--model".into(), name.clone()]);
            }
            if let Some(effort) = model.effort {
                args.extend(["--effort".into(), effort.flag().into()]);
            }
            if let Some(session) = resume {
                // A new session id for each follow-up, so the one resumed
                // stays as it was: undoing this run resumes from there.
                args.extend(["--resume".into(), session.into(), "--fork-session".into()]);
            }
            // Images attached to prompts are named by their path there.
            args.extend(["--add-dir".into(), attachments.display().to_string()]);
            // Publishing is always a person's call, from spor.
            args.extend(["--disallowedTools".into(), "Bash(git push:*)".into()]);
            args.extend(["-p".into(), prompt.into()]);
            ("claude".into(), args)
        }
        AgentKind::Command(template) => {
            let command = template.replace("{prompt}", &shell_quote(prompt));
            ("sh".into(), vec!["-c".into(), command])
        }
    }
}

/// Run the setup script, its output in the run's log between two markers,
/// then the agent (`"$@"`) only if it worked. A failed setup ends the run
/// with its exit code.
const SETUP_THEN_AGENT: &str = r#"printf '%s\n' '{"type":"spor_setup","state":"start"}' >> "$SPOR_LOG"
sh "$SPOR_SETUP" >> "$SPOR_LOG" 2>&1 < /dev/null; code=$?
printf '{"type":"spor_setup","state":"done","code":%s}\n' "$code" >> "$SPOR_LOG"
if [ "$code" -ne 0 ]; then echo "$code" > "$SPOR_EXIT"; exit "$code"; fi
: > "$SPOR_SETUP_OK"
exec "$@""#;

/// Claude's arguments under the host: the prompt goes on stdin (as
/// stream-json, like follow-ups), and questions come back as control
/// requests instead of being refused.
fn hosted_args(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(args.len() + 4);
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-p" {
            // `-p <prompt>`: keep print mode, drop the prompt.
            out.push("-p".into());
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out.extend([
        "--input-format".into(),
        "stream-json".into(),
        "--permission-prompt-tool".into(),
        "stdio".into(),
    ]);
    out
}

/// Where a task's checkpoints live.
pub fn checkpoint_prefix(task_id: &str) -> String {
    format!("refs/spor/tasks/{task_id}")
}

fn kind_name(kind: &AgentKind) -> String {
    match kind {
        AgentKind::Claude => "claude".into(),
        AgentKind::Command(t) => format!("command {t}"),
    }
}

/// Read Claude Code's `stream-json` output into transcript entries.
pub fn parse_claude(log: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    // Each command's gist by its call id, to pair its result (#97).
    let mut commands: std::collections::HashMap<String, (String, String)> = Default::default();
    for line in log.lines().filter(|l| !l.trim().is_empty()) {
        let Some(event) = Json::parse(line) else {
            out.push(Entry::Output(line.to_string()));
            continue;
        };
        let start = out.len();
        match event.get("type").str() {
            Some("assistant") => {
                for part in event.get("message").get("content").array() {
                    match part.get("type").str() {
                        Some("thinking") => {
                            let text = part.get("thinking").str().unwrap_or("").trim();
                            if !text.is_empty() {
                                out.push(Entry::Thinking(text.to_string()));
                            }
                        }
                        Some("text") => {
                            let text = part.get("text").str().unwrap_or("").trim();
                            if !text.is_empty() {
                                out.push(Entry::Text(text.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let name = part.get("name").str().unwrap_or("Tool");
                            let detail = tool_detail(part.get("input"));
                            let id = part.get("id").str().unwrap_or_default();
                            if matches!(name, "Bash" | "Task" | "Agent") && !id.is_empty() {
                                commands.insert(id.to_string(), (name.to_string(), detail.clone()));
                            }
                            out.push(Entry::Tool {
                                name: name.to_string(),
                                detail,
                            });
                            if name == "TodoWrite" {
                                out.push(Entry::Todos(todos(part.get("input"))));
                            }
                            if matches!(name, "Task" | "Agent") && !id.is_empty() {
                                let input = part.get("input");
                                let text = |key: &str| {
                                    input.get(key).str().unwrap_or_default().trim().to_string()
                                };
                                out.push(Entry::Subagent {
                                    id: id.to_string(),
                                    agent: text("subagent_type"),
                                    description: text("description"),
                                    prompt: text("prompt"),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for part in event.get("message").get("content").array() {
                    if part.get("type").str() != Some("tool_result") {
                        continue;
                    }
                    let failed = part.get("is_error").bool() == Some(true);
                    let content = match part.get("content") {
                        Json::String(s) => s.clone(),
                        other => other
                            .array()
                            .iter()
                            .filter_map(|c| c.get("text").str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    };
                    if failed {
                        let first = content.trim().lines().next().unwrap_or("").to_string();
                        out.push(Entry::ToolError(first));
                    }
                    // A command's output, paired with its call (#97).
                    let call = part
                        .get("tool_use_id")
                        .str()
                        .and_then(|id| commands.get(id));
                    if let Some((name, detail)) = call {
                        let text = content.trim_end();
                        let count = text.chars().count();
                        let output: String = text
                            .chars()
                            .skip(count.saturating_sub(MAX_RESULT_CHARS))
                            .collect();
                        out.push(Entry::Result {
                            name: name.clone(),
                            detail: detail.clone(),
                            ok: !failed,
                            output,
                            truncated: count > MAX_RESULT_CHARS,
                        });
                    }
                }
            }
            Some("control_request")
                if event.get("request").get("subtype").str() == Some("can_use_tool") =>
            {
                let request = event.get("request");
                let tool = request.get("tool_name").str().unwrap_or("Tool");
                let kind = AskKind::of(tool, request.get("input"));
                out.push(Entry::Ask {
                    id: event
                        .get("request_id")
                        .str()
                        .unwrap_or_default()
                        .to_string(),
                    tool: tool.to_string(),
                    detail: ask_detail(&kind, request.get("input")),
                    // Answering a question or a plan is never "always".
                    session: match kind {
                        AskKind::Permission => covers(request.get("permission_suggestions")),
                        _ => None,
                    },
                    kind,
                });
            }
            Some("system") => match event.get("subtype").str() {
                Some("init") => {
                    let init = init_of(&event);
                    if !init.model.is_empty() {
                        out.push(Entry::Init(init));
                    }
                }
                Some("compact_boundary") => out.push(Entry::Compacted),
                _ => {}
            },
            Some("spor_answer") => out.push(Entry::Answer {
                id: event
                    .get("request_id")
                    .str()
                    .unwrap_or_default()
                    .to_string(),
                allow: event.get("allow").bool() == Some(true),
                message: event.get("message").str().unwrap_or_default().to_string(),
                always: event.get("always").bool() == Some(true),
            }),
            Some("spor_setup") => out.push(match event.get("state").str() {
                Some("done") => {
                    let code = event.get("code").num().map(|c| c as i32);
                    Entry::Setup {
                        ok: Some(code == Some(0)),
                        code,
                    }
                }
                _ => Entry::Setup {
                    ok: None,
                    code: None,
                },
            }),
            Some("spor_message") => out.push(Entry::Steer(
                event.get("text").str().unwrap_or_default().to_string(),
            )),
            Some("result") => out.push(Entry::Done {
                ok: event.get("is_error").bool() != Some(true)
                    && event.get("subtype").str().is_none_or(|s| s == "success"),
                // The result repeats the agent's last words; say them once.
                summary: {
                    let result = event.get("result").str().unwrap_or("").trim();
                    let said = out
                        .iter()
                        .rev()
                        .find_map(|e| match e {
                            Entry::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .is_some_and(|last| last == result);
                    if said {
                        String::new()
                    } else {
                        result.to_string()
                    }
                },
                cost_usd: event.get("total_cost_usd").num(),
                turns: event.get("num_turns").num().map(|n| n as u32),
                seconds: event.get("duration_ms").num().map(|ms| ms / 1000.0),
            }),
            _ => {}
        }
        // What a subagent did hangs under the call that started it (#99).
        if let Some(parent) = event.get("parent_tool_use_id").str() {
            for entry in out.split_off(start) {
                out.push(Entry::Nested {
                    parent: parent.to_string(),
                    entry: Box::new(entry),
                });
            }
        }
    }
    out
}

fn init_of(event: &Json) -> Init {
    Init {
        model: event.get("model").str().unwrap_or_default().to_string(),
        servers: event
            .get("mcp_servers")
            .array()
            .iter()
            .map(|m| {
                (
                    m.get("name").str().unwrap_or_default().to_string(),
                    m.get("status").str().unwrap_or_default().to_string(),
                )
            })
            .collect(),
    }
}

/// What a Claude Code log reports besides the transcript.
#[derive(Debug, Default, PartialEq)]
struct LogStats {
    session: Option<String>,
    cost: Option<f64>,
    usage: Usage,
    init: Option<Init>,
}

/// The session, cost, token use and `init` of a Claude Code log.
fn claude_log(log: &str) -> LogStats {
    let mut stats = LogStats::default();
    // A message arrives once per content block, each with its usage: count
    // it once, as of its last block.
    let mut pending: Option<(String, [u64; 4])> = None;
    let commit = |usage: &mut Usage, p: &mut Option<(String, [u64; 4])>, main: bool| {
        if let Some((_, [i, o, r, c])) = p.take() {
            usage.input += i;
            usage.output += o;
            usage.cache_read += r;
            usage.cache_created += c;
            if main {
                usage.context = i + r + c;
            }
        }
    };
    let mut pending_main = true;
    for line in log.lines() {
        // Only the events that carry something; skip parsing the rest.
        if !(line.contains("\"type\":\"assistant\"")
            || line.contains("\"type\":\"system\"")
            || line.contains("\"type\":\"result\""))
        {
            continue;
        }
        let Some(event) = Json::parse(line) else {
            continue;
        };
        match event.get("type").str() {
            Some("assistant") => {
                let message = event.get("message");
                let u = message.get("usage");
                let n = |k: &str| u.get(k).num().unwrap_or(0.0) as u64;
                let counts = [
                    n("input_tokens"),
                    n("output_tokens"),
                    n("cache_read_input_tokens"),
                    n("cache_creation_input_tokens"),
                ];
                let id = message.get("id").str().unwrap_or_default().to_string();
                if pending
                    .as_ref()
                    .is_some_and(|(p, _)| *p != id || id.is_empty())
                {
                    commit(&mut stats.usage, &mut pending, pending_main);
                }
                // A subagent's turns spend tokens but aren't the context.
                pending_main = matches!(event.get("parent_tool_use_id"), Json::Null);
                pending = Some((id, counts));
            }
            Some("system") if event.get("subtype").str() == Some("init") => {
                if let Some(id) = event.get("session_id").str() {
                    stats.session = Some(id.to_string());
                }
                stats.init = Some(init_of(&event));
            }
            Some("result") => {
                if let Some(id) = event.get("session_id").str() {
                    stats.session = Some(id.to_string());
                }
                if let Some(c) = event.get("total_cost_usd").num() {
                    stats.cost = Some(c);
                }
                if let Json::Object(models) = event.get("modelUsage") {
                    let window = models
                        .values()
                        .filter_map(|m| m.get("contextWindow").num())
                        .fold(0.0, f64::max);
                    if window > 0.0 {
                        stats.usage.window = Some(window as u64);
                    }
                }
            }
            _ => {}
        }
    }
    commit(&mut stats.usage, &mut pending, pending_main);
    stats
}

/// The gist of a tool call: its command, file, pattern or URL.
fn tool_detail(input: &Json) -> String {
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
    ] {
        if let Some(v) = input.get(key).str() {
            let line = v.lines().next().unwrap_or("");
            return if line.chars().count() > 120 {
                line.chars().take(119).chain(['…']).collect()
            } else {
                line.to_string()
            };
        }
    }
    String::new()
}

/// Is the process still there?
fn alive(pid: Option<&str>) -> bool {
    let Some(pid) = pid.filter(|p| !p.is_empty()) else {
        return false;
    };
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// What Claude Code's `permission_suggestions` would allow, in words:
/// "`npm test` commands", "file edits", "all commands". None when there
/// are none, or none that allow anything.
pub fn covers(suggestions: &Json) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for s in suggestions.array() {
        match s.get("type").str() {
            Some("addRules" | "replaceRules") if s.get("behavior").str() == Some("allow") => {
                for rule in s.get("rules").array() {
                    let tool = rule.get("toolName").str().unwrap_or("the tool");
                    parts.push(rule_words(tool, rule.get("ruleContent").str()));
                }
            }
            Some("setMode") => parts.push(match s.get("mode").str() {
                Some("acceptEdits") => "file edits".into(),
                Some("bypassPermissions") => "everything".into(),
                Some(mode) => format!("{mode} mode"),
                None => continue,
            }),
            Some("addDirectories") => {
                let dirs: Vec<&str> = s
                    .get("directories")
                    .array()
                    .iter()
                    .filter_map(Json::str)
                    .collect();
                if !dirs.is_empty() {
                    parts.push(format!("files in {}", dirs.join(", ")));
                }
            }
            _ => {}
        }
    }
    parts.dedup();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// One permission rule, `Bash(npm test:*)`, as words.
fn rule_words(tool: &str, content: Option<&str>) -> String {
    let content = content.map(str::trim).filter(|c| !c.is_empty());
    match (tool, content) {
        ("Bash", None) => "all commands".into(),
        ("Bash", Some(c)) => match c.strip_suffix(":*").or_else(|| c.strip_suffix(" *")) {
            Some(prefix) => format!("`{prefix}` commands"),
            None => format!("`{c}`"),
        },
        ("Edit" | "Write" | "MultiEdit" | "NotebookEdit", None) => "file edits".into(),
        ("Edit" | "Write" | "MultiEdit" | "NotebookEdit", Some(c)) => format!("edits to {c}"),
        ("Read", Some(c)) => format!("reading {c}"),
        ("WebFetch", Some(c)) => match c.strip_prefix("domain:") {
            Some(domain) => format!("fetching from {domain}"),
            None => format!("fetching {c}"),
        },
        (tool, None) => format!("all {tool} uses"),
        (tool, Some(c)) => format!("{tool}({c})"),
    }
}

/// Ask Claude Code one read-only question in a fork of `session` (when
/// there is one), from the task's folder, and return its reply. `effort`
/// is the `--effort` flag, if any. `what` completes "Claude took too long
/// to …" and "Claude couldn't …".
pub(crate) fn ask_readonly(
    task: &Task,
    session: Option<String>,
    effort: Option<&str>,
    prompt: &str,
    what: &str,
) -> Result<String, String> {
    let mut args: Vec<String> = [
        "--output-format",
        "json",
        "--permission-mode",
        "default",
        "--allowedTools",
        "Read,Grep,Glob,Bash(git log:*),Bash(git diff:*),Bash(git show:*)",
        "--disallowedTools",
        "Edit,Write,NotebookEdit,Bash(git push:*)",
    ]
    .map(String::from)
    .to_vec();
    if let Some(session) = session {
        args.extend(["--resume".into(), session, "--fork-session".into()]);
    }
    if let Some(effort) = effort {
        args.extend(["--effort".into(), effort.into()]);
    }
    args.extend(["-p".into(), prompt.into()]);
    let mut child = Command::new("claude")
        .args(&args)
        .current_dir(&task.worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't start claude: {e}"))?;
    // Read stdout on a thread, so a long reply can't fill the pipe while
    // we wait; give up after a few minutes rather than hang publishing.
    let mut out = child.stdout.take().ok_or("claude has no output")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        s
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Claude took too long to {what}"));
            }
            None => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    let stdout = reader.join().unwrap_or_default();
    let event = Json::parse(stdout.trim()).ok_or("Couldn't read Claude's reply")?;
    if event.get("is_error").bool() == Some(true) {
        return Err(format!("Claude couldn't {what}"));
    }
    Ok(event.get("result").str().unwrap_or("").to_string())
}

/// A pull request title and description for the task's work, written by
/// Claude Code. It continues (a fork of) the task's session when there is
/// one, so it knows why the work was done, not just what changed, and it
/// can only read: look at the branch, not change it.
pub fn describe_pull(tasks: &Tasks, task: &Task) -> Result<(String, String), String> {
    let prompt = format!(
        "Write the pull request for this branch's work: `{branch}` into `{base}`. \
         Look at what it changes (`git log {start}..HEAD`, `git diff {start}...HEAD`) \
         and, if this conversation has it, why.\n\n\
         Reply with only the pull request: its title on the first line (under 72 \
         characters, no prefix, no quotes), a blank line, then the description in \
         GitHub markdown: what changed and why, and anything a reviewer should look \
         at closely. Only say how it was tested if it was. Don't change any files.",
        branch = task.branch,
        base = task.base,
        start = task.start(),
    );
    let result = ask_readonly(
        task,
        Runs::of(tasks, task).session(),
        None,
        &prompt,
        "describe the pull request",
    )?;
    parse_pull_text(&result).ok_or_else(|| "Claude's description had no title".into())
}

/// A reply of a title line, a blank line and a description, as (title,
/// description). Tolerates a fence around it all, a `#` or `Title:` on
/// the title, and quotes around it.
fn parse_pull_text(reply: &str) -> Option<(String, String)> {
    let mut text = reply.trim();
    if let Some(inner) = text.strip_prefix("```") {
        // Drop the fence's language tag, if any, and the closing fence.
        let inner = inner.split_once('\n').map_or("", |(_, rest)| rest);
        text = inner.trim_end().strip_suffix("```").unwrap_or(inner).trim();
    }
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let mut title = first.trim().trim_start_matches('#').trim();
    for prefix in ["Title:", "title:", "**Title:**"] {
        title = title.strip_prefix(prefix).unwrap_or(title).trim();
    }
    let title = title
        .trim_matches(|c| c == '"' || c == '`' || c == '*')
        .trim();
    if title.is_empty() {
        return None;
    }
    let title = if title.chars().count() > 72 {
        title.chars().take(71).chain(['…']).collect()
    } else {
        title.to_string()
    };
    Some((title, rest.trim().to_string()))
}

/// Is the agent there to run? Why not, if not.
pub fn missing(kind: &AgentKind) -> Option<String> {
    match kind {
        AgentKind::Claude if !available("claude") => {
            Some("Claude Code isn't installed, or isn't on the PATH".into())
        }
        AgentKind::Command(c) if c.trim().is_empty() => Some("The agent command is empty".into()),
        _ => None,
    }
}

/// What starting an agent on a task would run (`Runs::plan`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub kind: AgentKind,
    pub model: Model,
    pub prompt: String,
    /// The session it continues, if any.
    pub resume: Option<String>,
}

impl Runs {
    /// What to run for a task: its own prompt (`message` None) with the
    /// `chosen` agent and model, or a follow-up. A follow-up goes to the
    /// agent that did the work, on the model it used, whatever is chosen
    /// now, and continues its session; with no session to continue (a
    /// custom command, or Claude without one), the whole task goes again
    /// with the feedback after it.
    pub fn plan(
        &self,
        task: &Task,
        message: Option<&str>,
        chosen: Option<(&AgentKind, &Model)>,
    ) -> Result<Plan, String> {
        let previous = message
            .and_then(|_| self.latest())
            .map(|r| (r.kind, r.model));
        let (kind, model) = previous
            .or_else(|| chosen.map(|(k, m)| (k.clone(), m.clone())))
            .ok_or("Pick an agent first")?;
        if let Some(e) = missing(&kind) {
            return Err(e);
        }
        let resume = message.and_then(|_| self.session());
        let prompt = match message {
            Some(m) if resume.is_none() => format!(
                "{}\n\nFeedback on the work so far:\n{}",
                task.prompt.trim(),
                m.trim()
            ),
            Some(m) => m.trim().to_string(),
            None => task.prompt.clone(),
        };
        Ok(Plan {
            kind,
            model,
            prompt,
            resume,
        })
    }

    /// Start an agent on the task, as `plan` says.
    pub fn follow_up(
        &self,
        task: &Task,
        message: Option<&str>,
        chosen: Option<(&AgentKind, &Model)>,
        permissions: Permissions,
    ) -> Result<Run, String> {
        let plan = self.plan(task, message, chosen)?;
        self.start_with(
            task,
            &plan.kind,
            &plan.prompt,
            plan.resume.as_deref(),
            permissions,
            &plan.model,
        )
    }
}

/// Is `program` on the PATH?
pub fn available(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| is_executable(&dir.join(program)))
    })
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commands_output_is_kept_with_its_call() {
        let log = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"Bash","input":{"command":"cargo test"}},{"type":"tool_use","id":"b","name":"Read","input":{"file_path":"/w/x"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b","content":"file text"},{"type":"tool_result","tool_use_id":"a","content":[{"type":"text","text":"test result: ok. 3 passed"}]}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"c","name":"Bash","input":{"command":"cd app && pnpm build"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"c","is_error":true,"content":"Exit code 2\nerror TS1"}]}}"#;
        let entries = parse_claude(log);
        let results: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                Entry::Result {
                    detail, ok, output, ..
                } => Some((detail.as_str(), *ok, output.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("cargo test", true, "test result: ok. 3 passed"),
                ("cd app && pnpm build", false, "Exit code 2\nerror TS1"),
            ],
            "only commands, matched by id"
        );
        assert!(entries.contains(&Entry::ToolError("Exit code 2".into())));
        assert_eq!(
            checks_of(&entries),
            Checks {
                tests: Some(true),
                build: Some(false)
            }
        );
    }

    #[test]
    fn a_subagents_events_hang_under_its_call() {
        let log = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Split it up."},{"type":"tool_use","id":"t1","name":"Task","input":{"description":"Find the routes","subagent_type":"Explore","prompt":"List every route."}}]}}
{"type":"assistant","parent_tool_use_id":"t1","message":{"content":[{"type":"tool_use","id":"g1","name":"Grep","input":{"pattern":"route"}},{"type":"text","text":"Found three."}]}}
{"type":"user","parent_tool_use_id":"t1","message":{"content":[{"type":"tool_result","tool_use_id":"g1","content":"a.rs:1"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"Three routes: a, b, c."}]}}"#;
        let entries = parse_claude(log);
        assert_eq!(entries[0], Entry::Thinking("Split it up.".into()));
        assert!(matches!(&entries[1], Entry::Tool { name, .. } if name == "Task"));
        assert_eq!(
            entries[2],
            Entry::Subagent {
                id: "t1".into(),
                agent: "Explore".into(),
                description: "Find the routes".into(),
                prompt: "List every route.".into()
            }
        );
        let nested: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                Entry::Nested { parent, entry } => Some((parent.as_str(), &**entry)),
                _ => None,
            })
            .collect();
        assert_eq!(nested.len(), 2);
        assert!(nested.iter().all(|(p, _)| *p == "t1"));
        assert!(matches!(nested[0].1, Entry::Tool { name, .. } if name == "Grep"));
        assert_eq!(nested[1].1, &Entry::Text("Found three.".into()));
        assert!(
            matches!(entries.last(), Some(Entry::Result { name, output, .. })
                if name == "Task" && output == "Three routes: a, b, c."),
            "the subagent's report is the call's result, at the top: {entries:?}"
        );
    }

    #[test]
    fn long_output_keeps_its_end() {
        let long = "x".repeat(MAX_RESULT_CHARS) + "THE END";
        let log = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"a","name":"Bash","input":{{"command":"make"}}}}]}}}}
{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"a","content":"{long}"}}]}}}}"#
        );
        let entries = parse_claude(&log);
        let Some(Entry::Result {
            output, truncated, ..
        }) = entries.last()
        else {
            panic!("{entries:?}")
        };
        assert!(*truncated && output.ends_with("THE END"));
        assert_eq!(output.chars().count(), MAX_RESULT_CHARS);
    }

    #[test]
    fn test_and_build_commands_are_recognised() {
        assert_eq!(check_kind("cargo test --all"), Some(CheckKind::Tests));
        assert_eq!(check_kind("cd a && npm test"), Some(CheckKind::Tests));
        assert_eq!(check_kind("tsc --noEmit"), Some(CheckKind::Build));
        assert_eq!(check_kind("cargo testify"), None);
        assert_eq!(check_kind("echo cargo test"), None);
        assert_eq!(check_kind("ls"), None);
        assert_eq!(checks_of(&[]), Checks::default());
    }

    #[test]
    fn a_plan_is_read_from_todo_write() {
        let log = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"Read the routes","activeForm":"Reading the routes","status":"completed"},{"content":"Migrate the tests","activeForm":"Migrating the tests","status":"in_progress"},{"content":"Run them","status":"pending"}]}}]}}"#;
        let entries = parse_claude(log);
        assert!(matches!(&entries[0], Entry::Tool { name, .. } if name == "TodoWrite"));
        let Entry::Todos(list) = &entries[1] else {
            panic!("{entries:?}")
        };
        assert_eq!(list.len(), 3);
        assert_eq!(list[2].active, "Run them", "no activeForm: the step itself");
        assert_eq!(
            plan_progress(list),
            (1, 3, Some("Migrating the tests".to_string()))
        );
    }

    #[test]
    fn pull_text_is_a_title_line_then_the_description() {
        let (title, body) =
            parse_pull_text("Fix login on Safari\n\nThe cookie was `SameSite=None`.\n").unwrap();
        assert_eq!(title, "Fix login on Safari");
        assert_eq!(body, "The cookie was `SameSite=None`.");

        let (title, body) =
            parse_pull_text("```markdown\n# Title: \"Fix it\"\n\nBecause.\n```").unwrap();
        assert_eq!(title, "Fix it");
        assert_eq!(body, "Because.");

        let (title, body) = parse_pull_text("Just a title").unwrap();
        assert_eq!(title, "Just a title");
        assert_eq!(body, "");

        assert_eq!(parse_pull_text("  \n"), None);
        assert_eq!(
            parse_pull_text(&"x".repeat(100)).unwrap().0.chars().count(),
            72
        );
    }

    #[test]
    fn suggested_rules_in_words() {
        let say = |json: &str| covers(&Json::parse(json).unwrap());
        assert_eq!(say("[]"), None);
        assert_eq!(
            say(
                r#"[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"npm test:*"}],"behavior":"allow","destination":"session"}]"#
            ),
            Some("`npm test` commands".into())
        );
        assert_eq!(
            say(
                r#"[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"make"},{"toolName":"Bash"}],"behavior":"allow","destination":"session"}]"#
            ),
            Some("`make`, all commands".into())
        );
        assert_eq!(
            say(
                r#"[{"type":"setMode","mode":"acceptEdits","destination":"session"},{"type":"addDirectories","directories":["/tmp/x"],"destination":"session"}]"#
            ),
            Some("file edits, files in /tmp/x".into())
        );
        assert_eq!(
            say(
                r#"[{"type":"addRules","rules":[{"toolName":"WebFetch","ruleContent":"domain:docs.rs"}],"behavior":"allow","destination":"session"}]"#
            ),
            Some("fetching from docs.rs".into())
        );
        // Rules that deny or ask don't allow anything.
        assert_eq!(
            say(
                r#"[{"type":"addRules","rules":[{"toolName":"Bash"}],"behavior":"deny","destination":"session"}]"#
            ),
            None
        );
    }

    #[test]
    fn claude_events_become_a_transcript() {
        let log = r#"{"type":"system","subtype":"init","session_id":"s-1"}
{"type":"stream_event","event":{}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Looking at it."},{"type":"tool_use","name":"Bash","input":{"command":"cargo test\nmore"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","is_error":true,"content":"Permission to use Bash has been denied.\nDetails"}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/w/src/lib.rs"}}]}}
not json at all
{"type":"assistant","message":{"content":[{"type":"text","text":"Fixed it."}]}}
{"type":"result","subtype":"success","is_error":false,"result":"Fixed it.","total_cost_usd":0.25,"num_turns":4,"duration_ms":12500,"session_id":"s-1"}"#;
        let entries = parse_claude(log);
        assert_eq!(
            entries,
            vec![
                Entry::Text("Looking at it.".into()),
                Entry::Tool {
                    name: "Bash".into(),
                    detail: "cargo test".into()
                },
                Entry::ToolError("Permission to use Bash has been denied.".into()),
                Entry::Tool {
                    name: "Edit".into(),
                    detail: "/w/src/lib.rs".into()
                },
                Entry::Output("not json at all".into()),
                Entry::Text("Fixed it.".into()),
                Entry::Done {
                    ok: true,
                    summary: String::new(),
                    cost_usd: Some(0.25),
                    turns: Some(4),
                    seconds: Some(12.5),
                },
            ]
        );
        let stats = claude_log(log);
        assert_eq!(
            (stats.session, stats.cost),
            (Some("s-1".into()), Some(0.25))
        );
    }

    #[test]
    fn claude_log_counts_tokens_once_per_message() {
        let log = r#"{"type":"system","subtype":"init","session_id":"s-1","model":"claude-opus-5-5","mcp_servers":[{"name":"gh","status":"connected"},{"name":"db","status":"failed"}]}
{"type":"assistant","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":1000,"cache_creation_input_tokens":200},"content":[{"type":"text","text":"a"}]}}
{"type":"assistant","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":7,"cache_read_input_tokens":1000,"cache_creation_input_tokens":200},"content":[{"type":"tool_use","name":"Bash","input":{}}]}}
{"type":"assistant","parent_tool_use_id":"t1","message":{"id":"s1","usage":{"input_tokens":3,"output_tokens":1,"cache_read_input_tokens":50,"cache_creation_input_tokens":0},"content":[]}}
{"type":"assistant","message":{"id":"m2","usage":{"input_tokens":20,"output_tokens":9,"cache_read_input_tokens":1100,"cache_creation_input_tokens":300},"content":[]}}
{"type":"result","subtype":"success","is_error":false,"result":"","total_cost_usd":0.5,"modelUsage":{"claude-opus-5-5":{"contextWindow":1000000}}}"#;
        let stats = claude_log(log);
        assert_eq!(
            stats.usage,
            Usage {
                input: 33,
                output: 17,
                cache_read: 2150,
                cache_created: 500,
                context: 1420,
                window: Some(1_000_000),
            }
        );
        assert_eq!(stats.usage.fill(None), Some(0));
        let init = stats.init.unwrap();
        assert_eq!(init.model, "claude-opus-5-5");
        assert_eq!(init.failed(), vec!["db"]);
    }

    #[test]
    fn context_window_is_guessed_without_a_report() {
        let mut u = Usage {
            context: 162_000,
            ..Usage::default()
        };
        assert_eq!(u.fill(Some("opus")), Some(81));
        assert_eq!(u.fill(Some("opus[1m]")), Some(16));
        u.context = 300_000;
        assert_eq!(u.fill(None), Some(30));
        assert_eq!(Usage::default().fill(None), None);
    }

    #[test]
    fn compaction_and_init_enter_the_transcript() {
        let log = r#"{"type":"system","subtype":"init","model":"m","mcp_servers":[]}
{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto","pre_tokens":150000}}"#;
        assert_eq!(
            parse_claude(log),
            vec![
                Entry::Init(Init {
                    model: "m".into(),
                    servers: vec![]
                }),
                Entry::Compacted
            ]
        );
    }

    #[test]
    fn claude_command_line() {
        let (program, args) = command_for(
            &AgentKind::Claude,
            "Fix it",
            Some("s-1"),
            Permissions::commands(false),
            &Model::new(Some("opus"), Some(Effort::High)).unwrap(),
            Path::new("/r/.git/spor/attachments"),
        );
        assert_eq!(program, "claude");
        assert!(args.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(args.windows(2).any(|w| w == ["--effort", "high"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--permission-mode", "acceptEdits"]));
        assert!(args.windows(2).any(|w| w == ["--resume", "s-1"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--add-dir", "/r/.git/spor/attachments"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--disallowedTools", "Bash(git push:*)"]));
        assert_eq!(&args[args.len() - 2..], ["-p", "Fix it"]);
    }

    #[test]
    fn claude_defaults_leave_the_model_to_claude() {
        let (_, args) = command_for(
            &AgentKind::Claude,
            "Fix it",
            None,
            Permissions::commands(false),
            &Model::new(Some("  "), None).unwrap(),
            Path::new("/r/.git/spor/attachments"),
        );
        assert!(!args.iter().any(|a| a == "--model" || a == "--effort"));
    }

    #[test]
    fn model_names_stay_names() {
        assert!(Model::new(Some("claude-opus-5-5"), None).is_ok());
        assert!(Model::new(Some("opus[1m]"), None).is_ok());
        assert!(Model::new(Some("--dangerously-skip-permissions"), None).is_err());
        assert!(Model::new(Some("opus; rm"), None).is_err());
        assert_eq!(
            Model::new(Some("opus"), Some(Effort::XHigh))
                .unwrap()
                .describe(),
            Some("Opus · xhigh effort".into())
        );
    }

    #[test]
    fn custom_command_quotes_the_prompt() {
        let (program, args) = command_for(
            &AgentKind::Command("codex exec {prompt}".into()),
            "Don't",
            None,
            Permissions::commands(true),
            &Model::default(),
            Path::new("/r/.git/spor/attachments"),
        );
        assert_eq!(program, "sh");
        assert_eq!(args, ["-c", r"codex exec 'Don'\''t'"]);
    }
}
