//! Claude Code, headless (`claude -p`), with its `stream-json` events. It
//! is the one agent that runs under spor's host (`crate::host`), so it can
//! ask, and be steered and interrupted while it works.

use super::adapter::{Adapter, Launch, LogStats, Prepare, Prepared};
use super::{
    ask_detail, covers, plan_progress, spor_event, todos, tool_detail, AskKind, Entry, Init,
    Progress, Usage, MAX_RESULT_CHARS,
};
use crate::json::Json;

pub struct Claude;

impl Adapter for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn label(&self) -> &'static str {
        "Claude Code"
    }

    fn program(&self) -> Option<&'static str> {
        Some("claude")
    }

    fn command(&self, launch: &Launch, extra: Vec<String>) -> (String, Vec<String>) {
        let mut args: Vec<String> = [
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            launch.permissions.mode.flag(),
        ]
        .map(String::from)
        .to_vec();
        if let Some(name) = &launch.model.name {
            args.extend(["--model".into(), name.clone()]);
        }
        if let Some(effort) = launch.model.effort {
            args.extend(["--effort".into(), effort.flag().into()]);
        }
        if let Some(session) = launch.resume {
            // A new session id for each follow-up, so the one resumed
            // stays as it was: undoing this run resumes from there.
            args.extend(["--resume".into(), session.into(), "--fork-session".into()]);
        }
        // Images attached to prompts are named by their path there.
        args.extend(["--add-dir".into(), launch.attachments.display().to_string()]);
        // Publishing is always a person's call, from spor.
        args.extend(["--disallowedTools".into(), "Bash(git push:*)".into()]);
        args.extend(extra);
        args.extend(["-p".into(), launch.prompt.into()]);
        ("claude".into(), args)
    }

    /// What it's told goes on its system prompt. spor's MCP server comes
    /// in a config file beside the run, with the tools that need no
    /// permission allowed.
    fn prepare(&self, run: &Prepare) -> Prepared {
        let mut args = vec![
            "--append-system-prompt".to_string(),
            run.context.to_string(),
        ];
        if let Some(server) = run.mcp {
            let file = run.file("mcp.json");
            let config = format!(
                r#"{{"mcpServers":{{"{}":{{{}}}}}}}"#,
                crate::mcp::SERVER,
                server.json_fields()
            );
            if std::fs::write(&file, config).is_ok() {
                args.extend([
                    "--mcp-config".into(),
                    file.display().to_string(),
                    "--allowedTools".into(),
                    crate::mcp::READ_ONLY_TOOLS.join(","),
                ]);
            }
        }
        Prepared {
            args,
            env: Vec::new(),
        }
    }

    /// Under the host the prompt goes on stdin (as stream-json, like
    /// follow-ups), and questions come back as control requests instead
    /// of being refused.
    fn hosted_args(&self, args: &[String]) -> Option<Vec<String>> {
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
        Some(out)
    }

    fn parse(&self, log: &str) -> Vec<Entry> {
        parse_claude(log)
    }

    fn stats(&self, log: &str) -> LogStats {
        claude_log(log)
    }

    /// Its plan's progress (its latest `TodoWrite`) and its latest tool
    /// call. Reads only the lines that carry them, so a list of tasks can
    /// ask each one.
    fn progress(&self, log: &str) -> Progress {
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
        out
    }

    fn ends_with_result(&self) -> bool {
        true
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
            // spor's own lines (answers, setup, steering).
            Some(t) if t.starts_with("spor_") => out.extend(spor_event(&event)),
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

/// The session, cost, token use and `init` of a Claude Code log.
pub(super) fn claude_log(log: &str) -> LogStats {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{checks_of, command_for, AgentKind, Checks, Effort, Model, Permissions};
    use std::path::Path;

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
}
