//! Searching transcripts, and finding the task behind a commit (#108).

use spor::agent::{AgentKind, Mode, Permissions, RunState, Runs};
use spor::git::Repo;
use spor::search::{self, Where};
use spor::tasks::{LandMode, Task, Tasks};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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
    assert!(out.status.success(), "git {args:?}");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-search-{}-{}",
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
    let tasks = Tasks::open(&Repo::new(&root)).unwrap();
    Fixture { dir, root, tasks }
}

const ASK: Permissions = Permissions {
    mode: Mode::AcceptEdits,
};

fn task_with(f: &Fixture, prompts: &[&str]) -> Task {
    let task = f
        .tasks
        .create(prompts[0], "main")
        .and_then(|t| f.tasks.start_branch(&t))
        .unwrap();
    let runs = Runs::of(&f.tasks, &task);
    for prompt in prompts {
        runs.start(&task, &AgentKind::Command("true".into()), prompt, None, ASK)
            .unwrap();
        let start = Instant::now();
        while runs.latest().unwrap().state == RunState::Running
            && start.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    task
}

#[test]
fn prompts_and_follow_ups_are_found_with_the_run_they_were_in() {
    let f = fixture();
    let a = task_with(&f, &["Add a flag", "drop the old endpoint, we decided"]);
    let b = task_with(&f, &["Fix the docs"]);

    let hits = search::transcripts(&f.tasks, "DROP endpoint");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].task, a.id);
    assert_eq!((hits[0].run, hits[0].kind), (1, Where::Prompt));
    assert!(hits[0].snippet.contains("drop the old endpoint"));

    let docs = search::transcripts(&f.tasks, "docs");
    assert_eq!((docs[0].task.as_str(), docs[0].run), (b.id.as_str(), 0));
    assert!(search::transcripts(&f.tasks, "  ").is_empty());
    assert!(search::transcripts(&f.tasks, "nothing like this").is_empty());
}

#[test]
fn a_landed_task_keeps_its_conversation_and_its_commit_names_it() {
    let f = fixture();
    let task = task_with(&f, &["Reword the app text"]);
    std::fs::write(task.worktree.join("app.txt"), "two\n").unwrap();
    git(&task.worktree, &["add", "."]);
    git(&task.worktree, &["commit", "--quiet", "-m", "Reword"]);
    f.tasks.land(&task, LandMode::Squash, false).unwrap();

    // Cleaning up after the done task keeps what was said.
    f.tasks.clean_up();
    let hits = search::transcripts(&f.tasks, "reword");
    assert!(hits.iter().any(|h| h.task == task.id), "{hits:?}");

    let message = git_out(&f.root, &["log", "-1", "--format=%B"]);
    assert_eq!(Task::trailer_id(&message), Some(task.id.as_str()));
    assert_eq!(Task::trailer_id("Just a commit\n\nNo trailer"), None);
    assert_eq!(Task::trailer_id("Subject\n\nSpor-Task:  \n"), None);
}
