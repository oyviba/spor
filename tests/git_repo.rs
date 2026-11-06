//! The git layer against real repositories in temporary directories.

use spor::git::{
    ConflictSide, DiffKind, FileStatus, LineAction, Operation, Outcome, RebaseStep, Repo, ResetMode,
};
use spor::patch::{LinePos, Side};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A throwaway repository, deleted when dropped.
struct Fixture {
    dir: PathBuf,
    repo: Repo,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    for (k, v) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    Fixture {
        repo: Repo::new(&root),
        dir,
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

impl Fixture {
    fn write(&self, path: &str, text: &str) {
        let full = self.repo.path(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.repo.path(path)).unwrap()
    }

    fn commit_file(&self, path: &str, text: &str, message: &str) -> String {
        self.write(path, text);
        self.repo.stage(path).unwrap();
        self.repo.commit(message).unwrap();
        self.repo.head_sha().unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        git(self.repo.root(), args)
    }

    fn subjects(&self) -> Vec<String> {
        self.git(&["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn index_contents(&self, path: &str) -> String {
        self.git(&["show", &format!(":{path}")])
    }
}

#[test]
fn commands_run_in_their_own_repository() {
    let a = fixture();
    let b = fixture();
    a.commit_file("a.txt", "a\n", "in a");
    b.commit_file("b.txt", "b\n", "in b");
    // Whatever the process's directory is, each handle sees only its own.
    assert_eq!(a.subjects(), ["in a"]);
    assert_eq!(b.subjects(), ["in b"]);
    assert_eq!(a.repo.log_all(10).unwrap()[0].subject, "in a");
    assert_eq!(b.repo.log_all(10).unwrap()[0].subject, "in b");
}

#[test]
fn discover_finds_the_top_level() {
    let f = fixture();
    f.write("sub/dir/x.txt", "x\n");
    let found = Repo::discover(&f.repo.path("sub/dir")).unwrap();
    assert_eq!(
        found.root().canonicalize().unwrap(),
        f.repo.root().canonicalize().unwrap()
    );
}

#[test]
fn stage_some_lines_of_a_file() {
    let f = fixture();
    f.commit_file("f.txt", "one\ntwo\nthree\nfour\n", "start");
    f.write("f.txt", "one\nTWO\nthree\nFOUR\n");
    // Stage only the second change: "-four" (old 4) through "+FOUR" (new 4).
    f.repo
        .apply_lines(
            "f.txt",
            DiffKind::Unstaged,
            LineAction::Stage,
            LinePos::new(Side::Old, 4),
            LinePos::new(Side::New, 4),
        )
        .unwrap();
    assert_eq!(f.index_contents("f.txt"), "one\ntwo\nthree\nFOUR\n");
    assert_eq!(f.read("f.txt"), "one\nTWO\nthree\nFOUR\n");
}

#[test]
fn unstage_and_discard_lines_and_undo() {
    let f = fixture();
    f.commit_file("f.txt", "a\nb\nc\n", "start");
    f.write("f.txt", "A\nb\nC\n");
    f.repo.stage("f.txt").unwrap();
    // Unstage the change to the last line, as numbered in the staged diff.
    f.repo
        .apply_lines(
            "f.txt",
            DiffKind::Staged,
            LineAction::Unstage,
            LinePos::new(Side::Old, 3),
            LinePos::new(Side::New, 3),
        )
        .unwrap();
    assert_eq!(f.index_contents("f.txt"), "A\nb\nc\n");
    // Now discard it from the working tree, then take that back.
    let applied = f
        .repo
        .apply_lines(
            "f.txt",
            DiffKind::Unstaged,
            LineAction::Discard,
            LinePos::new(Side::Old, 3),
            LinePos::new(Side::New, 3),
        )
        .unwrap();
    assert_eq!(f.read("f.txt"), "A\nb\nc\n");
    f.repo.unapply(&applied).unwrap();
    assert_eq!(f.read("f.txt"), "A\nb\nC\n");
}

#[test]
fn line_actions_check_which_diff_they_came_from() {
    let f = fixture();
    f.commit_file("f.txt", "a\n", "start");
    f.write("f.txt", "b\n");
    let pos = LinePos::new(Side::New, 1);
    assert!(f
        .repo
        .apply_lines("f.txt", DiffKind::Staged, LineAction::Stage, pos, pos)
        .is_err());
}

#[test]
fn stage_part_of_an_untracked_file() {
    let f = fixture();
    f.commit_file("keep.txt", "k\n", "start");
    f.write("new.txt", "first\nsecond\n");
    f.repo
        .apply_lines(
            "new.txt",
            DiffKind::Unstaged,
            LineAction::Stage,
            LinePos::new(Side::New, 1),
            LinePos::new(Side::New, 1),
        )
        .unwrap();
    assert_eq!(f.index_contents("new.txt"), "first\n");
}

#[test]
fn amend_replaces_the_last_commit() {
    let f = fixture();
    f.commit_file("f.txt", "1\n", "first");
    f.commit_file("f.txt", "2\n", "secnod");
    assert_eq!(f.repo.head_message().unwrap(), "secnod");
    f.write("g.txt", "g\n");
    f.repo.stage("g.txt").unwrap();
    f.repo.amend(Some("second")).unwrap();
    assert_eq!(f.subjects(), ["second", "first"]);
    assert!(f
        .git(&["show", "--name-only", "--format="])
        .contains("g.txt"));
    f.repo.amend(None).unwrap();
    assert_eq!(f.subjects(), ["second", "first"]);
}

#[test]
fn cherry_pick_and_revert() {
    let f = fixture();
    f.commit_file("base.txt", "base\n", "base");
    f.git(&["switch", "--quiet", "-c", "feature"]);
    let pick = f.commit_file("feature.txt", "feature\n", "add feature");
    f.git(&["switch", "--quiet", "main"]);
    assert_eq!(f.repo.cherry_pick(&pick).unwrap(), Outcome::Done);
    assert_eq!(f.subjects()[0], "add feature");
    assert_eq!(f.read("feature.txt"), "feature\n");
    let head = f.repo.head_sha().unwrap();
    assert_eq!(f.repo.revert(&head).unwrap(), Outcome::Done);
    assert!(f.subjects()[0].starts_with("Revert"));
    assert!(!f.repo.path("feature.txt").exists());
}

fn conflicting_branches(f: &Fixture) {
    f.commit_file("f.txt", "base\n", "base");
    f.git(&["switch", "--quiet", "-c", "other"]);
    f.commit_file("f.txt", "theirs\n", "theirs");
    f.git(&["switch", "--quiet", "main"]);
    f.commit_file("f.txt", "ours\n", "ours");
}

#[test]
fn conflict_regions_from_a_real_merge() {
    use spor::conflict::{self, Choice, Segment};
    let f = fixture();
    conflicting_branches(&f);
    f.repo.merge("other").unwrap();
    let segments = conflict::parse(&f.read("f.txt"));
    assert_eq!(conflict::count(&segments), 1);
    let Segment::Conflict(r) = &segments[0] else {
        panic!("{segments:?}")
    };
    assert_eq!((r.ours.as_str(), r.theirs.as_str()), ("ours\n", "theirs\n"));
    assert_eq!(r.theirs_label, "other");
    f.write(
        "f.txt",
        &conflict::resolve(&segments, &[Choice::Both]).unwrap(),
    );
    f.repo.mark_resolved("f.txt").unwrap();
    assert_eq!(f.repo.continue_operation().unwrap(), Outcome::Done);
    assert_eq!(f.read("f.txt"), "ours\ntheirs\n");
}

#[test]
fn merge_conflict_resolve_and_continue() {
    let f = fixture();
    conflicting_branches(&f);
    let outcome = f.repo.merge("other").unwrap();
    assert_eq!(outcome, Outcome::Conflicts(vec!["f.txt".into()]));
    assert_eq!(f.repo.operation(), Some(Operation::Merge));
    let status = f.repo.status().unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].status, FileStatus::Conflicted);
    // Can't continue with conflicts left.
    assert!(matches!(
        f.repo.continue_operation().unwrap(),
        Outcome::Conflicts(_)
    ));
    f.repo.resolve_with("f.txt", ConflictSide::Theirs).unwrap();
    assert_eq!(f.read("f.txt"), "theirs\n");
    assert_eq!(f.repo.continue_operation().unwrap(), Outcome::Done);
    assert_eq!(f.repo.operation(), None);
    assert_eq!(f.repo.parents("HEAD").unwrap().len(), 2);
    assert!(f.repo.has_merges_since("HEAD~1"));
}

#[test]
fn abort_puts_things_back() {
    let f = fixture();
    conflicting_branches(&f);
    let before = f.repo.head_sha().unwrap();
    assert!(matches!(
        f.repo.merge("other").unwrap(),
        Outcome::Conflicts(_)
    ));
    f.repo.abort_operation().unwrap();
    assert_eq!(f.repo.operation(), None);
    assert_eq!(f.repo.head_sha().unwrap(), before);
    assert_eq!(f.read("f.txt"), "ours\n");
}

#[test]
fn mark_resolved_takes_the_edited_file() {
    let f = fixture();
    conflicting_branches(&f);
    assert!(matches!(
        f.repo.cherry_pick("other").unwrap(),
        Outcome::Conflicts(_)
    ));
    assert_eq!(f.repo.operation(), Some(Operation::CherryPick));
    f.write("f.txt", "both\n");
    f.repo.mark_resolved("f.txt").unwrap();
    assert_eq!(f.repo.continue_operation().unwrap(), Outcome::Done);
    assert_eq!(f.read("f.txt"), "both\n");
    assert_eq!(f.subjects()[0], "theirs");
}

#[test]
fn rebase_replays_onto_another_branch() {
    let f = fixture();
    f.commit_file("a.txt", "a\n", "a");
    f.git(&["switch", "--quiet", "-c", "feature"]);
    f.commit_file("b.txt", "b\n", "b");
    f.git(&["switch", "--quiet", "main"]);
    f.commit_file("c.txt", "c\n", "c");
    f.git(&["switch", "--quiet", "feature"]);
    assert_eq!(f.repo.rebase("main").unwrap(), Outcome::Done);
    assert_eq!(f.subjects(), ["b", "c", "a"]);
}

#[test]
fn rebase_stops_on_conflicts_and_continues() {
    let f = fixture();
    conflicting_branches(&f);
    f.git(&["switch", "--quiet", "other"]);
    assert!(matches!(
        f.repo.rebase("main").unwrap(),
        Outcome::Conflicts(_)
    ));
    assert_eq!(f.repo.operation(), Some(Operation::Rebase));
    f.repo.resolve_with("f.txt", ConflictSide::Theirs).unwrap();
    assert_eq!(f.repo.continue_operation().unwrap(), Outcome::Done);
    assert_eq!(f.repo.operation(), None);
    assert_eq!(f.subjects(), ["theirs", "ours", "base"]);
}

#[test]
fn interactive_rebase_rewrites_history() {
    let f = fixture();
    let base = f.commit_file("f.txt", "0\n", "base");
    let one = f.commit_file("one.txt", "1\n", "one");
    let two = f.commit_file("two.txt", "2\n", "two");
    let three = f.commit_file("three.txt", "3\n", "three");
    let four = f.commit_file("four.txt", "4\n", "four");
    assert!(!f.repo.has_merges_since(&base));
    let since = f.repo.commits_since(&base).unwrap();
    assert_eq!(since.len(), 4);
    assert_eq!(since[0].subject, "one");
    let plan = [
        RebaseStep::Reword(three.clone(), "third, it's quoted".into()),
        RebaseStep::Pick(one),
        RebaseStep::Fixup(two),
        RebaseStep::Drop(four),
    ];
    assert_eq!(
        f.repo.rebase_interactive(&base, &plan).unwrap(),
        Outcome::Done
    );
    assert_eq!(f.subjects(), ["one", "third, it's quoted", "base"]);
    assert!(f.repo.path("two.txt").exists());
    assert!(!f.repo.path("four.txt").exists());
}

#[test]
fn a_branch_since_it_left_its_base() {
    let f = fixture();
    f.commit_file("a.txt", "a\n", "a");
    f.git(&["switch", "--quiet", "-c", "topic"]);
    f.commit_file("b.txt", "b\nb\n", "add b");
    f.commit_file("a.txt", "a\nmore\n", "grow a");
    f.git(&["switch", "--quiet", "main"]);
    // main moving on doesn't count as the branch's work.
    f.commit_file("c.txt", "c\n", "c on main");

    let subjects: Vec<String> = f
        .repo
        .branch_commits("main", "topic")
        .unwrap()
        .into_iter()
        .map(|c| c.subject)
        .collect();
    assert_eq!(subjects, ["grow a", "add b"]);
    assert_eq!(f.repo.branch_numstat("main", "topic"), (2, 3, 0));
    let patch = f.repo.branch_patch("main", "topic").unwrap();
    assert!(patch.contains("+more") && patch.contains("b/b.txt") && !patch.contains("c.txt"));
    assert!(f.repo.branch_commits("main", "main").unwrap().is_empty());
}

#[test]
fn branches_delete_restore_rename() {
    let f = fixture();
    f.commit_file("a.txt", "a\n", "a");
    f.git(&["switch", "--quiet", "-c", "topic"]);
    let tip = f.commit_file("b.txt", "b\n", "b");
    f.git(&["switch", "--quiet", "main"]);
    // Not merged: refused without force.
    assert!(f.repo.delete_branch("topic", false).is_err());
    let sha = f.repo.delete_branch("topic", true).unwrap();
    assert_eq!(sha, tip);
    f.repo.restore_branch("topic", &sha).unwrap();
    f.repo.rename_branch("topic", "feat/topic").unwrap();
    let names: Vec<String> = f
        .repo
        .list_branches()
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, ["feat/topic", "main"]);
    assert!(f.repo.valid_branch_name("feat/x"));
    assert!(!f.repo.valid_branch_name("bad..name"));
}

#[test]
fn stashes_apply_drop_restore() {
    let f = fixture();
    f.commit_file("f.txt", "a\n", "a");
    f.write("f.txt", "b\n");
    f.write("new.txt", "n\n");
    f.repo.stash_save(Some("my work"), true).unwrap();
    assert_eq!(f.read("f.txt"), "a\n");
    assert!(!f.repo.path("new.txt").exists());
    let stashes = f.repo.stashes();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.ends_with("my work"));
    f.repo.stash_apply_at(0).unwrap();
    assert_eq!(f.read("f.txt"), "b\n");
    assert_eq!(f.repo.stashes().len(), 1);
    let dropped = f.repo.stash_drop_at(0).unwrap();
    assert_eq!(f.repo.stashes().len(), 0);
    f.repo.restore_stash(&dropped).unwrap();
    assert_eq!(f.repo.stashes()[0].sha, dropped.sha);
}

#[test]
fn tags_and_reset() {
    let f = fixture();
    let first = f.commit_file("f.txt", "1\n", "first");
    f.commit_file("f.txt", "2\n", "second");
    f.repo
        .create_tag("v1", &first, Some("release one"))
        .unwrap();
    f.repo.create_tag("light", "HEAD", None).unwrap();
    assert_eq!(f.repo.resolve("v1").unwrap(), first);
    f.repo.reset(&first, ResetMode::Soft).unwrap();
    assert_eq!(f.repo.head_sha().unwrap(), first);
    assert_eq!(f.index_contents("f.txt"), "2\n");
    f.repo.reset(&first, ResetMode::Hard).unwrap();
    assert_eq!(f.read("f.txt"), "1\n");
}

#[test]
fn search_blame_and_file_history() {
    let f = fixture();
    f.commit_file("f.txt", "one\n", "Add the parser");
    let second = f.commit_file("f.txt", "one\ntwo\n", "Fix a crash");
    f.commit_file("g.txt", "g\n", "Unrelated");
    let hits = f.repo.search_commits("PARSER", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(f.repo.search_commits("test", 10).unwrap().len(), 3);
    assert_eq!(f.repo.search_commits(&second[..8], 10).unwrap()[0], second);
    let blame = f.repo.blame("f.txt").unwrap();
    assert_eq!(blame.len(), 2);
    assert_eq!(blame[1].hash, second);
    assert_eq!(blame[1].summary, "Fix a crash");
    assert_eq!(blame[1].text, "two");
    assert_eq!(blame[0].line, 1);
    let history = f.repo.file_log("f.txt", 10).unwrap();
    assert_eq!(history.len(), 2);

    // As of the first commit there's one line; uncommitted lines show up
    // in the working tree's blame with an all-zero hash.
    let then = f.repo.blame_at("f.txt", Some(&history[1].hash)).unwrap();
    assert_eq!(then.len(), 1);
    assert_eq!(then[0].summary, "Add the parser");
    f.write("f.txt", "one\ntwo\nthree\n");
    let now = f.repo.blame_at("f.txt", None).unwrap();
    assert_eq!(now.len(), 3);
    assert!(now[2].hash.chars().all(|c| c == '0'));
    assert_eq!(now[2].text, "three");
}

#[test]
fn worktrees_add_list_remove() {
    let f = fixture();
    f.commit_file("a.txt", "a\n", "a");
    let path = f.dir.join("agent-1");
    let wt = f
        .repo
        .add_worktree(&path, "agent/one", Some("main"))
        .unwrap();
    wt.write_file_for_test();
    let list = f.repo.worktrees().unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[0].is_main);
    assert_eq!(list[1].branch.as_deref(), Some("agent/one"));
    // The new working tree is its own repository handle.
    assert_eq!(wt.tracking().unwrap().branch.as_deref(), Some("agent/one"));
    assert!(f.repo.remove_worktree(&path, false).is_err());
    f.repo.remove_worktree(&path, true).unwrap();
    assert_eq!(f.repo.worktrees().unwrap().len(), 1);
}

trait WriteForTest {
    fn write_file_for_test(&self);
}

impl WriteForTest for Repo {
    /// Leave an uncommitted change, so removing the worktree needs force.
    fn write_file_for_test(&self) {
        std::fs::write(self.path("dirty.txt"), "x\n").unwrap();
    }
}

#[test]
fn unstaged_patch_leaves_out_conflicted_files() {
    let f = fixture();
    f.commit_file("other.txt", "1\n", "other");
    conflicting_branches(&f);
    assert!(matches!(
        f.repo.merge("other").unwrap(),
        Outcome::Conflicts(_)
    ));
    f.write("other.txt", "2\n");
    let patch = f.repo.patch(DiffKind::Unstaged, &[]).unwrap();
    assert!(patch.contains("other.txt"));
    assert!(!patch.contains("f.txt"));
}
