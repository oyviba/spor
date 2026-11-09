//! Work: your own branches, listed beside agent tasks and pull requests.
//! A branch is a piece of work once it has commits its base doesn't, or
//! while it's checked out. Agent branches are tasks and come from `agents`.
//!
//! [`work_items`] is all of it at once (#78): tasks, branches and what
//! GitHub says (from `cache`), joined by `spor::work::items` into one list
//! of work items with their stage and who they wait on.

use crate::App;
use serde::Serialize;
use spor::agent::Runs;
use spor::git::Repo;
use spor::github as gh;
use spor::tasks::Tasks;
use spor::work::{
    self, AgentState, BranchWork, Checks, Owner, Stage, TaskWork, Waiting, Why, WorkKey,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

// ---- Work items (#78) --------------------------------------------------------

#[derive(Serialize, Debug, PartialEq)]
pub(crate) struct ItemPull {
    number: u64,
    title: String,
    /// open, draft, merged, closed.
    state: String,
    /// none, approved, changesrequested.
    review: String,
    author: String,
    url: String,
}

/// Whose work it is.
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum ItemOwner {
    You,
    Agent { name: String },
    Teammate { login: String },
}

/// Who has to do something next; `why` only when it's you.
#[derive(Serialize, Debug, PartialEq)]
pub(crate) struct ItemWaiting {
    /// you, agent, teammate, ci.
    who: &'static str,
    /// question, unseen, review, checksfailed, changesrequested, ready.
    why: Option<&'static str>,
}

/// A work item as the UI reads it: `spor::work::WorkItem`, in words.
#[derive(Serialize, Debug, PartialEq)]
pub(crate) struct Item {
    /// `branch:<name>` or `issue:<number>`; unique and stable.
    key: String,
    title: String,
    branch: Option<String>,
    base: Option<String>,
    worktree: Option<String>,
    task: Option<String>,
    issue: Option<u64>,
    pull: Option<ItemPull>,
    /// passed, failed, running.
    checks: Option<&'static str>,
    owner: ItemOwner,
    /// planned, inprogress, inreview, landed, closed.
    stage: &'static str,
    waiting_on: Option<ItemWaiting>,
    updated: i64,
}

impl From<work::WorkItem> for Item {
    fn from(w: work::WorkItem) -> Self {
        Item {
            key: match &w.key {
                WorkKey::Branch(name) => format!("branch:{name}"),
                WorkKey::Issue(n) => format!("issue:{n}"),
            },
            title: w.title,
            branch: w.branch,
            base: w.base,
            worktree: w.worktree.map(|p| p.display().to_string()),
            task: w.task,
            issue: w.issue,
            pull: w.pull.map(|p| ItemPull {
                number: p.number,
                title: p.title,
                state: crate::name_of(&p.state),
                review: crate::name_of(&p.review),
                author: p.author,
                url: p.url,
            }),
            checks: w.checks.map(|c| match c {
                Checks::Passed => "passed",
                Checks::Failed => "failed",
                Checks::Running => "running",
            }),
            owner: match w.owner {
                Owner::You => ItemOwner::You,
                Owner::Agent(name) => ItemOwner::Agent { name },
                Owner::Teammate(login) => ItemOwner::Teammate { login },
            },
            stage: match w.stage {
                Stage::Planned => "planned",
                Stage::InProgress => "inprogress",
                Stage::InReview => "inreview",
                Stage::Landed => "landed",
                Stage::Closed => "closed",
            },
            waiting_on: w.waiting_on.map(|wait| match wait {
                Waiting::You(why) => ItemWaiting {
                    who: "you",
                    why: Some(match why {
                        Why::Question => "question",
                        Why::Unseen => "unseen",
                        Why::Review => "review",
                        Why::ChecksFailed => "checksfailed",
                        Why::ChangesRequested => "changesrequested",
                        Why::Ready => "ready",
                    }),
                },
                Waiting::Agent => ItemWaiting {
                    who: "agent",
                    why: None,
                },
                Waiting::Teammate => ItemWaiting {
                    who: "teammate",
                    why: None,
                },
                Waiting::Ci => ItemWaiting {
                    who: "ci",
                    why: None,
                },
            }),
            updated: w.updated,
        }
    }
}

/// What GitHub says about one repository, from the cache. Anything it
/// can't say (not GitHub, `gh` not signed in) is left empty: the items are
/// still there, without pull requests or checks.
#[derive(Default)]
struct Hub {
    me: String,
    pulls: Vec<gh::Pull>,
    review_requested: Vec<u64>,
    runs: Vec<gh::Run>,
    issues: Vec<gh::Issue>,
}

fn hub(cache: &crate::cache::Github, root: &Path) -> Hub {
    let Ok(remote) = gh::repo(root) else {
        return Hub::default();
    };
    // Each is one `gh` call when it isn't cached; ask them side by side.
    std::thread::scope(|s| {
        let me = s.spawn(|| cache.me(&remote.host));
        let pulls = s.spawn(|| cache.open_pulls(root));
        let reviews = s.spawn(|| cache.review_numbers(&remote));
        let runs = s.spawn(|| cache.runs(root));
        let issues = s.spawn(|| cache.assigned_issues(root));
        fn take<T: Default>(h: std::thread::ScopedJoinHandle<Result<T, String>>) -> T {
            h.join().ok().and_then(Result::ok).unwrap_or_default()
        }
        Hub {
            me: take(me),
            pulls: take(pulls),
            review_requested: take(reviews),
            runs: take(runs),
            issues: take(issues),
        }
    })
}

/// Every task, with its latest run. `seen` is when you last looked at each
/// task, by id, as the UI keeps it.
fn task_work(repo: &Repo, seen: &HashMap<String, i64>) -> Vec<TaskWork> {
    let Ok(tasks) = Tasks::open(repo) else {
        return Vec::new();
    };
    tasks
        .list()
        .into_iter()
        .map(|task| {
            let runs = Runs::of(&tasks, &task);
            let agent = runs.latest().map(|r| AgentState {
                kind: r.kind.clone(),
                at: r.ended.unwrap_or(r.started),
                asks: runs.asks().len(),
                state: r.state,
                seen: seen.get(&task.id).copied(),
            });
            TaskWork { task, agent }
        })
        .collect()
}

/// Your local branches with something on them: commits the base doesn't
/// have, or checked out now (a branch just made has nothing yet).
fn branch_work(repo: &Repo, base: Option<&str>) -> Vec<BranchWork> {
    let checkouts: HashMap<String, PathBuf> = repo
        .worktrees()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|w| Some((w.branch?, w.path)))
        .collect();
    let current = repo.tracking().ok().and_then(|t| t.branch);
    repo.list_branches()
        .unwrap_or_default()
        .into_iter()
        .filter(|b| !b.is_remote)
        .filter_map(|b| {
            let ahead = base.is_none_or(|base| {
                !repo
                    .branch_commits(base, &b.name)
                    .unwrap_or_default()
                    .is_empty()
            });
            let checked_out = current.as_deref() == Some(b.name.as_str());
            if !ahead && !checked_out {
                return None;
            }
            Some(BranchWork {
                worktree: checkouts.get(&b.name).cloned(),
                updated: repo.commit_time(&b.name),
                // Branches already in the base are left out above, but the
                // one you're on: that's work just started, not landed.
                landed: false,
                name: b.name,
            })
        })
        .collect()
}

fn gather(repo: &Repo, cache: &crate::cache::Github, seen: &HashMap<String, i64>) -> Vec<Item> {
    let base = base_of(repo);
    let trunk = base
        .as_deref()
        .map(|b| b.strip_prefix("origin/").unwrap_or(b).to_string())
        .unwrap_or_else(|| "main".into());
    let (hub, tasks, branches) = std::thread::scope(|s| {
        let hub = s.spawn(|| hub(cache, repo.root()));
        let tasks = task_work(repo, seen);
        let branches = branch_work(repo, base.as_deref());
        (hub.join().unwrap_or_default(), tasks, branches)
    });
    work::items(&work::Sources {
        me: &hub.me,
        trunk: &trunk,
        tasks: &tasks,
        branches: &branches,
        pulls: &hub.pulls,
        review_requested: &hub.review_requested,
        runs: &hub.runs,
        issues: &hub.issues,
    })
    .into_iter()
    .map(Item::from)
    .collect()
}

/// Every work item in the open repository, most recently changed first:
/// tasks, your branches, open pull requests and issues assigned to you,
/// each once. GitHub's part comes from the shared cache, so this is cheap
/// to call on every `work-changed`.
#[tauri::command(async)]
pub(crate) fn work_items(
    app: State<App>,
    seen: Option<HashMap<String, i64>>,
) -> Result<Vec<Item>, String> {
    let repo = app.repo()?;
    Ok(gather(&repo, &app.github, &seen.unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Github;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository whose origin is the stand-in `gh`'s demo/greeter, with
    /// `main`, a teammate's branch with a commit on it, and one of yours
    /// that's already in main. Every `gh` call goes to `log`.
    fn greeter() -> (Repo, PathBuf) {
        let dir = std::env::temp_dir().join(format!("spor-work-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@example.com"]);
        git(&dir, &["config", "user.name", "T"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        git(
            &dir,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/demo/greeter.git",
            ],
        );
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        git(&dir, &["branch", "old/done"]);
        git(&dir, &["checkout", "-q", "-b", "feat/farewell"]);
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "Say farewell"]);
        git(&dir, &["checkout", "-q", "main"]);

        // The stand-in gh, ahead of any real one. Only this test asks gh
        // anything, so changing PATH for the process is safe.
        let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake/bin");
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{path}", bin.canonicalize().unwrap().display()),
        );
        let log = dir.join("gh.log");
        std::env::set_var("FAKE_GH_LOG", &log);
        (Repo::discover(&dir).unwrap(), log)
    }

    fn calls(log: &Path, what: &str) -> usize {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with(what))
            .count()
    }

    #[test]
    fn work_items_join_tasks_branches_and_github_from_one_cache() {
        let (repo, log) = greeter();
        let cache = Github::default();
        let items = gather(&repo, &cache, &HashMap::new());
        let by_key = |k: &str| {
            items
                .iter()
                .find(|i| i.key == k)
                .unwrap_or_else(|| panic!("no {k} in {items:#?}"))
        };

        // A teammate's branch, local and with a pull request: one item.
        let farewell = by_key("branch:feat/farewell");
        assert_eq!(farewell.pull.as_ref().map(|p| p.number), Some(10));
        assert_eq!(
            farewell.owner,
            ItemOwner::Teammate {
                login: "grace".into()
            }
        );
        // It's a draft: still in progress, and the teammate's to finish.
        assert_eq!(farewell.stage, "inprogress");
        assert_eq!(
            farewell.waiting_on,
            Some(ItemWaiting {
                who: "teammate",
                why: None
            })
        );
        // A pull request waiting on your review, with no local branch.
        let readme = by_key("branch:fix/readme-install");
        assert_eq!(
            readme.waiting_on,
            Some(ItemWaiting {
                who: "you",
                why: Some("review")
            })
        );
        // An issue assigned to you, with no branch yet; not a teammate's.
        assert_eq!(by_key("issue:7").stage, "planned");
        assert!(items.iter().all(|i| i.key != "issue:6"));
        // Neither the trunk nor a branch already in it.
        assert!(items
            .iter()
            .all(|i| i.key != "branch:main" && i.key != "branch:old/done"));
        // Closed pull requests aren't asked for.
        assert!(items.iter().all(|i| i.key != "branch:try/shout"));

        // The sections reading the same things don't ask again.
        gather(&repo, &cache, &HashMap::new());
        let remote = gh::repo(repo.root()).unwrap();
        cache.open_pulls(repo.root()).unwrap();
        cache.yours(repo.root(), &remote).unwrap();
        cache.review_numbers(&remote).unwrap();
        cache.runs(repo.root()).unwrap();
        cache.assigned_issues(repo.root()).unwrap();
        for what in [
            "pr list",
            "run list",
            "issue list",
            "search prs",
            "api user",
        ] {
            assert_eq!(calls(&log, what), 1, "{what}");
        }

        // A fetch moved refs: not again straight away, however often.
        cache.touch(repo.root());
        cache.touch(repo.root());
        cache.open_pulls(repo.root()).unwrap();
        assert_eq!(calls(&log, "pr list"), 1);
        // You merged something from here: asked again at once, once.
        cache.clear(repo.root());
        cache.open_pulls(repo.root()).unwrap();
        cache.open_pulls(repo.root()).unwrap();
        assert_eq!(calls(&log, "pr list"), 2);
        assert_eq!(calls(&log, "api user"), 1);

        let _ = std::fs::remove_dir_all(repo.root());
    }
}
