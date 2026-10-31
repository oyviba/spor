//! GitHub as a place to work, not just a badge: pull requests with their
//! description, checks, reviews and comments; Actions runs with their jobs,
//! steps and logs; and the things you do to them (merge, approve, comment,
//! re-run, cancel).
//!
//! Everything goes through the GitHub CLI (`gh`), which already holds the
//! user's credentials. Its built-in jq flattens each answer to tagged,
//! tab-separated lines, so the core still parses no JSON. Every call is slow
//! (network) — run them off the UI thread.

use crate::remote::{self, ChecksState, Host, PrState, ReviewState};
use std::path::Path;
use std::process::Command;

/// The repository `gh` is pointed at: `origin`, named explicitly so a second
/// remote never makes `gh` ask which one is meant.
#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    /// `github.com`, or an Enterprise host.
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl Repo {
    /// `HOST/OWNER/REPO`, the form `gh -R` takes.
    fn selector(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.name)
    }
}

/// Find the GitHub repository behind `dir`, or say why there isn't one.
pub fn repo(dir: &Path) -> Result<Repo, String> {
    let info = remote::detect_in(dir).ok_or("This repository has no origin remote")?;
    if info.host != Host::GitHub {
        return Err("Pull requests and Actions work with GitHub repositories".into());
    }
    remote::cli_tool(&info.host).ok_or(
        "Install the GitHub CLI (gh) and run `gh auth login` to see pull requests and Actions",
    )?;
    let host = info
        .web_url
        .split_once("://")
        .map_or(info.web_url.as_str(), |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("github.com")
        .to_string();
    Ok(Repo {
        host,
        owner: info.owner,
        name: info.repo,
    })
}

/// Run `gh` against `repo` and return its output, or its first error line.
fn gh(dir: &Path, repo: &Repo, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("gh");
    cmd.current_dir(dir).args(args);
    // `gh api` takes the host separately; everything else takes -R.
    if args.first() == Some(&"api") {
        cmd.args(["--hostname", &repo.host]);
    } else {
        cmd.args(["-R", &repo.selector()]);
    }
    let out = cmd.output().map_err(|e| format!("Couldn't run gh: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("gh failed")
            .to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── Shared vocabulary ────────────────────────────────────────────────────────

/// Where a check, run, job or step stands. GitHub reports a status
/// (queued / in progress / completed) and, once completed, a conclusion;
/// this folds the two into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Queued,
    Running,
    Success,
    Failure,
    Cancelled,
    Skipped,
    Neutral,
}

impl Status {
    /// From a status / conclusion pair, or a commit status's single state.
    pub fn from_parts(status: &str, conclusion: &str) -> Self {
        match conclusion.to_ascii_uppercase().as_str() {
            "SUCCESS" => return Status::Success,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED" => {
                return Status::Failure
            }
            "CANCELLED" => return Status::Cancelled,
            "SKIPPED" | "STALE" => return Status::Skipped,
            "NEUTRAL" => return Status::Neutral,
            _ => {}
        }
        match status.to_ascii_uppercase().as_str() {
            "IN_PROGRESS" | "PENDING" => Status::Running,
            "SUCCESS" => Status::Success,
            "FAILURE" | "ERROR" => Status::Failure,
            "COMPLETED" => Status::Neutral,
            _ => Status::Queued, // QUEUED, WAITING, REQUESTED, EXPECTED
        }
    }

    /// Still going: queued or running.
    pub fn active(self) -> bool {
        matches!(self, Status::Queued | Status::Running)
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Running => "In progress",
            Status::Success => "Success",
            Status::Failure => "Failed",
            Status::Cancelled => "Cancelled",
            Status::Skipped => "Skipped",
            Status::Neutral => "Neutral",
        }
    }
}

/// Undo jq's `@tsv` escaping.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => {}
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `2026-10-30T11:34:05Z` (or with an offset or fraction) to Unix seconds.
/// 0 when empty or unreadable — GitHub uses `0001-01-01T00:00:00Z` for
/// "hasn't happened", which also comes out as ≤ 0.
pub fn parse_time(s: &str) -> i64 {
    let s = s.trim();
    if s.len() < 19 {
        return 0;
    }
    let num = |range: std::ops::Range<usize>| s.get(range)?.parse::<i64>().ok();
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(sec)) = (
        num(0..4),
        num(5..7),
        num(8..10),
        num(11..13),
        num(14..16),
        num(17..19),
    ) else {
        return 0;
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut t = days * 86_400 + h * 3600 + mi * 60 + sec;
    // A trailing ±HH:MM offset.
    let rest = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
        let oh = rest
            .get(1..3)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let om = rest
            .get(4..6)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let off = oh * 3600 + om * 60;
        t += if sign == '+' { -off } else { off };
    }
    t.max(0)
}

/// "1m 23s", "2h 5m", "8s".
pub fn duration(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
    }
}

// ── Pull requests ────────────────────────────────────────────────────────────

/// Which pull requests the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullFilter {
    Open,
    /// Open ones you opened.
    Yours,
    /// Open ones waiting for your review.
    ReviewRequested,
    /// Merged and closed.
    Closed,
}

impl PullFilter {
    pub const ALL: [PullFilter; 4] = [
        PullFilter::Open,
        PullFilter::Yours,
        PullFilter::ReviewRequested,
        PullFilter::Closed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PullFilter::Open => "Open",
            PullFilter::Yours => "Yours",
            PullFilter::ReviewRequested => "Review Requested",
            PullFilter::Closed => "Closed",
        }
    }
}

/// A pull request as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pull {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub head: String,
    pub base: String,
    pub state: PrState,
    pub review: ReviewState,
    pub checks: ChecksState,
    /// Unix seconds.
    pub updated: i64,
    pub additions: usize,
    pub deletions: usize,
    pub url: String,
}

const PULL_FIELDS: &str =
    "number,title,author,headRefName,baseRefName,state,isDraft,reviewDecision,statusCheckRollup,updatedAt,additions,deletions,url";

/// The jq that turns one PR object into a `Pull` line, prefixed by `tag`.
fn pull_jq(tag: &str) -> String {
    format!(
        r#"["{tag}", (.number|tostring), .title, (.author.login // ""), .headRefName, .baseRefName, (if .isDraft and .state == "OPEN" then "DRAFT" else .state end), (.reviewDecision // ""), ([.statusCheckRollup[]? | ((.conclusion // .state // "") | if . == "" then "PENDING" else . end)] | join(",")), .updatedAt, (.additions|tostring), (.deletions|tostring), .url] | @tsv"#
    )
}

fn parse_pull(f: &[&str]) -> Option<Pull> {
    if f.len() < 12 {
        return None;
    }
    Some(Pull {
        number: f[0].parse().ok()?,
        title: unescape(f[1]),
        author: f[2].to_string(),
        head: f[3].to_string(),
        base: f[4].to_string(),
        state: match f[5] {
            "DRAFT" => PrState::Draft,
            "MERGED" => PrState::Merged,
            "CLOSED" => PrState::Closed,
            _ => PrState::Open,
        },
        review: match f[6] {
            "APPROVED" => ReviewState::Approved,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            _ => ReviewState::None,
        },
        checks: remote::rollup_checks(f[7]),
        updated: parse_time(f[8]),
        additions: f[9].parse().unwrap_or(0),
        deletions: f[10].parse().unwrap_or(0),
        url: f[11].to_string(),
    })
}

fn parse_pulls(out: &str) -> Vec<Pull> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.first() == Some(&"pr")).then(|| parse_pull(&f[1..]))?
        })
        .collect()
}

/// The pull requests `filter` picks, most recently updated first.
pub fn list_pulls(dir: &Path, filter: PullFilter) -> Result<Vec<Pull>, String> {
    let repo = repo(dir)?;
    let jq = format!(".[] | {}", pull_jq("pr"));
    let mut args = vec!["pr", "list", "--limit", "100", "--json", PULL_FIELDS];
    match filter {
        PullFilter::Open => args.extend(["--state", "open"]),
        PullFilter::Yours => args.extend(["--state", "open", "--author", "@me"]),
        PullFilter::ReviewRequested => {
            args.extend(["--state", "open", "--search", "review-requested:@me"])
        }
        PullFilter::Closed => args.extend(["--state", "closed"]),
    }
    args.extend(["--jq", &jq]);
    let mut pulls = parse_pulls(&gh(dir, &repo, &args)?);
    pulls.sort_by_key(|p| std::cmp::Reverse(p.updated));
    Ok(pulls)
}

/// Whether GitHub would let the PR merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeable {
    Yes,
    Conflicting,
    /// Not computed yet, or not applicable (merged, closed).
    Unknown,
}

/// One check on the PR's head commit: an Actions job, another app's check
/// run, or a commit status.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub name: String,
    /// The workflow an Actions job belongs to; empty otherwise.
    pub workflow: String,
    pub status: Status,
    pub url: String,
    pub started: i64,
    pub completed: i64,
    /// Set when the check is an Actions job, so it can be opened in-app.
    pub run_id: Option<u64>,
    pub job_id: Option<u64>,
}

/// What a review said, for timeline entries that are reviews.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

/// A comment or a review, in the order they happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub at: i64,
    pub verdict: Option<Verdict>,
}

/// Everything the PR page shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PullDetail {
    pub pull: Pull,
    pub body: String,
    pub mergeable: Mergeable,
    pub changed_files: usize,
    pub commits: usize,
    pub checks: Vec<Check>,
    pub timeline: Vec<Comment>,
}

/// One `gh pr view` call, flattened to a `pr` line, a `meta` line, and one
/// line per check, review and comment.
fn pull_detail_jq() -> String {
    format!(
        concat!(
            "({pull}), ",
            r#"(["meta", .body, (.mergeable // ""), (.changedFiles|tostring), (.commits|length|tostring)] | @tsv), "#,
            r#"(.statusCheckRollup[]? | if .__typename == "StatusContext" then ["check", .context, "", (.state // ""), "", (.targetUrl // ""), (.startedAt // ""), ""] else ["check", .name, (.workflowName // ""), (.status // ""), (.conclusion // ""), (.detailsUrl // ""), (.startedAt // ""), (.completedAt // "")] end | @tsv), "#,
            r#"(.reviews[]? | ["review", (.author.login // ""), .body, .submittedAt, .state] | @tsv), "#,
            r#"(.comments[]? | ["comment", (.author.login // ""), .body, .createdAt] | @tsv)"#,
        ),
        pull = pull_jq("pr")
    )
}

/// `…/actions/runs/123/job/456` → (123, 456).
fn actions_ids(url: &str) -> (Option<u64>, Option<u64>) {
    let mut run = None;
    let mut job = None;
    let mut parts = url.split('/');
    while let Some(p) = parts.next() {
        match p {
            "runs" => run = parts.next().and_then(|v| v.parse().ok()),
            "job" | "jobs" => job = parts.next().and_then(|v| v.parse().ok()),
            _ => {}
        }
    }
    (run, job)
}

fn parse_pull_detail(out: &str) -> Option<PullDetail> {
    let mut pull = None;
    let mut body = String::new();
    let mut mergeable = Mergeable::Unknown;
    let mut changed_files = 0;
    let mut commits = 0;
    let mut checks = Vec::new();
    let mut timeline = Vec::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["pr", rest @ ..] => pull = parse_pull(rest),
            ["meta", b, m, files, count, ..] => {
                body = unescape(b);
                mergeable = match *m {
                    "MERGEABLE" => Mergeable::Yes,
                    "CONFLICTING" => Mergeable::Conflicting,
                    _ => Mergeable::Unknown,
                };
                changed_files = files.parse().unwrap_or(0);
                commits = count.parse().unwrap_or(0);
            }
            ["check", name, workflow, status, conclusion, url, started, completed, ..] => {
                let (run_id, job_id) = actions_ids(url);
                checks.push(Check {
                    name: unescape(name),
                    workflow: unescape(workflow),
                    status: Status::from_parts(status, conclusion),
                    url: url.to_string(),
                    started: parse_time(started),
                    completed: parse_time(completed),
                    run_id,
                    job_id,
                });
            }
            ["review", author, body, at, state, ..] => {
                let verdict = match *state {
                    "APPROVED" => Verdict::Approved,
                    "CHANGES_REQUESTED" => Verdict::ChangesRequested,
                    "DISMISSED" => Verdict::Dismissed,
                    _ => Verdict::Commented,
                };
                let body = unescape(body);
                // A bare "Commented" review is the shell of inline
                // comments, which the timeline doesn't show.
                if verdict == Verdict::Commented && body.trim().is_empty() {
                    continue;
                }
                timeline.push(Comment {
                    author: author.to_string(),
                    body,
                    at: parse_time(at),
                    verdict: Some(verdict),
                });
            }
            ["comment", author, body, at, ..] => timeline.push(Comment {
                author: author.to_string(),
                body: unescape(body),
                at: parse_time(at),
                verdict: None,
            }),
            _ => {}
        }
    }
    // Failing first, then running, then the rest; by name within each.
    let rank = |s: Status| match s {
        Status::Failure => 0,
        Status::Running | Status::Queued => 1,
        Status::Cancelled => 2,
        _ => 3,
    };
    checks.sort_by(|a, b| {
        rank(a.status)
            .cmp(&rank(b.status))
            .then_with(|| a.workflow.cmp(&b.workflow))
            .then_with(|| a.name.cmp(&b.name))
    });
    timeline.sort_by_key(|c| c.at);
    Some(PullDetail {
        pull: pull?,
        body,
        mergeable,
        changed_files,
        commits,
        checks,
        timeline,
    })
}

/// A pull request with its description, checks, reviews and comments.
pub fn pull_detail(dir: &Path, number: u64) -> Result<PullDetail, String> {
    let repo = repo(dir)?;
    let number = number.to_string();
    let fields = format!("{PULL_FIELDS},body,mergeable,changedFiles,commits,reviews,comments");
    let jq = pull_detail_jq();
    let out = gh(
        dir,
        &repo,
        &["pr", "view", &number, "--json", &fields, "--jq", &jq],
    )?;
    parse_pull_detail(&out).ok_or_else(|| format!("Couldn't read pull request #{number}"))
}

/// The PR's whole diff, as a patch.
pub fn pull_diff(dir: &Path, number: u64) -> Result<String, String> {
    let repo = repo(dir)?;
    gh(
        dir,
        &repo,
        &["pr", "diff", &number.to_string(), "--color", "never"],
    )
}

/// How `merge_pull` combines the branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const ALL: [MergeMethod; 3] =
        [MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase];

    pub fn label(self) -> &'static str {
        match self {
            MergeMethod::Merge => "Create a Merge Commit",
            MergeMethod::Squash => "Squash and Merge",
            MergeMethod::Rebase => "Rebase and Merge",
        }
    }

    fn flag(self) -> &'static str {
        match self {
            MergeMethod::Merge => "--merge",
            MergeMethod::Squash => "--squash",
            MergeMethod::Rebase => "--rebase",
        }
    }
}

/// Merge the PR on GitHub. Its branch stays; GitHub can delete it on merge.
pub fn merge_pull(dir: &Path, number: u64, method: MergeMethod) -> Result<(), String> {
    let repo = repo(dir)?;
    gh(
        dir,
        &repo,
        &[
            "pr",
            "merge",
            &number.to_string(),
            method.flag(),
            "--delete-branch=false",
        ],
    )
    .map(drop)
}

/// Check the PR's branch out locally (works for forks too).
pub fn checkout_pull(dir: &Path, number: u64) -> Result<(), String> {
    let repo = repo(dir)?;
    gh(dir, &repo, &["pr", "checkout", &number.to_string()]).map(drop)
}

/// Approve the PR.
pub fn approve_pull(dir: &Path, number: u64) -> Result<(), String> {
    let repo = repo(dir)?;
    gh(
        dir,
        &repo,
        &["pr", "review", &number.to_string(), "--approve"],
    )
    .map(drop)
}

/// Add a comment to the PR's conversation.
pub fn comment_on_pull(dir: &Path, number: u64, body: &str) -> Result<(), String> {
    let repo = repo(dir)?;
    gh(
        dir,
        &repo,
        &["pr", "comment", &number.to_string(), "--body", body],
    )
    .map(drop)
}

/// Take a draft out of draft.
pub fn mark_ready(dir: &Path, number: u64) -> Result<(), String> {
    let repo = repo(dir)?;
    gh(dir, &repo, &["pr", "ready", &number.to_string()]).map(drop)
}

/// Close the PR without merging, or reopen a closed one.
pub fn set_pull_open(dir: &Path, number: u64, open: bool) -> Result<(), String> {
    let repo = repo(dir)?;
    let verb = if open { "reopen" } else { "close" };
    gh(dir, &repo, &["pr", verb, &number.to_string()]).map(drop)
}

// ── Actions ──────────────────────────────────────────────────────────────────

/// One workflow run.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: u64,
    /// The run's number within its workflow (`#42`).
    pub number: u64,
    pub workflow: String,
    /// The commit subject or PR title the run is for.
    pub title: String,
    pub branch: String,
    pub sha: String,
    /// push, pull_request, workflow_dispatch, schedule, …
    pub event: String,
    pub status: Status,
    pub attempt: u64,
    pub created: i64,
    pub updated: i64,
    pub url: String,
}

const RUN_FIELDS: &str =
    "databaseId,number,workflowName,displayTitle,headBranch,headSha,event,status,conclusion,attempt,createdAt,updatedAt,url";

const RUN_JQ: &str = r#"["run", (.databaseId|tostring), (.number|tostring), .workflowName, .displayTitle, .headBranch, .headSha, .event, (.status // ""), (.conclusion // ""), ((.attempt // 1)|tostring), .createdAt, .updatedAt, .url] | @tsv"#;

fn parse_run(f: &[&str]) -> Option<Run> {
    if f.len() < 13 {
        return None;
    }
    Some(Run {
        id: f[0].parse().ok()?,
        number: f[1].parse().unwrap_or(0),
        workflow: unescape(f[2]),
        title: unescape(f[3]),
        branch: f[4].to_string(),
        sha: f[5].to_string(),
        event: f[6].to_string(),
        status: Status::from_parts(f[7], f[8]),
        attempt: f[9].parse().unwrap_or(1),
        created: parse_time(f[10]),
        updated: parse_time(f[11]),
        url: f[12].to_string(),
    })
}

fn parse_runs(out: &str) -> Vec<Run> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.first() == Some(&"run")).then(|| parse_run(&f[1..]))?
        })
        .collect()
}

/// The most recent workflow runs, newest first; only `branch`'s if given.
pub fn list_runs(dir: &Path, branch: Option<&str>) -> Result<Vec<Run>, String> {
    let repo = repo(dir)?;
    let jq = format!(".[] | {RUN_JQ}");
    let mut args = vec!["run", "list", "--limit", "60", "--json", RUN_FIELDS];
    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    }
    args.extend(["--jq", &jq]);
    Ok(parse_runs(&gh(dir, &repo, &args)?))
}

/// One step of a job.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub number: u64,
    pub name: String,
    pub status: Status,
}

/// One job of a run, with its steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: Status,
    pub started: i64,
    pub completed: i64,
    pub url: String,
    pub steps: Vec<Step>,
}

impl Job {
    /// How long it ran (or has been running, given `now`).
    pub fn elapsed(&self, now: i64) -> Option<i64> {
        if self.started <= 0 {
            return None;
        }
        let end = if self.completed > 0 {
            self.completed
        } else {
            now
        };
        Some(end - self.started)
    }
}

/// A run and its jobs.
#[derive(Debug, Clone, PartialEq)]
pub struct RunDetail {
    pub run: Run,
    pub jobs: Vec<Job>,
}

fn run_detail_jq() -> String {
    format!(
        concat!(
            "({run}), ",
            r#"(.jobs[]? | (["job", (.databaseId|tostring), .name, (.status // ""), (.conclusion // ""), (.startedAt // ""), (.completedAt // ""), (.url // "")] | @tsv), "#,
            r#"(.steps[]? | ["step", (.number|tostring), .name, (.status // ""), (.conclusion // "")] | @tsv))"#,
        ),
        run = RUN_JQ
    )
}

fn parse_run_detail(out: &str) -> Option<RunDetail> {
    let mut run = None;
    let mut jobs: Vec<Job> = Vec::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["run", rest @ ..] => run = parse_run(rest),
            ["job", id, name, status, conclusion, started, completed, url, ..] => jobs.push(Job {
                id: id.parse().unwrap_or(0),
                name: unescape(name),
                status: Status::from_parts(status, conclusion),
                started: parse_time(started),
                completed: parse_time(completed),
                url: url.to_string(),
                steps: Vec::new(),
            }),
            ["step", number, name, status, conclusion, ..] => {
                if let Some(job) = jobs.last_mut() {
                    job.steps.push(Step {
                        number: number.parse().unwrap_or(0),
                        name: unescape(name),
                        status: Status::from_parts(status, conclusion),
                    });
                }
            }
            _ => {}
        }
    }
    Some(RunDetail { run: run?, jobs })
}

/// A run with its jobs and their steps.
pub fn run_detail(dir: &Path, id: u64) -> Result<RunDetail, String> {
    let repo = repo(dir)?;
    let id = id.to_string();
    let fields = format!("{RUN_FIELDS},jobs");
    let jq = run_detail_jq();
    let out = gh(
        dir,
        &repo,
        &["run", "view", &id, "--json", &fields, "--jq", &jq],
    )?;
    parse_run_detail(&out).ok_or_else(|| format!("Couldn't read run {id}"))
}

/// Run the whole workflow again, or only its failed jobs.
pub fn rerun(dir: &Path, id: u64, failed_only: bool) -> Result<(), String> {
    let repo = repo(dir)?;
    let id = id.to_string();
    let mut args = vec!["run", "rerun", id.as_str()];
    if failed_only {
        args.push("--failed");
    }
    gh(dir, &repo, &args).map(drop)
}

/// Stop a queued or running run.
pub fn cancel_run(dir: &Path, id: u64) -> Result<(), String> {
    let repo = repo(dir)?;
    gh(dir, &repo, &["run", "cancel", &id.to_string()]).map(drop)
}

/// What a log line is, for coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Plain,
    /// `##[group]` — a step's or a section's title.
    Group,
    /// `[command]` — what the runner ran.
    Command,
    Error,
    Warning,
    Notice,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    pub kind: LogKind,
    pub text: String,
}

/// Remove ANSI escape sequences (`ESC [ … letter`).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// A compiler's or tool's own error line: `error: …`, `error[E0425]: …`,
/// `Error: …`.
fn is_error_line(line: &str) -> bool {
    let rest = line
        .strip_prefix("error")
        .or_else(|| line.strip_prefix("Error"));
    match rest {
        Some(rest) => rest.starts_with(": ") || (rest.starts_with('[') && rest.contains("]: ")),
        None => false,
    }
}

/// Parse a raw job log: drop the runner's timestamps and ANSI colors, and
/// turn its workflow commands into line kinds.
pub fn parse_log(raw: &str) -> Vec<LogLine> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches('\u{feff}');
            // Every line starts with an RFC 3339 timestamp and a space.
            let line = match line.split_once(' ') {
                Some((stamp, rest)) if stamp.len() >= 20 && parse_time(stamp) > 0 => rest,
                _ => line,
            };
            let line = strip_ansi(line);
            let (kind, text) = if let Some(t) = line.strip_prefix("##[group]") {
                (LogKind::Group, t)
            } else if line.starts_with("##[endgroup]") {
                return None;
            } else if let Some(t) = line.strip_prefix("##[error]") {
                (LogKind::Error, t)
            } else if let Some(t) = line.strip_prefix("##[warning]") {
                (LogKind::Warning, t)
            } else if let Some(t) = line.strip_prefix("##[notice]") {
                (LogKind::Notice, t)
            } else if let Some(t) = line.strip_prefix("##[debug]") {
                (LogKind::Plain, t)
            } else if let Some(t) = line.strip_prefix("[command]") {
                (LogKind::Command, t)
            } else if is_error_line(&line) {
                (LogKind::Error, line.as_str())
            } else {
                (LogKind::Plain, line.as_str())
            };
            Some(LogLine {
                kind,
                text: text.replace('\t', "    "),
            })
        })
        .collect()
}

/// A finished job's log. GitHub only serves it once the job is done.
pub fn job_log(dir: &Path, job_id: u64) -> Result<Vec<LogLine>, String> {
    let repo = repo(dir)?;
    let path = format!(
        "repos/{}/{}/actions/jobs/{job_id}/logs",
        repo.owner, repo.name
    );
    gh(dir, &repo, &["api", &path]).map(|raw| parse_log(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_tsv() {
        assert_eq!(unescape(r"a\nb\tc\\d"), "a\nb\tc\\d");
        assert_eq!(unescape(r"line\r\nnext"), "line\nnext");
        assert_eq!(unescape("plain"), "plain");
    }

    #[test]
    fn parse_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), 0);
        assert_eq!(parse_time("2024-02-29T12:00:00Z"), 1_709_208_000);
        assert_eq!(parse_time("2024-02-29T14:00:00+02:00"), 1_709_208_000);
        assert_eq!(parse_time("2024-02-29T12:00:00.123Z"), 1_709_208_000);
        assert_eq!(parse_time("0001-01-01T00:00:00Z"), 0);
        assert_eq!(parse_time(""), 0);
        assert_eq!(parse_time("garbage"), 0);
    }

    #[test]
    fn durations() {
        assert_eq!(duration(8), "8s");
        assert_eq!(duration(83), "1m 23s");
        assert_eq!(duration(7500), "2h 5m");
    }

    #[test]
    fn status_folds_status_and_conclusion() {
        assert_eq!(Status::from_parts("completed", "success"), Status::Success);
        assert_eq!(Status::from_parts("COMPLETED", "FAILURE"), Status::Failure);
        assert_eq!(Status::from_parts("in_progress", ""), Status::Running);
        assert_eq!(Status::from_parts("queued", ""), Status::Queued);
        assert_eq!(Status::from_parts("completed", "skipped"), Status::Skipped);
        // A commit status has only a state.
        assert_eq!(Status::from_parts("PENDING", ""), Status::Running);
        assert_eq!(Status::from_parts("SUCCESS", ""), Status::Success);
        assert!(Status::Queued.active() && !Status::Cancelled.active());
    }

    #[test]
    fn parses_pull_list() {
        let out = "pr\t12\tAdd login\\twith tabs\toyviba\tfeat/login\tmain\tOPEN\tAPPROVED\tSUCCESS,SKIPPED\t2026-10-30T10:00:00Z\t120\t4\thttps://github.com/o/r/pull/12\n\
                   pr\t11\tWIP\tsomeone\twip\tmain\tDRAFT\t\t\t2026-10-29T10:00:00Z\t1\t0\thttps://github.com/o/r/pull/11\n\
                   junk\n";
        let pulls = parse_pulls(out);
        assert_eq!(pulls.len(), 2);
        assert_eq!(pulls[0].title, "Add login\twith tabs");
        assert_eq!(pulls[0].review, ReviewState::Approved);
        assert_eq!(pulls[0].checks, ChecksState::Passing);
        assert_eq!(pulls[0].additions, 120);
        assert_eq!(pulls[1].state, PrState::Draft);
        assert_eq!(pulls[1].checks, ChecksState::None);
    }

    #[test]
    fn parses_pull_detail() {
        let out = "pr\t7\tFix it\toyviba\tfix/x\tmain\tOPEN\t\tFAILURE,SUCCESS\t2026-10-30T10:00:00Z\t3\t1\thttps://github.com/o/r/pull/7\n\
                   meta\tFixes the thing.\\n\\n- one\\n- two\tMERGEABLE\t2\t3\n\
                   check\tcheck\tCI\tCOMPLETED\tSUCCESS\thttps://github.com/o/r/actions/runs/100/job/200\t2026-10-30T10:00:00Z\t2026-10-30T10:01:23Z\n\
                   check\tmacos-app\tCI\tCOMPLETED\tFAILURE\thttps://github.com/o/r/actions/runs/100/job/201\t2026-10-30T10:00:00Z\t2026-10-30T10:05:00Z\n\
                   check\tvercel\t\tSUCCESS\t\thttps://vercel.com/x\t\t\n\
                   review\talice\tLooks good\t2026-10-30T11:00:00Z\tAPPROVED\n\
                   review\tbob\t\t2026-10-30T09:00:00Z\tCOMMENTED\n\
                   comment\tcarol\tWhat about \\\\n?\t2026-10-30T10:30:00Z\n";
        let d = parse_pull_detail(out).unwrap();
        assert_eq!(d.pull.number, 7);
        assert_eq!(d.body, "Fixes the thing.\n\n- one\n- two");
        assert_eq!(d.mergeable, Mergeable::Yes);
        assert_eq!((d.changed_files, d.commits), (2, 3));
        assert_eq!(d.checks.len(), 3);
        // Failing first.
        assert_eq!(d.checks[0].name, "macos-app");
        assert_eq!(d.checks[0].run_id, Some(100));
        assert_eq!(d.checks[0].job_id, Some(201));
        let vercel = d.checks.iter().find(|c| c.name == "vercel").unwrap();
        assert_eq!(vercel.status, Status::Success);
        assert_eq!(vercel.job_id, None);
        // The empty COMMENTED review is dropped; the rest are in time order.
        assert_eq!(d.timeline.len(), 2);
        assert_eq!(d.timeline[0].author, "carol");
        assert_eq!(d.timeline[0].body, "What about \\n?");
        assert_eq!(d.timeline[1].verdict, Some(Verdict::Approved));
    }

    #[test]
    fn parses_runs_and_jobs() {
        let out = "run\t555\t42\tCI\tAdd a thing\tmain\tabc123\tpush\tcompleted\tfailure\t2\t2026-10-30T10:00:00Z\t2026-10-30T10:06:00Z\thttps://github.com/o/r/actions/runs/555\n\
                   job\t901\tcheck\tcompleted\tsuccess\t2026-10-30T10:00:10Z\t2026-10-30T10:03:00Z\thttps://github.com/o/r/actions/runs/555/job/901\n\
                   step\t1\tSet up job\tcompleted\tsuccess\n\
                   step\t2\tTest\tcompleted\tsuccess\n\
                   job\t902\tmacos-app\tin_progress\t\t2026-10-30T10:00:10Z\t\thttps://github.com/o/r/actions/runs/555/job/902\n\
                   step\t1\tBundle\tin_progress\t\n";
        let d = parse_run_detail(out).unwrap();
        assert_eq!(d.run.id, 555);
        assert_eq!(d.run.status, Status::Failure);
        assert_eq!(d.run.attempt, 2);
        assert_eq!(d.jobs.len(), 2);
        assert_eq!(d.jobs[0].steps.len(), 2);
        assert_eq!(d.jobs[0].elapsed(0), Some(170));
        assert_eq!(d.jobs[1].status, Status::Running);
        assert_eq!(d.jobs[1].steps[0].status, Status::Running);
        assert_eq!(parse_runs(out).len(), 1);
    }

    #[test]
    fn actions_ids_from_urls() {
        assert_eq!(
            actions_ids("https://github.com/o/r/actions/runs/12/job/34"),
            (Some(12), Some(34))
        );
        assert_eq!(actions_ids("https://example.com/status"), (None, None));
    }

    #[test]
    fn parses_logs() {
        let raw = "\u{feff}2026-10-30T10:00:00.1234567Z ##[group]Run cargo test\n\
                   2026-10-30T10:00:00.2Z [command]/usr/bin/cargo test\n\
                   2026-10-30T10:00:01.0Z \u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m spor v0.1.0\n\
                   2026-10-30T10:00:02.0Z ##[endgroup]\n\
                   2026-10-30T10:00:02.5Z error[E0425]: cannot find value `x` in this scope\n\
                   2026-10-30T10:00:02.6Z errors: 0\n\
                   2026-10-30T10:00:03.0Z ##[error]Process completed with exit code 101.\n\
                   no timestamp here\n";
        let lines = parse_log(raw);
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0].kind, LogKind::Group);
        assert_eq!(lines[0].text, "Run cargo test");
        assert_eq!(lines[1].kind, LogKind::Command);
        assert_eq!(lines[2].text, "   Compiling spor v0.1.0");
        assert_eq!(lines[3].kind, LogKind::Error);
        assert_eq!(lines[4].kind, LogKind::Plain);
        assert_eq!(lines[5].kind, LogKind::Error);
        assert_eq!(lines[6].text, "no timestamp here");
    }
}
