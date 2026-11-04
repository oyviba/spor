//! Watch the repository and tell the UI what changed (#38), instead of the
//! UI polling for it. One `repo-changed` event carries what moved:
//!
//! - `refs`: `.git/HEAD`, `.git/refs/**`, `.git/packed-refs` (a commit, a
//!   switch, a fetch): History, the branch, ahead/behind
//! - `index`: `.git/index` (stage, unstage, commit): Changes, rail counts
//! - `worktree`: a file in the working tree that isn't ignored: Changes
//! - `tasks`: `.git/spor/tasks/**` (task files, agent logs and exit codes)
//!   and task folders' HEAD and index: Agents, the open transcript
//!
//! Events arriving close together are batched (one event at most every
//! quarter second while things keep changing). The working tree is watched
//! one directory at a time, only where git would look: `.gitignore`d trees
//! like `target/` and `node_modules/` aren't watched at all, which keeps a
//! big repository inside the OS's watch limits. The UI still polls slowly
//! as a backstop, for events a filesystem doesn't report (network mounts).

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use spor::git::Repo;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// What changed, as the UI gets it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct Changed {
    pub refs: bool,
    pub index: bool,
    pub worktree: bool,
    pub tasks: bool,
}

impl Changed {
    fn any(self) -> bool {
        self.refs || self.index || self.worktree || self.tasks
    }
}

/// A running watch; dropping it stops it.
pub(crate) struct Watch {
    stop: mpsc::Sender<PathBuf>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        // An empty path ends the batching thread, which drops the watcher.
        let _ = self.stop.send(PathBuf::new());
    }
}

/// Where things are, resolved once.
struct Places {
    root: PathBuf,
    git: PathBuf,
}

impl Places {
    /// What a changed path means; None for what nobody shows (objects,
    /// logs, lock files on their own).
    fn classify(&self, path: &Path) -> Option<Kind> {
        if let Ok(rel) = path.strip_prefix(&self.git) {
            let rel = rel.to_string_lossy();
            let rel = rel.strip_suffix(".lock").unwrap_or(&rel);
            return match rel {
                "HEAD" | "packed-refs" | "ORIG_HEAD" | "MERGE_HEAD" | "CHERRY_PICK_HEAD"
                | "REVERT_HEAD" | "REBASE_HEAD" => Some(Kind::Refs),
                "index" => Some(Kind::Index),
                r if r.starts_with("refs/spor/") => None,
                r if r.starts_with("refs/") => Some(Kind::Refs),
                r if r.starts_with("spor/tasks/") => Some(Kind::Tasks),
                r if r.starts_with("worktrees/")
                    && (r.ends_with("/HEAD") || r.ends_with("/index")) =>
                {
                    Some(Kind::Tasks)
                }
                _ => None,
            };
        }
        let rel = path.strip_prefix(&self.root).ok()?;
        if rel.as_os_str().is_empty() || rel.starts_with(".git") {
            return None;
        }
        Some(Kind::Worktree(rel.to_path_buf()))
    }
}

enum Kind {
    Refs,
    Index,
    Tasks,
    Worktree(PathBuf),
}

/// Start watching `repo`, sending `repo-changed` to the app's windows.
pub(crate) fn start(app: AppHandle, repo: &Repo) -> Result<Watch, String> {
    let places = Places {
        root: canonical(repo.root()),
        git: canonical(&repo.common_dir()?),
    };
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let events = tx.clone();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            if event.kind.is_access() {
                return;
            }
            for path in event.paths {
                let _ = events.send(path);
            }
        }
    })
    .map_err(|e| format!("Couldn't watch the repository: {e}"))?;
    let watcher = Arc::new(Mutex::new(watcher));
    let watched = Arc::new(Mutex::new(HashSet::new()));
    {
        let mut w = watcher.lock().unwrap();
        let git = &places.git;
        let _ = w.watch(git, RecursiveMode::NonRecursive);
        let _ = w.watch(&git.join("refs"), RecursiveMode::Recursive);
        let tasks = git.join("spor").join("tasks");
        let _ = std::fs::create_dir_all(&tasks);
        let _ = w.watch(&tasks, RecursiveMode::Recursive);
        if git.join("worktrees").is_dir() {
            let _ = w.watch(&git.join("worktrees"), RecursiveMode::Recursive);
        }
    }
    watch_tree(repo, &places.root, &watcher, &watched);

    let repo = repo.clone();
    std::thread::spawn(move || {
        // Keeps the watcher alive as long as the thread runs.
        let watcher = watcher;
        while let Ok(first) = rx.recv() {
            if first.as_os_str().is_empty() {
                return;
            }
            // Gather what else comes in the next moments.
            let mut paths = vec![first];
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(250) {
                match rx.recv_timeout(Duration::from_millis(60)) {
                    Ok(p) if p.as_os_str().is_empty() => return,
                    Ok(p) => paths.push(p),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            let mut changed = Changed::default();
            let mut tree = Vec::new();
            for p in &paths {
                match places.classify(p) {
                    Some(Kind::Refs) => changed.refs = true,
                    Some(Kind::Index) => changed.index = true,
                    Some(Kind::Tasks) => changed.tasks = true,
                    Some(Kind::Worktree(rel)) => tree.push(rel),
                    None => {}
                }
            }
            if !tree.is_empty() {
                tree.sort();
                tree.dedup();
                if !all_ignored(&repo, &tree) {
                    changed.worktree = true;
                    // New directories get watched too.
                    watch_tree(&repo, &places.root, &watcher, &watched);
                }
            }
            if changed.any() {
                let _ = app.emit("repo-changed", changed);
            }
        }
    });
    Ok(Watch { stop: tx })
}

/// Watch every directory git would look in: the root, and each directory
/// holding a tracked or untracked-but-not-ignored file.
fn watch_tree(
    repo: &Repo,
    root: &Path,
    watcher: &Mutex<RecommendedWatcher>,
    watched: &Mutex<HashSet<PathBuf>>,
) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--directory",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output();
    let Ok(out) = out else { return };
    let mut dirs: HashSet<PathBuf> = HashSet::from([root.to_path_buf()]);
    for file in out.stdout.split(|b| *b == 0).filter(|f| !f.is_empty()) {
        let rel = PathBuf::from(String::from_utf8_lossy(file).trim_end_matches('/'));
        let mut dir = rel.parent();
        // An untracked directory comes as `dir/`: watch it as well.
        if file.ends_with(b"/") {
            dirs.insert(root.join(&rel));
        }
        while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
            if !dirs.insert(root.join(d)) {
                break;
            }
            dir = d.parent();
        }
    }
    let mut watched = watched.lock().unwrap();
    let mut w = watcher.lock().unwrap();
    for dir in dirs {
        if !watched.contains(&dir) && w.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
            watched.insert(dir);
        }
    }
}

/// Are all of `paths` (relative to the root) ignored by git?
fn all_ignored(repo: &Repo, paths: &[PathBuf]) -> bool {
    use std::io::Write as _;
    let child = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["check-ignore", "-z", "--stdin"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    if let Some(mut stdin) = child.stdin.take() {
        for p in paths {
            let _ = stdin.write_all(p.to_string_lossy().as_bytes());
            let _ = stdin.write_all(b"\0");
        }
    }
    let Ok(out) = child.wait_with_output() else {
        return false;
    };
    let ignored = out
        .stdout
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .count();
    ignored == paths.len()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
