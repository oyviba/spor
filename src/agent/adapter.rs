//! What spor needs from each agent it runs, in one place (`Adapter`): how
//! to start it, what to give it before it starts (what it's told, spor's
//! MCP server), how to read its log back, and what it can do. `Runs`
//! drives every agent through this, so a new agent is a new adapter and a
//! new `AgentKind`, not a branch in every function that runs one.

use super::{Entry, Init, Model, Permissions, Progress, Usage};
use crate::host;
use std::path::{Path, PathBuf};

/// One run's ask of an agent: the command line, before anything spor adds.
pub struct Launch<'a> {
    pub prompt: &'a str,
    /// The agent's own session to continue, for a follow-up.
    pub resume: Option<&'a str>,
    pub permissions: Permissions,
    pub model: &'a Model,
    /// Where images attached to prompts are kept: the agent may read there.
    pub attachments: &'a Path,
}

/// What a run gets beside its command line.
pub struct Prepare<'a> {
    /// What the agent is told about where it is (`Runs::context`).
    pub context: &'a str,
    /// spor's MCP server for this run, when spor can serve one.
    pub mcp: Option<&'a McpServer>,
    /// The run's directory, for any file the agent's setup needs.
    pub dir: &'a Path,
    /// What the run's files are called there: `run-N`.
    pub stem: &'a str,
}

impl Prepare<'_> {
    /// A file beside the run's log: `run-N.<ext>`.
    pub fn file(&self, ext: &str) -> PathBuf {
        self.dir.join(format!("{}.{ext}", self.stem))
    }
}

/// What `Adapter::prepare` adds: arguments that go before the prompt, and
/// environment variables.
#[derive(Debug, Default, PartialEq)]
pub struct Prepared {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// How to start spor's MCP server (#106): the app's own executable, as a
/// stdio server for one task.
pub struct McpServer {
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl McpServer {
    /// `"command":…,"args":[…]`, for the JSON configs agents read.
    pub fn json_fields(&self) -> String {
        format!(
            r#""command":{},"args":[{}]"#,
            host::quote(&self.command.to_string_lossy()),
            self.args
                .iter()
                .map(|a| host::quote(a))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    /// The tools that need no permission, by their names on the server
    /// (`other_tasks`, not `mcp__spor__other_tasks`). `ask` isn't among
    /// them: it's a question for the person.
    pub fn read_only_tools() -> Vec<&'static str> {
        let prefix = format!("mcp__{}__", crate::mcp::SERVER);
        crate::mcp::READ_ONLY_TOOLS
            .iter()
            .map(|t| t.strip_prefix(prefix.as_str()).unwrap_or(t))
            .collect()
    }
}

/// What an agent's log reports besides the transcript.
#[derive(Debug, Default, PartialEq)]
pub struct LogStats {
    /// The session a follow-up continues.
    pub session: Option<String>,
    pub cost: Option<f64>,
    pub usage: Usage,
    pub init: Option<Init>,
}

/// One agent spor can run.
pub trait Adapter {
    /// How a run's meta and the UI name it: `claude`, `copilot`, `command`.
    fn id(&self) -> &'static str;

    /// How people name it: "Claude Code".
    fn label(&self) -> &'static str;

    /// The executable it runs, for a known agent: spor checks it's on the
    /// PATH before starting one.
    fn program(&self) -> Option<&'static str> {
        None
    }

    /// The program and arguments for a run, with `extra` (from `prepare`)
    /// before the prompt.
    fn command(&self, launch: &Launch, extra: Vec<String>) -> (String, Vec<String>);

    /// Give the agent what it's told and spor's MCP server, each the way
    /// this agent takes them: flags, or files beside the run. Best effort:
    /// what can't be written is left out and the run still goes.
    fn prepare(&self, _run: &Prepare) -> Prepared {
        Prepared::default()
    }

    /// Its arguments under spor's host (`crate::host`), where it can ask,
    /// be steered and be interrupted while it works. None if it can't run
    /// there.
    fn hosted_args(&self, _args: &[String]) -> Option<Vec<String>> {
        None
    }

    /// Its log as transcript entries.
    fn parse(&self, log: &str) -> Vec<Entry>;

    /// Its session, cost, token use and model, from its log.
    fn stats(&self, _log: &str) -> LogStats {
        LogStats::default()
    }

    /// What it's doing while it runs: its latest tool call, and its plan
    /// when it keeps one where spor can see it.
    fn progress(&self, log: &str) -> Progress {
        let tool = self.parse(log).into_iter().rev().find_map(|e| match e {
            Entry::Tool { name, detail } => Some((name, detail)),
            _ => None,
        });
        Progress {
            tool,
            ..Progress::default()
        }
    }

    /// Its log always ends with a result entry of its own. If not, spor
    /// adds one when it finishes.
    fn ends_with_result(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentKind;

    /// A run directory of its own, emptied first.
    fn run_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("spor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn server() -> McpServer {
        McpServer {
            command: "/Applications/Spor.app/spor".into(),
            args: vec!["__spor-mcp".into(), "/r".into(), "t1".into()],
        }
    }

    #[test]
    fn every_agent_is_found_again_by_its_name() {
        let mut kinds = AgentKind::KNOWN.to_vec();
        kinds.push(AgentKind::Command("codex exec {prompt}".into()));
        for kind in kinds {
            assert_eq!(AgentKind::from_name(&kind.name()), kind);
        }
        let ids: Vec<&str> = AgentKind::KNOWN.iter().map(AgentKind::id).collect();
        assert_eq!(ids, ["claude", "copilot"]);
    }

    #[test]
    fn claude_is_told_on_its_system_prompt_and_given_the_server() {
        let dir = run_dir("prepare-claude");
        let server = server();
        let prepared = AgentKind::Claude.adapter().prepare(&Prepare {
            context: "You are in spor.",
            mcp: Some(&server),
            dir: &dir,
            stem: "run-0",
        });
        assert_eq!(
            &prepared.args[..2],
            ["--append-system-prompt", "You are in spor."]
        );
        let config = std::fs::read_to_string(dir.join("run-0.mcp.json")).unwrap();
        assert!(config.contains(r#""spor":{"command":"/Applications/Spor.app/spor""#));
        assert!(prepared.args.windows(2).any(|w| w[0] == "--mcp-config"));
        assert!(prepared.env.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn copilot_is_told_in_an_agents_file_and_given_the_server() {
        let dir = run_dir("prepare-copilot");
        let server = server();
        let prepared = AgentKind::Copilot.adapter().prepare(&Prepare {
            context: "You are in spor.",
            mcp: Some(&server),
            dir: &dir,
            stem: "run-0",
        });
        let (key, dirs) = &prepared.env[0];
        assert_eq!(key, "COPILOT_CUSTOM_INSTRUCTIONS_DIRS");
        let instructions = dir.join("run-0.instructions");
        assert!(dirs.ends_with(&instructions.display().to_string()));
        assert_eq!(
            std::fs::read_to_string(instructions.join("AGENTS.md")).unwrap(),
            "You are in spor."
        );
        let config = std::fs::read_to_string(dir.join("run-0.mcp.json")).unwrap();
        // Only the tools that need no permission; never `ask`.
        assert!(config.contains(r#""type":"local""#));
        assert!(config.contains(r#""other_tasks""#));
        assert!(!config.contains(r#""ask""#));
        assert_eq!(prepared.args[1], "--allow-tool=spor");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_command_gets_nothing_added() {
        let dir = run_dir("prepare-command");
        let kind = AgentKind::Command("true".into());
        let prepared = kind.adapter().prepare(&Prepare {
            context: "You are in spor.",
            mcp: Some(&server()),
            dir: &dir,
            stem: "run-0",
        });
        assert_eq!(prepared, Prepared::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
