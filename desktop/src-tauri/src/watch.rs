//! Watching the open repository, so the window reloads when something
//! changes instead of on timers (#38). File events are sorted by what they
//! touch and sent to the UI as one of four events:
//!
//! - `refs-changed`: HEAD, a branch or tag, `packed-refs`, or a merge or
//!   rebase starting or stopping (History, the branch, ahead/behind)
//! - `index-changed`: `.git/index` (Changes, the rail's counts)
//! - `worktree-changed`: a file in the working tree that `.gitignore`
//!   doesn't exclude, at most once a second (Changes)
//! - `tasks-changed`: `.git/spor/tasks/**`, or an agent's folder staging or
//!   committing (Agents, and the open task's transcript)
//!
//! A burst (a checkout writing a thousand files) comes out as one event of
//! each kind. The UI still reloads every 30 s in case an event is missed,
//! on a network filesystem say.

use notify::{EventKind, RecursiveMode, Watcher as _};
use spor::git::Repo;
use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How long to gather a burst of file events before sending them.
const SETTLE: Duration = Duration::from_millis(100);
/// The least time between two `worktree-changed`: a build or an agent can
/// write files many times a second, and each one costs a `git status`.
const WORKTREE_EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Change {
    Refs,
    Index,
    Worktree,
    Tasks,
}

impl Change {
    /// The event's name, as the UI listens for it.
    pub(crate) fn event(self) -> &'static str {
        match self {
            Change::Refs => "refs-changed",
            Change::Index => "index-changed",
            Change::Worktree => "worktree-changed",
            Change::Tasks => "tasks-changed",
        }
    }
}

/// Where a repository keeps the things each kind of change is about.
#[derive(Debug, Clone)]
struct Layout {
    /// The working tree.
    root: PathBuf,
    /// This checkout's own git directory: its HEAD and index.
    git_dir: PathBuf,
    /// The git directory every worktree shares: refs and spor's tasks.
    common_dir: PathBuf,
}

impl Layout {
    fn of(repo: &Repo) -> Result<Layout, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo.root())
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ])
            .output()
            .map_err(|e| format!("Couldn't run git: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        let (Some(git_dir), Some(common_dir)) = (lines.next(), lines.next()) else {
            return Err(crate::first_line(&String::from_utf8_lossy(&out.stderr)));
        };
        // Events come with the real path (on macOS, /private/var for /var).
        let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        Ok(Layout {
            root: real(repo.root()),
            git_dir: real(Path::new(git_dir)),
            common_dir: real(Path::new(common_dir)),
        })
    }

    /// The folders to watch, recursively: the working tree, and the git
    /// directories that aren't inside it (a linked worktree's).
    fn watch_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.root.clone()];
        for dir in [&self.common_dir, &self.git_dir] {
            if !roots.iter().any(|r| dir.starts_with(r)) {
                roots.push(dir.clone());
            }
        }
        roots
    }

    /// What a change to `path` is about, if anything the UI shows.
    fn classify(&self, path: &Path) -> Option<Change> {
        if path.extension().is_some_and(|e| e == "lock") {
            // git writes `index.lock` and renames it to `index`: the rename
            // is the change.
            return None;
        }
        if let Ok(rel) = path.strip_prefix(&self.git_dir) {
            let first = rel.components().next().and_then(|c| c.as_os_str().to_str());
            match first {
                Some("index") if rel.components().count() == 1 => return Some(Change::Index),
                Some(
                    "HEAD" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD" | "REBASE_HEAD"
                    | "rebase-merge" | "rebase-apply",
                ) => return Some(Change::Refs),
                _ => {}
            }
        }
        if let Ok(rel) = path.strip_prefix(&self.common_dir) {
            let parts: Vec<&str> = rel
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();
            return match parts.as_slice() {
                ["packed-refs"] | ["refs", ..] => Some(Change::Refs),
                ["spor", "tasks", ..] => Some(Change::Tasks),
                // An agent's folder (another worktree) staging or committing.
                ["worktrees", _, "index" | "HEAD"] => Some(Change::Tasks),
                // Ignore rules changed: so may what Changes lists.
                ["info", "exclude"] => Some(Change::Worktree),
                _ => None,
            };
        }
        if let Ok(rel) = path.strip_prefix(&self.root) {
            // A linked worktree's `.git` is a file; its git dir is elsewhere.
            if rel.components().next() == Some(Component::Normal(".git".as_ref())) {
                return None;
            }
            return Some(Change::Worktree);
        }
        None
    }
}

/// Which changed working-tree files git ignores, asked once a batch with
/// `git check-ignore`. Top-level folders found ignored (`target/`,
/// `node_modules/`) are remembered, so a build's churn costs nothing.
#[derive(Default)]
struct Ignores {
    dirs: Vec<PathBuf>,
}

impl Ignores {
    /// Whether any of `paths` (under `root`) is a change git would notice.
    fn any_seen(&mut self, root: &Path, paths: &[PathBuf]) -> bool {
        let rel: Vec<&Path> = paths
            .iter()
            .filter_map(|p| p.strip_prefix(root).ok())
            .collect();
        if rel
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == ".gitignore"))
        {
            // The rules changed: forget what they said, and say so.
            self.dirs.clear();
            return true;
        }
        let rel: Vec<&Path> = rel
            .into_iter()
            .filter(|p| !p.as_os_str().is_empty() && !self.dirs.iter().any(|d| p.starts_with(d)))
            .collect();
        if rel.is_empty() {
            return false;
        }
        // Each path, and its top-level folder (as `dir/`) to remember.
        let mut ask: BTreeSet<String> = BTreeSet::new();
        for p in &rel {
            ask.insert(p.to_string_lossy().into_owned());
            let mut parts = p.components();
            if let (Some(top), Some(_)) = (parts.next(), parts.next()) {
                ask.insert(format!("{}/", top.as_os_str().to_string_lossy()));
            }
        }
        let Some(ignored) = check_ignore(root, &ask) else {
            return true; // git failed: say it changed, rather than miss it.
        };
        for dir in ignored.iter().filter_map(|i| i.strip_suffix('/')) {
            self.dirs.push(PathBuf::from(dir));
        }
        rel.iter().any(|p| {
            !ignored.contains(p.to_string_lossy().as_ref())
                && !self.dirs.iter().any(|d| p.starts_with(d))
        })
    }
}

/// The ones of `paths` git ignores. Tracked files are never ignored.
fn check_ignore(root: &Path, paths: &BTreeSet<String>) -> Option<BTreeSet<String>> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--stdin", "-z"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut input = Vec::new();
    for p in paths {
        input.extend_from_slice(p.as_bytes());
        input.push(0);
    }
    child.stdin.take()?.write_all(&input).ok()?;
    let out = child.wait_with_output().ok()?;
    // 0: some are ignored, 1: none are.
    if !matches!(out.status.code(), Some(0 | 1)) {
        return None;
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect(),
    )
}

/// A running watcher: dropping it stops it, and its thread.
pub(crate) struct Watch {
    _watcher: notify::RecommendedWatcher,
}

/// Watch `repo`, calling `emit` with each change (from another thread).
pub(crate) fn start(repo: &Repo, emit: impl Fn(Change) + Send + 'static) -> Result<Watch, String> {
    let layout = Layout::of(repo)?;
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // Reads (an open, say) change nothing.
        match event {
            Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                let _ = tx.send(event.paths);
            }
            _ => {}
        }
    })
    .map_err(|e| format!("Couldn't watch the repository: {e}"))?;
    for dir in layout.watch_roots() {
        watcher
            .watch(&dir, RecursiveMode::Recursive)
            .map_err(|e| format!("Couldn't watch {}: {e}", dir.display()))?;
    }
    std::thread::spawn(move || sort_events(&layout, rx, emit));
    Ok(Watch { _watcher: watcher })
}

/// Turn file events into changes until the watcher is dropped: git's after
/// `SETTLE`, the working tree's no more than once every `WORKTREE_EVERY`.
fn sort_events(layout: &Layout, rx: Receiver<Vec<PathBuf>>, emit: impl Fn(Change)) {
    let mut ignores = Ignores::default();
    let mut git: Option<(Instant, BTreeSet<Change>)> = None;
    let mut tree: Option<(Instant, Vec<PathBuf>)> = None;
    let mut tree_last: Option<Instant> = None;
    loop {
        let git_due = git.as_ref().map(|(since, _)| *since + SETTLE);
        let tree_due = tree.as_ref().map(|(since, _)| {
            (*since + SETTLE).max(tree_last.map_or(*since, |t| t + WORKTREE_EVERY))
        });
        let received = match git_due.into_iter().chain(tree_due).min() {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
        };
        match received {
            Ok(paths) => {
                for path in paths {
                    match layout.classify(&path) {
                        Some(Change::Worktree) => tree
                            .get_or_insert_with(|| (Instant::now(), Vec::new()))
                            .1
                            .push(path),
                        Some(change) => {
                            git.get_or_insert_with(|| (Instant::now(), BTreeSet::new()))
                                .1
                                .insert(change);
                        }
                        None => {}
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if git_due.is_some_and(|due| due <= now) {
            for change in git.take().map(|(_, c)| c).unwrap_or_default() {
                emit(change);
            }
        }
        if tree_due.is_some_and(|due| due <= now) {
            let paths = tree.take().map(|(_, p)| p).unwrap_or_default();
            tree_last = Some(now);
            if ignores.any_seen(&layout.root, &paths) {
                emit(Change::Worktree);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository with one commit, a `.gitignore` for `target/` and
    /// `*.log`, and a linked worktree beside it.
    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("spor-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.email", "spor@example.com"]);
        git(&root, &["config", "user.name", "Spor"]);
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        let linked = base.canonicalize().unwrap().join("linked");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "side",
                linked.to_str().unwrap(),
            ],
        );
        (root, linked)
    }

    #[test]
    fn sorts_paths_by_what_they_change() {
        let (root, linked) = scratch("classify");
        let main = Layout::of(&Repo::new(&root)).unwrap();
        let g = root.join(".git");
        assert_eq!(main.classify(&g.join("HEAD")), Some(Change::Refs));
        assert_eq!(
            main.classify(&g.join("refs/heads/main")),
            Some(Change::Refs)
        );
        assert_eq!(main.classify(&g.join("packed-refs")), Some(Change::Refs));
        assert_eq!(
            main.classify(&g.join("rebase-merge/done")),
            Some(Change::Refs)
        );
        assert_eq!(main.classify(&g.join("index")), Some(Change::Index));
        assert_eq!(main.classify(&g.join("index.lock")), None);
        assert_eq!(main.classify(&g.join("refs/heads/main.lock")), None);
        assert_eq!(
            main.classify(&g.join("spor/tasks/fix.d/run-1.jsonl")),
            Some(Change::Tasks)
        );
        assert_eq!(
            main.classify(&g.join("worktrees/linked/index")),
            Some(Change::Tasks)
        );
        assert_eq!(main.classify(&g.join("objects/ab/cdef")), None);
        assert_eq!(main.classify(&g.join("logs/HEAD")), None);
        assert_eq!(main.classify(&g.join("FETCH_HEAD")), None);
        assert_eq!(
            main.classify(&root.join("src/a.rs")),
            Some(Change::Worktree)
        );
        assert_eq!(main.classify(&std::env::temp_dir().join("elsewhere")), None);
        assert_eq!(main.watch_roots(), vec![root.clone()]);

        // A linked worktree: its own HEAD and index, the shared refs.
        let side = Layout::of(&Repo::new(&linked)).unwrap();
        let own = g.join("worktrees/linked");
        assert_eq!(side.classify(&own.join("index")), Some(Change::Index));
        assert_eq!(side.classify(&own.join("HEAD")), Some(Change::Refs));
        assert_eq!(side.classify(&g.join("index")), None);
        assert_eq!(
            side.classify(&g.join("refs/heads/side")),
            Some(Change::Refs)
        );
        assert_eq!(side.classify(&linked.join(".git")), None);
        assert_eq!(side.classify(&linked.join("a.txt")), Some(Change::Worktree));
        assert_eq!(side.classify(&root.join("a.txt")), None);
        assert_eq!(side.watch_roots(), vec![linked.clone(), g.clone()]);
    }

    #[test]
    fn skips_what_gitignore_excludes() {
        let (root, _) = scratch("ignores");
        let mut ignores = Ignores::default();
        let at = |p: &str| vec![root.join(p)];
        assert!(!ignores.any_seen(&root, &at("target/debug/build.o")));
        assert_eq!(ignores.dirs, vec![PathBuf::from("target")]);
        assert!(!ignores.any_seen(&root, &at("debug.log")));
        assert!(ignores.any_seen(&root, &at("a.txt")));
        assert!(ignores.any_seen(&root, &[root.join("target/x"), root.join("src/new.rs")]));
        // Changing the rules is a change, and forgets what they said.
        assert!(ignores.any_seen(&root, &at(".gitignore")));
        assert!(ignores.dirs.is_empty());
    }

    /// The real thing: change the repository and see the events come.
    #[test]
    fn sends_events_when_the_repository_changes() {
        let (root, _) = scratch("events");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let _watch = start(&Repo::new(&root), move |c| sink.lock().unwrap().push(c)).unwrap();
        // FSEvents can report what happened just before it started.
        std::thread::sleep(Duration::from_millis(500));
        let wait_for = |want: &[Change]| {
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until {
                let got = seen.lock().unwrap().clone();
                if want.iter().all(|w| got.contains(w)) {
                    return got;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("waited for {want:?}, saw {:?}", seen.lock().unwrap());
        };

        seen.lock().unwrap().clear();
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        wait_for(&[Change::Worktree]);

        // An ignored file: nothing.
        std::thread::sleep(WORKTREE_EVERY + SETTLE);
        seen.lock().unwrap().clear();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/out"), "x").unwrap();
        std::thread::sleep(WORKTREE_EVERY + SETTLE * 5);
        assert_eq!(*seen.lock().unwrap(), vec![]);

        git(&root, &["add", "a.txt"]);
        wait_for(&[Change::Index]);
        git(&root, &["commit", "-q", "-m", "second"]);
        wait_for(&[Change::Refs]);

        std::fs::create_dir_all(root.join(".git/spor/tasks")).unwrap();
        std::fs::write(root.join(".git/spor/tasks/fix"), "task").unwrap();
        wait_for(&[Change::Tasks]);
    }
}
