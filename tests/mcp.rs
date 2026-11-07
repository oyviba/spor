//! Spor's MCP server (#106): the tools, called as Claude Code would.

use spor::agent::Runs;
use spor::git::Repo;
use spor::json::Json;
use spor::mcp::{respond, Context};
use spor::tasks::Tasks;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    dir: PathBuf,
    root: PathBuf,
    tasks: Tasks,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-mcp-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("app");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@x"),
        ("commit.gpgsign", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    std::fs::write(root.join("app.txt"), "one\ntwo\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "start"]);
    let tasks = Tasks::open(&Repo::new(&root)).unwrap();
    Fixture { dir, root, tasks }
}

fn ctx(f: &Fixture, task: &str) -> Context {
    Context {
        repo_root: f.root.display().to_string(),
        task: task.into(),
        log: f.dir.join("run.log").display().to_string(),
    }
}

/// Call a tool; its text and whether it failed.
fn call(ctx: &Context, tool: &str, args: &str) -> (String, bool) {
    let msg = format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#
    );
    let reply = Json::parse(&respond(&msg, ctx).unwrap()).unwrap();
    let result = reply.get("result");
    (
        result.get("content").array()[0]
            .get("text")
            .str()
            .unwrap()
            .to_string(),
        result.get("isError").bool() == Some(true),
    )
}

#[test]
fn it_speaks_mcp() {
    let f = fixture();
    let c = ctx(&f, "none");
    let init = respond(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        &c,
    )
    .unwrap();
    assert!(
        init.contains(r#""protocolVersion":"2025-03-26""#) && init.contains("tools"),
        "{init}"
    );
    assert!(respond(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        &c
    )
    .is_none());
    let list =
        Json::parse(&respond(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, &c).unwrap())
            .unwrap();
    let names: Vec<&str> = list
        .get("result")
        .get("tools")
        .array()
        .iter()
        .filter_map(|t| t.get("name").str())
        .collect();
    assert_eq!(
        names,
        [
            "ask",
            "report_progress",
            "other_tasks",
            "pr_comments",
            "ci_log",
            "why"
        ]
    );
}

#[test]
fn other_tasks_shows_the_rest_and_where_they_collide() {
    let f = fixture();
    let a = f
        .tasks
        .create("Rewrite the app text", "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    let b = f
        .tasks
        .create("Add a readme", "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    std::fs::write(a.worktree.join("app.txt"), "changed\n").unwrap();
    std::fs::write(b.worktree.join("app.txt"), "also changed\n").unwrap();
    std::fs::write(b.worktree.join("README.md"), "hi\n").unwrap();

    let (text, err) = call(&ctx(&f, &a.id), "other_tasks", "{}");
    assert!(!err);
    assert!(
        text.contains(&b.id) && text.contains("Add a readme"),
        "{text}"
    );
    assert!(!text.contains("Rewrite the app text"), "not itself: {text}");
    assert!(
        text.contains("README.md, app.txt") || text.contains("app.txt, README.md"),
        "{text}"
    );
    assert!(
        text.contains("Conflicts with you: both change app.txt"),
        "{text}"
    );
}

#[test]
fn why_finds_the_task_behind_a_line() {
    let f = fixture();
    let t = f
        .tasks
        .create("Reword the second line", "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    std::fs::write(t.worktree.join("app.txt"), "one\nTWO\n").unwrap();
    git(&t.worktree, &["add", "."]);
    git(&t.worktree, &["commit", "--quiet", "-m", &t.land_message()]);

    let c = ctx(&f, &t.id);
    let (text, err) = call(&c, "why", r#"{"path":"app.txt","line":2}"#);
    assert!(!err, "{text}");
    assert!(
        text.contains(&format!("Task {} made the change", t.id)),
        "{text}"
    );
    assert!(text.contains("Reword the second line"), "{text}");
    // A line from before any agent.
    let (text, _) = call(&c, "why", r#"{"path":"app.txt","line":1}"#);
    assert!(text.contains("No agent task is recorded"), "{text}");
    // Only inside the repository.
    assert!(call(&c, "why", r#"{"path":"../x","line":1}"#).1);
}

#[test]
fn ask_returns_the_answer_and_progress_goes_to_the_log() {
    let f = fixture();
    let t = f
        .tasks
        .create("Anything", "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    let c = ctx(&f, &t.id);
    assert_eq!(
        call(&c, "ask", r#"{"question":"Which?","answer":"SQLite"}"#),
        ("SQLite".to_string(), false)
    );
    assert!(
        call(&c, "ask", r#"{"question":"Which?"}"#).1,
        "nobody was asked"
    );

    std::fs::write(&c.log, "").unwrap();
    assert!(
        !call(
            &c,
            "report_progress",
            r#"{"step":2,"of":5,"note":"Wiring it up"}"#
        )
        .1
    );
    let log = std::fs::read_to_string(&c.log).unwrap();
    assert!(
        log.contains(r#""type":"spor_progress","step":2,"of":5,"note":"Wiring it up""#),
        "{log}"
    );
}

#[test]
fn pr_tools_say_so_when_there_is_no_pull_request() {
    let f = fixture();
    let t = f
        .tasks
        .create("Anything", "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    let c = ctx(&f, &t.id);
    assert!(call(&c, "pr_comments", "{}").0.contains("no pull request"));
    assert!(call(&c, "ci_log", "{}").1);
}

#[test]
fn the_agent_is_told_where_it_is_and_what_the_others_touch() {
    let f = fixture();
    let a = f.tasks.create("Rewrite the app text", "main").unwrap();
    let b = f.tasks.create("Add a readme\n\nwith more", "main").unwrap();
    std::fs::write(b.worktree.join("README.md"), "hi\n").unwrap();
    std::fs::create_dir_all(a.worktree.join(".spor")).unwrap();
    std::fs::write(a.worktree.join(".spor/context.md"), "Use tabs.\n").unwrap();

    let text = Runs::of(&f.tasks, &a).context(&a);
    assert!(
        text.contains(&a.branch) && text.contains("Don't push"),
        "{text}"
    );
    assert!(
        text.contains(&format!("- {}: Add a readme", b.id)),
        "{text}"
    );
    assert!(!text.contains("with more"), "only the first line: {text}");
    assert!(text.contains("changed so far: README.md"), "{text}");
    assert!(!text.contains("Rewrite the app text"), "not itself: {text}");
    assert!(text.trim_end().ends_with("Use tabs."), "{text}");
}

#[test]
fn a_lone_task_hears_of_no_others() {
    let f = fixture();
    let a = f.tasks.create("Rewrite the app text", "main").unwrap();
    let text = Runs::of(&f.tasks, &a).context(&a);
    assert!(!text.contains("Other agents"), "{text}");
}
