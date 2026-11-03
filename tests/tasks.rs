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
    assert_eq!((stats.additions, stats.deletions), (2, 1));
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
