//! Pull requests and Actions, through spor's `gh` wrappers
//! (`spor::github`). Every call says plainly why it can't work (no origin,
//! not GitHub, no `gh`) so the sections can show that instead of a list.

use crate::{first_line, name_of, App};
use serde::Serialize;
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
        }
    }
}

fn root(app: &App) -> Result<std::path::PathBuf, String> {
    Ok(app.repo()?.root().to_path_buf())
}

/// `filter`: open, yours, review, closed.
#[tauri::command(async)]
pub(crate) fn pulls(app: State<App>, filter: String) -> Result<Vec<Pull>, String> {
    let filter = match filter.as_str() {
        "yours" => gh::PullFilter::Yours,
        "review" => gh::PullFilter::ReviewRequested,
        "closed" => gh::PullFilter::Closed,
        _ => gh::PullFilter::Open,
    };
    Ok(gh::list_pulls(&root(&app)?, filter)
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
    })
}

#[tauri::command(async)]
pub(crate) fn pull_diff(app: State<App>, number: u64) -> Result<String, String> {
    gh::pull_diff(&root(&app)?, number).map_err(|e| first_line(&e))
}

/// `action`: merge, squash, rebase (merge methods), approve, comment (with
/// `body`), checkout, ready, close, reopen.
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
        "merge" => gh::merge_pull(&dir, n, gh::MergeMethod::Merge).map(|_| format!("Merged #{n}")),
        "squash" => gh::merge_pull(&dir, n, gh::MergeMethod::Squash)
            .map(|_| format!("Squashed and merged #{n}")),
        "rebase" => gh::merge_pull(&dir, n, gh::MergeMethod::Rebase)
            .map(|_| format!("Rebased and merged #{n}")),
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
        "close" => gh::set_pull_open(&dir, n, false).map(|_| format!("Closed #{n}")),
        "reopen" => gh::set_pull_open(&dir, n, true).map(|_| format!("Reopened #{n}")),
        other => return Err(format!("Unknown action {other}")),
    };
    result.map_err(|e| first_line(&e))
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

#[tauri::command(async)]
pub(crate) fn runs(
    app: State<App>,
    branch: Option<String>,
    workflow: Option<String>,
) -> Result<Vec<Run>, String> {
    Ok(
        gh::list_runs(&root(&app)?, branch.as_deref(), workflow.as_deref())
            .map_err(|e| first_line(&e))?
            .into_iter()
            .map(Run::from)
            .collect(),
    )
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
    match action.as_str() {
        "rerun" => gh::rerun(&dir, id, false).map(|_| "Re-running all jobs".to_string()),
        "rerun_failed" => gh::rerun(&dir, id, true).map(|_| "Re-running failed jobs".to_string()),
        "cancel" => gh::cancel_run(&dir, id).map(|_| "Cancelling the run".to_string()),
        other => return Err(format!("Unknown action {other}")),
    }
    .map_err(|e| first_line(&e))
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

/// `filter`: assigned, yours, open, closed.
#[tauri::command(async)]
pub(crate) fn issues(app: State<App>, filter: String) -> Result<Vec<Issue>, String> {
    let filter = match filter.as_str() {
        "assigned" => gh::IssueFilter::Assigned,
        "yours" => gh::IssueFilter::Yours,
        "closed" => gh::IssueFilter::Closed,
        _ => gh::IssueFilter::Open,
    };
    Ok(gh::list_issues(&root(&app)?, filter)
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
    match action.as_str() {
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
    }
    .map_err(|e| first_line(&e))
}
