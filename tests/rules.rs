//! Rules worth writing down (#107): corrections collected across tasks, and
//! the line added to CLAUDE.md.

use spor::agent::{AgentKind, Mode, Permissions, RunState, Runs};
use spor::git::Repo;
use spor::rules;
use spor::tasks::{Task, Tasks};
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

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-rules-{}-{}",
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

/// A task whose agent ran once per prompt (each prompt after the first is a follow-up).
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
fn follow_ups_across_tasks_are_collected_without_the_first_prompts() {
    let f = fixture();
    let a = task_with(
        &f,
        &["Add a flag", "use pnpm, not npm", "use pnpm, not npm"],
    );
    let b = task_with(&f, &["Fix the docs", "we use pnpm here"]);
    let found = rules::collect(&f.tasks);
    let texts: Vec<(&str, &str)> = found
        .iter()
        .map(|c| (c.task.as_str(), c.text.as_str()))
        .collect();
    assert!(texts.contains(&(a.id.as_str(), "use pnpm, not npm")));
    assert!(texts.contains(&(b.id.as_str(), "we use pnpm here")));
    assert!(!texts
        .iter()
        .any(|(_, t)| *t == "Add a flag" || *t == "Fix the docs"));
    assert_eq!(
        texts.iter().filter(|(t, _)| *t == a.id).count(),
        1,
        "the same message twice in a row counts once"
    );
}

#[test]
fn adding_a_rule_edits_claude_md_and_it_is_not_suggested_again() {
    let f = fixture();
    // No CLAUDE.md yet: it is made. An AGENTS.md is used when that's what there is.
    assert_eq!(rules::rules_file(&f.root), f.root.join("CLAUDE.md"));
    std::fs::write(f.root.join("AGENTS.md"), "# Agents\nBe kind.").unwrap();
    assert_eq!(rules::rules_file(&f.root), f.root.join("AGENTS.md"));

    let path = rules::add(&f.tasks, "Use pnpm, not npm.").unwrap();
    assert_eq!(path, f.root.join("AGENTS.md"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# Agents\nBe kind.\n- Use pnpm, not npm.\n"
    );
    rules::decline(&f.tasks, "Tests go beside the file.").unwrap();
    assert!(rules::add(&f.tasks, "  ").is_err());

    // Added and turned-down rules are left out of what's suggested next.
    let suggest = |rule: &str| rules::Suggestion {
        rule: rule.into(),
        tasks: vec!["a".into(), "b".into()],
    };
    let left = rules::without_known(
        &f.tasks,
        vec![
            suggest("use pnpm, not npm"),
            suggest("Tests go beside the file"),
            suggest("Never edit the generated client."),
        ],
    );
    assert_eq!(left, vec![suggest("Never edit the generated client.")]);
}
