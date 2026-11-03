//! Running agents in task folders, and reading back what they did.
//!
//! A task can have several runs: the first with its prompt, later ones with
//! feedback. Each run's files sit in the task's run directory
//! (`.git/spor/tasks/<id>.d/`): `run-N.meta` (what was asked, how, when),
//! `run-N.log` (everything it printed) and `run-N.exit` (its exit code, once
//! it ends). The agent runs under a small `sh` wrapper that writes the exit
//! file, so a run's state survives spor quitting and starting again, and the
//! agent keeps working if spor quits.

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

/// How much the agent may do without asking (headless agents can't ask).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// Run shell commands. Without it Claude Code may edit files but its
    /// commands are refused, which it reports and works around.
    pub commands: bool,
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
}

/// The runs of one task.
#[derive(Debug, Clone)]
pub struct Runs {
    dir: PathBuf,
}

impl Runs {
    pub fn of(tasks: &Tasks, task: &Task) -> Self {
        Self {
            dir: tasks.run_dir(&task.id),
        }
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
        // "$@" is the agent; its exit code lands in the exit file however
        // it ends, short of the machine going down.
        let script = r#""$@" > "$SPOR_LOG" 2>&1; echo $? > "$SPOR_EXIT""#;
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(script)
            .arg("spor-agent")
            .arg(&program)
            .args(&args)
            .current_dir(&task.worktree)
            .env("SPOR_LOG", &log)
            .env("SPOR_EXIT", &exit)
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
        })
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

    /// The session to continue with feedback: the latest one any run
    /// reported.
    pub fn session(&self) -> Option<String> {
        self.list().into_iter().rev().find_map(|r| r.session)
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
                    out.extend(lines[skip..].iter().map(|l| Entry::Output(l.to_string())));
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

    fn run(&self, index: usize) -> Option<Run> {
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
                if permissions.commands {
                    "bypassPermissions"
                } else {
                    "acceptEdits"
                },
            ]
            .map(String::from)
            .to_vec();
            if let Some(session) = resume {
                args.extend(["--resume".into(), session.into()]);
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
            Permissions { commands: false },
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
            Permissions { commands: true },
        );
        assert_eq!(program, "sh");
        assert_eq!(args, ["-c", r"codex exec 'Don'\''t'"]);
    }
}
