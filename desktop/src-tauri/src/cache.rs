//! One GitHub cache per repository (#78), shared by every section that shows
//! what's on GitHub: Pull requests, Actions, History's badges, Work and the
//! Inbox. Each kind of answer (open pull requests, recent runs, issues
//! assigned to you; per host, your login and the pull requests waiting on
//! your review) is one `gh` call, kept until it's old or something says it
//! changed:
//!
//! - **Fresh for [`FRESH`]** when nothing says otherwise, so sections that
//!   open together, and the Inbox polling every repository, share one call.
//! - **Stale** when the watcher sees refs move (a fetch, a push, a
//!   checkout): asked again on the next read, but never sooner than
//!   [`FLOOR`] after the last ask, however many events arrive.
//! - **Cleared** after something you did on GitHub from here (merged,
//!   commented, opened a pull request, re-ran a job), so the next read shows
//!   it at once.
//!
//! A slot holds its lock while it asks, so callers that arrive meanwhile
//! wait for that one answer instead of starting their own. Failures are
//! kept like answers: a repository that isn't on GitHub isn't asked again
//! on every read.

use spor::github as gh;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long an answer is good for when nothing says it changed. The
/// sections poll every 30–60 s, so each poll asks at most once.
pub(crate) const FRESH: Duration = Duration::from_secs(30);
/// The least time between two asks of the same kind after a change.
pub(crate) const FLOOR: Duration = Duration::from_secs(5);

struct Entry<T> {
    at: Instant,
    value: Result<T, String>,
    stale: bool,
}

/// One cached answer.
pub(crate) struct Slot<T> {
    entry: Mutex<Option<Entry<T>>>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot {
            entry: Mutex::new(None),
        }
    }
}

impl<T: Clone> Slot<T> {
    /// The kept answer while it's fresh, else `fetch`'s.
    pub(crate) fn get(&self, fetch: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.get_at(Instant::now(), fetch)
    }

    fn get_at(&self, now: Instant, fetch: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let mut entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = entry.as_ref() {
            let age = now.saturating_duration_since(e.at);
            if age < if e.stale { FLOOR } else { FRESH } {
                return e.value.clone();
            }
        }
        let value = fetch();
        *entry = Some(Entry {
            at: now,
            value: value.clone(),
            stale: false,
        });
        value
    }

    /// Ask again on the next read, once [`FLOOR`] has passed.
    pub(crate) fn touch(&self) {
        if let Some(e) = self
            .entry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            e.stale = true;
        }
    }

    /// Ask again on the next read.
    pub(crate) fn clear(&self) {
        *self.entry.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// What's kept for one repository.
#[derive(Default)]
struct RepoCache {
    pulls: Slot<Vec<gh::Pull>>,
    runs: Slot<Vec<gh::Run>>,
    issues: Slot<Vec<gh::Issue>>,
}

impl RepoCache {
    fn touch(&self) {
        self.pulls.touch();
        self.runs.touch();
        self.issues.touch();
    }

    fn clear(&self) {
        self.pulls.clear();
        self.runs.clear();
        self.issues.clear();
    }
}

/// What's kept for one host: who you are there, and what waits on your
/// review across all its repositories (one search, not one per repository).
#[derive(Default)]
struct HostCache {
    /// Only an answer is kept: your login doesn't change while the app
    /// runs, but `gh` may not be signed in yet the first time.
    me: Mutex<Option<String>>,
    reviews: Slot<Vec<gh::ReviewRequest>>,
}

#[derive(Default)]
pub(crate) struct Github {
    repos: Mutex<HashMap<PathBuf, Arc<RepoCache>>>,
    hosts: Mutex<HashMap<String, Arc<HostCache>>>,
}

impl Github {
    fn repo(&self, root: &Path) -> Arc<RepoCache> {
        let mut repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        repos.entry(root.to_path_buf()).or_default().clone()
    }

    fn host(&self, host: &str) -> Arc<HostCache> {
        let mut hosts = self.hosts.lock().unwrap_or_else(|e| e.into_inner());
        hosts.entry(host.to_string()).or_default().clone()
    }

    /// The open pull requests of the repository at `root`, newest first.
    pub(crate) fn open_pulls(&self, root: &Path) -> Result<Vec<gh::Pull>, String> {
        self.repo(root)
            .pulls
            .get(|| gh::list_pulls(root, gh::PullFilter::Open))
    }

    /// Its recent Actions runs, newest first, on any branch.
    pub(crate) fn runs(&self, root: &Path) -> Result<Vec<gh::Run>, String> {
        self.repo(root).runs.get(|| gh::list_runs(root, None, None))
    }

    /// Its open issues assigned to you.
    pub(crate) fn assigned_issues(&self, root: &Path) -> Result<Vec<gh::Issue>, String> {
        self.repo(root)
            .issues
            .get(|| gh::list_issues(root, gh::IssueFilter::Assigned))
    }

    /// Your login on `host`.
    pub(crate) fn me(&self, host: &str) -> Result<String, String> {
        let cache = self.host(host);
        let mut me = cache.me.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(login) = me.as_ref() {
            return Ok(login.clone());
        }
        let login = gh::login(host)?;
        *me = Some(login.clone());
        Ok(login)
    }

    /// Open pull requests on `host` waiting on your review, in any
    /// repository.
    pub(crate) fn review_requests(&self, host: &str) -> Result<Vec<gh::ReviewRequest>, String> {
        self.host(host).reviews.get(|| gh::review_requests(host))
    }

    /// The open pull requests of `root` that you opened.
    pub(crate) fn yours(&self, root: &Path, remote: &gh::Repo) -> Result<Vec<gh::Pull>, String> {
        let me = self.me(&remote.host)?;
        Ok(self
            .open_pulls(root)?
            .into_iter()
            .filter(|p| p.author.eq_ignore_ascii_case(&me))
            .collect())
    }

    /// The numbers of `remote`'s pull requests waiting on your review.
    pub(crate) fn review_numbers(&self, remote: &gh::Repo) -> Result<Vec<u64>, String> {
        let name = format!("{}/{}", remote.owner, remote.name);
        Ok(self
            .review_requests(&remote.host)?
            .into_iter()
            .filter(|r| r.repo.eq_ignore_ascii_case(&name))
            .map(|r| r.number)
            .collect())
    }

    /// Refs moved in the repository at `root` (a fetch, a push): ask again
    /// soon.
    pub(crate) fn touch(&self, root: &Path) {
        self.repo(root).touch();
        for host in self
            .hosts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            host.reviews.touch();
        }
    }

    /// You changed something on GitHub from the repository at `root`: ask
    /// again on the next read.
    pub(crate) fn clear(&self, root: &Path) {
        self.repo(root).clear();
        for host in self
            .hosts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            host.reviews.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn counted(calls: &Cell<u32>, value: u32) -> impl FnOnce() -> Result<u32, String> + '_ {
        move || {
            calls.set(calls.get() + 1);
            Ok(value)
        }
    }

    #[test]
    fn keeps_an_answer_while_its_fresh() {
        let slot = Slot::default();
        let calls = Cell::new(0);
        let t0 = Instant::now();
        assert_eq!(slot.get_at(t0, counted(&calls, 1)), Ok(1));
        assert_eq!(slot.get_at(t0 + FRESH / 2, counted(&calls, 2)), Ok(1));
        assert_eq!(calls.get(), 1);
        assert_eq!(slot.get_at(t0 + FRESH, counted(&calls, 3)), Ok(3));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_touch_asks_again_but_not_before_the_floor() {
        let slot = Slot::default();
        let calls = Cell::new(0);
        let t0 = Instant::now();
        slot.get_at(t0, counted(&calls, 1)).unwrap();
        slot.touch();
        // Many events in a burst: still one ask per FLOOR.
        assert_eq!(slot.get_at(t0 + FLOOR / 2, counted(&calls, 2)), Ok(1));
        assert_eq!(slot.get_at(t0 + FLOOR, counted(&calls, 3)), Ok(3));
        slot.touch();
        assert_eq!(
            slot.get_at(t0 + FLOOR + FLOOR / 2, counted(&calls, 4)),
            Ok(3)
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_clear_asks_again_at_once() {
        let slot = Slot::default();
        let calls = Cell::new(0);
        let t0 = Instant::now();
        slot.get_at(t0, counted(&calls, 1)).unwrap();
        slot.clear();
        assert_eq!(slot.get_at(t0, counted(&calls, 2)), Ok(2));
    }

    #[test]
    fn keeps_failures_too() {
        let slot: Slot<u32> = Slot::default();
        let t0 = Instant::now();
        assert!(slot.get_at(t0, || Err("not GitHub".into())).is_err());
        assert_eq!(
            slot.get_at(t0 + FLOOR, || Ok(1)),
            Err("not GitHub".to_string())
        );
    }

    #[test]
    fn callers_at_the_same_time_share_one_ask() {
        let slot: Arc<Slot<u32>> = Arc::default();
        let calls = Arc::new(Mutex::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (slot, calls) = (slot.clone(), calls.clone());
                std::thread::spawn(move || {
                    slot.get(|| {
                        *calls.lock().unwrap() += 1;
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(7)
                    })
                })
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap(), Ok(7));
        }
        assert_eq!(*calls.lock().unwrap(), 1);
    }
}
