//! Changes: the working tree and the index, committing, syncing with the
//! remote, stashes, and finishing (or abandoning) a merge or rebase that
//! stopped on conflicts.

use crate::undo::{self, Undo};
use crate::{first_line, name_of, App};
use serde::{Deserialize, Serialize};
use spor::git::{ConflictSide, DiffKind, LineAction, Outcome, ResetMode};
use spor::patch::{LinePos, Side};
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
        .ok_or_else(|| {
            format!(
                "{path} has no {} changes",
                if staged { "staged" } else { "unstaged" }
            )
        })?;
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
    for e in status
        .iter()
        .filter(|e| e.status.is_staged() && paths.contains(&e.path))
    {
        repo.unstage(e).map_err(|e| first_line(&e))?;
    }
    app.undo
        .push(label("Unstage", &paths), Undo::Unstaged(paths));
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
        LinePos::new(
            if self.side == "old" {
                Side::Old
            } else {
                Side::New
            },
            self.line,
        )
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
    app.undo
        .push(format!("{verb} {path}"), Undo::Lines(applied));
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

/// Fetch, pull, push, force-push, or pull and push ("sync"). A passphrase
/// or password git or ssh asks for comes up in the app (the askpass
/// bridge); where it can't, git fails with its own message instead of
/// hanging.
///
/// - Fetch also brings local branches that are only behind their upstream
///   (`main`, while you're on a feature branch) up to it.
/// - Pull rebases, stashing uncommitted changes around it, and links a
///   branch with no upstream to its namesake on the remote first.
/// - A push the remote refuses because it has commits we don't says so in
///   words the window recognises ([`spor::askpass::is_push_behind`]), and
///   offers Pull and push.
/// - Force push is for a branch rewritten after it was pushed (amend,
///   rebase, rewrite): it refuses if someone else pushed meanwhile.
#[tauri::command(async)]
pub(crate) fn sync(app: State<App>, action: String) -> Result<String, String> {
    let repo = app.repo()?;
    match action.as_str() {
        "fetch" => {
            if repo.remotes().is_empty() {
                return Err("This repository has no remote to fetch from".into());
            }
            spor::askpass::git_network(repo.root(), &["fetch", "--all", "--prune"])?;
            Ok(with_fast_forwards("Fetched", &repo))
        }
        "pull" => {
            let pulled = pull(&app, &repo)?;
            Ok(with_fast_forwards(&pulled, &repo))
        }
        "push" => push(&app, &repo, false),
        "force-push" => push(&app, &repo, true),
        "sync" => {
            let tracking = repo.tracking()?;
            if let Some(branch) = &tracking.branch {
                if tracking.upstream.is_none() && repo.unlinked_remote_branch(branch).is_none() {
                    // Nothing on the remote to pull: publishing is all there is.
                    return push(&app, &repo, false);
                }
            }
            let pulled = pull(&app, &repo)?;
            let tracking = repo.tracking()?;
            let pushed = if tracking.ahead > 0 || tracking.upstream.is_none() {
                push(&app, &repo, false)?
            } else {
                "nothing to push".into()
            };
            Ok(with_fast_forwards(
                &format!("{pulled}, {}", lower_first(&pushed)),
                &repo,
            ))
        }
        other => Err(format!("Unknown sync action {other}")),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// `message`, and which other branches fetching brought up to date.
fn with_fast_forwards(message: &str, repo: &spor::git::Repo) -> String {
    let moved = repo.fast_forward_branches();
    if moved.is_empty() {
        return message.to_string();
    }
    let names: Vec<String> = moved
        .iter()
        .map(|m| format!("{} (+{})", m.branch, m.commits))
        .collect();
    format!("{message} — brought {} up to date", names.join(", "))
}

/// Pull the checked-out branch, rebasing. Undoable.
fn pull(app: &App, repo: &spor::git::Repo) -> Result<String, String> {
    let tracking = repo.tracking()?;
    let branch = tracking
        .branch
        .clone()
        .ok_or("You're not on a branch (detached HEAD) — there's nothing to pull into")?;
    let mut linked = None;
    if tracking.upstream.is_none() {
        match repo.unlinked_remote_branch(&branch) {
            Some(remote_branch) => {
                repo.set_upstream(&branch, &remote_branch)?;
                linked = Some(remote_branch);
            }
            None => {
                return Err(format!(
                "{branch} isn't on the remote yet, so there's nothing to pull — publish it instead"
            ))
            }
        }
    }
    let before = repo.head_sha().ok();
    let stashes = repo.stashes().len();
    let args = repo.pull_args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let Err(e) = spor::askpass::git_network(repo.root(), &args) {
        if repo.operation().is_some() {
            return Err(format!(
                "The pull stopped on a conflict between your commits and what came in — resolve it in Changes, or abort ({e})"
            ));
        }
        return Err(e);
    }
    let after = repo.head_sha().ok();
    if let (Some(before), Some(after)) = (&before, &after) {
        if before != after {
            app.undo.push(
                "Pull",
                Undo::Moved {
                    branch: Some(branch.clone()),
                    before: before.clone(),
                    after: after.clone(),
                    mode: ResetMode::Keep,
                },
            );
        }
    }
    let upstream = repo.tracking()?.upstream.unwrap_or_default();
    let n = before
        .map(|b| repo.count_commits(&format!("{b}..{upstream}")))
        .unwrap_or(0);
    let mut message = match n {
        0 => "Already up to date".to_string(),
        1 => "Pulled 1 commit".into(),
        n => format!("Pulled {n} commits"),
    };
    if let Some(remote_branch) = linked {
        message = format!("Linked {branch} to {remote_branch}. {message}");
    }
    if repo.stashes().len() > stashes {
        // git put the changes back with conflict markers where they clash,
        // and kept a copy in the stash list.
        message.push_str(
            ". Your uncommitted changes clash with what came in: resolve the conflicts in Changes, \
             then drop the \"autostash\" stash (a copy, kept in case)",
        );
    }
    Ok(message)
}

/// Push (or publish) the checked-out branch. Pushing makes its commits
/// final: their undos go.
fn push(app: &App, repo: &spor::git::Repo, force: bool) -> Result<String, String> {
    let tracking = repo.tracking()?;
    let branch = tracking
        .branch
        .clone()
        .ok_or("You're not on a branch (detached HEAD) — create one to push")?;
    let args = repo.push_args_for(&branch, force)?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    spor::askpass::git_network(repo.root(), &args)?;
    app.undo.forget_history();
    // A new branch's pull request, or new checks on it.
    app.github.clear(repo.root());
    Ok(match (&tracking.upstream, tracking.ahead) {
        (None, _) => format!("Published {branch}"),
        _ if force => format!("Force-pushed {branch}"),
        (_, 1) => "Pushed 1 commit".into(),
        (_, 0) => "Pushed".into(),
        (_, n) => format!("Pushed {n} commits"),
    })
}

/// Push a branch that isn't checked out (publishing it if it's new).
#[tauri::command(async)]
pub(crate) fn push_branch(app: State<App>, name: String) -> Result<String, String> {
    let repo = app.repo()?;
    let published = repo.resolve(&format!("{name}@{{upstream}}")).is_err();
    let args = repo.push_args_for(&name, false)?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    spor::askpass::git_network(repo.root(), &args)?;
    app.undo.forget_history();
    app.github.clear(repo.root());
    Ok(if published {
        format!("Published {name}")
    } else {
        format!("Pushed {name}")
    })
}

/// The background fetch: never asks for anything, and says nothing when it
/// can't (no network, no stored credentials). The window's state poll sees
/// what it brought in.
#[tauri::command(async)]
pub(crate) fn auto_fetch(app: State<App>) -> Result<(), String> {
    let repo = app.repo()?;
    if repo.remotes().is_empty() {
        return Ok(());
    }
    spor::askpass::git_network_quiet(repo.root(), &["fetch", "--all", "--prune"]).map(|_| ())
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

/// Apply a stash and keep it.
#[tauri::command(async)]
pub(crate) fn stash_apply(app: State<App>, index: usize) -> Result<String, String> {
    app.repo()?
        .stash_apply_at(index)
        .map_err(|e| first_line(&e))?;
    Ok("Applied the stash — it's still in the list".into())
}

/// Drop a stash (⌘Z stores it again).
#[tauri::command(async)]
pub(crate) fn stash_drop(app: State<App>, index: usize) -> Result<String, String> {
    let stash = app
        .repo()?
        .stash_drop_at(index)
        .map_err(|e| first_line(&e))?;
    let label = format!("Drop stash “{}”", stash.message);
    app.undo.push(label, Undo::StashDropped(stash));
    Ok("Dropped the stash".into())
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
    match app
        .repo()?
        .continue_operation()
        .map_err(|e| first_line(&e))?
    {
        Outcome::Done => Ok("Done".into()),
        Outcome::Conflicts(files) => Err(format!(
            "The next commit conflicts too, in {}",
            files.join(", ")
        )),
    }
}

/// Leave out the commit a rebase, cherry-pick or revert stopped on.
#[tauri::command(async)]
pub(crate) fn skip_operation(app: State<App>) -> Result<String, String> {
    match app.repo()?.skip_commit().map_err(|e| first_line(&e))? {
        Outcome::Done => Ok("Skipped that commit".into()),
        Outcome::Conflicts(files) => Err(format!(
            "Skipped — the next commit conflicts too, in {}",
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

/// A conflicted file as text and conflict regions, for picking a side per
/// region.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ConflictPart {
    Text {
        text: String,
    },
    Conflict {
        ours: String,
        ours_label: String,
        theirs: String,
        theirs_label: String,
        base: Option<String>,
    },
}

#[tauri::command(async)]
pub(crate) fn conflict_regions(app: State<App>, path: String) -> Result<Vec<ConflictPart>, String> {
    let repo = app.repo()?;
    let text = std::fs::read_to_string(repo.path(&path))
        .map_err(|e| format!("Couldn't read {path}: {e}"))?;
    Ok(spor::conflict::parse(&text)
        .into_iter()
        .map(|s| match s {
            spor::conflict::Segment::Text(text) => ConflictPart::Text { text },
            spor::conflict::Segment::Conflict(r) => ConflictPart::Conflict {
                ours: r.ours,
                ours_label: r.ours_label,
                theirs: r.theirs,
                theirs_label: r.theirs_label,
                base: r.base,
            },
        })
        .collect())
}

/// Write the file with a side picked for each region (ours, theirs, both,
/// both_theirs_first), and mark it resolved.
#[tauri::command(async)]
pub(crate) fn resolve_regions(
    app: State<App>,
    path: String,
    choices: Vec<String>,
) -> Result<String, String> {
    use spor::conflict::{self, Choice};
    let repo = app.repo()?;
    let file = repo.path(&path);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("Couldn't read {path}: {e}"))?;
    let choices = choices
        .iter()
        .map(|c| match c.as_str() {
            "ours" => Ok(Choice::Ours),
            "theirs" => Ok(Choice::Theirs),
            "both" => Ok(Choice::Both),
            "both_theirs_first" => Ok(Choice::BothTheirsFirst),
            other => Err(format!("Unknown choice {other}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let resolved = conflict::resolve(&conflict::parse(&text), &choices)?;
    std::fs::write(&file, resolved).map_err(|e| format!("Couldn't write {path}: {e}"))?;
    repo.mark_resolved(&path).map_err(|e| first_line(&e))?;
    Ok(format!("Resolved {path}"))
}
