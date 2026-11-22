//! Pull requests and Actions, through spor's `gh` wrappers
//! (`spor::github`). Every call says plainly why it can't work (no origin,
//! not GitHub, no `gh`) so the sections can show that instead of a list.

use crate::{first_line, name_of, App};
use serde::{Deserialize, Serialize};
use spor::github as gh;
use tauri::State;

#[derive(Serialize, Clone)]
pub(crate) struct Pull {
    number: u64,
    title: String,
    author: String,
    head: String,
    base: String,
    /// open, draft, merged, closed.
    state: String,
    /// none, approved, changesrequested.
    review: String,
    /// none, pending, passing, failing.
    checks: String,
    updated: i64,
    additions: usize,
    deletions: usize,
    url: String,
    /// Whose review is requested and not given yet: logins, and teams as
    /// `org/slug`.
    reviewers: Vec<String>,
}

impl From<gh::Pull> for Pull {
    fn from(p: gh::Pull) -> Self {
        Self {
            number: p.number,
            title: p.title,
            author: p.author,
            head: p.head,
            base: p.base,
            state: name_of(&p.state),
            review: name_of(&p.review),
            checks: name_of(&p.checks),
            updated: p.updated,
            additions: p.additions,
            deletions: p.deletions,
            url: p.url,
            reviewers: p.reviewers,
        }
    }
}

fn root(app: &App) -> Result<std::path::PathBuf, String> {
    Ok(app.repo()?.root().to_path_buf())
}

/// `filter`: open, yours, review, closed. Open, yours and review come out
/// of one cached list of open pull requests (see `cache`), so the sections
/// that show them share one `gh pr list`.
#[tauri::command(async)]
pub(crate) fn pulls(app: State<App>, filter: String) -> Result<Vec<Pull>, String> {
    let dir = root(&app)?;
    let cache = &app.github;
    let list = match filter.as_str() {
        "yours" => cache.yours(&dir, &gh::repo(&dir)?),
        "review" => {
            let numbers = cache
                .review_numbers(&gh::repo(&dir)?)
                .map_err(|e| first_line(&e))?;
            cache.open_pulls(&dir).map(|all| {
                all.into_iter()
                    .filter(|p| numbers.contains(&p.number))
                    .collect()
            })
        }
        "closed" => gh::list_pulls(&dir, gh::PullFilter::Closed),
        _ => cache.open_pulls(&dir),
    };
    Ok(list
        .map_err(|e| first_line(&e))?
        .into_iter()
        .map(Pull::from)
        .collect())
}

#[derive(Serialize)]
pub(crate) struct Check {
    name: String,
    workflow: String,
    status: String,
    url: String,
    started: i64,
    completed: i64,
    run_id: Option<u64>,
    job_id: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct Comment {
    author: String,
    body: String,
    at: i64,
    /// approved, changesrequested, commented, dismissed; None for a comment.
    verdict: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PullDetail {
    pull: Pull,
    body: String,
    /// yes, conflicting, unknown.
    mergeable: String,
    changed_files: usize,
    commits: usize,
    checks: Vec<Check>,
    timeline: Vec<Comment>,
    assignees: Vec<String>,
    labels: Vec<String>,
}

#[tauri::command(async)]
pub(crate) fn pull_detail(app: State<App>, number: u64) -> Result<PullDetail, String> {
    let d = gh::pull_detail(&root(&app)?, number).map_err(|e| first_line(&e))?;
    Ok(PullDetail {
        pull: d.pull.into(),
        body: d.body,
        mergeable: name_of(&d.mergeable),
        changed_files: d.changed_files,
        commits: d.commits,
        checks: d
            .checks
            .into_iter()
            .map(|c| Check {
                name: c.name,
                workflow: c.workflow,
                status: name_of(&c.status),
                url: c.url,
                started: c.started,
                completed: c.completed,
                run_id: c.run_id,
                job_id: c.job_id,
            })
            .collect(),
        timeline: d
            .timeline
            .into_iter()
            .map(|c| Comment {
                author: c.author,
                body: c.body,
                at: c.at,
                verdict: c.verdict.map(|v| name_of(&v)),
            })
            .collect(),
        assignees: d.assignees,
        labels: d.labels,
    })
}

#[tauri::command(async)]
pub(crate) fn pull_diff(app: State<App>, number: u64) -> Result<String, String> {
    gh::pull_diff(&root(&app)?, number).map_err(|e| first_line(&e))
}

/// `action`: merge, squash, rebase (the ways to land it on GitHub), approve, comment (with
/// `body`), checkout, ready, draft (back to draft), close, reopen.
#[tauri::command(async)]
pub(crate) fn pull_action(
    app: State<App>,
    number: u64,
    action: String,
    body: Option<String>,
) -> Result<String, String> {
    let dir = root(&app)?;
    let n = number;
    let result = match action.as_str() {
        "merge" => gh::merge_pull(&dir, n, gh::MergeMethod::Merge)
            .map(|_| format!("Landed #{n} with a merge commit")),
        "squash" => {
            gh::merge_pull(&dir, n, gh::MergeMethod::Squash).map(|_| format!("Landed #{n}"))
        }
        "rebase" => gh::merge_pull(&dir, n, gh::MergeMethod::Rebase)
            .map(|_| format!("Landed #{n} by rebasing")),
        "approve" => {
            if let Some(b) = body.as_deref().filter(|b| !b.trim().is_empty()) {
                gh::comment_on_pull(&dir, n, b)?;
            }
            gh::approve_pull(&dir, n).map(|_| format!("Approved #{n}"))
        }
        "comment" => {
            let b = body.unwrap_or_default();
            if b.trim().is_empty() {
                return Err("Write a comment first".into());
            }
            gh::comment_on_pull(&dir, n, &b).map(|_| format!("Commented on #{n}"))
        }
        "checkout" => gh::checkout_pull(&dir, n).map(|_| format!("Checked out #{n}")),
        "ready" => gh::mark_ready(&dir, n).map(|_| format!("#{n} is ready for review")),
        "draft" => gh::mark_draft(&dir, n).map(|_| format!("#{n} is a draft again")),
        "close" => gh::set_pull_open(&dir, n, false).map(|_| format!("Closed #{n}")),
        "reopen" => gh::set_pull_open(&dir, n, true).map(|_| format!("Reopened #{n}")),
        other => return Err(format!("Unknown action {other}")),
    };
    app.github.clear(&dir);
    result.map_err(|e| first_line(&e))
}

/// Who's asked to review a PR, who's assigned and how it's labelled:
/// what to add and what to take off.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct PullEdit {
    add_reviewers: Vec<String>,
    remove_reviewers: Vec<String>,
    add_assignees: Vec<String>,
    remove_assignees: Vec<String>,
    add_labels: Vec<String>,
    remove_labels: Vec<String>,
}

impl From<PullEdit> for gh::PullEdit {
    fn from(e: PullEdit) -> Self {
        gh::PullEdit {
            add_reviewers: e.add_reviewers,
            remove_reviewers: e.remove_reviewers,
            add_assignees: e.add_assignees,
            remove_assignees: e.remove_assignees,
            add_labels: e.add_labels,
            remove_labels: e.remove_labels,
        }
    }
}

/// Request or withdraw reviews, assign, label (#149).
#[tauri::command(async)]
pub(crate) fn pull_edit(app: State<App>, number: u64, edit: PullEdit) -> Result<String, String> {
    let dir = root(&app)?;
    let edit: gh::PullEdit = edit.into();
    if edit.is_empty() {
        return Ok(String::new());
    }
    let message = edit_message(number, &edit);
    let result = gh::edit_pull(&dir, number, &edit).map(|_| message);
    app.github.clear(&dir);
    result.map_err(|e| first_line(&e))
}

/// "Asked ada to review #12", or a plain "Updated #12" for a mix.
fn edit_message(n: u64, e: &gh::PullEdit) -> String {
    let parts = [
        (&e.add_reviewers, "Asked", " to review"),
        (&e.remove_reviewers, "No longer asking", " to review"),
        (&e.add_assignees, "Assigned", " to"),
        (&e.remove_assignees, "Unassigned", " from"),
        (&e.add_labels, "Added", " to"),
        (&e.remove_labels, "Removed", " from"),
    ];
    let mut said = parts.iter().filter(|(v, _, _)| !v.is_empty());
    match (said.next(), said.next()) {
        (Some((names, verb, tail)), None) => format!("{verb} {}{tail} #{n}", names.join(", ")),
        _ => format!("Updated #{n}"),
    }
}

/// Who can be asked to review or be assigned (collaborators, then teams),
/// and the labels there are.
#[derive(Serialize)]
pub(crate) struct PullCandidates {
    people: Vec<String>,
    labels: Vec<String>,
}

#[tauri::command(async)]
pub(crate) fn pull_candidates(app: State<App>) -> Result<PullCandidates, String> {
    let dir = root(&app)?;
    // Without push access GitHub won't list collaborators; labels still work.
    let people = gh::collaborators(&dir).unwrap_or_default();
    let labels = gh::labels(&dir).map_err(|e| first_line(&e))?;
    Ok(PullCandidates { people, labels })
}

/// What a new pull request starts with in this repository (#149).
#[derive(Serialize, Deserialize)]
pub(crate) struct PullPreset {
    draft: bool,
    reviewers: Vec<String>,
    assignees: Vec<String>,
    labels: Vec<String>,
}

impl From<gh::PullPreset> for PullPreset {
    fn from(p: gh::PullPreset) -> Self {
        Self {
            draft: p.draft,
            reviewers: p.reviewers,
            assignees: p.assignees,
            labels: p.labels,
        }
    }
}

impl From<PullPreset> for gh::PullPreset {
    fn from(p: PullPreset) -> Self {
        Self {
            draft: p.draft,
            reviewers: p.reviewers,
            assignees: p.assignees,
            labels: p.labels,
        }
    }
}

#[tauri::command(async)]
pub(crate) fn pull_preset(app: State<App>) -> Result<PullPreset, String> {
    Ok(gh::PullPreset::load(&root(&app)?).into())
}

#[tauri::command(async)]
pub(crate) fn set_pull_preset(app: State<App>, preset: PullPreset) -> Result<(), String> {
    gh::PullPreset::from(preset).save(&root(&app)?)
}

#[derive(Serialize)]
pub(crate) struct Run {
    id: u64,
    number: u64,
    workflow: String,
    title: String,
    branch: String,
    sha: String,
    event: String,
    /// queued, running, success, failure, cancelled, skipped, neutral.
    status: String,
    attempt: u64,
    created: i64,
    updated: i64,
    url: String,
}

impl From<gh::Run> for Run {
    fn from(r: gh::Run) -> Self {
        Self {
            id: r.id,
            number: r.number,
            workflow: r.workflow,
            title: r.title,
            branch: r.branch,
            sha: r.sha,
            event: r.event,
            status: name_of(&r.status),
            attempt: r.attempt,
            created: r.created,
            updated: r.updated,
            url: r.url,
        }
    }
}

/// Every branch's runs come from the cache; one branch's or workflow's are
/// asked for, since the cached list holds only the most recent.
#[tauri::command(async)]
pub(crate) fn runs(
    app: State<App>,
    branch: Option<String>,
    workflow: Option<String>,
) -> Result<Vec<Run>, String> {
    let dir = root(&app)?;
    let list = if branch.is_none() && workflow.is_none() {
        app.github.runs(&dir)
    } else {
        gh::list_runs(&dir, branch.as_deref(), workflow.as_deref())
    };
    Ok(list
        .map_err(|e| first_line(&e))?
        .into_iter()
        .map(Run::from)
        .collect())
}

#[tauri::command(async)]
pub(crate) fn workflows(app: State<App>) -> Result<Vec<String>, String> {
    gh::list_workflows(&root(&app)?).map_err(|e| first_line(&e))
}

#[derive(Serialize)]
pub(crate) struct Step {
    number: u64,
    name: String,
    status: String,
}

#[derive(Serialize)]
pub(crate) struct Job {
    id: u64,
    name: String,
    status: String,
    started: i64,
    completed: i64,
    url: String,
    steps: Vec<Step>,
}

#[derive(Serialize)]
pub(crate) struct RunDetail {
    run: Run,
    jobs: Vec<Job>,
}

#[tauri::command(async)]
pub(crate) fn run_detail(app: State<App>, id: u64) -> Result<RunDetail, String> {
    let d = gh::run_detail(&root(&app)?, id).map_err(|e| first_line(&e))?;
    Ok(RunDetail {
        run: d.run.into(),
        jobs: d
            .jobs
            .into_iter()
            .map(|j| Job {
                id: j.id,
                name: j.name,
                status: name_of(&j.status),
                started: j.started,
                completed: j.completed,
                url: j.url,
                steps: j
                    .steps
                    .into_iter()
                    .map(|s| Step {
                        number: s.number,
                        name: s.name,
                        status: name_of(&s.status),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// `action`: rerun, rerun_failed, cancel.
#[tauri::command(async)]
pub(crate) fn run_action(app: State<App>, id: u64, action: String) -> Result<String, String> {
    let dir = root(&app)?;
    let result = match action.as_str() {
        "rerun" => gh::rerun(&dir, id, false).map(|_| "Re-running all jobs".to_string()),
        "rerun_failed" => gh::rerun(&dir, id, true).map(|_| "Re-running failed jobs".to_string()),
        "cancel" => gh::cancel_run(&dir, id).map(|_| "Cancelling the run".to_string()),
        other => return Err(format!("Unknown action {other}")),
    };
    app.github.clear(&dir);
    result.map_err(|e| first_line(&e))
}

#[derive(Serialize)]
pub(crate) struct LogLine {
    /// plain, group, command, error, warning, notice.
    kind: String,
    text: String,
}

#[tauri::command(async)]
pub(crate) fn job_log(app: State<App>, id: u64) -> Result<Vec<LogLine>, String> {
    Ok(gh::job_log(&root(&app)?, id)
        .map_err(|e| first_line(&e))?
        .into_iter()
        .map(|l| LogLine {
            kind: name_of(&l.kind),
            text: l.text,
        })
        .collect())
}

/// A failed job (its failing step and the end of its log), as a follow-up
/// for the agent working on its branch.
#[tauri::command(async)]
pub(crate) fn job_feedback(app: State<App>, run: u64, job: u64) -> Result<String, String> {
    let failure = gh::fetch_job_failure(&root(&app)?, run, job).map_err(|e| first_line(&e))?;
    gh::failure_feedback(&[failure], None).ok_or_else(|| "Nothing failed".to_string())
}

// ---- Issues ----------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct Issue {
    number: u64,
    title: String,
    author: String,
    open: bool,
    labels: Vec<String>,
    assignees: Vec<String>,
    comments: usize,
    updated: i64,
    url: String,
}

impl From<gh::Issue> for Issue {
    fn from(i: gh::Issue) -> Self {
        Self {
            number: i.number,
            title: i.title,
            author: i.author,
            open: i.open,
            labels: i.labels,
            assignees: i.assignees,
            comments: i.comments,
            updated: i.updated,
            url: i.url,
        }
    }
}

/// `filter`: assigned (cached, as Work reads it too), yours, open, closed.
#[tauri::command(async)]
pub(crate) fn issues(app: State<App>, filter: String) -> Result<Vec<Issue>, String> {
    let dir = root(&app)?;
    let list = match filter.as_str() {
        "assigned" => app.github.assigned_issues(&dir),
        "yours" => gh::list_issues(&dir, gh::IssueFilter::Yours),
        "closed" => gh::list_issues(&dir, gh::IssueFilter::Closed),
        _ => gh::list_issues(&dir, gh::IssueFilter::Open),
    };
    Ok(list
        .map_err(|e| first_line(&e))?
        .into_iter()
        .map(Issue::from)
        .collect())
}

#[derive(Serialize)]
pub(crate) struct IssueDetail {
    issue: Issue,
    body: String,
    comments: Vec<Comment>,
    /// The prompt an agent task on it starts from.
    prompt: String,
}

#[tauri::command(async)]
pub(crate) fn issue_detail(app: State<App>, number: u64) -> Result<IssueDetail, String> {
    let d = gh::issue_detail(&root(&app)?, number).map_err(|e| first_line(&e))?;
    let prompt = d.task_prompt();
    Ok(IssueDetail {
        issue: d.issue.into(),
        body: d.body,
        comments: d
            .comments
            .into_iter()
            .map(|c| Comment {
                author: c.author,
                body: c.body,
                at: c.at,
                verdict: None,
            })
            .collect(),
        prompt,
    })
}

/// `action`: comment (with `body`), close, reopen.
#[tauri::command(async)]
pub(crate) fn issue_action(
    app: State<App>,
    number: u64,
    action: String,
    body: Option<String>,
) -> Result<String, String> {
    let dir = root(&app)?;
    let n = number;
    let result = match action.as_str() {
        "comment" => {
            let b = body.unwrap_or_default();
            if b.trim().is_empty() {
                return Err("Write a comment first".into());
            }
            gh::comment_on_issue(&dir, n, &b).map(|_| format!("Commented on #{n}"))
        }
        "close" => gh::set_issue_open(&dir, n, false).map(|_| format!("Closed #{n}")),
        "reopen" => gh::set_issue_open(&dir, n, true).map(|_| format!("Reopened #{n}")),
        other => return Err(format!("Unknown action {other}")),
    };
    app.github.clear(&dir);
    result.map_err(|e| first_line(&e))
}
