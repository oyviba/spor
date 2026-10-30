//! GitHub state for the Pull Requests and Actions tabs: what's loaded, what's
//! selected, and the background `gh` calls that fill it in and act on it.
//! Drawing is in `pulls.rs` and `runs.rs`.
//!
//! Lists stay up while a fresh copy loads, so refreshing never blanks the
//! page. While the tab shows something still running (a check, a run), it
//! is reloaded every few seconds; otherwise every minute.

use super::app::{Spor, Tab};
use super::support::first_line;
use gpui_kit::component::{
    diff::{DiffFile, DiffMode, DiffState},
    input::TextareaState,
};
use gpui_kit::{
    AppContext as _, Context, Entity, ScrollStrategy, Task, UniformListScrollHandle, Window,
};
use spor::github::{
    self, Check, LogKind, LogLine, MergeMethod, Pull, PullDetail, PullFilter, Run, RunDetail,
};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// How often the open tab is reloaded while something on it is running.
const LIVE_EVERY: Duration = Duration::from_secs(8);
/// How many live ticks between reloads when nothing is running.
const IDLE_TICKS: u32 = 8;

/// Something fetched from GitHub.
#[derive(Clone)]
pub enum Load<T> {
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(t) => Some(t),
            _ => None,
        }
    }

    fn needs_load(&self) -> bool {
        matches!(self, Load::Idle | Load::Failed(_))
    }
}

/// The sections of a pull request's page.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PullTab {
    Conversation,
    Checks,
    Files,
}

pub struct Hub {
    /// The repository everything here was loaded for.
    root: Option<PathBuf>,
    pub pull_filter: PullFilter,
    pub pulls: Load<Rc<Vec<Pull>>>,
    pub pull: Option<u64>,
    pub pull_detail: Load<Rc<PullDetail>>,
    pub pull_tab: PullTab,
    pub pull_diff: Entity<DiffState>,
    /// Which PR's diff `pull_diff` holds, or is loading.
    pub pull_diff_load: Load<u64>,
    pub comment: Entity<TextareaState>,
    /// Actions shows only the checked-out branch's runs.
    pub runs_this_branch: bool,
    pub runs: Load<Rc<Vec<Run>>>,
    pub run: Option<u64>,
    pub run_detail: Load<Rc<RunDetail>>,
    pub job: Option<u64>,
    pub log: Load<Rc<Vec<LogLine>>>,
    log_for: Option<u64>,
    pub log_scroll: UniformListScrollHandle,
    /// A GitHub action in flight ("Merging"); its buttons show a spinner.
    pub busy: Option<&'static str>,
    _pulls_task: Task<()>,
    _pull_task: Task<()>,
    _diff_task: Task<()>,
    _runs_task: Task<()>,
    _run_task: Task<()>,
    _log_task: Task<()>,
    _act_task: Task<()>,
    _poll_task: Task<()>,
}

impl Hub {
    pub fn new(diff_mode: DiffMode, window: &mut Window, cx: &mut Context<Spor>) -> Self {
        Self {
            root: None,
            pull_filter: PullFilter::Open,
            pulls: Load::Idle,
            pull: None,
            pull_detail: Load::Idle,
            pull_tab: PullTab::Conversation,
            pull_diff: cx.new(|cx| DiffState::new([], cx).with_mode(diff_mode)),
            pull_diff_load: Load::Idle,
            comment: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Leave a comment")
                    .auto_grow(3, 12)
            }),
            runs_this_branch: false,
            runs: Load::Idle,
            run: None,
            run_detail: Load::Idle,
            job: None,
            log: Load::Idle,
            log_for: None,
            log_scroll: UniformListScrollHandle::new(),
            busy: None,
            _pulls_task: Task::ready(()),
            _pull_task: Task::ready(()),
            _diff_task: Task::ready(()),
            _runs_task: Task::ready(()),
            _run_task: Task::ready(()),
            _log_task: Task::ready(()),
            _act_task: Task::ready(()),
            _poll_task: Task::ready(()),
        }
    }

    /// Forget everything when another repository opens.
    fn reset(&mut self, root: Option<PathBuf>, cx: &mut Context<Spor>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.pulls = Load::Idle;
        self.pull = None;
        self.pull_detail = Load::Idle;
        self.pull_diff_load = Load::Idle;
        self.pull_diff
            .update(cx, |state, cx| state.set_files([], cx));
        self.runs = Load::Idle;
        self.run = None;
        self.run_detail = Load::Idle;
        self.job = None;
        self.log = Load::Idle;
        self.log_for = None;
        self.busy = None;
    }

    /// Anything on screen still queued or running.
    fn live(&self, tab: Tab) -> bool {
        match tab {
            Tab::PullRequests => self
                .pull_detail
                .ready()
                .is_some_and(|d| d.checks.iter().any(|c| c.status.active())),
            Tab::Actions => {
                self.runs
                    .ready()
                    .is_some_and(|runs| runs.iter().any(|r| r.status.active()))
                    || self
                        .run_detail
                        .ready()
                        .is_some_and(|d| d.run.status.active())
            }
            _ => false,
        }
    }
}

/// Unix seconds now.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Spor {
    fn hub_root(&self) -> Option<PathBuf> {
        self.repo.as_ref().map(|r| r.root.clone())
    }

    /// Called when a repository opens or closes.
    pub(crate) fn hub_repo_changed(&mut self, cx: &mut Context<Self>) {
        let root = self.hub_root();
        self.hub.reset(root, cx);
    }

    /// Reload the open GitHub tab every few seconds while something on it is
    /// running, otherwise now and then.
    pub(crate) fn start_hub_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut idle = 0u32;
        self.hub._poll_task = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(LIVE_EVERY).await;
            let alive = this.update(cx, |this, cx| {
                if !matches!(this.tab, Tab::PullRequests | Tab::Actions) || this.repo.is_none() {
                    return;
                }
                idle += 1;
                let live = this.hub.live(this.tab);
                if !live && idle < IDLE_TICKS {
                    return;
                }
                idle = 0;
                match this.tab {
                    Tab::PullRequests => {
                        this.load_pulls(cx);
                        if let Some(n) = this.hub.pull {
                            this.load_pull_detail(n, cx);
                        }
                    }
                    _ => {
                        this.load_runs(cx);
                        if let Some(id) = this.hub.run {
                            this.load_run_detail(id, cx);
                        }
                    }
                }
            });
            if alive.is_err() {
                break;
            }
        });
    }

    // ── Pull requests ────────────────────────────────────────────────────────

    pub(crate) fn show_pulls(&mut self, cx: &mut Context<Self>) {
        self.tab = Tab::PullRequests;
        self.hub_repo_changed(cx);
        if self.hub.pulls.needs_load() {
            self.load_pulls(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_pull_filter(&mut self, filter: PullFilter, cx: &mut Context<Self>) {
        if self.hub.pull_filter == filter {
            return;
        }
        self.hub.pull_filter = filter;
        self.hub.pulls = Load::Loading;
        self.load_pulls(cx);
        cx.notify();
    }

    pub(crate) fn load_pulls(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.hub_root() else { return };
        let filter = self.hub.pull_filter;
        if self.hub.pulls.ready().is_none() {
            self.hub.pulls = Load::Loading;
        }
        self.hub._pulls_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move { github::list_pulls(&root, filter) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hub.root.as_ref() != Some(&root) || this.hub.pull_filter != filter {
                    return;
                }
                match result {
                    Ok(list) => {
                        let first = list.first().map(|p| p.number);
                        this.hub.pulls = Load::Ready(Rc::new(list));
                        if this.hub.pull.is_none() {
                            if let Some(n) = first {
                                this.select_pull(n, cx);
                            }
                        }
                    }
                    // A failed refresh keeps the list that's up.
                    Err(_) if this.hub.pulls.ready().is_some() => {}
                    Err(e) => this.hub.pulls = Load::Failed(e),
                }
                cx.notify();
            });
        });
    }

    pub(crate) fn select_pull(&mut self, number: u64, cx: &mut Context<Self>) {
        if self.hub.pull == Some(number) && self.hub.pull_detail.ready().is_some() {
            return;
        }
        self.hub.pull = Some(number);
        self.hub.pull_detail = Load::Loading;
        self.hub.pull_diff_load = Load::Idle;
        self.load_pull_detail(number, cx);
        if self.hub.pull_tab == PullTab::Files {
            self.load_pull_diff(number, cx);
        }
        cx.notify();
    }

    /// Show pull request `number` in the Pull Requests tab.
    pub(crate) fn show_pull(&mut self, number: u64, cx: &mut Context<Self>) {
        self.show_pulls(cx);
        self.select_pull(number, cx);
    }

    fn load_pull_detail(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(root) = self.hub_root() else { return };
        self.hub._pull_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move { github::pull_detail(&root, number) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hub.root.as_ref() != Some(&root) || this.hub.pull != Some(number) {
                    return;
                }
                match result {
                    Ok(detail) => {
                        // Keep the list's row in step with what the page says.
                        if let Load::Ready(list) = &mut this.hub.pulls {
                            if let Some(row) = Rc::make_mut(list)
                                .iter_mut()
                                .find(|p| p.number == detail.pull.number)
                            {
                                *row = detail.pull.clone();
                            }
                        }
                        this.hub.pull_detail = Load::Ready(Rc::new(detail));
                    }
                    Err(_) if this.hub.pull_detail.ready().is_some() => {}
                    Err(e) => this.hub.pull_detail = Load::Failed(e),
                }
                cx.notify();
            });
        });
    }

    pub(crate) fn set_pull_tab(&mut self, tab: PullTab, cx: &mut Context<Self>) {
        self.hub.pull_tab = tab;
        if let (PullTab::Files, Some(number)) = (tab, self.hub.pull) {
            let loaded = match self.hub.pull_diff_load {
                Load::Ready(n) => n == number,
                Load::Loading => true,
                Load::Idle | Load::Failed(_) => false,
            };
            if !loaded {
                self.load_pull_diff(number, cx);
            }
        }
        cx.notify();
    }

    fn load_pull_diff(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(root) = self.hub_root() else { return };
        self.hub.pull_diff_load = Load::Loading;
        self.hub
            .pull_diff
            .update(cx, |state, cx| state.set_files([], cx));
        self.hub._diff_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move {
                        github::pull_diff(&root, number)
                            .map(|patch| DiffFile::parse(&patch).unwrap_or_default())
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hub.root.as_ref() != Some(&root) || this.hub.pull != Some(number) {
                    return;
                }
                match result {
                    Ok(files) => {
                        this.hub
                            .pull_diff
                            .update(cx, |state, cx| state.set_files(files, cx));
                        this.hub.pull_diff_load = Load::Ready(number);
                    }
                    Err(e) => this.hub.pull_diff_load = Load::Failed(e),
                }
                cx.notify();
            });
        });
    }

    /// Open a check: an Actions job in the Actions tab, anything else on
    /// the web.
    pub(crate) fn open_check(&mut self, check: &Check, cx: &mut Context<Self>) {
        match check.run_id {
            Some(run) => self.show_run(run, check.job_id, cx),
            None if !check.url.is_empty() => cx.open_url(&check.url),
            None => {}
        }
    }

    pub(crate) fn merge_pull(
        &mut self,
        number: u64,
        method: MergeMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.gh_action(
            "Merging",
            format!("Merged #{number}"),
            move |root| github::merge_pull(root, number, method),
            move |this, _, cx| {
                this.load_pulls(cx);
                this.load_pull_detail(number, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn checkout_pull(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.gh_action(
            "Checking out",
            format!("Checked out #{number}"),
            move |root| github::checkout_pull(root, number),
            |_, _, _| {}, // The poll loop sees the new HEAD and reloads.
            window,
            cx,
        );
    }

    pub(crate) fn approve_pull(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.gh_action(
            "Approving",
            format!("Approved #{number}"),
            move |root| github::approve_pull(root, number),
            move |this, _, cx| this.load_pull_detail(number, cx),
            window,
            cx,
        );
    }

    pub(crate) fn mark_ready(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.gh_action(
            "Updating",
            format!("#{number} is ready for review"),
            move |root| github::mark_ready(root, number),
            move |this, _, cx| {
                this.load_pulls(cx);
                this.load_pull_detail(number, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn set_pull_open(
        &mut self,
        number: u64,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let done = if open { "Reopened" } else { "Closed" };
        self.gh_action(
            if open { "Reopening" } else { "Closing" },
            format!("{done} #{number}"),
            move |root| github::set_pull_open(root, number, open),
            move |this, _, cx| {
                this.load_pulls(cx);
                this.load_pull_detail(number, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn comment_on_pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(number) = self.hub.pull else { return };
        let body = self.hub.comment.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        self.gh_action(
            "Commenting",
            "Comment added".to_string(),
            move |root| github::comment_on_pull(root, number, &body),
            move |this, window, cx| {
                this.hub
                    .comment
                    .update(cx, |state, cx| state.set_value("", window, cx));
                this.load_pull_detail(number, cx);
            },
            window,
            cx,
        );
    }

    // ── Actions ──────────────────────────────────────────────────────────────

    pub(crate) fn show_runs(&mut self, cx: &mut Context<Self>) {
        self.tab = Tab::Actions;
        self.hub_repo_changed(cx);
        if self.hub.runs.needs_load() {
            self.load_runs(cx);
        }
        cx.notify();
    }

    /// Show run `id` in the Actions tab, with `job` (or its most telling
    /// job) selected.
    pub(crate) fn show_run(&mut self, id: u64, job: Option<u64>, cx: &mut Context<Self>) {
        self.show_runs(cx);
        self.select_run(id, cx);
        if let Some(job) = job {
            self.hub.job = Some(job);
        }
    }

    pub(crate) fn set_runs_this_branch(&mut self, this_branch: bool, cx: &mut Context<Self>) {
        if self.hub.runs_this_branch == this_branch {
            return;
        }
        self.hub.runs_this_branch = this_branch;
        self.hub.runs = Load::Loading;
        self.load_runs(cx);
        cx.notify();
    }

    pub(crate) fn load_runs(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.hub_root() else { return };
        let branch = self
            .hub
            .runs_this_branch
            .then(|| self.repo.as_ref()?.tracking.branch.clone())
            .flatten();
        let this_branch = self.hub.runs_this_branch;
        if self.hub.runs.ready().is_none() {
            self.hub.runs = Load::Loading;
        }
        self.hub._runs_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move { github::list_runs(&root, branch.as_deref()) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hub.root.as_ref() != Some(&root) || this.hub.runs_this_branch != this_branch
                {
                    return;
                }
                match result {
                    Ok(list) => {
                        let first = list.first().map(|r| r.id);
                        this.hub.runs = Load::Ready(Rc::new(list));
                        if this.hub.run.is_none() {
                            if let Some(id) = first {
                                this.select_run(id, cx);
                            }
                        }
                    }
                    Err(_) if this.hub.runs.ready().is_some() => {}
                    Err(e) => this.hub.runs = Load::Failed(e),
                }
                cx.notify();
            });
        });
    }

    pub(crate) fn select_run(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.hub.run == Some(id) && self.hub.run_detail.ready().is_some() {
            return;
        }
        self.hub.run = Some(id);
        self.hub.run_detail = Load::Loading;
        self.hub.job = None;
        self.hub.log = Load::Idle;
        self.hub.log_for = None;
        self.load_run_detail(id, cx);
        cx.notify();
    }

    fn load_run_detail(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(root) = self.hub_root() else { return };
        self.hub._run_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move { github::run_detail(&root, id) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hub.root.as_ref() != Some(&root) || this.hub.run != Some(id) {
                    return;
                }
                match result {
                    Ok(detail) => {
                        if let Load::Ready(list) = &mut this.hub.runs {
                            if let Some(row) = Rc::make_mut(list)
                                .iter_mut()
                                .find(|r| r.id == detail.run.id)
                            {
                                *row = detail.run.clone();
                            }
                        }
                        // Keep the job if it's still there, else pick the
                        // first failed one, else the first.
                        let job = this
                            .hub
                            .job
                            .filter(|j| detail.jobs.iter().any(|x| x.id == *j))
                            .or_else(|| {
                                detail
                                    .jobs
                                    .iter()
                                    .find(|j| j.status == github::Status::Failure)
                                    .or(detail.jobs.first())
                                    .map(|j| j.id)
                            });
                        this.hub.run_detail = Load::Ready(Rc::new(detail));
                        if let Some(job) = job {
                            this.select_job(job, cx);
                        }
                    }
                    Err(_) if this.hub.run_detail.ready().is_some() => {}
                    Err(e) => this.hub.run_detail = Load::Failed(e),
                }
                cx.notify();
            });
        });
    }

    /// Select a job and load its log once it has finished.
    pub(crate) fn select_job(&mut self, id: u64, cx: &mut Context<Self>) {
        self.hub.job = Some(id);
        let finished = self
            .hub
            .run_detail
            .ready()
            .and_then(|d| d.jobs.iter().find(|j| j.id == id))
            .is_some_and(|j| !j.status.active());
        if !finished {
            self.hub.log = Load::Idle;
            self.hub.log_for = None;
        } else if self.hub.log_for != Some(id) || self.hub.log.needs_load() {
            self.load_log(id, cx);
        }
        cx.notify();
    }

    fn load_log(&mut self, job: u64, cx: &mut Context<Self>) {
        let Some(root) = self.hub_root() else { return };
        self.hub.log_for = Some(job);
        self.hub.log = Load::Loading;
        self.hub._log_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move { github::job_log(&root, job) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hub.log_for != Some(job) {
                    return;
                }
                this.hub.log = match result {
                    Ok(lines) => {
                        // A failed job opens at its first error.
                        let error = lines.iter().position(|l| l.kind == LogKind::Error);
                        this.hub.log_scroll.scroll_to_item(
                            error.unwrap_or(0),
                            if error.is_some() {
                                ScrollStrategy::Center
                            } else {
                                ScrollStrategy::Top
                            },
                        );
                        Load::Ready(Rc::new(lines))
                    }
                    Err(e) => Load::Failed(e),
                };
                cx.notify();
            });
        });
    }

    pub(crate) fn rerun(
        &mut self,
        id: u64,
        failed_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let what = if failed_only {
            "failed jobs"
        } else {
            "all jobs"
        };
        self.gh_action(
            "Re-running",
            format!("Re-running {what}"),
            move |root| github::rerun(root, id, failed_only),
            move |this, _, cx| {
                this.load_runs(cx);
                this.load_run_detail(id, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn cancel_run(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.gh_action(
            "Cancelling",
            "Cancelling the run".to_string(),
            move |root| github::cancel_run(root, id),
            move |this, _, cx| {
                this.load_runs(cx);
                this.load_run_detail(id, cx);
            },
            window,
            cx,
        );
    }

    /// Run a `gh` action in the background, one at a time, and say how it
    /// went; `after` runs when it worked.
    fn gh_action(
        &mut self,
        label: &'static str,
        done: String,
        run: impl FnOnce(&Path) -> Result<(), String> + Send + 'static,
        after: impl FnOnce(&mut Spor, &mut Window, &mut Context<Spor>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hub.busy.is_some() {
            return;
        }
        let Some(root) = self.hub_root() else { return };
        self.hub.busy = Some(label);
        cx.notify();
        self.hub._act_task = cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { run(&root) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.hub.busy = None;
                match result {
                    Ok(()) => {
                        this.ok(done, window, cx);
                        after(this, window, cx);
                    }
                    Err(e) => this.error(format!("{label} failed: {}", first_line(&e)), window, cx),
                }
                cx.notify();
            });
        });
    }
}
