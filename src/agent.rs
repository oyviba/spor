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
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Supervised, Mode::AcceptEdits, Mode::Auto, Mode::Full];

    /// Claude Code's `--permission-mode`.
    pub fn flag(self) -> &'static str {
        match self {
            Mode::Supervised => "manual",
            Mode::AcceptEdits => "acceptEdits",
            Mode::Auto => "auto",
            Mode::Full => "bypassPermissions",
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
    pub cost_usd: Option<f64>,
    /// Undone: its changes were taken back, and follow-ups resume from
    /// before it.
    pub undone: bool,
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
    /// The folder's setup script (#23): started (`ok` None), or done. Its
    /// output is the `Output` lines between.
    Setup { ok: Option<bool>, code: Option<i32> },
}

/// A question the agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub id: String,
    pub tool: String,
    pub detail: String,
    /// What "Allow for this session" would cover, if it's offered.
    pub session: Option<String>,
}

/// The runs of one task.
#[derive(Debug, Clone)]
pub struct Runs {
    dir: PathBuf,
    /// Where the task's checkpoints live: `refs/spor/tasks/<id>`.
    refs: String,
    worktree: PathBuf,
    base: String,
    /// The main checkout, for setup scripts that copy from it.
    repo_root: PathBuf,
}

impl Runs {
    pub fn of(tasks: &Tasks, task: &Task) -> Self {
        Self {
            dir: tasks.run_dir(&task.id),
            refs: checkpoint_prefix(&task.id),
            worktree: task.worktree.clone(),
            base: task.base.clone(),
            repo_root: tasks.root().to_path_buf(),
        }
    }

    /// The setup script a new task's folder still needs (#23): the
    /// repository's own `.git/spor/setup` if there is one, else the
    /// committed `.spor/setup`; none once setup worked (or was skipped).
    pub fn setup_script(&self, task: &Task) -> Option<PathBuf> {
        if self.dir.join("setup.ok").exists() {
            return None;
        }
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
        if self.latest().is_some_and(|r| r.state == RunState::Running) {
            return Err(format!("An agent is already working on {}", task.id));
        }
        if !task.worktree.exists() {
            return Err(format!("{}'s folder is gone", task.id));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let index = self.count();
        let (program, args) = command_for(kind, prompt, resume, permissions);
        let log = self.file(index, "log");
        let exit = self.file(index, "exit");
        // What the folder was like before, for this run's diff and undo.
        // Best effort: a run without checkpoints still runs.
        let _ = checkpoint::capture(&task.worktree, &self.checkpoint_ref(index, "before"));
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
            .collect(),
            None => ["sh", "-c", script, "spor-agent", &program]
                .into_iter()
                .map(Into::into)
                .chain(args.iter().map(Into::into))
                .collect(),
        };
        // A new folder's setup script runs first, in the same detached
        // process, and the agent only if it worked (#23).
        let setup = self.setup_script(task);
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
        cmd.current_dir(&task.worktree)
            .env("SPOR_LOG", &log)
            .env("SPOR_EXIT", &exit)
            .env("SPOR_AFTER", self.checkpoint_ref(index, "after"))
            .env("SPOR_SNAPSHOT", checkpoint::SNAPSHOT_SH)
            .env("SPOR_PROMPT", prompt)
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
            ("prompt", prompt.to_string()),
            ("started", started.to_string()),
            ("pid", pid.to_string()),
        ];
        let text: String = meta
            .iter()
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
            cost_usd: None,
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
                } if !answered.contains(id.as_str()) => Some(Ask {
                    id: id.clone(),
                    tool: tool.clone(),
                    detail: detail.clone(),
                    session: session.clone(),
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
        let wt = &self.worktree;
        let before = checkpoint::resolve(wt, &self.checkpoint_ref(run.index, "before"))
            .ok_or("This run has no checkpoint")?;
        checkpoint::capture(wt, &self.checkpoint_ref(run.index, "undone"))?;
        checkpoint::restore(wt, &before)?;
        std::fs::write(self.file(run.index, "undone"), "").map_err(|e| e.to_string())?;
        Ok(run.index)
    }

    /// Take back [`Runs::undo_last`]: the folder as it was, and the run
    /// counts again.
    pub fn redo(&self, index: usize) -> Result<(), String> {
        let wt = &self.worktree;
        let undone = checkpoint::resolve(wt, &self.checkpoint_ref(index, "undone"))
            .ok_or("Nothing to redo")?;
        if self.latest().is_some_and(|r| r.state == RunState::Running) {
            return Err("An agent is working; stop it first".into());
        }
        checkpoint::restore(wt, &undone)?;
        std::fs::remove_file(self.file(index, "undone")).map_err(|e| e.to_string())?;
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
        let (session, cost_usd) = match kind {
            AgentKind::Claude => {
                let log = std::fs::read_to_string(self.file(index, "log")).unwrap_or_default();
                claude_session(&log)
            }
            AgentKind::Command(_) => (None, None),
        };
        Some(Run {
            index,
            kind,
            prompt: self.meta(index, "prompt").unwrap_or_default(),
            state,
            started,
            ended,
            session,
            cost_usd,
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

    fn file(&self, index: usize, ext: &str) -> PathBuf {
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
            if let Some(session) = resume {
                // A new session id for each follow-up, so the one resumed
                // stays as it was: undoing this run resumes from there.
                args.extend(["--resume".into(), session.into(), "--fork-session".into()]);
            }
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
    for line in log.lines().filter(|l| !l.trim().is_empty()) {
        let Some(event) = Json::parse(line) else {
            out.push(Entry::Output(line.to_string()));
            continue;
        };
        match event.get("type").str() {
            Some("assistant") => {
                for part in event.get("message").get("content").array() {
                    match part.get("type").str() {
                        Some("text") => {
                            let text = part.get("text").str().unwrap_or("").trim();
                            if !text.is_empty() {
                                out.push(Entry::Text(text.to_string()));
                            }
                        }
                        Some("tool_use") => out.push(Entry::Tool {
                            name: part.get("name").str().unwrap_or("Tool").to_string(),
                            detail: tool_detail(part.get("input")),
                        }),
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for part in event.get("message").get("content").array() {
                    if part.get("type").str() == Some("tool_result")
                        && part.get("is_error").bool() == Some(true)
                    {
                        let content = match part.get("content") {
                            Json::String(s) => s.clone(),
                            other => other
                                .array()
                                .iter()
                                .filter_map(|c| c.get("text").str())
                                .collect::<Vec<_>>()
                                .join(" "),
                        };
                        let first = content.trim().lines().next().unwrap_or("").to_string();
                        out.push(Entry::ToolError(first));
                    }
                }
            }
            Some("control_request")
                if event.get("request").get("subtype").str() == Some("can_use_tool") =>
            {
                let request = event.get("request");
                out.push(Entry::Ask {
                    id: event
                        .get("request_id")
                        .str()
                        .unwrap_or_default()
                        .to_string(),
                    tool: request.get("tool_name").str().unwrap_or("Tool").to_string(),
                    detail: tool_detail(request.get("input")),
                    session: covers(request.get("permission_suggestions")),
                });
            }
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
    }
    out
}

/// The session id and cost a Claude Code log reports.
fn claude_session(log: &str) -> (Option<String>, Option<f64>) {
    let mut session = None;
    let mut cost = None;
    for line in log.lines() {
        // Only the events that carry them; skip parsing the rest.
        if !(line.contains("\"session_id\"")
            && (line.contains("\"init\"") || line.contains("\"result\"")))
        {
            continue;
        }
        if let Some(event) = Json::parse(line) {
            if let Some(id) = event.get("session_id").str() {
                session = Some(id.to_string());
            }
            if let Some(c) = event.get("total_cost_usd").num() {
                cost = Some(c);
            }
        }
    }
    (session, cost)
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
        assert_eq!(claude_session(log), (Some("s-1".into()), Some(0.25)));
    }

    #[test]
    fn claude_command_line() {
        let (program, args) = command_for(
            &AgentKind::Claude,
            "Fix it",
            Some("s-1"),
            Permissions::commands(false),
        );
        assert_eq!(program, "claude");
        assert!(args
            .windows(2)
            .any(|w| w == ["--permission-mode", "acceptEdits"]));
        assert!(args.windows(2).any(|w| w == ["--resume", "s-1"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--disallowedTools", "Bash(git push:*)"]));
        assert_eq!(&args[args.len() - 2..], ["-p", "Fix it"]);
    }

    #[test]
    fn custom_command_quotes_the_prompt() {
        let (program, args) = command_for(
            &AgentKind::Command("codex exec {prompt}".into()),
            "Don't",
            None,
            Permissions::commands(true),
        );
        assert_eq!(program, "sh");
        assert_eq!(args, ["-c", r"codex exec 'Don'\''t'"]);
    }
}
