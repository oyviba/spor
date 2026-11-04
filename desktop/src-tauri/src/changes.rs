//! Changes: the working tree and the index, committing, syncing with the
//! remote, stashes, and finishing (or abandoning) a merge or rebase that
//! stopped on conflicts.

use crate::undo::{self, Undo};
use crate::{first_line, name_of, App};
use serde::{Deserialize, Serialize};
use spor::git::{ConflictSide, DiffKind, LineAction, Outcome, ResetMode};
use spor::patch::{LinePos, Side};
use std::process::Command;
use tauri::State;

#[derive(Serialize)]
pub(crate) struct FileEntry {
    path: String,
    orig_path: Option<String>,
    /// staged, stageddeleted, modified, deleted, untracked, conflicted.
    status: String,
    staged: bool,
}

#[tauri::command(async)]
pub(crate) fn status(app: State<App>) -> Result<Vec<FileEntry>, String> {
    Ok(app
        .repo()?
        .status()?
        .into_iter()
        .map(|e| FileEntry {
            staged: e.status.is_staged(),
            status: name_of(&e.status),
            path: e.path,
            orig_path: e.orig_path,
        })
        .collect())
}

/// The diff of one file: what's staged (`staged`) or what isn't.
#[tauri::command(async)]
pub(crate) fn file_diff(app: State<App>, path: String, staged: bool) -> Result<String, String> {
    let repo = app.repo()?;
    let status = repo.status()?;
    let entry = status
        .iter()
        .find(|e| e.path == path && e.status.is_staged() == staged)
        .ok_or_else(|| format!("{path} has no {} changes", if staged { "staged" } else { "unstaged" }))?;
    repo.diff_entry(entry)
}

#[tauri::command(async)]
pub(crate) fn stage(app: State<App>, paths: Vec<String>) -> Result<(), String> {
    let repo = app.repo()?;
    for p in &paths {
        repo.stage(p).map_err(|e| first_line(&e))?;
    }
    app.undo.push(label("Stage", &paths), Undo::Staged(paths));
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn unstage(app: State<App>, paths: Vec<String>) -> Result<(), String> {
    let repo = app.repo()?;
    let status = repo.status()?;
    for e in status.iter().filter(|e| e.status.is_staged() && paths.contains(&e.path)) {
        repo.unstage(e).map_err(|e| first_line(&e))?;
    }
    app.undo.push(label("Unstage", &paths), Undo::Unstaged(paths));
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct Pos {
    /// "old" for a removed line, "new" otherwise.
    side: String,
    line: usize,
}

impl Pos {
    fn get(&self) -> LinePos {
        LinePos::new(if self.side == "old" { Side::Old } else { Side::New }, self.line)
    }
}

/// Stage (or, from the staged diff, unstage; or discard) the changed lines
/// between `from` and `to`.
#[tauri::command(async)]
pub(crate) fn stage_lines(
    app: State<App>,
    path: String,
    staged: bool,
    discard: bool,
    from: Pos,
    to: Pos,
) -> Result<(), String> {
    let repo = app.repo()?;
    let (kind, action, verb) = match (staged, discard) {
        (true, _) => (DiffKind::Staged, LineAction::Unstage, "Unstage lines in"),
        (false, true) => (DiffKind::Unstaged, LineAction::Discard, "Discard lines in"),
        (false, false) => (DiffKind::Unstaged, LineAction::Stage, "Stage lines in"),
    };
    let applied = repo
        .apply_lines(&path, kind, action, from.get(), to.get())
        .map_err(|e| first_line(&e))?;
    app.undo.push(format!("{verb} {path}"), Undo::Lines(applied));
    Ok(())
}

/// Throw away unstaged changes to files; untracked ones are deleted.
#[tauri::command(async)]
pub(crate) fn discard(app: State<App>, paths: Vec<String>) -> Result<(), String> {
    let repo = app.repo()?;
    let snapshot = undo::snapshot(&repo, &paths);
    for p in &paths {
        if undo::untracked(&repo, p) {
            repo.remove_untracked(p)
        } else {
            repo.discard_worktree(p)
        }
        .map_err(|e| first_line(&e))?;
    }
    app.undo.push(label("Discard", &paths), snapshot);
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn commit(app: State<App>, message: String, amend: bool) -> Result<String, String> {
    let repo = app.repo()?;
    let message = message.trim();
    if message.is_empty() {
        return Err("Write a commit message first".into());
    }
    let branch = repo.tracking()?.branch;
    let before = repo.head_sha().ok();
    if amend {
        repo.amend(Some(message))
    } else {
        repo.commit(message)
    }
    .map_err(|e| first_line(&e))?;
    let after = repo.head_sha()?;
    let subject = message.lines().next().unwrap_or("").to_string();
    // Undo moves the branch back, keeping the changes staged. A first
    // commit has nothing to go back to.
    if let Some(before) = before {
        app.undo.push(
            format!("{} “{subject}”", if amend { "Amend" } else { "Commit" }),
            Undo::Moved {
                branch,
                before,
                after: after.clone(),
                mode: ResetMode::Soft,
            },
        );
    }
    Ok(format!(
        "{} {}",
        if amend { "Amended" } else { "Committed" },
        &after[..7.min(after.len())]
    ))
}

/// The last commit's message, to start an amend from.
#[tauri::command(async)]
pub(crate) fn last_message(app: State<App>) -> Result<String, String> {
    app.repo()?.head_message()
}

/// Fetch, pull (rebasing) or push. Git never prompts: a remote that needs a
/// password it can't get from a helper fails with git's own message.
#[tauri::command(async)]
pub(crate) fn sync(app: State<App>, action: String) -> Result<String, String> {
    let repo = app.repo()?;
    let args: Vec<String> = match action.as_str() {
        "fetch" => vec!["fetch".into(), "--all".into(), "--prune".into()],
        "pull" => vec!["pull".into(), "--rebase".into()],
        "push" => repo.push_args()?,
        other => return Err(format!("Unknown sync action {other}")),
    };
    let branch = repo.tracking()?.branch;
    let before = repo.head_sha().ok();
    let out = Command::new("git")
        .current_dir(repo.root())
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Couldn't run git: {e}"))?;
    if !out.status.success() {
        return Err(first_line(&String::from_utf8_lossy(&out.stderr)));
    }
    if action == "pull" {
        if let (Some(before), Ok(after)) = (before, repo.head_sha()) {
            if before != after {
                app.undo.push(
                    "Pull",
                    Undo::Moved {
                        branch,
                        before,
                        after,
                        mode: ResetMode::Keep,
                    },
                );
            }
        }
    }
    Ok(match action.as_str() {
        "fetch" => "Fetched".into(),
        "pull" => "Pulled".into(),
        _ => "Pushed".into(),
    })
}

#[derive(Serialize)]
pub(crate) struct StashInfo {
    index: usize,
    message: String,
}

#[tauri::command(async)]
pub(crate) fn stashes(app: State<App>) -> Result<Vec<StashInfo>, String> {
    Ok(app
        .repo()?
        .stashes()
        .into_iter()
        .map(|s| StashInfo {
            index: s.index,
            message: s.message,
        })
        .collect())
}

#[tauri::command(async)]
pub(crate) fn stash(app: State<App>, message: Option<String>) -> Result<String, String> {
    let message = message.filter(|m| !m.trim().is_empty());
    app.repo()?
        .stash_save(message.as_deref(), true)
        .map_err(|e| first_line(&e))?;
    Ok("Stashed your changes".into())
}

#[tauri::command(async)]
pub(crate) fn stash_pop(app: State<App>, index: usize) -> Result<String, String> {
    let repo = app.repo()?;
    if let Err(e) = repo.stash_pop_at(index) {
        // git's first line here is "Auto-merging …"; say what happened.
        let conflicted: Vec<String> = repo
            .status()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.status == spor::git::FileStatus::Conflicted)
            .map(|e| e.path)
            .collect();
        return Err(if conflicted.is_empty() {
            first_line(&e)
        } else {
            format!(
                "The stash conflicts with your changes in {} — resolve it in Changes. The stash is kept.",
                conflicted.join(", ")
            )
        });
    }
    Ok("Applied and dropped the stash".into())
}

/// Settle one conflicted file: take our side, theirs, or mark it resolved
/// as it is in the working tree (after editing it).
#[tauri::command(async)]
pub(crate) fn resolve(app: State<App>, path: String, how: String) -> Result<(), String> {
    let repo = app.repo()?;
    match how.as_str() {
        "ours" => repo.resolve_with(&path, ConflictSide::Ours),
        "theirs" => repo.resolve_with(&path, ConflictSide::Theirs),
        _ => repo.mark_resolved(&path),
    }
    .map_err(|e| first_line(&e))
}

#[tauri::command(async)]
pub(crate) fn continue_operation(app: State<App>) -> Result<String, String> {
    match app.repo()?.continue_operation().map_err(|e| first_line(&e))? {
        Outcome::Done => Ok("Done".into()),
        Outcome::Conflicts(files) => Err(format!(
            "The next commit conflicts too, in {}",
            files.join(", ")
        )),
    }
}

#[tauri::command(async)]
pub(crate) fn abort_operation(app: State<App>) -> Result<String, String> {
    app.repo()?.abort_operation().map_err(|e| first_line(&e))?;
    Ok("Aborted — everything is as it was".into())
}

fn label(verb: &str, paths: &[String]) -> String {
    match paths {
        [one] => format!("{verb} {one}"),
        many => format!("{verb} {} files", many.len()),
    }
}
