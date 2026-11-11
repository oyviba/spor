//! History: the commit graph, a commit's details, and what can be done to a
//! commit or a branch from the graph.
//!
//! The lanes come from `spor::graph::assign_lanes`. What to draw in each row
//! is worked out here, so the web side only maps segments to SVG paths and
//! never sees the lane bookkeeping. Rows are compact: colors are indices into one palette.

use crate::undo::Undo;
use crate::{first_line, App};
use serde::{Deserialize, Serialize};
use spor::color::color_for;
use spor::git::{Outcome, RebaseStep, Repo, ResetMode};
use spor::graph::{assign_lanes, GraphRow};
use std::collections::{HashMap, HashSet};
use tauri::State;

/// More lanes than this are folded into the last one.
const MAX_LANES: usize = 12;

/// How one lane is drawn in one row. The UI knows the shapes:
/// `through` top to bottom; `top`/`bottom` a half; `in` the node's own lane
/// from the top to the node, `out` from the node down; `join` a curve from
/// the top of a lane into the node (a branch reaching its fork point);
/// `fork` a curve from the node to the bottom of a lane (a merge parent).
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Shape {
    Through,
    Top,
    Bottom,
    In,
    Out,
    Join,
    Fork,
}

#[derive(Serialize)]
pub(crate) struct Segment {
    lane: usize,
    shape: Shape,
    color: usize,
    /// The default branch's line, drawn bold.
    bold: bool,
}

#[derive(Serialize)]
pub(crate) struct Row {
    hash: String,
    short: String,
    subject: String,
    author: String,
    time: i64,
    /// Branch and tag names pointing here; tags start with "tag: ".
    refs: Vec<String>,
    head: bool,
    merge: bool,
    lane: usize,
    color: usize,
    segments: Vec<Segment>,
}

#[derive(Serialize)]
pub(crate) struct Graph {
    rows: Vec<Row>,
    /// Lane colors as `#rrggbb`, tuned for dark backgrounds.
    palette: Vec<String>,
    lanes: usize,
    /// git log stopped at the limit: there's more to load.
    more: bool,
    main: Option<String>,
}

#[tauri::command(async)]
pub(crate) fn graph(app: State<App>, limit: usize) -> Result<Graph, String> {
    let repo = app.repo()?;
    let remotes = repo.remotes();
    let commits = repo.log_all(limit.max(1))?;
    let more = commits.len() >= limit;
    let chain: HashSet<String> = repo.main_chain().unwrap_or_default().into_iter().collect();
    let rows = assign_lanes(&commits, &chain, &remotes);
    let main = repo.main_branch();
    // The default branch's tip row: lane 0 is bold from there down.
    let main_tip = main.as_ref().and_then(|m| {
        rows.iter()
            .position(|r| r.commit.refs.iter().any(|x| x == m))
    });

    let mut palette: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut color = |family: &str, hint: &str| -> usize {
        let (r, g, b) = color_for(family, hint);
        let hex = format!("#{r:02x}{g:02x}{b:02x}");
        *index.entry(hex.clone()).or_insert_with(|| {
            palette.push(hex);
            palette.len() - 1
        })
    };

    let mut lanes = 1;
    let rows = rows
        .iter()
        .enumerate()
        .map(|(ix, row)| {
            let hint = row
                .commit
                .refs
                .first()
                .cloned()
                .unwrap_or_else(|| row.commit.hash.clone());
            let family_of = |lane: usize| -> String {
                if lane == row.lane {
                    row.branch_family.clone()
                } else {
                    row.lane_families
                        .get(lane)
                        .and_then(|f| f.clone())
                        .unwrap_or_else(|| "_".into())
                }
            };
            let bold_top = main_tip.is_some_and(|t| ix > t);
            let bold_bottom = main_tip.is_some_and(|t| ix >= t);
            let mut segments = Vec::new();
            let count = row
                .lanes_before
                .len()
                .max(row.lanes_after.len())
                .max(row.lane + 1)
                .min(MAX_LANES);
            lanes = lanes.max(count);
            for lane in 0..count {
                let c = color(&family_of(lane), &hint);
                let mut add = |shape, top_half: bool| {
                    segments.push(Segment {
                        lane,
                        shape,
                        color: c,
                        bold: lane == 0 && if top_half { bold_top } else { bold_bottom },
                    })
                };
                shapes(row, lane, &mut add);
            }
            Row {
                hash: row.commit.hash.clone(),
                short: row.commit.short.clone(),
                subject: row.commit.subject.clone(),
                author: row.commit.author.clone(),
                time: row.commit.timestamp,
                refs: row.commit.refs.clone(),
                head: row.commit.head_ref.is_some(),
                merge: row.commit.parents.len() > 1,
                lane: row.lane.min(MAX_LANES - 1),
                color: color(&row.branch_family, &hint),
                segments,
            }
        })
        .collect();
    Ok(Graph {
        rows,
        palette,
        lanes,
        more,
        main,
    })
}

/// The shapes lane `lane` needs in `row`, as `paint_row` draws them. `add`
/// takes the shape and whether it's in the top half (for the bold main line).
fn shapes(row: &GraphRow, lane: usize, add: &mut impl FnMut(Shape, bool)) {
    let hash = row.commit.hash.as_str();
    let before = row.lanes_before.get(lane).and_then(|s| s.as_deref());
    let after = row.lanes_after.get(lane).and_then(|s| s.as_deref());
    if lane == row.lane {
        if before == Some(hash) {
            add(Shape::In, true);
        }
        if after.is_some() {
            add(Shape::Out, false);
        }
        return;
    }
    let passing = before.is_some() && before == after;
    if before == Some(hash) {
        add(Shape::Join, true);
    } else if passing {
        add(Shape::Through, true);
    } else if before.is_some() {
        add(Shape::Top, true);
    }
    if let Some(a) = after.filter(|_| !passing) {
        if row.commit.parents.iter().skip(1).any(|p| p == a) {
            add(Shape::Fork, false);
        } else {
            add(Shape::Bottom, false);
        }
    }
}

#[tauri::command(async)]
pub(crate) fn search_commits(app: State<App>, query: String) -> Result<Vec<String>, String> {
    let repo = app.repo()?;
    let query = query.trim();
    // `path:src/main.rs` — the commits that touched a file (following renames).
    if let Some(path) = query.strip_prefix("path:") {
        let path = path.trim();
        if path.is_empty() {
            return Ok(Vec::new());
        }
        return Ok(repo
            .file_log(path, 2000)?
            .into_iter()
            .map(|c| c.hash)
            .collect());
    }
    // `code:needle` — the commits that added or removed some text.
    if let Some(text) = query.strip_prefix("code:") {
        return repo.search_code(text.trim(), 5000);
    }
    // Otherwise every word, in the message, author, hash or a branch or tag name.
    repo.search_commits(query, 5000)
}

#[derive(Serialize)]
pub(crate) struct CommitDetail {
    hash: String,
    parents: Vec<String>,
    author: String,
    email: String,
    date: String,
    time: i64,
    message: String,
    patch: String,
    /// The agent task that made it, from the `Spor-Task` trailer (#108).
    task: Option<TaskRef>,
}

/// An agent task behind a commit.
#[derive(Serialize)]
pub(crate) struct TaskRef {
    id: String,
    title: String,
    prompt: String,
    /// "open", "landed" or "closed".
    state: &'static str,
}

/// The task a commit message names, if spor still has it.
fn task_behind(tasks: &spor::tasks::Tasks, message: &str) -> Option<TaskRef> {
    let task = tasks.get(spor::tasks::Task::trailer_id(message)?)?;
    Some(TaskRef {
        state: match task.state {
            spor::tasks::TaskState::Open => "open",
            spor::tasks::TaskState::Landed(_) => "landed",
            spor::tasks::TaskState::Closed => "closed",
        },
        title: task.title(),
        prompt: task.prompt.trim().chars().take(400).collect(),
        id: task.id,
    })
}

#[tauri::command(async)]
pub(crate) fn commit_detail(app: State<App>, hash: String) -> Result<CommitDetail, String> {
    let repo = app.repo()?;
    let details = repo.commit_details(&hash)?;
    let parents = repo.parents(&hash)?;
    let patch = repo.commit_patch(&hash, &parents)?;
    let task = spor::tasks::Tasks::open(&repo)
        .ok()
        .and_then(|tasks| task_behind(&tasks, &details.message));
    Ok(CommitDetail {
        task,
        hash,
        parents,
        author: details.author,
        email: details.email,
        date: details.date,
        time: details.author_time,
        message: details.message,
        patch,
    })
}

#[derive(Serialize)]
pub(crate) struct BranchInfo {
    name: String,
    current: bool,
    remote: bool,
    agent: bool,
}

#[tauri::command(async)]
pub(crate) fn branches(app: State<App>) -> Result<Vec<BranchInfo>, String> {
    Ok(app
        .repo()?
        .list_branches()?
        .into_iter()
        .map(|b| BranchInfo {
            agent: b.name.starts_with("agent/"),
            name: b.name,
            current: b.is_current,
            remote: b.is_remote,
        })
        .collect())
}

/// What a switch says when uncommitted changes are in the way. The UI
/// offers to stash them and try again ("Stash and switch").
const BLOCKED: &str = "BLOCKED: ";

/// Switch with `switch`, stashing uncommitted changes first when `stash`.
/// Records the way back (and, for a new branch, its deletion) for Undo.
fn switching(
    app: &App,
    repo: &Repo,
    target: &str,
    stash: bool,
    created: Option<&str>,
    switch: impl FnOnce() -> Result<(), String>,
) -> Result<String, String> {
    let from = repo.tracking()?.branch;
    if stash {
        repo.stash_push().map_err(|e| first_line(&e))?;
    }
    if let Err(e) = switch() {
        if stash {
            // Put the changes back where they were.
            let _ = repo.stash_pop();
        }
        return Err(if spor::git::is_worktree_conflict(&e) {
            format!("{BLOCKED}Switching to {target} would overwrite uncommitted changes")
        } else {
            first_line(&e)
        });
    }
    if let Some(name) = created {
        app.undo.push(
            format!("Create {name}"),
            Undo::Created {
                name: name.to_string(),
            },
        );
    }
    let now = repo
        .tracking()?
        .branch
        .unwrap_or_else(|| target.to_string());
    if let Some(from) = from {
        app.undo
            .push(format!("Switch to {now}"), Undo::Checkout { from });
    }
    Ok(match (created, stash) {
        (Some(_), false) => format!("Created and switched to {now}"),
        (Some(_), true) => format!("Stashed your changes, created {now} and switched to it"),
        (None, false) => format!("Switched to {now}"),
        (None, true) => {
            format!("Stashed your changes and switched to {now} — they're under Stashes in Changes")
        }
    })
}

/// Check out a local branch, or a remote one as a new local branch that
/// tracks it.
#[tauri::command(async)]
pub(crate) fn checkout(app: State<App>, branch: String, stash: bool) -> Result<String, String> {
    let repo = app.repo()?;
    switching(&app, &repo, &branch, stash, None, || {
        repo.checkout_branch(&branch)
    })
}

/// Create a branch at `at` and switch to it.
#[tauri::command(async)]
pub(crate) fn create_branch(
    app: State<App>,
    name: String,
    at: String,
    stash: bool,
) -> Result<String, String> {
    let repo = app.repo()?;
    let name = name.trim().to_string();
    if !repo.valid_branch_name(&name) {
        return Err(format!("“{name}” isn't a valid branch name"));
    }
    switching(&app, &repo, &name, stash, Some(&name), || {
        repo.create_branch_at(&name, &at)
    })
}

#[tauri::command(async)]
pub(crate) fn delete_branch(app: State<App>, name: String) -> Result<String, String> {
    let repo = app.repo()?;
    let sha = repo
        .delete_branch(&name, true)
        .map_err(|e| first_line(&e))?;
    app.undo.push(
        format!("Delete {name}"),
        Undo::Deleted {
            name: name.clone(),
            sha,
        },
    );
    Ok(format!("Deleted {name}"))
}

/// Run an operation that moves the checked-out branch, recording how to
/// move it back. Conflicts leave git mid-operation; the app's conflict
/// banner takes it from there, and Abort is the way back.
fn moving(
    app: &App,
    repo: &Repo,
    label: String,
    op: impl FnOnce() -> Result<Outcome, String>,
) -> Result<String, String> {
    let branch = repo.tracking()?.branch;
    let before = repo.head_sha()?;
    match op().map_err(|e| first_line(&e))? {
        Outcome::Done => {
            let after = repo.head_sha()?;
            if after != before {
                app.undo.push(
                    label.clone(),
                    Undo::Moved {
                        branch,
                        before,
                        after,
                        mode: ResetMode::Keep,
                    },
                );
            }
            Ok(label)
        }
        Outcome::Conflicts(files) => Err(format!(
            "Stopped on conflicts in {} — resolve them in Changes, or abort",
            files.join(", ")
        )),
    }
}

#[tauri::command(async)]
pub(crate) fn cherry_pick(app: State<App>, hash: String) -> Result<String, String> {
    let repo = app.repo()?;
    let short: String = hash.chars().take(7).collect();
    moving(&app, &repo, format!("Cherry-pick {short}"), || {
        repo.cherry_pick(&hash)
    })
}

#[tauri::command(async)]
pub(crate) fn revert(app: State<App>, hash: String) -> Result<String, String> {
    let repo = app.repo()?;
    let short: String = hash.chars().take(7).collect();
    moving(&app, &repo, format!("Revert {short}"), || {
        repo.revert(&hash)
    })
}

#[tauri::command(async)]
pub(crate) fn merge_branch(app: State<App>, branch: String) -> Result<String, String> {
    let repo = app.repo()?;
    let into = repo.tracking()?.branch.unwrap_or_else(|| "HEAD".into());
    moving(&app, &repo, format!("Merge {branch} into {into}"), || {
        repo.merge(&branch)
    })
}

#[tauri::command(async)]
pub(crate) fn rename_branch(app: State<App>, old: String, new: String) -> Result<String, String> {
    let repo = app.repo()?;
    let new = new.trim().to_string();
    if !repo.valid_branch_name(&new) {
        return Err(format!("“{new}” isn't a valid branch name"));
    }
    repo.rename_branch(&old, &new).map_err(|e| first_line(&e))?;
    app.undo.push(
        format!("Rename {old} to {new}"),
        Undo::Renamed {
            old: old.clone(),
            new: new.clone(),
        },
    );
    Ok(format!("Renamed {old} to {new}"))
}

/// Delete a branch on its remote: `origin/feat/x` removes `feat/x` from
/// `origin`. Can't be undone from here, so the UI asks first.
#[tauri::command(async)]
pub(crate) fn delete_remote_branch(app: State<App>, name: String) -> Result<String, String> {
    let repo = app.repo()?;
    let remotes = repo.remotes();
    let (remote, branch) = remotes
        .iter()
        .find_map(|r| {
            name.strip_prefix(&format!("{r}/"))
                .map(|b| (r.clone(), b.to_string()))
        })
        .ok_or_else(|| format!("{name} isn't a remote branch"))?;
    spor::askpass::git_network(repo.root(), &["push", &remote, "--delete", &branch])?;
    Ok(format!("Deleted {branch} from {remote}"))
}

/// Rebase the checked-out branch onto `onto` (a branch or a commit).
#[tauri::command(async)]
pub(crate) fn rebase_onto(app: State<App>, onto: String) -> Result<String, String> {
    let repo = app.repo()?;
    let branch = repo.tracking()?.branch.unwrap_or_else(|| "HEAD".into());
    let label = format!("Rebase {branch} onto {}", short_ref(&onto));
    moving(&app, &repo, label, || repo.rebase(&onto))
}

/// Move the checked-out branch to `hash`. `soft` keeps the changes staged,
/// `mixed` in the files, `hard` throws them away (the UI asks first).
#[tauri::command(async)]
pub(crate) fn reset_to(app: State<App>, hash: String, mode: String) -> Result<String, String> {
    let repo = app.repo()?;
    let (mode, how) = match mode.as_str() {
        "soft" => (ResetMode::Soft, "keeping the changes staged"),
        "mixed" => (ResetMode::Mixed, "keeping the changes in the files"),
        "hard" => (ResetMode::Hard, "discarding the changes"),
        other => return Err(format!("Unknown reset mode {other}")),
    };
    let branch = repo.tracking()?.branch;
    let before = repo.head_sha()?;
    repo.reset(&hash, mode).map_err(|e| first_line(&e))?;
    let after = repo.head_sha()?;
    let short = short_ref(&hash);
    if before != after {
        // Back to where it was: the commits return; with soft and mixed
        // the changes they'd held are still in the index or the files.
        app.undo.push(
            format!("Reset to {short}"),
            Undo::Moved {
                branch: branch.clone(),
                before,
                after,
                mode: if mode == ResetMode::Hard {
                    ResetMode::Keep
                } else {
                    ResetMode::Soft
                },
            },
        );
    }
    Ok(format!(
        "Reset {} to {short}, {how}",
        branch.as_deref().unwrap_or("HEAD")
    ))
}

/// Look at a commit without a branch (a detached HEAD).
#[tauri::command(async)]
pub(crate) fn checkout_commit(
    app: State<App>,
    hash: String,
    stash: bool,
) -> Result<String, String> {
    let repo = app.repo()?;
    let short = short_ref(&hash);
    switching(&app, &repo, &short, stash, None, || {
        repo.checkout_detached(&hash)
    })
}

#[derive(Serialize)]
pub(crate) struct TagInfo {
    name: String,
    /// The commit it's on.
    hash: String,
}

#[tauri::command(async)]
pub(crate) fn tags(app: State<App>) -> Result<Vec<TagInfo>, String> {
    let repo = app.repo()?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)%09%(*objectname)%09%(objectname)",
            "refs/tags",
        ])
        .output()
        .map_err(|e| format!("Couldn't run git: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let name = it.next()?.to_string();
            let peeled = it.next().unwrap_or("");
            let own = it.next().unwrap_or("");
            let hash = if peeled.is_empty() { own } else { peeled };
            Some(TagInfo {
                name,
                hash: hash.to_string(),
            })
        })
        .collect())
}

#[tauri::command(async)]
pub(crate) fn create_tag(
    app: State<App>,
    name: String,
    hash: String,
    message: Option<String>,
) -> Result<String, String> {
    let repo = app.repo()?;
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(format!("“{name}” isn't a valid tag name"));
    }
    repo.create_tag(&name, &hash, message.as_deref())
        .map_err(|e| first_line(&e))?;
    app.undo.push(
        format!("Tag {name}"),
        Undo::TagCreated { name: name.clone() },
    );
    Ok(format!("Tagged {} as {name}", short_ref(&hash)))
}

#[tauri::command(async)]
pub(crate) fn delete_tag(app: State<App>, name: String) -> Result<String, String> {
    let repo = app.repo()?;
    let target = repo.tag_target(&name).map_err(|e| first_line(&e))?;
    repo.delete_tag(&name).map_err(|e| first_line(&e))?;
    app.undo.push(
        format!("Delete tag {name}"),
        Undo::TagDeleted {
            name: name.clone(),
            target,
        },
    );
    Ok(format!("Deleted tag {name}"))
}

/// Push a tag to the default remote, or delete it there.
#[tauri::command(async)]
pub(crate) fn push_tag(app: State<App>, name: String, delete: bool) -> Result<String, String> {
    let repo = app.repo()?;
    let remote = repo
        .remotes()
        .into_iter()
        .next()
        .ok_or("This repository has no remote")?;
    let refspec = format!("refs/tags/{name}");
    if delete {
        spor::askpass::git_network(repo.root(), &["push", &remote, "--delete", &refspec])?;
        Ok(format!("Deleted tag {name} from {remote}"))
    } else {
        spor::askpass::git_network(repo.root(), &["push", &remote, &refspec])?;
        Ok(format!("Pushed tag {name} to {remote}"))
    }
}

#[derive(Serialize)]
pub(crate) struct WorktreeInfo {
    path: String,
    branch: Option<String>,
    head: String,
    main: bool,
    /// The one this window has open.
    current: bool,
    prunable: bool,
    /// An agent task's folder: removed by landing or discarding the task.
    agent: bool,
}

#[tauri::command(async)]
pub(crate) fn worktrees(app: State<App>) -> Result<Vec<WorktreeInfo>, String> {
    let repo = app.repo()?;
    let here = std::fs::canonicalize(repo.root()).unwrap_or_else(|_| repo.root().to_path_buf());
    Ok(repo
        .worktrees()?
        .into_iter()
        .map(|w| {
            let canon = std::fs::canonicalize(&w.path).unwrap_or_else(|_| w.path.clone());
            WorktreeInfo {
                current: canon == here,
                agent: w.branch.as_deref().is_some_and(|b| b.starts_with("agent/")),
                path: w.path.display().to_string(),
                branch: w.branch,
                head: w.head,
                main: w.is_main,
                prunable: w.prunable,
            }
        })
        .collect())
}

/// Remove a linked worktree. git refuses one with uncommitted changes.
#[tauri::command(async)]
pub(crate) fn remove_worktree(app: State<App>, path: String) -> Result<String, String> {
    let repo = app.repo()?;
    repo.remove_worktree(std::path::Path::new(&path), false)
        .map_err(|e| first_line(&e))?;
    Ok(format!("Removed the worktree at {path}"))
}

#[derive(Serialize)]
pub(crate) struct PlanCommit {
    hash: String,
    short: String,
    subject: String,
    /// The whole message, to start a reword from.
    message: String,
    author: String,
    time: i64,
}

/// The commits an interactive rebase from `base` would rewrite, oldest
/// first. Refuses history with merges, which a linear plan would flatten.
#[tauri::command(async)]
pub(crate) fn rewrite_plan(app: State<App>, base: String) -> Result<Vec<PlanCommit>, String> {
    let repo = app.repo()?;
    if repo.tracking()?.branch.is_none() {
        return Err("Switch to a branch first".into());
    }
    if !repo.is_ancestor(&base, "HEAD") {
        return Err(format!(
            "{} isn't on the checked-out branch",
            short_ref(&base)
        ));
    }
    if repo.has_merges_since(&base) {
        return Err("There are merges after that commit — rewriting would flatten them".into());
    }
    repo.commits_since(&base)?
        .into_iter()
        .map(|c| {
            let message = repo
                .commit_details(&c.hash)
                .map(|d| d.message)
                .unwrap_or_else(|_| c.subject.clone());
            Ok(PlanCommit {
                hash: c.hash,
                short: c.short,
                subject: c.subject,
                message,
                author: c.author,
                time: c.timestamp,
            })
        })
        .collect()
}

#[derive(Deserialize)]
pub(crate) struct PlanStep {
    hash: String,
    /// pick, reword, squash, fixup or drop.
    action: String,
    message: Option<String>,
}

/// Rewrite the commits after `base` following `plan`, oldest first.
#[tauri::command(async)]
pub(crate) fn rewrite(
    app: State<App>,
    base: String,
    plan: Vec<PlanStep>,
) -> Result<String, String> {
    let repo = app.repo()?;
    let mut kept = 0;
    let steps = plan
        .into_iter()
        .map(|s| {
            if s.action != "drop" {
                kept += 1;
            }
            Ok(match s.action.as_str() {
                "pick" => RebaseStep::Pick(s.hash),
                "reword" => RebaseStep::Reword(
                    s.hash,
                    s.message
                        .filter(|m| !m.trim().is_empty())
                        .ok_or("A reworded commit needs a message")?,
                ),
                "squash" => RebaseStep::Squash(s.hash),
                "fixup" => RebaseStep::Fixup(s.hash),
                "drop" => RebaseStep::Drop(s.hash),
                other => return Err(format!("Unknown step {other}")),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let Some(RebaseStep::Squash(_) | RebaseStep::Fixup(_)) =
        steps.iter().find(|s| !matches!(s, RebaseStep::Drop(_)))
    {
        return Err(
            "The first commit can't be squashed — there's nothing before it to fold into".into(),
        );
    }
    let label = format!("Rewrite history after {}", short_ref(&base));
    moving(&app, &repo, label, || {
        repo.rebase_interactive(&base, &steps)
    })?;
    Ok(format!(
        "Rewrote history: {kept} {} after {}",
        if kept == 1 { "commit" } else { "commits" },
        short_ref(&base)
    ))
}

/// Open pull requests by branch, for the badges on the graph's labels.
/// Empty where `gh` isn't there or the remote isn't GitHub.
#[derive(Serialize)]
pub(crate) struct PrBadge {
    branch: String,
    number: u64,
    draft: bool,
    /// none, approved, changesrequested.
    review: String,
    /// none, pending, passing, failing.
    checks: String,
}

#[tauri::command(async)]
pub(crate) fn pr_badges(app: State<App>) -> Result<Vec<PrBadge>, String> {
    let repo = app.repo()?;
    Ok(app
        .github
        .open_pulls(repo.root())
        .unwrap_or_default()
        .into_iter()
        .map(|p| PrBadge {
            draft: p.state == spor::remote::PrState::Draft,
            review: crate::name_of(&p.review),
            checks: crate::name_of(&p.checks),
            branch: p.head,
            number: p.number,
        })
        .collect())
}

/// The checked-out branch's pull request page, or the host's page to open
/// one. Needs the branch published.
#[tauri::command(async)]
pub(crate) fn pull_request_url(app: State<App>) -> Result<String, String> {
    let repo = app.repo()?;
    let tracking = repo.tracking()?;
    let head = tracking.branch.ok_or("Switch to a branch first")?;
    let info =
        spor::remote::detect_in(repo.root()).ok_or("Couldn't tell where origin is hosted")?;
    let base = repo.default_base_branch().unwrap_or_else(|| "main".into());
    if base == head {
        return Err(format!(
            "You're on {base} — switch to a feature branch first"
        ));
    }
    if tracking.upstream.is_none() {
        return Err("Publish this branch first (Push)".into());
    }
    let existing = app
        .github
        .open_pulls(repo.root())
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.head == head);
    Ok(match existing {
        Some(pr) => format!("{}/pull/{}", info.web_url, pr.number),
        None => spor::remote::compare_url(&info, &base, &head),
    })
}

/// A hash shortened, a ref name left alone.
fn short_ref(r: &str) -> String {
    if r.len() >= 12 && r.chars().all(|c| c.is_ascii_hexdigit()) {
        r[..7].to_string()
    } else {
        r.to_string()
    }
}

#[derive(Serialize)]
pub(crate) struct BlameRow {
    /// Empty for a line that isn't committed yet.
    hash: String,
    author: String,
    time: i64,
    summary: String,
    line: usize,
    text: String,
    /// The agent task that made this line's commit, if spor has it (#108).
    task: Option<String>,
}

/// Who last changed each line of `path`, as of `rev` (the working tree when
/// `None`, uncommitted lines included).
#[tauri::command(async)]
pub(crate) fn blame(
    app: State<App>,
    path: String,
    rev: Option<String>,
) -> Result<Vec<BlameRow>, String> {
    let repo = app.repo()?;
    let tasks = spor::tasks::Tasks::open(&repo).ok();
    // Which commit came from which task: asked once per commit.
    let mut made_by: std::collections::HashMap<String, Option<String>> = Default::default();
    Ok(repo
        .blame_at(&path, rev.as_deref())
        .map_err(|e| first_line(&e))?
        .into_iter()
        .map(|l| {
            let committed = !l.hash.chars().all(|c| c == '0');
            let task = if committed {
                made_by
                    .entry(l.hash.clone())
                    .or_insert_with(|| {
                        let message = repo.message(&l.hash).ok()?;
                        let id = spor::tasks::Task::trailer_id(&message)?;
                        tasks.as_ref()?.get(id).map(|t| t.id)
                    })
                    .clone()
            } else {
                None
            };
            BlameRow {
                task,
                hash: if committed { l.hash } else { String::new() },
                author: if committed { l.author } else { "You".into() },
                time: l.time,
                summary: if committed {
                    l.summary
                } else {
                    "Not committed yet".into()
                },
                line: l.line,
                text: l.text,
            }
        })
        .collect())
}

/// Merge `source` into `target`, checking `target` out first when it isn't
/// already (which needs a clean working tree). ⌘Z takes back the merge; a
/// second ⌘Z switches back.
#[tauri::command(async)]
pub(crate) fn merge_into(
    app: State<App>,
    source: String,
    target: String,
) -> Result<String, String> {
    let repo = app.repo()?;
    let from = repo.tracking()?.branch;
    if from.as_deref() != Some(target.as_str()) {
        if !repo.status()?.is_empty() {
            return Err(format!(
                "Commit or stash your changes before merging into {target}"
            ));
        }
        repo.checkout_branch(&target).map_err(|e| first_line(&e))?;
        if let Some(from) = from {
            app.undo
                .push(format!("Switch to {target}"), Undo::Checkout { from });
        }
    }
    moving(&app, &repo, format!("Merge {source} into {target}"), || {
        repo.merge(&source)
    })
}

/// The host's page for a pull request from `source` into `target` (remote
/// prefixes dropped: `origin/feat/x` is `feat/x`).
#[tauri::command(async)]
pub(crate) fn pull_request_between(
    app: State<App>,
    source: String,
    target: String,
) -> Result<String, String> {
    let repo = app.repo()?;
    let info =
        spor::remote::detect_in(repo.root()).ok_or("Couldn't tell where origin is hosted")?;
    let remotes = repo.remotes();
    let local = |name: &str| -> String {
        remotes
            .iter()
            .find_map(|r| name.strip_prefix(&format!("{r}/")))
            .unwrap_or(name)
            .to_string()
    };
    let (head, base) = (local(&source), local(&target));
    if head == base {
        return Err("That's the same branch".into());
    }
    Ok(spor::remote::compare_url(&info, &base, &head))
}
