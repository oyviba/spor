//! Spor as an MCP server for its agents (#106).
//!
//! The agent can only print text that spor reads after the fact. This is
//! the other direction: a small MCP server over stdio, started by Claude
//! Code for each run (`--mcp-config` names a file in the run directory) as
//! the app's executable with [`ARG`] as its first argument, like the agent
//! host. The agent asks for what it needs when it needs it: the other
//! tasks, the pull request's review threads, the failing CI log, and why a
//! line exists.
//!
//! Tools are read-only apart from two. `report_progress` appends a line to
//! the run's log. `ask` is a question for the person: Claude Code asks
//! permission to call it like any tool, spor shows that as a question in
//! the Inbox (see `AskKind::of`), and the answer comes back in the call's
//! input as `answer`. Nothing here publishes or writes to git, and
//! everything is scoped to the task's repository.

use crate::git::Repo;
use crate::github;
use crate::json::Json;
use crate::tasks::{Task, TaskState, Tasks};
use std::io::{BufRead, Write};
use std::path::Path;

/// The first argument that turns an app's executable into the MCP server.
pub const ARG: &str = "__spor-mcp";

/// The server's name, and so the prefix of its tools in Claude Code.
pub const SERVER: &str = "spor";

/// `ask`, as Claude Code names it.
pub const ASK_TOOL: &str = "mcp__spor__ask";

/// The tools that need no permission: they only read.
pub const READ_ONLY_TOOLS: [&str; 5] = [
    "mcp__spor__other_tasks",
    "mcp__spor__pr_comments",
    "mcp__spor__ci_log",
    "mcp__spor__why",
    "mcp__spor__report_progress",
];

/// What a call needs to know: where the task is.
pub struct Context {
    /// The repository spor has the task in.
    pub repo_root: String,
    pub task: String,
    /// The run's log, which `report_progress` appends to.
    pub log: String,
}

/// `ARG <repo root> <task id> <run log>`: serve MCP on stdin and stdout.
pub fn main(args: &[String]) -> i32 {
    let [repo_root, task, log] = args else {
        eprintln!("usage: {ARG} <repo root> <task id> <run log>");
        return 2;
    };
    let ctx = Context {
        repo_root: repo_root.clone(),
        task: task.clone(),
        log: log.clone(),
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if let Some(reply) = respond(&line, &ctx) {
            if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    0
}

/// The reply to one JSON-RPC message; None for notifications.
pub fn respond(line: &str, ctx: &Context) -> Option<String> {
    let msg = Json::parse(line.trim())?;
    let id = msg.get("id");
    if matches!(id, Json::Null) {
        return None;
    }
    let id = id.to_json();
    let result = match msg.get("method").str()? {
        "initialize" => {
            let version = msg
                .get("params")
                .get("protocolVersion")
                .str()
                .unwrap_or("2025-06-18");
            format!(
                r#"{{"protocolVersion":{},"capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"{SERVER}","version":"0.1.0"}}}}"#,
                crate::host::quote(version)
            )
        }
        "ping" => "{}".into(),
        "tools/list" => format!(r#"{{"tools":[{}]}}"#, tool_list().join(",")),
        "tools/call" => {
            let params = msg.get("params");
            let name = params.get("name").str().unwrap_or_default();
            let (text, is_error) = match call(name, params.get("arguments"), ctx) {
                Ok(text) => (text, false),
                Err(e) => (e, true),
            };
            format!(
                r#"{{"content":[{{"type":"text","text":{}}}],"isError":{is_error}}}"#,
                crate::host::quote(&text)
            )
        }
        other => {
            return Some(format!(
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32601,"message":{}}}}}"#,
                crate::host::quote(&format!("Unknown method {other}"))
            ))
        }
    };
    Some(format!(
        r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#
    ))
}

fn tool(name: &str, description: &str, properties: &str, required: &str) -> String {
    format!(
        r#"{{"name":"{name}","description":{},"inputSchema":{{"type":"object","properties":{{{properties}}},"required":[{required}]}}}}"#,
        crate::host::quote(description)
    )
}

fn tool_list() -> Vec<String> {
    vec![
        tool(
            "ask",
            "Ask the person you're working for a question and wait for the answer. Use it when you need a decision you can't make yourself.",
            r#""question":{"type":"string"},"options":{"type":"array","items":{"type":"string"},"description":"Answers to pick from; leave out for a free-text answer"}"#,
            r#""question""#,
        ),
        tool(
            "report_progress",
            "Tell spor how far along you are, for the task's live status.",
            r#""step":{"type":"integer","description":"The step you're on"},"of":{"type":"integer","description":"How many steps there are"},"note":{"type":"string"}"#,
            r#""step","of""#,
        ),
        tool(
            "other_tasks",
            "The other active tasks in this repository: their prompts, the files they changed so far, and which of those you're changing too.",
            "",
            "",
        ),
        tool(
            "pr_comments",
            "The unresolved review threads on this task's pull request.",
            "",
            "",
        ),
        tool(
            "ci_log",
            "The failing CI jobs of this task's pull request, with the end of each log (the part up to the failing step).",
            r#""job":{"type":"string","description":"Only the job with this name"}"#,
            "",
        ),
        tool(
            "why",
            "Why a line exists: blames it, and if an agent's task made the change, returns that task's prompt and the part of its transcript that touched the file.",
            r#""path":{"type":"string"},"line":{"type":"integer"}"#,
            r#""path","line""#,
        ),
    ]
}

fn call(name: &str, args: &Json, ctx: &Context) -> Result<String, String> {
    let tasks = Tasks::open(&Repo::new(&ctx.repo_root))?;
    let task = tasks
        .get(&ctx.task)
        .ok_or_else(|| format!("Spor doesn't know the task {}", ctx.task))?;
    match name {
        "ask" => match args.get("answer").str() {
            Some(answer) => Ok(answer.to_string()),
            None => Err("The question wasn't put to anyone. Carry on with your best guess.".into()),
        },
        "report_progress" => report_progress(args, ctx),
        "other_tasks" => Ok(other_tasks(&tasks, &task)),
        "pr_comments" => pr_comments(&task),
        "ci_log" => ci_log(&task, args.get("job").str()),
        "why" => why(
            &tasks,
            &task,
            args.get("path").str().ok_or("path is required")?,
            args.get("line").num().ok_or("line is required")? as usize,
        ),
        other => Err(format!("No tool named {other}")),
    }
}

fn report_progress(args: &Json, ctx: &Context) -> Result<String, String> {
    let step = args.get("step").num().ok_or("step is required")?;
    let of = args.get("of").num().ok_or("of is required")?;
    let note = args.get("note").str().unwrap_or_default();
    let mut log = std::fs::OpenOptions::new()
        .append(true)
        .open(&ctx.log)
        .map_err(|e| e.to_string())?;
    writeln!(
        log,
        r#"{{"type":"spor_progress","step":{},"of":{},"note":{}}}"#,
        step as i64,
        of as i64,
        crate::host::quote(note)
    )
    .map_err(|e| e.to_string())?;
    Ok("Noted.".into())
}

/// Every file the task has changed, committed or not.
pub(crate) fn changed_files(task: &Task) -> Vec<String> {
    let wt = task.repo();
    let start = wt
        .merge_base(task.start(), "HEAD")
        .unwrap_or_else(|_| task.start().to_string());
    let mut files = wt.changed_paths(&start, "HEAD");
    files.extend(wt.status().unwrap_or_default().into_iter().map(|e| e.path));
    files.sort();
    files.dedup();
    files
}

/// The other open tasks that have a folder: id, prompt, files changed so far.
pub(crate) fn others_touching(tasks: &Tasks, me: &Task) -> Vec<(String, String, Vec<String>)> {
    tasks
        .list()
        .into_iter()
        .filter(|t| t.state == TaskState::Open && t.id != me.id && t.worktree.exists())
        .map(|t| {
            let files = changed_files(&t);
            (t.id, t.prompt, files)
        })
        .collect()
}

fn other_tasks(tasks: &Tasks, me: &Task) -> String {
    let mine = changed_files(me);
    let others: Vec<Task> = tasks
        .list()
        .into_iter()
        .filter(|t| t.state == TaskState::Open && t.id != me.id && t.worktree.exists())
        .collect();
    if others.is_empty() {
        return "No other tasks are active in this repository.".into();
    }
    let mut out = String::new();
    for t in others {
        let files = changed_files(&t);
        out.push_str(&format!(
            "## {} (branch {})\nPrompt: {}\n",
            t.id,
            t.branch,
            t.prompt.trim()
        ));
        if files.is_empty() {
            out.push_str("Changed so far: nothing\n");
        } else {
            out.push_str(&format!("Changed so far: {}\n", files.join(", ")));
        }
        let shared: Vec<&String> = files.iter().filter(|f| mine.contains(f)).collect();
        if !shared.is_empty() {
            out.push_str(&format!(
                "Conflicts with you: both change {}\n",
                shared
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn pr_comments(task: &Task) -> Result<String, String> {
    let number = task.pull.ok_or("This task has no pull request yet")?;
    let repo = github::repo(&task.worktree)?;
    let threads = github::unresolved_threads(&task.worktree, &repo, number)?;
    if threads.is_empty() {
        return Ok(format!(
            "Pull request #{number} has no unresolved review threads."
        ));
    }
    let mut out = format!("Unresolved review threads on pull request #{number}:\n");
    for t in threads {
        let place = if t.line > 0 {
            format!("{}:{}", t.path, t.line)
        } else {
            t.path.clone()
        };
        out.push_str(&format!("\n`{place}`\n"));
        for (author, body) in t.comments {
            out.push_str(&format!(
                "- {author}: {}\n",
                body.trim().replace('\n', "\n  ")
            ));
        }
    }
    Ok(out)
}

fn ci_log(task: &Task, job: Option<&str>) -> Result<String, String> {
    let number = task.pull.ok_or("This task has no pull request yet")?;
    let detail = github::pull_detail(&task.worktree, number)?;
    let mut failures = github::pull_failures(&task.worktree, &detail)?;
    if let Some(job) = job {
        failures.retain(|f| f.job.eq_ignore_ascii_case(job));
    }
    github::failure_feedback(&failures, Some(&detail.pull))
        .ok_or_else(|| "No failing jobs.".to_string())
}

fn why(tasks: &Tasks, task: &Task, path: &str, line: usize) -> Result<String, String> {
    let repo = task.repo();
    // Stay inside the task's folder.
    if Path::new(path).is_absolute() || path.split('/').any(|p| p == "..") {
        return Err("path must be inside the repository".into());
    }
    let blame = repo.blame_runs("HEAD", path, &[(line, 1)]);
    let Some(b) = blame.first() else {
        return Err(format!("Couldn't blame {path}:{line}"));
    };
    let mut out = format!(
        "{path}:{line} was last changed by {} in {} “{}”.\n",
        b.author,
        &b.hash[..b.hash.len().min(10)],
        b.summary
    );
    let message = repo.message(&b.hash).unwrap_or_default();
    let Some(id) = message
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix("Spor-Task:"))
        .map(str::trim)
    else {
        out.push_str("No agent task is recorded for that commit.");
        return Ok(out);
    };
    let Some(origin) = tasks.get(id) else {
        out.push_str(&format!("Task {id} made it, but spor no longer has it."));
        return Ok(out);
    };
    out.push_str(&format!(
        "Task {id} made the change. Its prompt:\n{}\n",
        origin.prompt.trim()
    ));
    let transcript = crate::agent::Runs::of(tasks, &origin).transcript();
    let touching = relevant_transcript(&transcript, path);
    if !touching.is_empty() {
        out.push_str(&format!("\nWhat the agent did around {path}:\n{touching}"));
    }
    Ok(out)
}

/// The agent's words just before each tool call that touched `path`.
fn relevant_transcript(entries: &[crate::agent::Entry], path: &str) -> String {
    use crate::agent::Entry;
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let Entry::Tool { name, detail } = e else {
            continue;
        };
        if !detail.contains(path) {
            continue;
        }
        if let Some(Entry::Text(said)) = entries[..i]
            .iter()
            .rev()
            .find(|e| matches!(e, Entry::Text(_)))
        {
            out.push(format!("> {}", said.replace('\n', "\n> ")));
        }
        out.push(format!("{name} {detail}"));
    }
    out.dedup();
    out.join("\n")
}
