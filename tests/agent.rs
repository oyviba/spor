//! Agents running in task folders: real processes, a fake `claude`.

use spor::agent::{AgentKind, Entry, Permissions, RunState, Runs};
use spor::checkpoint;
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
        // A new session id per run (as --fork-session gives), counted in
        // the worktree's own git dir, where restoring a checkpoint can't
        // reach it.
        let script = r#"#!/bin/sh
printf '%s\n' "$@" > claude-args.txt
n_file="$(git rev-parse --git-dir)/fake-claude-n"
n=$(( $(cat "$n_file" 2>/dev/null || echo 41) + 1 )); echo $n > "$n_file"
echo "agent was here $n" >> agent.txt
echo '{"type":"system","subtype":"init","session_id":"sess-'$n'"}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"agent.txt"}}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Wrote agent.txt."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Done.","total_cost_usd":0.01,"num_turns":2,"duration_ms":900,"session_id":"sess-'$n'"}'
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

const NO_COMMANDS: Permissions = Permissions {
    mode: spor::agent::Mode::AcceptEdits,
};

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
    // The wrapper went down with the agent; stop took the after checkpoint.
    assert!(checkpoint::resolve(&task.worktree, &runs.checkpoint_ref(0, "after")).is_some());
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
    // Each follow-up forks the session, so the one resumed stays as it
    // was (undoing a run resumes from there).
    assert!(
        args.windows(3)
            .any(|w| w == ["--resume", "sess-42", "--fork-session"]),
        "{args:?}"
    );
    assert_eq!(runs.session().as_deref(), Some("sess-43"));
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

// ---- Checkpoints ----------------------------------------------------------

fn read(dir: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(path)).ok()
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_checkpoint_holds_everything_and_restores_it() {
    let f = fixture();
    let root = f.repo.root().to_path_buf();
    std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "-qm", "ignore build"]);
    let head = git_out(&root, &["rev-parse", "HEAD"]);
    // Staged, unstaged, untracked and ignored work.
    std::fs::write(root.join("app.txt"), "one\nstaged\n").unwrap();
    git(&root, &["add", "app.txt"]);
    std::fs::write(root.join("app.txt"), "one\nstaged\nunstaged\n").unwrap();
    std::fs::write(root.join("new.txt"), "untracked\n").unwrap();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join("build/out.bin"), "ignored\n").unwrap();
    let staged = git_out(&root, &["diff", "--cached"]);

    let snap = checkpoint::capture(&root, "refs/spor/test/snap").unwrap();
    assert_eq!(
        checkpoint::resolve(&root, "refs/spor/test/snap").as_deref(),
        Some(snap.as_str())
    );
    assert_eq!(
        git_out(&root, &["diff", "--cached"]),
        staged,
        "the index is left alone"
    );
    assert_eq!(
        git_out(&root, &["rev-parse", "HEAD"]),
        head,
        "no commit on the branch"
    );
    let files = git_out(&root, &["ls-tree", "-r", "--name-only", &snap]);
    assert!(
        files.contains("new.txt") && !files.contains("build/"),
        "{files}"
    );
    assert_eq!(
        git_out(&root, &["show", &format!("{snap}:app.txt")]),
        "one\nstaged\nunstaged"
    );

    // Move on: commit, delete, add files; then go back.
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "later"]);
    std::fs::remove_file(root.join("new.txt")).unwrap();
    std::fs::write(root.join("app.txt"), "rewritten\n").unwrap();
    std::fs::write(root.join("stray.txt"), "made later\n").unwrap();
    checkpoint::restore(&root, &snap).unwrap();
    assert_eq!(
        git_out(&root, &["rev-parse", "HEAD"]),
        head,
        "the branch is back"
    );
    assert_eq!(
        read(&root, "app.txt").as_deref(),
        Some("one\nstaged\nunstaged\n")
    );
    assert_eq!(read(&root, "new.txt").as_deref(), Some("untracked\n"));
    assert!(
        read(&root, "stray.txt").is_none(),
        "files made later are gone"
    );
    assert_eq!(
        read(&root, "build/out.bin").as_deref(),
        Some("ignored\n"),
        "ignored files stay"
    );
}

fn run_command(task: &spor::tasks::Task, runs: &Runs, command: &str) -> usize {
    let run = runs
        .start(
            task,
            &AgentKind::Command(command.into()),
            command,
            None,
            NO_COMMANDS,
        )
        .unwrap();
    assert_eq!(wait_until_done(runs), RunState::Finished);
    run.index
}

#[test]
fn each_run_has_its_own_diff() {
    let f = fixture();
    let task = f.tasks.create("Notes", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let first = run_command(&task, &runs, "echo first > note.txt");
    let second = run_command(&task, &runs, "echo second >> note.txt; echo two > app.txt");
    for which in ["before", "after"] {
        assert!(checkpoint::resolve(&task.worktree, &runs.checkpoint_ref(second, which)).is_some());
    }
    let one = runs.patch(first).unwrap().patch;
    assert!(one.contains("+first") && !one.contains("app.txt"), "{one}");
    let two = runs.patch(second).unwrap().patch;
    assert!(
        two.contains("+second") && !two.contains("+first") && two.contains("+two"),
        "{two}"
    );
}

#[test]
fn a_rebase_onto_the_base_is_left_out_of_the_runs_diff() {
    let f = fixture();
    let task = f.tasks.create("Notes", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    run_command(
        &task,
        &runs,
        "echo mine > note.txt; git add -A; git commit -qm note",
    );
    // main moves on; the next run rebases onto it and edits its own file.
    let root = f.repo.root().to_path_buf();
    std::fs::write(root.join("other.txt"), "from main\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "main moves"]);
    let rebase = run_command(&task, &runs, "git rebase -q main && echo more >> note.txt");
    let p = runs.patch(rebase).unwrap();
    assert_eq!(p.from_base, vec!["other.txt".to_string()]);
    assert!(
        p.patch.contains("+more") && !p.patch.contains("other.txt"),
        "{}",
        p.patch
    );
}

#[test]
fn the_last_run_can_be_undone_and_redone() {
    fake_claude();
    let f = fixture();
    let task = f.tasks.create("Leave marks", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let wt = &task.worktree;
    runs.start(&task, &AgentKind::Claude, "First", None, NO_COMMANDS)
        .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    let after_first = read(wt, "agent.txt").unwrap();
    let first_session = runs.session().unwrap();
    runs.start(
        &task,
        &AgentKind::Claude,
        "Second",
        Some(&first_session),
        NO_COMMANDS,
    )
    .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    assert_ne!(runs.session().unwrap(), first_session);
    assert_ne!(read(wt, "agent.txt").unwrap(), after_first);

    assert_eq!(runs.undo_last().unwrap(), 1);
    assert_eq!(
        read(wt, "agent.txt").unwrap(),
        after_first,
        "the folder is as the first run left it"
    );
    assert!(runs.list()[1].undone);
    assert_eq!(
        runs.session().unwrap(),
        first_session,
        "follow-ups resume from before the undone run"
    );

    runs.redo(1).unwrap();
    assert!(!runs.list()[1].undone);
    assert_ne!(read(wt, "agent.txt").unwrap(), after_first);
    assert_ne!(runs.session().unwrap(), first_session);

    // Undo twice: back to before the first run.
    runs.undo_last().unwrap();
    runs.undo_last().unwrap();
    assert!(read(wt, "agent.txt").is_none());
    assert!(runs.undoable().is_none());
}

#[test]
fn forgetting_a_task_deletes_its_checkpoints() {
    let f = fixture();
    let task = f.tasks.create("Note", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let run = run_command(&task, &runs, "echo hi > note.txt");
    let before = runs.checkpoint_ref(run, "before");
    assert!(checkpoint::resolve(f.repo.root(), &before).is_some());
    f.tasks.forget(&task.id).unwrap();
    assert!(checkpoint::resolve(f.repo.root(), &before).is_none());
}

// ---- Setup scripts (#23) ----------------------------------------------------

/// Commit `.spor/<name>` with `content` in the main checkout.
fn commit_spor_file(f: &Fixture, name: &str, content: &str) {
    let root = f.repo.root();
    std::fs::create_dir_all(root.join(".spor")).unwrap();
    std::fs::write(root.join(".spor").join(name), content).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", &format!("Add .spor/{name}")]);
}

#[test]
fn the_setup_script_runs_before_the_first_agent_only() {
    let f = fixture();
    commit_spor_file(
        &f,
        "setup",
        "echo \"setting up $SPOR_TASK from $SPOR_BASE\"\necho ready > prepared.txt\necho x >> setups.txt\ntest -d \"$SPOR_REPO/.git\"\n",
    );
    let task = f.tasks.create("Use the setup", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    assert!(runs.setup_script(&task).is_some());
    run_command(&task, &runs, "cat prepared.txt > seen.txt");
    assert_eq!(
        read(&task.worktree, "seen.txt").as_deref(),
        Some("ready\n"),
        "the agent ran after setup"
    );
    let t = runs.transcript();
    let start = t
        .iter()
        .position(|e| {
            *e == Entry::Setup {
                ok: None,
                code: None,
            }
        })
        .unwrap();
    let done = t
        .iter()
        .position(|e| {
            *e == Entry::Setup {
                ok: Some(true),
                code: Some(0),
            }
        })
        .unwrap();
    assert!(start < done);
    assert!(
        t[start..done].contains(&Entry::Output(format!("setting up {} from main", task.id))),
        "{t:?}"
    );
    // Later runs don't set up again.
    assert!(runs.setup_script(&task).is_none());
    run_command(&task, &runs, "true");
    assert_eq!(read(&task.worktree, "setups.txt").as_deref(), Some("x\n"));
}

#[test]
fn a_failing_setup_stops_the_agent_until_skipped() {
    let f = fixture();
    commit_spor_file(&f, "setup", "echo 'npm: not found'\nexit 3\n");
    let task = f.tasks.create("Broken setup", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    runs.start(
        &task,
        &AgentKind::Command("echo ran > ran.txt".into()),
        "x",
        None,
        NO_COMMANDS,
    )
    .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Failed(3));
    assert!(
        read(&task.worktree, "ran.txt").is_none(),
        "the agent didn't run"
    );
    let t = runs.transcript();
    assert!(t.contains(&Entry::Setup {
        ok: Some(false),
        code: Some(3)
    }));
    assert!(t.contains(&Entry::Output("npm: not found".into())));
    // Retrying runs setup again; skipping it lets the agent start.
    assert!(runs.setup_script(&task).is_some());
    runs.skip_setup().unwrap();
    run_command(&task, &runs, "echo ran > ran.txt");
    assert_eq!(read(&task.worktree, "ran.txt").as_deref(), Some("ran\n"));
}

#[test]
fn a_private_setup_script_wins() {
    let f = fixture();
    commit_spor_file(&f, "setup", "echo committed > which.txt\n");
    let private = f.repo.root().join(".git/spor");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("setup"), "echo private > which.txt\n").unwrap();
    let task = f.tasks.create("Which setup", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    run_command(&task, &runs, "true");
    assert_eq!(
        read(&task.worktree, "which.txt").as_deref(),
        Some("private\n")
    );
}

#[test]
fn listed_ignored_files_are_copied_into_a_new_folder() {
    let f = fixture();
    let root = f.repo.root().to_path_buf();
    std::fs::write(root.join(".gitignore"), ".env\nconfig/local.toml\n").unwrap();
    commit_spor_file(&f, "copy", "# secrets the checkout doesn't bring\n.env\nconfig/local.toml\nmissing.txt\n../outside.txt\n");
    std::fs::write(root.join(".env"), "TOKEN=1\n").unwrap();
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::write(root.join("config/local.toml"), "debug = true\n").unwrap();
    std::fs::write(f.dir.join("outside.txt"), "nope\n").unwrap();
    let task = f.tasks.create("Copy secrets", "main").unwrap();
    assert_eq!(read(&task.worktree, ".env").as_deref(), Some("TOKEN=1\n"));
    assert_eq!(
        read(&task.worktree, "config/local.toml").as_deref(),
        Some("debug = true\n")
    );
    assert!(read(&task.worktree, "missing.txt").is_none());
    assert!(
        !task.worktree.parent().unwrap().join("outside.txt").exists(),
        "paths outside the checkout are refused"
    );
}

// ---- The rules both apps follow (#39) ----------------------------------

#[test]
fn a_follow_up_goes_to_the_agent_that_did_the_work() {
    fake_claude();
    let f = fixture();
    let task = f.tasks.create("Leave a mark", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    // Starting needs an agent picked; the task's prompt goes as is.
    assert_eq!(
        runs.plan(&task, None, None).unwrap_err(),
        "Pick an agent first"
    );
    let first = runs.plan(&task, None, Some(&AgentKind::Claude)).unwrap();
    assert_eq!(first.kind, AgentKind::Claude);
    assert_eq!(first.prompt, "Leave a mark");
    assert_eq!(first.resume, None);
    runs.follow_up(&task, None, Some(&AgentKind::Claude), NO_COMMANDS)
        .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);

    // Whatever is picked now, feedback goes to Claude, into its session.
    let other = AgentKind::Command("true".into());
    let plan = runs
        .plan(&task, Some("  Now say hello  "), Some(&other))
        .unwrap();
    assert_eq!(plan.kind, AgentKind::Claude);
    assert_eq!(plan.prompt, "Now say hello");
    assert_eq!(plan.resume.as_deref(), Some("sess-42"));
}

#[test]
fn feedback_without_a_session_sends_the_whole_task_again() {
    let f = fixture();
    let task = f.tasks.create("Write a note", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let kind = AgentKind::Command("echo \"$SPOR_PROMPT\" > note.txt".into());
    runs.follow_up(&task, None, Some(&kind), NO_COMMANDS)
        .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    let plan = runs.plan(&task, Some("Shorter, please"), None).unwrap();
    assert_eq!(plan.kind, kind);
    assert_eq!(plan.resume, None);
    assert_eq!(
        plan.prompt,
        "Write a note\n\nFeedback on the work so far:\nShorter, please"
    );
    runs.follow_up(&task, Some("Shorter, please"), None, NO_COMMANDS)
        .unwrap();
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    assert_eq!(
        std::fs::read_to_string(task.worktree.join("note.txt")).unwrap(),
        "Write a note\n\nFeedback on the work so far:\nShorter, please\n"
    );
}

#[test]
fn an_agent_that_isnt_there_is_refused() {
    let f = fixture();
    let task = f.tasks.create("Anything", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let empty = AgentKind::Command("  ".into());
    assert!(spor::agent::missing(&empty).is_some());
    assert!(runs.plan(&task, None, Some(&empty)).is_err());
    assert!(runs.latest().is_none());
    assert_eq!(
        spor::agent::missing(&AgentKind::Command("codex exec {prompt}".into())),
        None
    );
}

#[test]
fn line_comments_read_file_line_comment() {
    use spor::agent::{with_comments, LineComment};
    let one = LineComment {
        path: "src/auth.rs".into(),
        start: 42,
        end: 42,
        removed: false,
        text: "use the existing helper ".into(),
    };
    let gone = LineComment {
        path: "src/old.rs".into(),
        start: 9,
        end: 7,
        removed: true,
        text: "keep these".into(),
    };
    assert_eq!(one.place(), "src/auth.rs:42");
    assert_eq!(gone.place(), "src/old.rs:7–9 (removed lines)");
    assert_eq!(
        LineComment {
            end: 9,
            start: 9,
            ..gone.clone()
        }
        .place(),
        "src/old.rs:9 (removed line)"
    );
    assert_eq!(
        with_comments(" Looks close. ", &[one.clone(), gone]),
        "Looks close.\n\nComments on the changes:\n\
         - src/auth.rs:42: use the existing helper\n\
         - src/old.rs:7–9 (removed lines): keep these"
    );
    assert_eq!(
        with_comments("", &[one]),
        "Comments on the changes:\n- src/auth.rs:42: use the existing helper"
    );
    assert_eq!(with_comments(" just text ", &[]), "just text");
}

#[test]
fn the_rebase_request_names_the_base_and_the_files() {
    let m = spor::agent::rebase_request("main", &["a.rs".into(), "b.rs".into()]);
    assert!(m.starts_with("main has moved on and this branch now conflicts with it in a.rs, b.rs."));
    assert!(m.contains("git rebase main"));
}

#[test]
fn land_and_discard_say_what_they_did() {
    let f = fixture();
    let task = f.tasks.create("Throw away", "main").unwrap();
    let undo = f.tasks.discard(&task).unwrap();
    assert_eq!(undo.label(), format!("Discarded {}", task.id));
    f.tasks.restore(&undo).unwrap();
    std::fs::write(task.worktree.join("x.txt"), "x\n").unwrap();
    let task = f.tasks.get(&task.id).unwrap();
    let undo = f
        .tasks
        .land(&task, spor::tasks::LandMode::Squash, true)
        .unwrap();
    assert_eq!(undo.label(), format!("Landed {} on main", task.id));
}

// ---- Follow-ups from #19–#23 (#41) ---------------------------------------

#[test]
fn queued_follow_ups_are_kept_with_the_task() {
    let f = fixture();
    let task = f.tasks.create("Queue", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    assert!(runs.queued().is_empty());
    runs.queue("first\nwith two lines").unwrap();
    runs.queue("second").unwrap();
    runs.queue("third").unwrap();
    // Another app (another Runs) sees the same queue.
    let other = Runs::of(&f.tasks, &task);
    assert_eq!(other.queued(), ["first\nwith two lines", "second", "third"]);
    other.unqueue(1).unwrap();
    assert!(other.unqueue(5).is_err());
    assert_eq!(runs.take_queued().as_deref(), Some("first\nwith two lines"));
    assert_eq!(runs.take_queued().as_deref(), Some("third"));
    assert_eq!(runs.take_queued(), None);
    assert!(!f.tasks.run_dir(&task.id).join("queue").exists());
}

#[test]
fn undo_from_an_earlier_run_takes_back_everything_after_it() {
    let f = fixture();
    let task = f.tasks.create("Marks", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let wt = &task.worktree;
    run_command(&task, &runs, "echo 1 > one.txt");
    run_command(&task, &runs, "echo 2 > two.txt");
    run_command(&task, &runs, "echo 3 > three.txt");
    assert_eq!(runs.undoable_from(), [0, 1, 2]);

    // The last on its own, then from the second: the second batch is runs
    // 1 (and 2, already undone).
    runs.undo_last().unwrap();
    assert_eq!(runs.undo_from(1).unwrap(), [1]);
    assert!(read(wt, "one.txt").is_some());
    assert!(read(wt, "two.txt").is_none());
    assert!(read(wt, "three.txt").is_none());
    assert_eq!(runs.undoable_from(), [0]);
    assert!(runs.undo_from(2).is_err(), "already undone");

    // Redo brings back that batch only.
    runs.redo(1).unwrap();
    assert!(read(wt, "two.txt").is_some());
    assert!(read(wt, "three.txt").is_none());
    let undone: Vec<bool> = runs.list().iter().map(|r| r.undone).collect();
    assert_eq!(undone, [false, false, true]);

    // From the first: everything.
    runs.redo(2).unwrap();
    assert_eq!(runs.undo_from(0).unwrap(), [0, 1, 2]);
    assert!(read(wt, "one.txt").is_none() && read(wt, "three.txt").is_none());
    runs.redo(0).unwrap();
    assert!(read(wt, "one.txt").is_some() && read(wt, "three.txt").is_some());
    assert!(runs.list().iter().all(|r| !r.undone));
}

#[test]
fn cleaning_up_removes_landed_and_discarded_tasks_checkpoints() {
    let f = fixture();
    let open = f.tasks.create("Keep", "main").unwrap();
    let landed = f.tasks.create("Land", "main").unwrap();
    let gone = f.tasks.create("Throw", "main").unwrap();
    for t in [&open, &landed, &gone] {
        run_command(t, &Runs::of(&f.tasks, t), &format!("echo x > {}.txt", t.id));
    }
    let landed_ref = Runs::of(&f.tasks, &landed).checkpoint_ref(0, "before");
    let gone_ref = Runs::of(&f.tasks, &gone).checkpoint_ref(0, "before");
    let open_ref = Runs::of(&f.tasks, &open).checkpoint_ref(0, "before");
    f.tasks
        .land(&landed, spor::tasks::LandMode::Squash, true)
        .unwrap();
    f.tasks.discard(&gone).unwrap();
    // Until cleaned up, an undo still has everything.
    assert!(checkpoint::resolve(f.repo.root(), &gone_ref).is_some());
    assert!(f.tasks.run_dir(&gone.id).exists());

    assert_eq!(f.tasks.clean_up(), 2);
    assert!(checkpoint::resolve(f.repo.root(), &landed_ref).is_none());
    assert!(checkpoint::resolve(f.repo.root(), &gone_ref).is_none());
    assert!(!f.tasks.run_dir(&gone.id).exists());
    // A landed task keeps its transcript; an open one keeps everything.
    assert!(f.tasks.run_dir(&landed.id).exists());
    assert!(checkpoint::resolve(f.repo.root(), &open_ref).is_some());
    assert_eq!(f.tasks.clean_up(), 0);
}

#[test]
fn the_setup_script_reports_how_it_went() {
    use spor::agent::SetupState;
    let f = fixture();
    commit_spor_file(&f, "setup", "echo ok\n");
    let task = f.tasks.create("Setup status", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let setup = runs.setup(&task);
    assert_eq!(setup.script.as_deref(), Some(Path::new(".spor/setup")));
    assert_eq!(setup.state, None);
    run_command(&task, &runs, "true");
    assert_eq!(runs.setup(&task).state, Some(SetupState::Worked));
    // Run again: due before the next agent, and it runs.
    runs.rerun_setup().unwrap();
    assert_eq!(runs.setup(&task).state, Some(SetupState::Due));
    assert!(runs.setup_script(&task).is_some());
    run_command(&task, &runs, "true");
    assert_eq!(runs.setup(&task).state, Some(SetupState::Worked));
    runs.skip_setup().unwrap();
    assert_eq!(runs.setup(&task).state, Some(SetupState::Skipped));
}

#[test]
fn a_queued_follow_up_goes_once_the_agent_is_idle() {
    let f = fixture();
    let task = f.tasks.create("Write a note", "main").unwrap();
    let runs = Runs::of(&f.tasks, &task);
    let kind = AgentKind::Command("sleep 0.3; echo \"$SPOR_PROMPT\" > note.txt".into());
    runs.follow_up(&task, None, Some(&kind), NO_COMMANDS)
        .unwrap();
    runs.queue("Shorter").unwrap();
    // Still working: it waits.
    assert!(!runs.send_queued(&task, None, NO_COMMANDS).unwrap());
    assert_eq!(runs.queued(), ["Shorter"]);
    wait_until_done(&runs);
    assert!(runs.send_queued(&task, None, NO_COMMANDS).unwrap());
    assert!(runs.queued().is_empty());
    assert_eq!(wait_until_done(&runs), RunState::Finished);
    assert_eq!(
        read(&task.worktree, "note.txt").as_deref(),
        Some("Write a note\n\nFeedback on the work so far:\nShorter\n")
    );
    assert!(!runs.send_queued(&task, None, NO_COMMANDS).unwrap());
}
