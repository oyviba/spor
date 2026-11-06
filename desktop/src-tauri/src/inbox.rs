//! The Inbox (#42): what's waiting for you across every repository the app
//! knows, not only the open one. Nothing is stored; it reads what the
//! sections already read. Agent tasks come from each repository's
//! `.git/spor/tasks` (no network); review requests are one `gh search` per
//! host, and failed runs one `gh run list` per GitHub repository.

use serde::Serialize;
use spor::agent::{RunState, Runs};
use spor::git::Repo;
use spor::github as gh;
use spor::tasks::{TaskState, Tasks};
use std::path::PathBuf;

fn discover(root: &str) -> Option<Repo> {
    Repo::discover(&PathBuf::from(root)).ok()
}

/// An open task, with just enough to say whether it waits for you.
#[derive(Serialize)]
pub(crate) struct InboxTask {
    /// The repository root, as the recent list has it.
    repo: String,
    id: String,
    title: String,
    /// Questions the agent is waiting on.
    asks: usize,
    /// The latest run: running, finished, failed, stopped; None before one.
    run: Option<&'static str>,
    started: Option<i64>,
    ended: Option<i64>,
    created: i64,
}

/// Every open task in `roots`. Cheap: task files and run logs, no git
/// diffs, so it can be polled.
#[tauri::command(async)]
pub(crate) fn inbox_tasks(roots: Vec<String>) -> Vec<InboxTask> {
    let mut out = Vec::new();
    for root in roots {
        let Some(tasks) = discover(&root).and_then(|r| Tasks::open(&r).ok()) else {
            continue;
        };
        for task in tasks.list() {
            if task.state != TaskState::Open {
                continue;
            }
            let runs = Runs::of(&tasks, &task);
            let latest = runs.latest();
            out.push(InboxTask {
                repo: root.clone(),
                id: task.id.clone(),
                title: task.title(),
                asks: runs.asks().len(),
                run: latest.as_ref().map(|r| match r.state {
                    RunState::Running => "running",
                    RunState::Finished => "finished",
                    RunState::Failed(_) => "failed",
                    RunState::Stopped => "stopped",
                }),
                started: latest.as_ref().map(|r| r.started),
                ended: latest.as_ref().and_then(|r| r.ended),
                created: task.created,
            });
        }
    }
    out
}

#[derive(Serialize)]
pub(crate) struct InboxReview {
    repo: String,
    number: u64,
    title: String,
    author: String,
    updated: i64,
    url: String,
}

#[derive(Serialize)]
pub(crate) struct InboxRun {
    repo: String,
    #[serde(flatten)]
    run: crate::github::Run,
}

#[derive(Serialize, Default)]
pub(crate) struct InboxGithub {
    reviews: Vec<InboxReview>,
    runs: Vec<InboxRun>,
    /// What couldn't be asked, one line each (`gh` not signed in, say).
    /// Repositories that aren't on GitHub are left out quietly.
    errors: Vec<String>,
}

/// Review requests and failing runs on your branches (local ones, agent
/// branches included) in every GitHub repository among `roots`.
#[tauri::command(async)]
pub(crate) fn inbox_github(roots: Vec<String>) -> InboxGithub {
    let repos: Vec<(String, Repo, gh::Repo)> = roots
        .into_iter()
        .filter_map(|root| {
            let repo = discover(&root)?;
            let remote = gh::repo(repo.root()).ok()?;
            Some((root, repo, remote))
        })
        .collect();
    let mut hosts: Vec<&str> = repos.iter().map(|(_, _, g)| g.host.as_str()).collect();
    hosts.sort();
    hosts.dedup();

    std::thread::scope(|s| {
        let searches: Vec<_> = hosts
            .iter()
            .map(|host| (host, s.spawn(move || gh::review_requests(host))))
            .collect();
        let lists: Vec<_> = repos
            .iter()
            .map(|(root, repo, _)| {
                (
                    root,
                    s.spawn(move || {
                        let branches: Vec<String> = repo
                            .list_branches()
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|b| !b.is_remote)
                            .map(|b| b.name)
                            .collect();
                        gh::list_runs(repo.root(), None, None)
                            .map(|runs| gh::failing_runs(runs, &branches))
                    }),
                )
            })
            .collect();

        let mut out = InboxGithub::default();
        for (host, search) in searches {
            match search.join().unwrap_or_else(|_| Err("gh failed".into())) {
                Ok(found) => out.reviews.extend(found.into_iter().filter_map(|r| {
                    let (root, _, _) = repos.iter().find(|(_, _, g)| {
                        g.host == **host
                            && format!("{}/{}", g.owner, g.name).eq_ignore_ascii_case(&r.repo)
                    })?;
                    Some(InboxReview {
                        repo: root.clone(),
                        number: r.number,
                        title: r.title,
                        author: r.author,
                        updated: r.updated,
                        url: r.url,
                    })
                })),
                Err(e) => out
                    .errors
                    .push(format!("{host}: {}", crate::first_line(&e))),
            }
        }
        for (root, list) in lists {
            match list.join().unwrap_or_else(|_| Err("gh failed".into())) {
                Ok(runs) => out.runs.extend(runs.into_iter().map(|r| InboxRun {
                    repo: root.clone(),
                    run: r.into(),
                })),
                Err(e) => out.errors.push(format!(
                    "{}: {}",
                    root.rsplit('/').next().unwrap_or(root),
                    crate::first_line(&e)
                )),
            }
        }
        out
    })
}
