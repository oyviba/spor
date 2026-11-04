//! History: the commit graph, a commit's details, and what can be done to a
//! commit or a branch from the graph.
//!
//! The lanes come from `spor::graph::assign_lanes`. What to draw in each row
//! is worked out here, mirroring the GPUI painter (`src/gui/graph.rs`), so
//! the web side only maps segments to SVG paths and never sees the lane
//! bookkeeping. Rows are compact: colors are indices into one palette.

use crate::undo::Undo;
use crate::{first_line, App};
use serde::Serialize;
use spor::color::color_for;
use spor::git::{Outcome, Repo, ResetMode};
use spor::graph::{assign_lanes, GraphRow};
use std::collections::{HashMap, HashSet};
use tauri::State;

/// More lanes than this are folded into the last one, as in the GPUI app.
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
    app.repo()?.search_commits(query.trim(), 500)
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
}

#[tauri::command(async)]
pub(crate) fn commit_detail(app: State<App>, hash: String) -> Result<CommitDetail, String> {
    let repo = app.repo()?;
    let details = repo.commit_details(&hash)?;
    let parents = repo.parents(&hash)?;
    let patch = repo.commit_patch(&hash, &parents)?;
    Ok(CommitDetail {
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
/// offers to stash them and try again (the GPUI app's "Stash and Switch").
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
