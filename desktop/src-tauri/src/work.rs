//! Work: your own branches, listed beside agent tasks and pull requests.
//! A branch is a piece of work once it has commits its base doesn't, or
//! while it's checked out. Agent branches are tasks and come from `agents`.

use crate::App;
use serde::Serialize;
use spor::git::Repo;
use tauri::State;

#[derive(Serialize)]
pub(crate) struct BranchCommit {
    hash: String,
    short: String,
    subject: String,
    author: String,
    time: i64,
}

#[derive(Serialize)]
pub(crate) struct WorkBranch {
    name: String,
    base: String,
    current: bool,
    /// Commits on it that the base doesn't have, newest first.
    commits: Vec<BranchCommit>,
    files: usize,
    additions: usize,
    deletions: usize,
    /// When its newest commit was made.
    updated: i64,
}

/// The branch new work starts from and lands on, as a revision that
/// resolves: the local branch, or origin's when there's no local one.
fn base_of(repo: &Repo) -> Option<String> {
    let base = repo.default_base_branch()?;
    if repo.resolve(&base).is_ok() {
        return Some(base);
    }
    let remote = format!("origin/{base}");
    repo.resolve(&remote).is_ok().then_some(remote)
}

#[tauri::command(async)]
pub(crate) fn work_branches(app: State<App>) -> Result<Vec<WorkBranch>, String> {
    let repo = app.repo()?;
    let Some(base) = base_of(&repo) else {
        return Ok(Vec::new());
    };
    let short_base = base.strip_prefix("origin/").unwrap_or(&base).to_string();
    let mut out = Vec::new();
    for b in repo.list_branches()? {
        if b.is_remote || b.name.starts_with("agent/") || b.name == short_base {
            continue;
        }
        let commits = repo.branch_commits(&base, &b.name).unwrap_or_default();
        if commits.is_empty() && !b.is_current {
            continue;
        }
        let (files, additions, deletions) = repo.branch_numstat(&base, &b.name);
        out.push(WorkBranch {
            updated: repo.commit_time(&b.name),
            name: b.name,
            base: short_base.clone(),
            current: b.is_current,
            commits: commits
                .into_iter()
                .map(|c| BranchCommit {
                    hash: c.hash,
                    short: c.short,
                    subject: c.subject,
                    author: c.author,
                    time: c.timestamp,
                })
                .collect(),
            files,
            additions,
            deletions,
        });
    }
    Ok(out)
}

/// `branch`'s changes since it left the base.
#[tauri::command(async)]
pub(crate) fn branch_patch(app: State<App>, branch: String) -> Result<String, String> {
    let repo = app.repo()?;
    let base = base_of(&repo).ok_or("This repository has no main branch to compare with")?;
    repo.branch_patch(&base, &branch)
}
