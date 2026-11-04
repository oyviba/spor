//! One undo stack for the whole app. Every command that changes the
//! repository pushes what it would take to put it back, with a label for the
//! toast; ⌘Z pops the latest. Kept while the app runs, like the GPUI app's.

use spor::git::{AppliedPatch, FileStatus, Repo, ResetMode};
use spor::tasks::{Tasks, Undoable};
use std::path::PathBuf;
use std::sync::Mutex;

pub(crate) enum Undo {
    /// A landed or discarded agent task.
    Task(Undoable),
    /// Files that were staged: unstage them again.
    Staged(Vec<String>),
    /// Files that were unstaged: stage them again.
    Unstaged(Vec<String>),
    /// Lines staged or unstaged: apply the patch the other way.
    Lines(AppliedPatch),
    /// Discarded files: their contents before (None: they didn't exist).
    Files(Vec<(PathBuf, Option<Vec<u8>>)>),
    /// A branch that moved (commit, amend, merge, cherry-pick, revert,
    /// pull): put it back, if it's still checked out and where we left it.
    Moved {
        branch: Option<String>,
        before: String,
        after: String,
        mode: ResetMode,
    },
    /// Switched branches: switch back.
    Checkout { from: String },
    /// A deleted branch: recreate it.
    Deleted { name: String, sha: String },
    /// A created branch: delete it.
    Created { name: String },
}

struct Entry {
    label: String,
    undo: Undo,
}

#[derive(Default)]
pub(crate) struct Stack(Mutex<Vec<Entry>>);

const DEPTH: usize = 50;

impl Stack {
    pub(crate) fn push(&self, label: impl Into<String>, undo: Undo) {
        let mut stack = self.0.lock().unwrap();
        stack.push(Entry {
            label: label.into(),
            undo,
        });
        let excess = stack.len().saturating_sub(DEPTH);
        stack.drain(..excess);
    }

    /// The label of what ⌘Z would undo.
    pub(crate) fn peek(&self) -> Option<String> {
        self.0.lock().unwrap().last().map(|e| e.label.clone())
    }

    pub(crate) fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    pub(crate) fn pop(&self, repo: &Repo) -> Result<String, String> {
        let Some(entry) = self.0.lock().unwrap().pop() else {
            return Err("Nothing to undo".into());
        };
        match apply(repo, entry.undo) {
            Ok(()) => Ok(format!("Undid: {}", entry.label)),
            Err(e) => Err(format!("Couldn't undo {}: {}", entry.label, crate::first_line(&e))),
        }
    }
}

fn apply(repo: &Repo, undo: Undo) -> Result<(), String> {
    match undo {
        Undo::Task(u) => Tasks::open(repo)?.restore(&u),
        Undo::Staged(paths) => {
            let status = repo.status()?;
            for entry in status
                .iter()
                .filter(|e| e.status.is_staged() && paths.contains(&e.path))
            {
                repo.unstage(entry)?;
            }
            Ok(())
        }
        Undo::Unstaged(paths) => paths.iter().try_for_each(|p| repo.stage(p)),
        Undo::Lines(patch) => repo.unapply(&patch),
        Undo::Files(files) => {
            for (path, contents) in files {
                match contents {
                    Some(bytes) => {
                        if let Some(dir) = path.parent() {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(&path, bytes).map_err(|e| e.to_string())?
                    }
                    None => {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            Ok(())
        }
        Undo::Moved {
            branch,
            before,
            after,
            mode,
        } => {
            let here = repo.tracking()?.branch;
            if here != branch {
                return Err(format!(
                    "Switch back to {} first",
                    branch.as_deref().unwrap_or("the detached HEAD")
                ));
            }
            if repo.head_sha()? != after {
                return Err("The branch has moved since".into());
            }
            repo.reset(&before, mode)
        }
        Undo::Checkout { from } => repo.checkout_branch(&from),
        Undo::Deleted { name, sha } => repo.restore_branch(&name, &sha),
        Undo::Created { name } => repo.delete_branch(&name, true).map(|_| ()),
    }
}

/// What discarding `paths` would lose, to put back on undo.
pub(crate) fn snapshot(repo: &Repo, paths: &[String]) -> Undo {
    Undo::Files(
        paths
            .iter()
            .map(|p| {
                let full = repo.path(p);
                let contents = std::fs::read(&full).ok();
                (full, contents)
            })
            .collect(),
    )
}

/// Is `path` untracked (so discarding it means deleting it)?
pub(crate) fn untracked(repo: &Repo, path: &str) -> bool {
    repo.status()
        .map(|s| {
            s.iter()
                .any(|e| e.path == path && e.status == FileStatus::Untracked)
        })
        .unwrap_or(false)
}
