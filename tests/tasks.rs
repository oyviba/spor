//! Agent tasks against real repositories in temporary directories.

use spor::git::Repo;
use spor::tasks::{LandMode, TaskState, Tasks};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

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

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-tasks-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("app");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    for (k, v) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    std::fs::write(root.join("app.txt"), "one\ntwo\nthree\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "start"]);
    let repo = Repo::new(&root);
    let tasks = Tasks::open(&repo).unwrap();
    Fixture { dir, repo, tasks }
}

fn work(path: &Path, file: &str, text: &str, message: Option<&str>) {
    std::fs::write(path.join(file), text).unwrap();
    if let Some(m) = message {
        git(path, &["add", "."]);
        git(path, &["commit", "--quiet", "-m", m]);
    }
}

fn subjects(dir: &Path) -> Vec<String> {
    git(dir, &["log", "--topo-order", "--format=%s"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn create_puts_a_branch_in_its_own_folder() {
    let f = fixture();
    let task = f.tasks.create("Fix the login redirect", "main").unwrap();
    assert_eq!(task.id, "fix-login-redirect");
    assert_eq!(task.branch, "agent/fix-login-redirect");
    assert!(task.worktree.ends_with("app.spor/fix-login-redirect"));
    assert!(task.worktree.join("app.txt").exists());
    assert_eq!(
        task.repo().tracking().unwrap().branch.as_deref(),
        Some("agent/fix-login-redirect")
    );
    // The same prompt again gets its own id.
    let again = f.tasks.create("Fix the login redirect", "main").unwrap();
    assert_eq!(again.id, "fix-login-redirect-2");
    let listed: Vec<String> = f.tasks.list().into_iter().map(|t| t.id).collect();
    assert_eq!(listed.len(), 2);
    assert!(f.tasks.create("  ", "main").is_err());
    // The open checkout is untouched.
    assert_eq!(f.repo.tracking().unwrap().branch.as_deref(), Some("main"));
}

#[test]
fn tasks_are_shared_by_every_worktree() {
    let f = fixture();
    let task = f.tasks.create("Add a parser", "main").unwrap();
    let from_task = Tasks::open(&task.repo()).unwrap();
    assert_eq!(from_task.list()[0].id, task.id);
}

#[test]
fn stats_count_commits_and_uncommitted_work() {
    let f = fixture();
    let task = f.tasks.create("Add a parser", "main").unwrap();
    assert!(f.tasks.stats(&task).is_empty());
    work(
        &task.worktree,
        "parser.rs",
        "fn parse() {}\n",
        Some("Add parser"),
    );
    work(&task.worktree, "app.txt", "one\nTWO\nthree\n", None);
    work(&task.worktree, "notes.txt", "n\n", None);
    let stats = f.tasks.stats(&task);
    assert_eq!(stats.commits, 1);
    assert_eq!(stats.uncommitted, 2);
    assert_eq!(stats.files, 3);
    // parser.rs (committed) +1, app.txt +1 −1, notes.txt (new) +1.
    assert_eq!((stats.additions, stats.deletions), (3, 1));
    assert!(!stats.base_moved);
    let patch = f.tasks.patch(&task).unwrap();
    for file in ["parser.rs", "app.txt", "notes.txt"] {
        assert!(patch.contains(file), "{file} missing from the patch");
    }
}

#[test]
fn land_squashes_onto_the_base_and_cleans_up() {
    let f = fixture();
    let task = f
        .tasks
        .create("Add a parser\n\nKeep it small.", "main")
        .unwrap();
    work(&task.worktree, "parser.rs", "a\n", Some("wip 1"));
    work(&task.worktree, "parser.rs", "a\nb\n", Some("wip 2"));
    f.tasks.land(&task, LandMode::Squash, false).unwrap();
    assert_eq!(subjects(f.repo.root()), ["Add a parser", "start"]);
    let body = git(f.repo.root(), &["log", "-1", "--format=%B"]);
    assert!(body.contains("Keep it small."));
    assert!(body.contains("Spor-Task: add-parser"));
    assert!(!task.worktree.exists());
    assert!(f.repo.resolve(&task.branch).is_err());
    let landed = f.tasks.get(&task.id).unwrap();
    assert!(matches!(landed.state, TaskState::Landed(_)));
}

#[test]
fn land_with_a_merge_keeps_the_commits() {
    let f = fixture();
    let task = f.tasks.create("Add a parser", "main").unwrap();
    work(&task.worktree, "parser.rs", "a\n", Some("wip 1"));
    f.tasks.land(&task, LandMode::Merge, false).unwrap();
    assert_eq!(subjects(f.repo.root()), ["Add a parser", "wip 1", "start"]);
}

#[test]
fn land_rebases_when_the_base_moved() {
    let f = fixture();
    let task = f.tasks.create("Add a parser", "main").unwrap();
    work(&task.worktree, "parser.rs", "a\n", Some("parser"));
    work(f.repo.root(), "other.txt", "o\n", Some("meanwhile on main"));
    assert!(f.tasks.stats(&task).base_moved);
    f.tasks.land(&task, LandMode::Merge, false).unwrap();
    // Rebased first: the merge's second parent descends from the new main.
    assert_eq!(
        subjects(f.repo.root()),
        ["Add a parser", "parser", "meanwhile on main", "start"]
    );
}

#[test]
fn land_refuses_conflicts_and_leaves_everything_alone() {
    let f = fixture();
    let task = f.tasks.create("Change two", "main").unwrap();
    work(
        &task.worktree,
        "app.txt",
        "one\nTask\nthree\n",
        Some("task's two"),
    );
    work(
        f.repo.root(),
        "app.txt",
        "one\nMain\nthree\n",
        Some("main's two"),
    );
    let err = f.tasks.land(&task, LandMode::Squash, false).unwrap_err();
    assert!(err.contains("conflicts"), "{err}");
    assert!(task.repo().operation().is_none());
    assert_eq!(subjects(&task.worktree), ["task's two", "start"]);
    assert_eq!(subjects(f.repo.root()), ["main's two", "start"]);
}

#[test]
fn land_asks_about_uncommitted_work() {
    let f = fixture();
    let task = f.tasks.create("Add notes", "main").unwrap();
    work(&task.worktree, "notes.txt", "n\n", None);
    assert!(f.tasks.land(&task, LandMode::Squash, false).is_err());
    f.tasks.land(&task, LandMode::Squash, true).unwrap();
    assert!(f.repo.path("notes.txt").exists());
}

#[test]
fn land_needs_the_base_checked_out() {
    let f = fixture();
    let task = f.tasks.create("Add notes", "main").unwrap();
    work(&task.worktree, "notes.txt", "n\n", Some("notes"));
    git(f.repo.root(), &["switch", "--quiet", "-c", "elsewhere"]);
    let err = f.tasks.land(&task, LandMode::Squash, false).unwrap_err();
    assert!(err.contains("Check out main"), "{err}");
}

#[test]
fn discard_and_land_can_be_undone() {
    let f = fixture();
    let task = f.tasks.create("Add notes", "main").unwrap();
    work(&task.worktree, "notes.txt", "n\n", Some("notes"));
    let undo = f.tasks.discard(&task).unwrap();
    assert!(!task.worktree.exists());
    assert!(f.tasks.list().is_empty());
    f.tasks.restore(&undo).unwrap();
    assert_eq!(f.tasks.list()[0].state, TaskState::Open);
    assert_eq!(subjects(&task.worktree), ["notes", "start"]);

    let undo = f.tasks.land(&task, LandMode::Squash, false).unwrap();
    assert_eq!(subjects(f.repo.root())[0], "Add notes");
    f.tasks.restore(&undo).unwrap();
    assert_eq!(subjects(f.repo.root()), ["start"]);
    assert_eq!(f.tasks.get(&task.id).unwrap().state, TaskState::Open);
    assert!(task.worktree.join("notes.txt").exists());
}

#[test]
fn issue_tasks_are_named_after_the_issue_and_close_it_when_landed() {
    let f = fixture();
    let task = f
        .tasks
        .create_for_issue(
            "Login fails on Safari (#7)\n\nThe whole issue",
            "main",
            7,
            "Login fails on Safari",
        )
        .unwrap();
    assert_eq!(task.id, "7-login-fails-safari");
    assert_eq!(task.branch, "agent/7-login-fails-safari");
    assert_eq!(task.issue, Some(7));
    assert_eq!(f.tasks.get(&task.id).unwrap().issue, Some(7));
    assert!(task.pull_request_body(None).starts_with("Closes #7"));

    work(&task.worktree, "app.txt", "fixed\n", Some("Fix it"));
    f.tasks.land(&task, LandMode::Squash, false).unwrap();
    let message = git(f.repo.root(), &["log", "-1", "--format=%B"]);
    assert_eq!(
        message.trim(),
        "Login fails on Safari (#7)\n\nCloses #7\n\nSpor-Task: 7-login-fails-safari"
    );
}

#[test]
fn publish_pushes_the_branch_before_opening_a_pull_request() {
    let f = fixture();
    let remote = f.dir.join("remote.git");
    git(
        &f.dir,
        &["init", "--quiet", "--bare", remote.to_str().unwrap()],
    );
    git(
        f.repo.root(),
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let task = f.tasks.create("Add a thing", "main").unwrap();

    let err = f
        .tasks
        .publish(&task, "origin", false, false, false)
        .unwrap_err();
    assert!(err.contains("no commits"), "{err}");

    work(&task.worktree, "new.txt", "new\n", None);
    let err = f
        .tasks
        .publish(&task, "origin", false, false, false)
        .unwrap_err();
    assert!(err.contains("uncommitted"), "{err}");

    // The push works; opening the PR can't (the remote isn't GitHub), and
    // then the task has no PR.
    let err = f
        .tasks
        .publish(&task, "origin", true, true, false)
        .unwrap_err();
    assert!(err.contains("GitHub"), "{err}");
    git(
        &remote,
        &["rev-parse", "--verify", "refs/heads/agent/add-thing"],
    );
    assert_eq!(f.tasks.get(&task.id).unwrap().pull, None);

    // Follow-up commits are counted until they're pushed.
    assert_eq!(f.tasks.stats(&task).unpushed, 0);
    work(&task.worktree, "new.txt", "newer\n", Some("Address review"));
    assert_eq!(f.tasks.stats(&task).unpushed, 1);
    f.tasks.push(&task, "origin").unwrap();
    assert_eq!(f.tasks.stats(&task).unpushed, 0);
}

#[test]
fn settle_cleans_up_after_a_merged_pull_request_and_can_be_undone() {
    let f = fixture();
    let task = f.tasks.create("Merged elsewhere", "main").unwrap();
    work(&task.worktree, "app.txt", "done\n", Some("Do it"));
    f.tasks.link_pull(&task, 12).unwrap();
    let task = f.tasks.get(&task.id).unwrap();
    assert_eq!(task.pull, Some(12));

    work(&task.worktree, "stray.txt", "stray\n", None);
    let err = f.tasks.settle(&task, "abc123").unwrap_err();
    assert!(err.contains("uncommitted"), "{err}");
    std::fs::remove_file(task.worktree.join("stray.txt")).unwrap();

    let undo = f.tasks.settle(&task, "abc123").unwrap();
    assert!(!task.worktree.exists());
    assert!(f.repo.resolve("refs/heads/agent/merged-elsewhere").is_err());
    let settled = f.tasks.get(&task.id).unwrap();
    assert_eq!(settled.state, TaskState::Landed("abc123".into()));
    assert_eq!(settled.pull, Some(12));

    f.tasks.restore(&undo).unwrap();
    let back = f.tasks.get(&task.id).unwrap();
    assert_eq!(back.state, TaskState::Open);
    assert_eq!(back.pull, Some(12));
    assert!(task.worktree.join("app.txt").exists());
}

#[test]
fn close_cleans_up_after_a_closed_pull_request_and_can_be_undone() {
    let f = fixture();
    let task = f.tasks.create("Closed elsewhere", "main").unwrap();
    work(&task.worktree, "app.txt", "done\n", Some("Do it"));
    f.tasks.link_pull(&task, 13).unwrap();
    let task = f.tasks.get(&task.id).unwrap();

    let undo = f.tasks.close(&task).unwrap();
    assert!(!task.worktree.exists());
    assert!(f.repo.resolve("refs/heads/agent/closed-elsewhere").is_err());
    let closed = f.tasks.get(&task.id).unwrap();
    assert_eq!(closed.state, TaskState::Closed);
    assert_eq!(closed.pull, Some(13));
    let err = f.tasks.close(&closed).unwrap_err();
    assert!(err.contains("already settled"), "{err}");

    // Closed tasks sort after open ones.
    let open = f.tasks.create("Still going", "main").unwrap();
    let ids: Vec<String> = f.tasks.list().into_iter().map(|t| t.id).collect();
    assert_eq!(ids, [open.id.clone(), task.id.clone()]);

    f.tasks.restore(&undo).unwrap();
    let back = f.tasks.get(&task.id).unwrap();
    assert_eq!(back.state, TaskState::Open);
    assert!(task.worktree.join("app.txt").exists());
}

#[test]
fn tasks_start_from_origin_and_land_once_the_base_is_pulled() {
    let f = fixture();
    let root = f.repo.root().to_path_buf();
    let remote = f.dir.join("remote.git");
    git(
        &f.dir,
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "--quiet", "-u", "origin", "main"]);
    // Someone else pushes to main; this checkout has fetched but not pulled.
    let other = f.dir.join("other");
    git(
        &f.dir,
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    work(&other, "theirs.txt", "theirs\n", Some("Their change"));
    git(&other, &["push", "--quiet", "origin", "main"]);
    git(&root, &["fetch", "--quiet", "origin"]);
    assert_eq!(f.repo.default_start().as_deref(), Some("origin/main"));

    let task = f.tasks.create("Add a thing", "origin/main").unwrap();
    assert_eq!(task.base, "main");
    assert_eq!(task.from.as_deref(), Some("origin/main"));
    assert_eq!(
        f.tasks.get(&task.id).unwrap().from.as_deref(),
        Some("origin/main")
    );
    assert!(
        task.worktree.join("theirs.txt").exists(),
        "starts from origin's main"
    );
    // Its work is only its own commits, not origin's.
    work(&task.worktree, "mine.txt", "mine\n", Some("My change"));
    let stats = f.tasks.stats(&task);
    assert_eq!((stats.commits, stats.base_moved), (1, false));

    // Landing on a main that's behind would carry origin's commit too.
    let err = f.tasks.land(&task, LandMode::Squash, false).unwrap_err();
    assert!(err.contains("pull main first"), "{err}");
    git(&root, &["merge", "--quiet", "--ff-only", "origin/main"]);
    f.tasks.land(&task, LandMode::Squash, false).unwrap();
    assert_eq!(subjects(&root), ["Add a thing", "Their change", "start"]);
}

#[test]
fn a_local_base_is_taken_as_is() {
    let f = fixture();
    assert_eq!(f.repo.default_start().as_deref(), Some("main"), "no origin");
    let task = f.tasks.create("Add a thing", "main").unwrap();
    assert_eq!((task.base.as_str(), task.from), ("main", None));
}
