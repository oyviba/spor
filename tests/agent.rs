//! Agents running in task folders: real processes, a fake `claude`.

use spor::agent::{AgentKind, Entry, Permissions, RunState, Runs};
use spor::git::Repo;
use spor::tasks::Tasks;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

struct Fixture {
    dir: PathBuf,
    repo: Repo,
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

/// A `claude` that prints a canned event stream and records its arguments.
fn fake_claude() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let bin = std::env::temp_dir().join(format!("spor-fake-claude-{}", std::process::id()));
        std::fs::create_dir_all(&bin).unwrap();
        let script = r#"#!/bin/sh
printf '%s\n' "$@" > claude-args.txt
echo "agent was here" > agent.txt
echo '{"type":"system","subtype":"init","session_id":"sess-42"}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"agent.txt"}}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Wrote agent.txt."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Done.","total_cost_usd":0.01,"num_turns":2,"duration_ms":900,"session_id":"sess-42"}'
"#;
        let path = bin.join("claude");
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    });
}

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-agent-{}-{}",
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
    std::fs::write(root.join("app.txt"), "one\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "start"]);
    let repo = Repo::new(&root);
    let tasks = Tasks::open(&repo).unwrap();
    Fixture { dir, repo, tasks }
}

fn wait_until_done(runs: &Runs) -> RunState {
    let start = Instant::now();
    loop {
        let state = runs.latest().unwrap().state;
        if state != RunState::Running || start.elapsed() > Duration::from_secs(10) {
            return state;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

const NO_COMMANDS: Permissions = Permissions { commands: false };

#[test]
fn a_custom_command_runs_in_the_task_folder() {
    let f = fixture();
    let task = f.tasks.create("Write a note", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let kind =
        AgentKind::Command("echo \"$SPOR_PROMPT\" > note.txt; echo {prompt}; echo bye".into());
    runs.start(&task, &kind, "Write a note", None, NO_COMMANDS)
        .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    assert_eq!(
        std::fs::read_to_string(task.worktree.join("note.txt")).unwrap(),
        "Write a note\n"
    );
    // The main checkout is untouched.
    assert!(!f.repo.path("note.txt").exists());
    let t = runs.transcript();
    assert_eq!(t[0], Entry::Prompt("Write a note".into()));
    assert_eq!(t[1], Entry::Output("Write a note".into()));
    assert!(matches!(t.last(), Some(Entry::Done { ok: true, .. })));
    assert_eq!(f.tasks.stats(&task).uncommitted, 1);
}

#[test]
fn a_failing_command_reports_its_exit_code() {
    let f = fixture();
    let task = f.tasks.create("Fail", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(
        &task,
        &AgentKind::Command("echo oops >&2; exit 3".into()),
        "Fail",
        None,
        NO_COMMANDS,
    )
    .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Failed(3));
    let t = runs.transcript();
    assert!(t.contains(&Entry::Output("oops".into())));
    assert!(matches!(t.last(), Some(Entry::Done { ok: false, .. })));
}

#[test]
fn stop_ends_the_agent_and_what_it_started() {
    let f = fixture();
    let task = f.tasks.create("Wait forever", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(
        &task,
        &AgentKind::Command("sleep 30; echo late > late.txt".into()),
        "x",
        None,
        NO_COMMANDS,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(runs.latest().unwrap().state, RunState::Running);
    // One at a time.
    assert!(runs
        .start(
            &task,
            &AgentKind::Command("true".into()),
            "y",
            None,
            NO_COMMANDS
        )
        .is_err());
    runs.stop().unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Stopped);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!task.worktree.join("late.txt").exists());
}

#[test]
fn claude_runs_and_follow_ups_resume_its_session() {
    fake_claude();
    let f = fixture();
    let task = f.tasks.create("Leave a mark", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(&task, &AgentKind::Claude, "Leave a mark", None, NO_COMMANDS)
        .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    assert!(task.worktree.join("agent.txt").exists());
    assert_eq!(runs.session().as_deref(), Some("sess-42"));
    assert_eq!(runs.latest().unwrap().cost_usd, Some(0.01));
    let t = runs.transcript();
    assert!(t.contains(&Entry::Text("Wrote agent.txt.".into())));
    assert!(t.contains(&Entry::Tool {
        name: "Write".into(),
        detail: "agent.txt".into()
    }));

    runs.start(
        &task,
        &AgentKind::Claude,
        "Now tidy up",
        runs.session().as_deref(),
        NO_COMMANDS,
    )
    .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    let args = std::fs::read_to_string(task.worktree.join("claude-args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(
        args.windows(2).any(|w| w == ["--resume", "sess-42"]),
        "{args:?}"
    );
    assert_eq!(args.last(), Some(&"Now tidy up"));
    assert_eq!(runs.list().len(), 2);
    let prompts: Vec<_> = runs
        .transcript()
        .into_iter()
        .filter(|e| matches!(e, Entry::Prompt(_)))
        .collect();
    assert_eq!(prompts.len(), 2);
}

#[test]
fn transcripts_survive_discard_and_undo() {
    let f = fixture();
    let task = f.tasks.create("Note", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(
        &task,
        &AgentKind::Command("echo hi; git add -A; git commit -qm x --allow-empty".into()),
        "Note",
        None,
        NO_COMMANDS,
    )
    .unwrap();
    wait_until_done(&runs);
    let undo = f.tasks.discard(&task).unwrap();
    f.tasks.restore(&undo).unwrap();
    assert_eq!(runs.list().len(), 1);
    f.tasks.forget(&task.id).unwrap();
    assert!(runs.list().is_empty());
}

#[test]
fn merge_conflicts_between_branches() {
    let f = fixture();
    let a = f.tasks.create("Edit one way", "main").unwrap();
    let b = f.tasks.create("Edit another way", "main").unwrap();
    let c = f.tasks.create("Edit elsewhere", "main").unwrap();
    for (task, file, text) in [
        (&a, "app.txt", "A\n"),
        (&b, "app.txt", "B\n"),
        (&c, "other.txt", "C\n"),
    ] {
        std::fs::write(task.worktree.join(file), text).unwrap();
        git(&task.worktree, &["add", "."]);
        git(&task.worktree, &["commit", "--quiet", "-m", "work"]);
    }
    assert_eq!(
        f.repo.merge_conflicts(&a.branch, &b.branch).unwrap(),
        ["app.txt"]
    );
    assert!(f
        .repo
        .merge_conflicts(&a.branch, &c.branch)
        .unwrap()
        .is_empty());
    assert!(f
        .repo
        .merge_conflicts("main", &a.branch)
        .unwrap()
        .is_empty());
}
