//! A snapshot of a repository's state, read from git in one go so the views
//! render from plain data. Loading touches only git and the filesystem, so it
//! can run on a background thread.

use gpui_kit::component::diff::DiffFile;
use spor::git::{self, Branch, FileStatus, StatusEntry, TrackingInfo};
use spor::graph::{self, GraphRow};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const LOG_LIMIT: usize = 2000;

/// One changed file in the working tree. git status reports a file that is
/// partly staged as two entries; here they're one row with a mixed checkbox.
#[derive(Clone, Debug)]
pub struct Change {
    pub path: String,
    pub orig_path: Option<String>,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub deleted: bool,
    pub added: bool,
    /// The raw status entries, for staging/unstaging.
    pub entries: Vec<StatusEntry>,
}

impl Change {
    fn from_status(status: &[StatusEntry], staged_new: &HashSet<String>) -> Vec<Change> {
        let mut out: Vec<Change> = Vec::new();
        for e in status {
            let idx = match out.iter().position(|c| c.path == e.path) {
                Some(i) => i,
                None => {
                    out.push(Change {
                        path: e.path.clone(),
                        orig_path: None,
                        staged: false,
                        unstaged: false,
                        untracked: false,
                        deleted: false,
                        added: false,
                        entries: Vec::new(),
                    });
                    out.len() - 1
                }
            };
            let c = &mut out[idx];
            match e.status {
                FileStatus::Staged => {
                    c.staged = true;
                    c.added |= staged_new.contains(&e.path);
                }
                FileStatus::StagedDeleted => {
                    c.staged = true;
                    c.deleted = true;
                }
                FileStatus::Modified => c.unstaged = true,
                FileStatus::Deleted => {
                    c.unstaged = true;
                    c.deleted = true;
                }
                FileStatus::Untracked => {
                    c.unstaged = true;
                    c.untracked = true;
                    c.added = true;
                }
            }
            if e.orig_path.is_some() {
                c.orig_path = e.orig_path.clone();
            }
            c.entries.push(e.clone());
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Fully staged, so its checkbox is ticked.
    pub fn included(&self) -> bool {
        self.staged && !self.unstaged
    }

    /// Staged entries, for unstaging.
    pub fn staged_entries(&self) -> Vec<StatusEntry> {
        self.entries
            .iter()
            .filter(|e| e.status.is_staged())
            .cloned()
            .collect()
    }

    /// Single-letter status, as git and Zed show it.
    pub fn letter(&self) -> &'static str {
        if self.untracked {
            "U"
        } else if self.deleted {
            "D"
        } else if self.added {
            "A"
        } else if self.orig_path.is_some() {
            "R"
        } else {
            "M"
        }
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub root: PathBuf,
    pub name: String,
    pub rows: Vec<GraphRow>,
    /// Row index of each ref's tip, keyed by ref name (tags as `tag:NAME`).
    pub ref_rows: HashMap<String, usize>,
    pub max_lanes: usize,
    pub changes: Vec<Change>,
    pub tracking: TrackingInfo,
    pub remotes: Vec<String>,
    pub branches: Vec<Branch>,
    /// Stash entries, newest first, as their messages.
    pub stashes: Vec<String>,
    /// The branch PRs target: origin's HEAD, else main/master/trunk.
    pub default_branch: Option<String>,
}

impl Snapshot {
    /// Read everything from the repository at `root`. Every git call in
    /// `spor::git` runs in the working directory, so the caller sets that.
    pub fn load(root: &Path) -> Result<Self, String> {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let remotes = git::remotes();
        let commits = git::log_all(LOG_LIMIT).unwrap_or_default();
        let chain: HashSet<String> = git::main_chain().unwrap_or_default().into_iter().collect();
        let rows = graph::assign_lanes(&commits, &chain, &remotes);
        let max_lanes = rows
            .iter()
            .map(|r| {
                r.lanes_after
                    .len()
                    .max(r.lanes_before.len())
                    .max(r.lane + 1)
            })
            .max()
            .unwrap_or(1);
        let mut ref_rows = HashMap::new();
        for (i, row) in rows.iter().enumerate() {
            for r in &row.commit.refs {
                let key = match r.strip_prefix("tag:") {
                    Some(t) => format!("tag:{}", t.trim()),
                    None => r.clone(),
                };
                ref_rows.entry(key).or_insert(i);
            }
        }
        let status = git::status()?;
        let changes = Change::from_status(&status, &git::staged_new_files());
        let mut branches = git::list_branches().unwrap_or_default();
        branches.sort_by_key(|b| b.name.to_lowercase());
        Ok(Self {
            root: root.to_path_buf(),
            name,
            rows,
            ref_rows,
            max_lanes,
            changes,
            tracking: git::tracking().unwrap_or_default(),
            remotes,
            branches,
            stashes: git::stash_list(),
            default_branch: git::default_base_branch(),
        })
    }

    pub fn head_row(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.commit.head_ref.is_some())
    }

    pub fn staged_count(&self) -> usize {
        self.changes.iter().filter(|c| c.staged).count()
    }

    /// `origin`, or the first remote.
    pub fn default_remote(&self) -> Option<String> {
        self.remotes
            .iter()
            .find(|r| *r == "origin")
            .or(self.remotes.first())
            .cloned()
    }

    /// Is there a remote-tracking branch for local branch `name`?
    pub fn is_published(&self, name: &str) -> bool {
        self.branches
            .iter()
            .any(|b| b.is_remote && self.without_remote(&b.name) == Some(name))
    }

    /// Strip a configured remote prefix: `origin/feat/x` → `feat/x`.
    pub fn without_remote<'a>(&self, name: &'a str) -> Option<&'a str> {
        let (remote, rest) = name.split_once('/')?;
        self.remotes.iter().any(|r| r == remote).then_some(rest)
    }
}

/// Everything uncommitted, parsed into diff files for display.
pub fn uncommitted_files(changes: &[Change]) -> Vec<DiffFile> {
    let untracked: Vec<String> = changes
        .iter()
        .filter(|c| c.untracked)
        .map(|c| c.path.clone())
        .collect();
    git::uncommitted_patch(&untracked)
        .ok()
        .and_then(|patch| DiffFile::parse(&patch).ok())
        .unwrap_or_default()
}

/// What a commit changed, parsed for display.
pub fn commit_files(hash: &str, parents: &[String]) -> Vec<DiffFile> {
    git::commit_patch(hash, parents)
        .ok()
        .and_then(|patch| DiffFile::parse(&patch).ok())
        .unwrap_or_default()
}

/// A cheap fingerprint of what the views show: HEAD, every ref, the stash
/// and the working-tree status. Polled in the background; when it changes,
/// the snapshot is reloaded.
pub fn fingerprint(root: &Path) -> Option<String> {
    let run = |args: &[&str]| -> Option<String> {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let refs = run(&["for-each-ref", "--format=%(refname) %(objectname)"])?;
    let head = run(&["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap_or_default();
    let status = run(&["status", "--porcelain=v1", "-z", "-uall"])?;
    let branch = run(&["symbolic-ref", "--quiet", "HEAD"]).unwrap_or_default();
    // Status alone misses further edits to an already-modified file.
    let numstat = run(&["diff", "--no-ext-diff", "--numstat", "HEAD"]).unwrap_or_default();
    Some(format!("{head}\n{branch}\n{refs}\n{status}\n{numstat}"))
}

/// What the repository sidebar shows for a repository that isn't open: its
/// branch and how far it is from its upstream (as of its last fetch).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepoHead {
    /// The branch, or `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The short commit HEAD is on, for a detached HEAD.
    pub short: String,
    pub ahead: usize,
    pub behind: usize,
}

impl RepoHead {
    /// The branch name, or what a detached HEAD points at.
    pub fn label(&self) -> String {
        match &self.branch {
            Some(branch) => branch.clone(),
            None if self.short.is_empty() => "detached".into(),
            None => format!("detached at {}", self.short),
        }
    }
}

/// Read a repository's branch and upstream counts without touching its
/// working tree, so it stays cheap for every repository in a workspace.
pub fn head(root: &Path) -> Option<RepoHead> {
    let run = |args: &[&str]| -> Option<String> {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let short = run(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let branch = run(&["symbolic-ref", "--short", "-q", "HEAD"]);
    if branch.is_none() && short.is_empty() && !root.join(".git").exists() {
        return None;
    }
    let (ahead, behind) = branch
        .as_ref()
        .and_then(|_| run(&["rev-list", "--left-right", "--count", "HEAD...@{u}"]))
        .map(|counts| {
            let mut it = counts
                .split_whitespace()
                .map(|n| n.parse::<usize>().unwrap_or(0));
            (it.next().unwrap_or(0), it.next().unwrap_or(0))
        })
        .unwrap_or((0, 0));
    Some(RepoHead {
        branch,
        short,
        ahead,
        behind,
    })
}
