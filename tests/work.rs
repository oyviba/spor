//! Work items: joining tasks, branches, pull requests, runs and issues.

use spor::agent::{AgentKind, RunState};
use spor::github::{Issue, Pull, Run, Status};
use spor::remote::{ChecksState, PrState, ReviewState};
use spor::tasks::{Task, TaskState};
use spor::work::{
    items, remote_agent, AgentState, BranchWork, Checks, Owner, Sources, Stage, TaskWork, Waiting,
    Why, WorkItem, WorkKey,
};
use std::path::PathBuf;

const ME: &str = "me";

#[derive(Default)]
struct World {
    tasks: Vec<TaskWork>,
    branches: Vec<BranchWork>,
    pulls: Vec<Pull>,
    review_requested: Vec<u64>,
    runs: Vec<Run>,
    issues: Vec<Issue>,
}

impl World {
    fn items(&self) -> Vec<WorkItem> {
        items(&Sources {
            me: ME,
            trunk: "main",
            tasks: &self.tasks,
            branches: &self.branches,
            pulls: &self.pulls,
            review_requested: &self.review_requested,
            runs: &self.runs,
            issues: &self.issues,
        })
    }

    fn only(&self) -> WorkItem {
        let mut all = self.items();
        assert_eq!(all.len(), 1, "{all:#?}");
        all.remove(0)
    }
}

fn task(id: &str) -> Task {
    Task {
        id: id.into(),
        prompt: format!("Do {id}\n\nDetails."),
        base: "main".into(),
        from: None,
        branch: format!("agent/{id}"),
        worktree: PathBuf::from(format!("/r.spor/{id}")),
        created: 100,
        state: TaskState::Open,
        issue: None,
        pull: None,
        pending: false,
    }
}

fn agent(state: RunState, at: i64, seen: Option<i64>) -> Option<AgentState> {
    Some(AgentState {
        kind: AgentKind::Claude,
        state,
        asks: 0,
        at,
        seen,
    })
}

fn branch(name: &str) -> BranchWork {
    BranchWork {
        name: name.into(),
        worktree: None,
        updated: 200,
        landed: false,
    }
}

fn pull(number: u64, head: &str, author: &str) -> Pull {
    Pull {
        number,
        title: format!("PR {number}"),
        author: author.into(),
        head: head.into(),
        base: "main".into(),
        state: PrState::Open,
        review: ReviewState::None,
        checks: ChecksState::None,
        updated: 300,
        additions: 1,
        deletions: 0,
        url: format!("https://github.com/o/r/pull/{number}"),
    }
}

fn run(workflow: &str, branch: &str, status: Status, created: i64) -> Run {
    Run {
        id: created as u64,
        number: 1,
        workflow: workflow.into(),
        title: "t".into(),
        branch: branch.into(),
        sha: "abc".into(),
        event: "push".into(),
        status,
        attempt: 1,
        created,
        updated: created,
        url: String::new(),
    }
}

fn issue(number: u64, assignees: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("Issue {number}"),
        author: "someone".into(),
        open: true,
        labels: vec![],
        assignees: assignees.iter().map(|a| a.to_string()).collect(),
        comments: 0,
        updated: 50,
        url: String::new(),
    }
}

// ── Joining ──────────────────────────────────────────────────────────────────

#[test]
fn task_pull_and_runs_on_one_branch_are_one_item() {
    let mut w = World::default();
    let mut t = task("fix");
    t.issue = Some(7);
    w.tasks.push(TaskWork {
        task: t,
        agent: agent(RunState::Finished, 150, Some(160)),
    });
    w.branches.push(branch("agent/fix"));
    w.pulls.push(pull(12, "agent/fix", ME));
    w.runs.push(run("CI", "agent/fix", Status::Success, 400));
    w.runs.push(run("CI", "other", Status::Failure, 500));
    w.issues.push(issue(7, &[ME]));

    let items = w.items();
    let it = items.iter().find(|i| i.task.is_some()).unwrap();
    assert_eq!(it.key, WorkKey::Branch("agent/fix".into()));
    assert_eq!(it.title, "Do fix");
    assert_eq!(it.base.as_deref(), Some("main"));
    assert_eq!(it.worktree, Some(PathBuf::from("/r.spor/fix")));
    assert_eq!(it.issue, Some(7));
    assert_eq!(it.pull.as_ref().unwrap().number, 12);
    assert_eq!(it.checks, Some(Checks::Passed));
    assert_eq!(it.owner, Owner::Agent("Claude Code".into()));
    assert_eq!(it.updated, 400);
    // The issue is part of the task's item, not a planned item of its own,
    // and a run alone doesn't make a branch an item.
    assert_eq!(items.len(), 1);
}

#[test]
fn trunk_is_never_an_item() {
    let mut w = World::default();
    w.branches.push(branch("main"));
    w.runs.push(run("CI", "main", Status::Failure, 1));
    assert!(w.items().is_empty());
}

#[test]
fn a_task_finds_its_pull_by_number_and_prefers_open_pulls() {
    let mut w = World::default();
    let mut t = task("fix");
    t.pull = Some(3);
    w.tasks.push(TaskWork {
        task: t,
        agent: None,
    });
    let mut old = pull(3, "agent/fix", ME);
    old.state = PrState::Closed;
    w.pulls.push(old);
    w.pulls.push(pull(4, "agent/fix", ME));
    assert_eq!(w.only().pull.unwrap().number, 3);

    w.tasks[0].task.pull = None;
    assert_eq!(w.only().pull.unwrap().number, 4);
}

#[test]
fn a_branch_named_after_an_issue_links_it() {
    let mut w = World::default();
    w.branches.push(branch("fix/42-login"));
    w.issues.push(issue(42, &[ME]));
    let it = w.only();
    assert_eq!(it.issue, Some(42));
    assert_eq!(it.title, "Issue 42");

    // A number that isn't a known issue is just part of the name.
    let mut w = World::default();
    w.branches.push(branch("2024-cleanup"));
    assert_eq!(w.only().issue, None);
}

#[test]
fn checks_come_from_the_latest_run_of_each_workflow() {
    let mut w = World::default();
    w.branches.push(branch("feat"));
    w.runs.push(run("CI", "feat", Status::Failure, 1));
    w.runs.push(run("CI", "feat", Status::Success, 2));
    w.runs.push(run("Lint", "feat", Status::Running, 2));
    assert_eq!(w.only().checks, Some(Checks::Running));
    w.runs.push(run("Lint", "feat", Status::Failure, 3));
    assert_eq!(w.only().checks, Some(Checks::Failed));
}

#[test]
fn items_are_newest_first() {
    let mut w = World::default();
    w.branches.push(branch("old"));
    w.branches[0].updated = 1;
    w.branches.push(branch("new"));
    w.branches[1].updated = 2;
    let keys: Vec<_> = w.items().into_iter().map(|i| i.key).collect();
    assert_eq!(
        keys,
        [WorkKey::Branch("new".into()), WorkKey::Branch("old".into())]
    );
}

// ── Stages ───────────────────────────────────────────────────────────────────

#[test]
fn planned_only_for_open_issues_assigned_to_you() {
    let mut w = World::default();
    w.issues.push(issue(1, &[ME]));
    w.issues.push(issue(2, &["teammate"]));
    let mut closed = issue(3, &[ME]);
    closed.open = false;
    w.issues.push(closed);
    let it = w.only();
    assert_eq!(it.key, WorkKey::Issue(1));
    assert_eq!(it.stage, Stage::Planned);
    assert_eq!(it.owner, Owner::You);
    assert_eq!(it.branch, None);
    assert_eq!(it.waiting_on, None);
}

#[test]
fn in_progress_for_a_branch_or_draft() {
    let mut w = World::default();
    w.branches.push(branch("feat"));
    let it = w.only();
    assert_eq!(it.stage, Stage::InProgress);
    assert_eq!(it.owner, Owner::You);
    assert_eq!(it.waiting_on, None);

    let mut p = pull(1, "feat", ME);
    p.state = PrState::Draft;
    w.pulls.push(p);
    assert_eq!(w.only().stage, Stage::InProgress);
}

#[test]
fn in_review_for_an_open_pull_or_an_unseen_finished_task() {
    let mut w = World::default();
    w.branches.push(branch("feat"));
    w.pulls.push(pull(1, "feat", ME));
    assert_eq!(w.only().stage, Stage::InReview);

    let mut w = World::default();
    w.tasks.push(TaskWork {
        task: task("t"),
        agent: agent(RunState::Finished, 150, None),
    });
    assert_eq!(w.only().stage, Stage::InReview);
    w.tasks[0].agent = agent(RunState::Finished, 150, Some(150));
    assert_eq!(w.only().stage, Stage::InProgress);
}

#[test]
fn landed_from_a_task_a_merged_pull_or_a_merged_branch() {
    let mut w = World::default();
    let mut t = task("t");
    t.state = TaskState::Landed("abc".into());
    w.tasks.push(TaskWork {
        task: t,
        agent: None,
    });
    let it = w.only();
    assert_eq!(it.stage, Stage::Landed);
    assert_eq!(it.worktree, None);
    assert_eq!(it.waiting_on, None);

    let mut w = World::default();
    let mut p = pull(1, "feat", ME);
    p.state = PrState::Merged;
    w.pulls.push(p);
    assert_eq!(w.only().stage, Stage::Landed);

    let mut w = World::default();
    w.branches.push(BranchWork {
        landed: true,
        ..branch("feat")
    });
    assert_eq!(w.only().stage, Stage::Landed);
}

#[test]
fn closed_when_its_pull_was_closed() {
    let mut w = World::default();
    let mut p = pull(1, "feat", ME);
    p.state = PrState::Closed;
    p.checks = ChecksState::Failing;
    w.pulls.push(p);
    let it = w.only();
    assert_eq!(it.stage, Stage::Closed);
    assert_eq!(it.waiting_on, None);
}

#[test]
fn closed_for_a_closed_task_whose_pull_isnt_listed() {
    let mut w = World::default();
    w.tasks.push(TaskWork {
        task: Task {
            state: TaskState::Closed,
            pull: Some(4),
            ..task("gone")
        },
        agent: agent(RunState::Finished, 200, None),
    });
    let it = w.only();
    assert_eq!(it.stage, Stage::Closed);
    assert_eq!(it.waiting_on, None);
    assert_eq!(it.worktree, None);
}

// ── Waiting on ───────────────────────────────────────────────────────────────

fn waiting_on_task(a: Option<AgentState>) -> Option<Waiting> {
    let mut w = World::default();
    w.tasks.push(TaskWork {
        task: task("t"),
        agent: a,
    });
    w.only().waiting_on
}

#[test]
fn waiting_on_you_for_a_question() {
    let mut a = agent(RunState::Running, 150, None).unwrap();
    a.asks = 1;
    assert_eq!(waiting_on_task(Some(a)), Some(Waiting::You(Why::Question)));
}

#[test]
fn waiting_on_the_agent_while_it_runs() {
    assert_eq!(
        waiting_on_task(agent(RunState::Running, 150, None)),
        Some(Waiting::Agent)
    );
}

#[test]
fn waiting_on_you_for_a_finished_task_you_havent_seen() {
    for state in [RunState::Finished, RunState::Failed(1), RunState::Stopped] {
        assert_eq!(
            waiting_on_task(agent(state.clone(), 150, Some(100))),
            Some(Waiting::You(Why::Unseen))
        );
        assert_eq!(waiting_on_task(agent(state, 150, Some(150))), None);
    }
}

#[test]
fn waiting_on_you_for_a_requested_review() {
    let mut w = World::default();
    w.pulls.push(pull(5, "their-feat", "teammate"));
    let it = w.only();
    assert_eq!(it.owner, Owner::Teammate("teammate".into()));
    assert_eq!(it.waiting_on, Some(Waiting::Teammate));

    w.review_requested.push(5);
    assert_eq!(w.only().waiting_on, Some(Waiting::You(Why::Review)));
}

#[test]
fn waiting_on_you_when_checks_fail_on_your_item() {
    let mut w = World::default();
    w.branches.push(branch("feat"));
    w.runs.push(run("CI", "feat", Status::Failure, 1));
    assert_eq!(w.only().waiting_on, Some(Waiting::You(Why::ChecksFailed)));

    // On a teammate's pull request, it's theirs to fix.
    let mut w = World::default();
    let mut p = pull(1, "their", "teammate");
    p.checks = ChecksState::Failing;
    w.pulls.push(p);
    assert_eq!(w.only().waiting_on, Some(Waiting::Teammate));
}

#[test]
fn waiting_on_ci_while_checks_run() {
    let mut w = World::default();
    let mut p = pull(1, "feat", ME);
    p.checks = ChecksState::Pending;
    p.review = ReviewState::Approved;
    w.pulls.push(p);
    assert_eq!(w.only().waiting_on, Some(Waiting::Ci));
}

#[test]
fn waiting_on_you_when_approved_and_green() {
    let mut w = World::default();
    let mut p = pull(1, "feat", ME);
    p.checks = ChecksState::Passing;
    p.review = ReviewState::Approved;
    w.pulls.push(p);
    assert_eq!(w.only().waiting_on, Some(Waiting::You(Why::Ready)));
}

#[test]
fn waiting_on_you_when_changes_are_requested() {
    let mut w = World::default();
    let mut p = pull(1, "feat", ME);
    p.review = ReviewState::ChangesRequested;
    w.pulls.push(p);
    assert_eq!(
        w.only().waiting_on,
        Some(Waiting::You(Why::ChangesRequested))
    );
}

#[test]
fn waiting_on_a_teammate_to_review_your_pull() {
    let mut w = World::default();
    w.pulls.push(pull(1, "feat", ME));
    assert_eq!(w.only().waiting_on, Some(Waiting::Teammate));
}

// ── Later work fits the same types ───────────────────────────────────────────

#[test]
fn cloud_agents_are_items_without_a_folder() {
    // #44: a cloud agent's branch and draft PR, nothing local.
    let mut w = World::default();
    let mut p = pull(9, "copilot/fix-login", "app/copilot-swe-agent");
    p.state = PrState::Draft;
    w.pulls.push(p);
    let it = w.only();
    assert_eq!(it.owner, Owner::Agent("Copilot".into()));
    assert_eq!(it.worktree, None);
    assert_eq!(it.task, None);
    assert_eq!(it.stage, Stage::InProgress);
    assert_eq!(it.waiting_on, Some(Waiting::Agent));

    // Once it's ready for review, it's waiting on a reviewer like any PR.
    w.pulls[0].state = PrState::Open;
    assert_eq!(w.only().waiting_on, Some(Waiting::Teammate));

    assert_eq!(remote_agent("claude/x", None), Some("Claude Code"));
    assert_eq!(remote_agent("feat/x", Some("dependabot[bot]")), None);
}

#[test]
fn stacked_tasks_have_another_items_branch_as_base() {
    // #46: a task started from another task's branch.
    let mut w = World::default();
    w.tasks.push(TaskWork {
        task: task("refactor"),
        agent: None,
    });
    let mut child = task("sso");
    child.base = "agent/refactor".into();
    w.tasks.push(TaskWork {
        task: child,
        agent: None,
    });
    let items = w.items();
    assert_eq!(items.len(), 2);
    let child = items
        .iter()
        .find(|i| i.task.as_deref() == Some("sso"))
        .unwrap();
    assert_eq!(child.base.as_deref(), Some("agent/refactor"));
    assert!(items.iter().any(|i| i.branch == child.base));
}
