//! GitHub Copilot CLI (`copilot -p`), with its JSONL session events. It
//! can't ask in prompt mode, so what its permission mode doesn't allow is
//! refused.

use super::adapter::{Adapter, Launch, LogStats, McpServer, Prepare, Prepared};
use super::{spor_event, tool_detail, Entry, Init, Mode, MAX_RESULT_CHARS};
use crate::host;
use crate::json::Json;

/// GitHub Copilot CLI's executable.
pub const COPILOT: &str = "copilot";

pub struct Copilot;

impl Adapter for Copilot {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn label(&self) -> &'static str {
        "Copilot CLI"
    }

    fn program(&self) -> Option<&'static str> {
        Some(COPILOT)
    }

    fn command(&self, launch: &Launch, extra: Vec<String>) -> (String, Vec<String>) {
        let mut args: Vec<String> = [
            "--output-format=json",
            "--stream=off",
            "--no-color",
            // Nobody can answer its question form in prompt mode.
            "--no-ask-user",
        ]
        .map(String::from)
        .to_vec();
        // It can't ask in prompt mode: what the mode doesn't allow is
        // refused (read-only commands always run).
        let mode = launch.permissions.mode;
        match mode {
            Mode::Supervised => {}
            Mode::AcceptEdits => args.push("--allow-tool=write".into()),
            Mode::Auto => args.push("--assisted-approval".into()),
            Mode::Full => args.push("--allow-all-tools".into()),
            Mode::Plan => args.push("--plan".into()),
        }
        if mode != Mode::Plan {
            // A resumed session keeps its mode: out of plan mode once
            // the plan is approved.
            args.push("--mode=interactive".into());
        }
        if let Some(name) = &launch.model.name {
            args.push(format!("--model={name}"));
        }
        if let Some(effort) = launch.model.effort {
            args.push(format!("--reasoning-effort={}", effort.flag()));
        }
        if let Some(session) = launch.resume {
            // Copilot can't fork a session: a follow-up continues it,
            // so undoing a run doesn't take back the conversation.
            args.push(format!("--resume={session}"));
        }
        args.push(format!("--add-dir={}", launch.attachments.display()));
        // Publishing is always a person's call, from spor. Deny rules
        // win over every allow.
        args.push("--deny-tool=shell(git push)".into());
        args.extend(extra);
        args.extend(["-p".into(), launch.prompt.into()]);
        (COPILOT.into(), args)
    }

    /// Copilot has no flag for added instructions; it reads an AGENTS.md
    /// from the directories `COPILOT_CUSTOM_INSTRUCTIONS_DIRS` names, so
    /// what it's told goes in one beside the run. spor's MCP server comes
    /// with only the tools that need no permission (`ask` is a question
    /// Copilot couldn't put to anyone in prompt mode), allowed.
    fn prepare(&self, run: &Prepare) -> Prepared {
        let mut out = Prepared::default();
        let dir = run.file("instructions");
        if std::fs::create_dir_all(&dir).is_ok()
            && std::fs::write(dir.join("AGENTS.md"), run.context).is_ok()
        {
            let dirs = match std::env::var("COPILOT_CUSTOM_INSTRUCTIONS_DIRS") {
                Ok(own) if !own.trim().is_empty() => format!("{own},{}", dir.display()),
                _ => dir.display().to_string(),
            };
            out.env
                .push(("COPILOT_CUSTOM_INSTRUCTIONS_DIRS".into(), dirs));
        }
        if let Some(server) = run.mcp {
            let file = run.file("mcp.json");
            let tools: Vec<String> = McpServer::read_only_tools()
                .into_iter()
                .map(host::quote)
                .collect();
            let config = format!(
                r#"{{"mcpServers":{{"{}":{{"type":"local",{},"tools":[{}]}}}}}}"#,
                crate::mcp::SERVER,
                server.json_fields(),
                tools.join(",")
            );
            if std::fs::write(&file, config).is_ok() {
                out.args.extend([
                    format!("--additional-mcp-config=@{}", file.display()),
                    format!("--allow-tool={}", crate::mcp::SERVER),
                ]);
            }
        }
        out
    }

    fn parse(&self, log: &str) -> Vec<Entry> {
        parse_copilot(log)
    }

    /// Its session, token use and model. Copilot spends premium requests
    /// rather than dollars, so there's no cost. (Its plan lives in its own
    /// database, out of spor's sight, so progress is its latest tool.)
    fn stats(&self, log: &str) -> LogStats {
        copilot_log(log)
    }
}

/// Copilot's tool, by the name Claude Code gives the same kind of tool, so
/// the transcript, its icons and the test and build checks treat them
/// alike. An MCP tool is `mcp__<server>__<tool>`, as Claude names it.
fn copilot_tool(data: &Json) -> String {
    if let (Some(server), Some(tool)) = (
        data.get("mcpServerName").str(),
        data.get("mcpToolName").str(),
    ) {
        return format!("mcp__{server}__{tool}");
    }
    let name = data.get("toolName").str().unwrap_or("Tool");
    match name {
        "bash" | "powershell" => "Bash",
        "read_bash" | "write_bash" | "stop_bash" => "BashOutput",
        "view" => "Read",
        "edit" | "str_replace_editor" | "apply_patch" => "Edit",
        "create" => "Write",
        "grep" | "rg" => "Grep",
        "glob" => "Glob",
        "web_fetch" => "WebFetch",
        "web_search" => "WebSearch",
        "task" => "Task",
        other => other,
    }
    .to_string()
}

/// Copilot tools that only narrate (what it's about to do, that it's
/// done): its words say the same, so they stay out of the transcript.
const COPILOT_QUIET_TOOLS: [&str; 2] = ["report_intent", "task_complete"];

/// Read Copilot CLI's `--output-format json` (JSONL session events) into
/// transcript entries.
pub fn parse_copilot(log: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    // Each command's and subagent's call by its id, to pair its result.
    let mut calls: std::collections::HashMap<String, (String, String)> = Default::default();
    for line in log.lines().filter(|l| !l.trim().is_empty()) {
        let Some(event) = Json::parse(line) else {
            out.push(Entry::Output(line.to_string()));
            continue;
        };
        let data = event.get("data");
        let start = out.len();
        match event.get("type").str().unwrap_or_default() {
            "session.start" | "session.resume" => {
                if let Some(model) = data.get("selectedModel").str() {
                    out.push(Entry::Init(Init {
                        model: model.to_string(),
                        servers: Vec::new(),
                    }));
                }
            }
            "session.model_change" => {
                if let Some(model) = data.get("newModel").str() {
                    out.push(Entry::Init(Init {
                        model: model.to_string(),
                        servers: Vec::new(),
                    }));
                }
            }
            "session.mcp_servers_loaded" => {
                let servers = copilot_servers(data);
                let init = out.iter_mut().rev().find_map(|e| match e {
                    Entry::Init(i) => Some(i),
                    _ => None,
                });
                if let Some(init) = init {
                    init.servers = servers;
                }
            }
            "assistant.reasoning" => {
                let text = data.get("content").str().unwrap_or("").trim();
                if !text.is_empty() {
                    out.push(Entry::Thinking(text.to_string()));
                }
            }
            "assistant.message" => {
                let text = data.get("content").str().unwrap_or("").trim();
                if !text.is_empty() {
                    out.push(Entry::Text(text.to_string()));
                }
            }
            "tool.execution_start" => {
                let raw = data.get("toolName").str().unwrap_or_default();
                if COPILOT_QUIET_TOOLS.contains(&raw) {
                    continue;
                }
                let name = copilot_tool(data);
                let input = data.get("arguments");
                let detail = tool_detail(input);
                let id = data.get("toolCallId").str().unwrap_or_default();
                if matches!(name.as_str(), "Bash" | "Task") && !id.is_empty() {
                    calls.insert(id.to_string(), (name.clone(), detail.clone()));
                }
                out.push(Entry::Tool {
                    name: name.clone(),
                    detail,
                });
                if name == "Task" && !id.is_empty() {
                    let text =
                        |key: &str| input.get(key).str().unwrap_or_default().trim().to_string();
                    out.push(Entry::Subagent {
                        id: id.to_string(),
                        agent: text("agent_type"),
                        description: text("description"),
                        prompt: text("prompt"),
                    });
                }
            }
            "tool.execution_complete" => {
                let failed = data.get("success").bool() == Some(false);
                let result = data.get("result");
                let content = result
                    .get("detailedContent")
                    .str()
                    .or_else(|| result.get("content").str())
                    .unwrap_or_default();
                if failed {
                    let reason = data
                        .get("error")
                        .get("message")
                        .str()
                        .unwrap_or(content)
                        .trim();
                    out.push(Entry::ToolError(
                        reason.lines().next().unwrap_or("").to_string(),
                    ));
                }
                let call = data.get("toolCallId").str().and_then(|id| calls.get(id));
                if let Some((name, detail)) = call {
                    let text = content.trim_end();
                    let count = text.chars().count();
                    out.push(Entry::Result {
                        name: name.clone(),
                        detail: detail.clone(),
                        ok: !failed,
                        output: text
                            .chars()
                            .skip(count.saturating_sub(MAX_RESULT_CHARS))
                            .collect(),
                        truncated: count > MAX_RESULT_CHARS,
                    });
                }
            }
            "session.compaction_complete" if data.get("success").bool() == Some(true) => {
                out.push(Entry::Compacted)
            }
            "session.error" => out.push(Entry::Output(format!(
                "Error: {}",
                data.get("message").str().unwrap_or("something went wrong")
            ))),
            "session.warning" => {
                if let Some(message) = data.get("message").str() {
                    out.push(Entry::Output(format!("Warning: {message}")));
                }
            }
            "result" => {
                let code = event.get("exitCode").num().unwrap_or(0.0) as i32;
                out.push(Entry::Done {
                    ok: code == 0,
                    summary: if code == 0 {
                        String::new()
                    } else {
                        format!("Exited with code {code}")
                    },
                    cost_usd: None,
                    turns: None,
                    seconds: event
                        .get("usage")
                        .get("sessionDurationMs")
                        .num()
                        .map(|ms| ms / 1000.0),
                });
            }
            // spor's own lines (setup, steering) read the same for any agent.
            t if t.starts_with("spor_") => out.extend(spor_event(&event)),
            _ => {}
        }
        // What a subagent did hangs under the call that started it.
        if let Some(parent) = data.get("parentToolCallId").str() {
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

/// The MCP servers Copilot loaded and how each went. Ones it didn't try
/// (disabled, still pending) are left out, so only real failures show.
fn copilot_servers(data: &Json) -> Vec<(String, String)> {
    data.get("servers")
        .array()
        .iter()
        .filter_map(|m| {
            let status = m.get("status").str()?;
            matches!(status, "connected" | "failed" | "needs-auth").then(|| {
                (
                    m.get("name").str().unwrap_or_default().to_string(),
                    status.to_string(),
                )
            })
        })
        .collect()
}

/// The session, token use and model of a Copilot log.
fn copilot_log(log: &str) -> LogStats {
    let mut stats = LogStats::default();
    for line in log.lines() {
        if !line.starts_with('{') {
            continue;
        }
        let Some(event) = Json::parse(line) else {
            continue;
        };
        let data = event.get("data");
        match event.get("type").str() {
            Some("session.start") => {
                if let Some(id) = data.get("sessionId").str() {
                    stats.session = Some(id.to_string());
                }
                if let Some(model) = data.get("selectedModel").str() {
                    stats.init.get_or_insert_with(Init::default).model = model.to_string();
                }
            }
            Some("session.resume") | Some("session.model_change") => {
                let model = data
                    .get("selectedModel")
                    .str()
                    .or_else(|| data.get("newModel").str());
                if let Some(model) = model {
                    let servers = stats.init.take().map(|i| i.servers).unwrap_or_default();
                    stats.init = Some(Init {
                        model: model.to_string(),
                        servers,
                    });
                }
            }
            Some("session.mcp_servers_loaded") => {
                stats.init.get_or_insert_with(Init::default).servers = copilot_servers(data);
            }
            Some("assistant.usage") => {
                let n = |k: &str| data.get(k).num().unwrap_or(0.0) as u64;
                stats.usage.input += n("inputTokens");
                stats.usage.output += n("outputTokens");
                // A subagent's calls spend tokens but aren't the context.
                if data.get("parentToolCallId").str().is_none() {
                    stats.usage.context = n("inputTokens");
                    if let Some(max) = data.get("maxPromptTokens").num() {
                        stats.usage.window = Some(max as u64);
                    }
                }
            }
            // How full the context is, as Copilot counts it.
            Some("session.usage_info") => {
                if let Some(current) = data.get("currentTokens").num() {
                    stats.usage.context = current as u64;
                }
                if let Some(limit) = data.get("tokenLimit").num().filter(|l| *l > 0.0) {
                    stats.usage.window = Some(limit as u64);
                }
            }
            Some("result") => {
                if let Some(id) = event.get("sessionId").str() {
                    stats.session = Some(id.to_string());
                }
            }
            _ => {}
        }
    }
    // Copilot reports the model it starts with even when none was
    // picked; with no model at all there's nothing to say.
    if stats.init.as_ref().is_some_and(|i| i.model.is_empty()) {
        stats.init = None;
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{checks_of, command_for, AgentKind, Effort, Model, Permissions};
    use std::path::Path;

    #[test]
    fn copilot_command_line() {
        let (program, args) = command_for(
            &AgentKind::Copilot,
            "Fix it",
            Some("0cb916db"),
            Permissions::commands(false),
            &Model::new(Some("auto"), Some(Effort::High)).unwrap(),
            Path::new("/r/.git/spor/attachments"),
        );
        assert_eq!(program, "copilot");
        for flag in [
            "--output-format=json",
            "--no-ask-user",
            "--allow-tool=write",
            "--mode=interactive",
            "--model=auto",
            "--reasoning-effort=high",
            "--resume=0cb916db",
            "--add-dir=/r/.git/spor/attachments",
            "--deny-tool=shell(git push)",
        ] {
            assert!(
                args.iter().any(|a| a == flag),
                "{flag} missing from {args:?}"
            );
        }
        assert!(!args.iter().any(|a| a == "--allow-all-tools"));
        assert_eq!(&args[args.len() - 2..], ["-p", "Fix it"]);
    }

    #[test]
    fn copilot_modes() {
        let flags = |mode| {
            command_for(
                &AgentKind::Copilot,
                "x",
                None,
                Permissions { mode },
                &Model::default(),
                Path::new("/a"),
            )
            .1
        };
        assert!(flags(Mode::Full).iter().any(|a| a == "--allow-all-tools"));
        assert!(flags(Mode::Auto).iter().any(|a| a == "--assisted-approval"));
        let plan = flags(Mode::Plan);
        assert!(plan.iter().any(|a| a == "--plan"));
        assert!(!plan.iter().any(|a| a == "--mode=interactive"));
        let supervised = flags(Mode::Supervised);
        assert!(!supervised
            .iter()
            .any(|a| a.starts_with("--allow") || a == "--assisted-approval"));
        // Without a pick, the model is Copilot's own.
        assert!(!supervised
            .iter()
            .any(|a| a.starts_with("--model") || a.starts_with("--reasoning-effort")));
    }

    const COPILOT_LOG: &str = r#"{"type":"session.start","id":"e1","parentId":null,"timestamp":"t","data":{"sessionId":"b198","selectedModel":"gpt-5.6-sol","copilotVersion":"1.0.114","producer":"copilot-agent","startTime":"t","version":1}}
{"type":"session.mcp_servers_loaded","id":"e2","parentId":"e1","timestamp":"t","ephemeral":true,"data":{"servers":[{"name":"spor","status":"connected"},{"name":"broken","status":"failed"},{"name":"off","status":"disabled"}]}}
{"type":"user.message","id":"e3","parentId":"e2","timestamp":"t","data":{"content":"Fix it"}}
{"type":"assistant.reasoning","id":"e4","parentId":"e3","timestamp":"t","data":{"reasoningId":"r1","content":"Look at the tests first."}}
{"type":"assistant.message","id":"e5","parentId":"e4","timestamp":"t","data":{"messageId":"m1","content":"Running the tests.","toolRequests":[{"toolCallId":"c1","name":"bash","arguments":{"command":"cargo test","description":"Run tests"}}]}}
{"type":"tool.execution_start","id":"e6","parentId":"e5","timestamp":"t","data":{"toolCallId":"c0","toolName":"report_intent","arguments":{"intent":"Testing"}}}
{"type":"tool.execution_start","id":"e7","parentId":"e5","timestamp":"t","data":{"toolCallId":"c1","toolName":"bash","arguments":{"command":"cargo test","description":"Run tests"}}}
{"type":"assistant.usage","id":"e8","parentId":"e7","timestamp":"t","ephemeral":true,"data":{"model":"gpt-5.6-sol","inputTokens":1200,"outputTokens":80,"maxPromptTokens":128000}}
{"type":"tool.execution_complete","id":"e9","parentId":"e7","timestamp":"t","data":{"toolCallId":"c1","success":false,"result":{"content":"test result: FAILED. 1 failed"},"error":{"message":"exit code 101"}}}
{"type":"tool.execution_start","id":"e10","parentId":"e9","timestamp":"t","data":{"toolCallId":"c2","toolName":"task","arguments":{"agent_type":"explore","description":"Find the parser","prompt":"Where is it?"}}}
{"type":"tool.execution_start","id":"e11","parentId":"e10","timestamp":"t","agentId":"a1","data":{"toolCallId":"c3","toolName":"grep","parentToolCallId":"c2","arguments":{"pattern":"fn parse"}}}
{"type":"tool.execution_start","id":"e12","parentId":"e11","timestamp":"t","data":{"toolCallId":"c4","toolName":"edit","arguments":{"path":"src/lib.rs"}}}
{"type":"session.usage_info","id":"e13","parentId":"e12","timestamp":"t","ephemeral":true,"data":{"currentTokens":64000,"tokenLimit":128000,"messagesLength":9}}
{"type":"assistant.message","id":"e14","parentId":"e13","timestamp":"t","data":{"messageId":"m2","content":"Fixed the parser."}}
Error: something odd on stderr
{"type":"result","timestamp":"t","sessionId":"b198","exitCode":0,"usage":{"premiumRequests":1,"sessionDurationMs":42000}}"#;

    #[test]
    fn copilot_events_become_a_transcript() {
        let entries = parse_copilot(COPILOT_LOG);
        assert_eq!(
            entries[0],
            Entry::Init(Init {
                model: "gpt-5.6-sol".into(),
                servers: vec![
                    ("spor".into(), "connected".into()),
                    ("broken".into(), "failed".into())
                ],
            })
        );
        assert_eq!(
            entries[1],
            Entry::Thinking("Look at the tests first.".into())
        );
        assert_eq!(entries[2], Entry::Text("Running the tests.".into()));
        // report_intent only narrates; bash reads as Claude's Bash.
        assert_eq!(
            entries[3],
            Entry::Tool {
                name: "Bash".into(),
                detail: "cargo test".into()
            }
        );
        assert_eq!(entries[4], Entry::ToolError("exit code 101".into()));
        assert!(matches!(
            &entries[5],
            Entry::Result { name, ok: false, output, .. } if name == "Bash" && output.contains("FAILED")
        ));
        assert_eq!(checks_of(&entries).tests, Some(false));
        assert!(
            matches!(&entries[7], Entry::Subagent { id, agent, .. } if id == "c2" && agent == "explore")
        );
        assert!(matches!(
            &entries[8],
            Entry::Nested { parent, entry } if parent == "c2"
                && **entry == Entry::Tool { name: "Grep".into(), detail: "fn parse".into() }
        ));
        assert_eq!(
            entries[9],
            Entry::Tool {
                name: "Edit".into(),
                detail: "src/lib.rs".into()
            }
        );
        assert_eq!(entries[10], Entry::Text("Fixed the parser.".into()));
        assert_eq!(
            entries[11],
            Entry::Output("Error: something odd on stderr".into())
        );
        assert!(matches!(
            entries[12],
            Entry::Done { ok: true, seconds: Some(s), .. } if s == 42.0
        ));
        assert_eq!(entries.len(), 13);
    }

    #[test]
    fn copilot_log_reports_session_and_context() {
        let stats = copilot_log(COPILOT_LOG);
        assert_eq!(stats.session.as_deref(), Some("b198"));
        assert_eq!(stats.cost, None);
        assert_eq!((stats.usage.input, stats.usage.output), (1200, 80));
        assert_eq!(stats.usage.context, 64000);
        assert_eq!(stats.usage.fill(None), Some(50));
        let init = stats.init.unwrap();
        assert_eq!(init.model, "gpt-5.6-sol");
        assert_eq!(init.failed(), ["broken"]);
        let progress = Copilot.progress(COPILOT_LOG);
        assert_eq!(
            progress.tool,
            Some(("Edit".to_string(), "src/lib.rs".to_string()))
        );
    }

    #[test]
    fn a_failed_copilot_run_says_so() {
        let log = r#"Error: Authentication token found but could not be validated.
{"type":"result","timestamp":"t","sessionId":"b198","exitCode":1,"usage":{"premiumRequests":0,"sessionDurationMs":1025}}"#;
        let entries = parse_copilot(log);
        assert!(matches!(&entries[0], Entry::Output(l) if l.contains("Authentication")));
        assert!(matches!(
            &entries[1],
            Entry::Done { ok: false, summary, .. } if summary == "Exited with code 1"
        ));
        assert_eq!(copilot_log(log).session.as_deref(), Some("b198"));
    }
}
