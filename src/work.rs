//! Work items: a branch together with whatever goes with it (the issue it
//! came from, a task folder and its agent, its pull request, its checks),
//! with the stage it's at and who it's waiting on. The words are the ones in
//! `docs/concepts.md` (#76).
//!
//! [`items`] is a pure join: it reads nothing itself. The backend gathers
//! tasks, branches, pull requests, runs and issues and hands them over in
//! [`Sources`]; everything that answers "what is this branch?" happens here,
//! once, rather than in each inspector.
//!
//! Things are matched by branch name: a task, a local branch, a pull request
//! whose head is that branch and the Actions runs on it come out as one
//! item. An issue with no branch yet is a *Planned* item when it's assigned
//! to you.

use crate::agent::{AgentKind, RunState};
use crate::github::{Issue, Pull, Run, Status};
use crate::remote::{ChecksState, PrState, ReviewState};
use crate::tasks::{Task, TaskState};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

// ── Inputs ───────────────────────────────────────────────────────────────────

/// A task, with what its agent is doing as far as the backend can tell.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskWork {
    pub task: Task,
    /// Its latest run; `None` when no agent has run in it.
    pub agent: Option<AgentState>,
}

/// A task's latest agent run.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentState {
    pub kind: AgentKind,
    pub state: RunState,
    /// Questions it's waiting on.
    pub asks: usize,
    /// Unix time it last did something (ended, or started).
    pub at: i64,
    /// Unix time you last looked at the task, if you have.
    pub seen: Option<i64>,
}

impl AgentState {
    /// It did something after you last looked.
    pub fn unseen(&self) -> bool {
        self.seen.is_none_or(|seen| seen < self.at)
    }
}

/// A local branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchWork {
    pub name: String,
    /// The folder it's checked out in: the main checkout, or another
    /// worktree.
    pub worktree: Option<PathBuf>,
    /// Unix time of its last commit.
    pub updated: i64,
    /// Everything on it is already in the trunk.
    pub landed: bool,
}

/// Everything [`items`] joins.
#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    /// Your GitHub login.
    pub me: &'a str,
    /// The branch work lands on by default (`main`); it's never an item.
    pub trunk: &'a str,
    pub tasks: &'a [TaskWork],
    pub branches: &'a [BranchWork],
    pub pulls: &'a [Pull],
    /// Pull requests your review is requested on.
    pub review_requested: &'a [u64],
    /// Actions runs, newest first.
    pub runs: &'a [Run],
    pub issues: &'a [Issue],
}

// ── The work item ────────────────────────────────────────────────────────────

/// What identifies a work item.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkKey {
    Branch(String),
    /// Planned: an issue with no branch yet.
    Issue(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PullSummary {
    pub number: u64,
    pub title: String,
    pub state: PrState,
    pub review: ReviewState,
    pub author: String,
    pub url: String,
}

impl PullSummary {
    fn of(pull: &Pull) -> Self {
        PullSummary {
            number: pull.number,
            title: pull.title.clone(),
            state: pull.state,
            review: pull.review,
            author: pull.author.clone(),
            url: pull.url.clone(),
        }
    }
}

/// Where its checks stand, from the pull request or the Actions runs on its
/// branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    Passed,
    Failed,
    Running,
}

/// Whose work it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    You,
    /// An agent, by name: a local task's ("Claude Code"), or one working in
    /// the cloud ("Copilot").
    Agent(String),
    /// A teammate, by login.
    Teammate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// An issue, no branch yet.
    Planned,
    InProgress,
    /// A pull request is open, or a task has finished and you haven't seen
    /// it.
    InReview,
    /// In its base.
    Landed,
    /// Its pull request was closed without landing.
    Closed,
}

/// Why it's waiting on you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Its agent asked something.
    Question,
    /// Its agent finished (or failed, or stopped) since you last looked.
    Unseen,
    /// Your review is requested.
    Review,
    /// Its checks failed.
    ChecksFailed,
    /// A reviewer asked for changes.
    ChangesRequested,
    /// Approved and green, but not landed.
    Ready,
}

/// Who has to do something next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waiting {
    You(Why),
    Agent,
    /// A teammate: the reviewer, or the pull request's author.
    Teammate,
    Ci,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkItem {
    pub key: WorkKey,
    pub title: String,
    pub branch: Option<String>,
    /// The branch it lands on: the trunk, or another item's branch when
    /// it's stacked.
    pub base: Option<String>,
    /// The task's folder, or the checkout the branch is in.
    pub worktree: Option<PathBuf>,
    /// The task's id.
    pub task: Option<String>,
    pub issue: Option<u64>,
    pub pull: Option<PullSummary>,
    pub checks: Option<Checks>,
    pub owner: Owner,
    pub stage: Stage,
    pub waiting_on: Option<Waiting>,
    /// Unix time anything on it last changed.
    pub updated: i64,
}

// ── The join ─────────────────────────────────────────────────────────────────

/// Every work item in `s`, most recently updated first.
pub fn items(s: &Sources) -> Vec<WorkItem> {
    // Everything that names a branch, gathered under it.
    #[derive(Default)]
    struct Parts<'a> {
        task: Option<&'a TaskWork>,
        branch: Option<&'a BranchWork>,
        pull: Option<&'a Pull>,
    }
    let mut by_branch: BTreeMap<&str, Parts> = BTreeMap::new();
    for t in s.tasks {
        by_branch.entry(&t.task.branch).or_default().task = Some(t);
    }
    for b in s.branches.iter().filter(|b| b.name != s.trunk) {
        by_branch.entry(&b.name).or_default().branch = Some(b);
    }
    for p in s.pulls.iter().filter(|p| p.head != s.trunk) {
        let parts = by_branch.entry(&p.head).or_default();
        if parts.pull.is_none_or(|q| prefer_pull(p, q)) {
            parts.pull = Some(p);
        }
    }
    // A task remembers its pull request by number, which wins over one
    // found by branch name.
    for parts in by_branch.values_mut() {
        if let Some(n) = parts.task.and_then(|t| t.task.pull) {
            if let Some(p) = s.pulls.iter().find(|p| p.number == n) {
                parts.pull = Some(p);
            }
        }
    }

    let mut out = Vec::new();
    let mut claimed: HashSet<u64> = HashSet::new();
    for (name, parts) in &by_branch {
        let item = branch_item(s, name, parts.task, parts.branch, parts.pull);
        claimed.extend(item.issue);
        out.push(item);
    }

    for issue in s.issues {
        if !issue.open || claimed.contains(&issue.number) {
            continue;
        }
        if !issue.assignees.iter().any(|a| a == s.me) {
            continue;
        }
        out.push(WorkItem {
            key: WorkKey::Issue(issue.number),
            title: issue.title.clone(),
            branch: None,
            base: None,
            worktree: None,
            task: None,
            issue: Some(issue.number),
            pull: None,
            checks: None,
            owner: Owner::You,
            stage: Stage::Planned,
            waiting_on: None,
            updated: issue.updated,
        });
    }

    out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.key.cmp(&b.key)));
    out
}

/// Of two pull requests from the same branch, the one that's still open,
/// else the newer.
fn prefer_pull(p: &Pull, over: &Pull) -> bool {
    let open = |p: &Pull| matches!(p.state, PrState::Open | PrState::Draft);
    match (open(p), open(over)) {
        (true, false) => true,
        (false, true) => false,
        _ => p.updated > over.updated,
    }
}

fn branch_item(
    s: &Sources,
    name: &str,
    task: Option<&TaskWork>,
    branch: Option<&BranchWork>,
    pull: Option<&Pull>,
) -> WorkItem {
    let agent = task.and_then(|t| t.agent.as_ref());
    let checks = pull_checks(pull).or_else(|| run_checks(s.runs, name));
    let issue = task
        .and_then(|t| t.task.issue)
        .or_else(|| issue_in_name(name).filter(|n| s.issues.iter().any(|i| i.number == *n)));

    let owner = if let Some(a) = agent {
        Owner::Agent(a.kind.label().to_string())
    } else if task.is_some() {
        Owner::You
    } else if let Some(name) = remote_agent(name, pull.map(|p| p.author.as_str())) {
        Owner::Agent(name.to_string())
    } else {
        match pull {
            Some(p) if p.author != s.me => Owner::Teammate(p.author.clone()),
            _ => Owner::You,
        }
    };

    let stage = if matches!(task.map(|t| &t.task.state), Some(TaskState::Landed(_))) {
        Stage::Landed
    } else if let Some(p) = pull {
        match p.state {
            PrState::Merged => Stage::Landed,
            PrState::Closed => Stage::Closed,
            PrState::Open => Stage::InReview,
            PrState::Draft if agent.is_some_and(finished_unseen) => Stage::InReview,
            PrState::Draft => Stage::InProgress,
        }
    } else if branch.is_some_and(|b| b.landed) {
        Stage::Landed
    } else if agent.is_some_and(finished_unseen) {
        Stage::InReview
    } else {
        Stage::InProgress
    };

    let waiting_on = waiting(s, &owner, stage, agent, pull, checks);

    let title = task
        .map(|t| t.task.title())
        .or_else(|| pull.map(|p| p.title.clone()))
        .or_else(|| {
            issue.and_then(|n| {
                s.issues
                    .iter()
                    .find(|i| i.number == n)
                    .map(|i| i.title.clone())
            })
        })
        .unwrap_or_else(|| name.to_string());
    let base = task
        .map(|t| t.task.base.clone())
        .or_else(|| pull.map(|p| p.base.clone()))
        .or_else(|| Some(s.trunk.to_string()));
    let worktree = match task {
        Some(t) if t.task.state == TaskState::Open => Some(t.task.worktree.clone()),
        Some(_) => None,
        None => branch.and_then(|b| b.worktree.clone()),
    };
    let updated = [
        task.map(|t| t.task.created),
        agent.map(|a| a.at),
        branch.map(|b| b.updated),
        pull.map(|p| p.updated),
        s.runs
            .iter()
            .filter(|r| r.branch == name)
            .map(|r| r.updated)
            .max(),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);

    WorkItem {
        key: WorkKey::Branch(name.to_string()),
        title,
        branch: Some(name.to_string()),
        base,
        worktree,
        task: task.map(|t| t.task.id.clone()),
        issue,
        pull: pull.map(PullSummary::of),
        checks,
        owner,
        stage,
        waiting_on,
        updated,
    }
}

fn finished_unseen(a: &AgentState) -> bool {
    a.state != RunState::Running && a.unseen()
}

/// Who's next, in order of what matters most to you.
fn waiting(
    s: &Sources,
    owner: &Owner,
    stage: Stage,
    agent: Option<&AgentState>,
    pull: Option<&Pull>,
    checks: Option<Checks>,
) -> Option<Waiting> {
    if matches!(stage, Stage::Landed | Stage::Closed | Stage::Planned) {
        return None;
    }
    if let Some(a) = agent {
        if a.asks > 0 {
            return Some(Waiting::You(Why::Question));
        }
        if a.state == RunState::Running {
            return Some(Waiting::Agent);
        }
        if a.unseen() {
            return Some(Waiting::You(Why::Unseen));
        }
    }
    let theirs = matches!(owner, Owner::Teammate(_));
    let open = pull.filter(|p| p.state == PrState::Open);
    if let Some(p) = pull {
        if p.state == PrState::Open && s.review_requested.contains(&p.number) {
            return Some(Waiting::You(Why::Review));
        }
        if theirs {
            return Some(Waiting::Teammate);
        }
        // A cloud agent works in a draft until it's done.
        if p.state == PrState::Draft && agent.is_none() && matches!(owner, Owner::Agent(_)) {
            return Some(Waiting::Agent);
        }
    }
    match checks {
        Some(Checks::Failed) if !theirs => return Some(Waiting::You(Why::ChecksFailed)),
        Some(Checks::Running) => return Some(Waiting::Ci),
        _ => {}
    }
    let p = open?;
    Some(match p.review {
        ReviewState::ChangesRequested => Waiting::You(Why::ChangesRequested),
        ReviewState::Approved => Waiting::You(Why::Ready),
        ReviewState::None => Waiting::Teammate,
    })
}

fn pull_checks(pull: Option<&Pull>) -> Option<Checks> {
    match pull?.checks {
        ChecksState::None => None,
        ChecksState::Pending => Some(Checks::Running),
        ChecksState::Passing => Some(Checks::Passed),
        ChecksState::Failing => Some(Checks::Failed),
    }
}

/// From the latest run of each workflow on `branch`: failed if any failed,
/// else running if any is, else passed.
fn run_checks(runs: &[Run], branch: &str) -> Option<Checks> {
    let mut latest: BTreeMap<&str, &Run> = BTreeMap::new();
    for r in runs.iter().filter(|r| r.branch == branch) {
        let e = latest.entry(&r.workflow).or_insert(r);
        if r.created > e.created {
            *e = r;
        }
    }
    if latest.is_empty() {
        return None;
    }
    let statuses: Vec<Status> = latest.values().map(|r| r.status).collect();
    Some(if statuses.contains(&Status::Failure) {
        Checks::Failed
    } else if statuses.iter().any(|s| s.active()) {
        Checks::Running
    } else {
        Checks::Passed
    })
}

/// The issue number a branch is named after: `77-work`, `agent/77-work`,
/// `fix/77`.
fn issue_in_name(branch: &str) -> Option<u64> {
    let last = branch.rsplit('/').next()?;
    let digits = last.split('-').next()?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The cloud agent (#44) a branch or pull request belongs to, by its
/// branch prefix or its author.
pub fn remote_agent(branch: &str, author: Option<&str>) -> Option<&'static str> {
    const PREFIXES: [(&str, &str); 3] = [
        ("claude/", "Claude Code"),
        ("copilot/", "Copilot"),
        ("codex/", "Codex"),
    ];
    if let Some((_, name)) = PREFIXES.iter().find(|(p, _)| branch.starts_with(p)) {
        return Some(name);
    }
    let author = author?.trim_start_matches("app/");
    match author {
        "copilot-swe-agent" | "Copilot" => Some("Copilot"),
        "claude" | "claude[bot]" => Some("Claude Code"),
        "chatgpt-codex-connector" => Some("Codex"),
        _ => None,
    }
}
