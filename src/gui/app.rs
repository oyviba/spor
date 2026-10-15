//! The workspace view, organized like Zed: a title bar with the repository
//! and branch, a left dock (the git panel or the branch list, toggled from
//! the status bar), a center pane with Changes and History tabs, and a
//! status bar. Everything git-related the views trigger lives here.

use super::assets::icon;
use super::history_options::HistoryOptions;
use super::palette::PrSearch;
use super::repo::{self, Change, RepoHead, Snapshot};
use super::support::{self, first_line, plural};
use super::theme::{self, Appearance};
use super::workspaces::Workspaces;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::{DiffMode, DiffState},
    input::{InputState, TextareaState},
    notification::Notification,
    ActiveTheme as _, Sizable as _, WindowExt as _,
};
use gpui_kit::{
    actions, prelude::FluentBuilder as _, App, AppContext as _, ClipboardItem, Context, Entity,
    FocusHandle, Focusable as _, InteractiveElement as _, KeyBinding, ParentElement as _,
    PathPromptOptions, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, UniformListScrollHandle, WeakEntity, Window,
};
use spor::git::{self, CommitDetails, StatusEntry};
use spor::remote::{self, PrInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the open repository is fetched in the background, so the app
/// can tell when there's something to pull.
const AUTO_FETCH_EVERY: Duration = Duration::from_secs(5 * 60);

actions!(
    spor,
    [
        ToggleDock,
        ToggleDockSide,
        ToggleRepositories,
        ShowGitPanel,
        ShowBranches,
        ShowChanges,
        ShowHistory,
        Refresh,
        UndoLast,
        Commit,
        OpenRepository,
        NewBranch,
        OlderCommit,
        NewerCommit,
        CheckoutSelected,
        Sync,
        ToggleDiffMode,
        EditTheme,
        Quit,
        NewWorkspace,
        OpenPalette,
        ShowShortcuts,
        Fetch,
        Pull,
        Push,
        OpenPullRequest,
        StashChanges,
        Workspace1,
        Workspace2,
        Workspace3,
        Workspace4,
        Workspace5,
        Workspace6,
        Workspace7,
        Workspace8,
        Workspace9,
    ]
);

/// The ⌘1–⌘9 action for the workspace at `ix`.
pub(crate) fn workspace_action(ix: usize) -> Option<Box<dyn gpui_kit::Action>> {
    Some(match ix {
        0 => Box::new(Workspace1),
        1 => Box::new(Workspace2),
        2 => Box::new(Workspace3),
        3 => Box::new(Workspace4),
        4 => Box::new(Workspace5),
        5 => Box::new(Workspace6),
        6 => Box::new(Workspace7),
        7 => Box::new(Workspace8),
        8 => Box::new(Workspace9),
        _ => return None,
    })
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-b", ToggleDock, Some("Spor")),
        KeyBinding::new("secondary-shift-e", ToggleRepositories, Some("Spor")),
        KeyBinding::new("secondary-shift-g", ShowGitPanel, Some("Spor")),
        KeyBinding::new("secondary-shift-b", ShowBranches, Some("Spor")),
        KeyBinding::new("secondary-shift-c", ShowChanges, Some("Spor")),
        KeyBinding::new("secondary-shift-h", ShowHistory, Some("Spor")),
        // Like Slack: ⌘1–⌘9 jump to a workspace.
        KeyBinding::new("secondary-1", Workspace1, Some("Spor")),
        KeyBinding::new("secondary-2", Workspace2, Some("Spor")),
        KeyBinding::new("secondary-3", Workspace3, Some("Spor")),
        KeyBinding::new("secondary-4", Workspace4, Some("Spor")),
        KeyBinding::new("secondary-5", Workspace5, Some("Spor")),
        KeyBinding::new("secondary-6", Workspace6, Some("Spor")),
        KeyBinding::new("secondary-7", Workspace7, Some("Spor")),
        KeyBinding::new("secondary-8", Workspace8, Some("Spor")),
        KeyBinding::new("secondary-9", Workspace9, Some("Spor")),
        KeyBinding::new("secondary-shift-w", NewWorkspace, Some("Spor")),
        KeyBinding::new("secondary-r", Refresh, Some("Spor")),
        KeyBinding::new("secondary-z", UndoLast, Some("Spor")),
        KeyBinding::new("secondary-enter", Commit, Some("Spor")),
        // The text area binds ⌘↩ itself; this more specific binding wins.
        KeyBinding::new("secondary-enter", Commit, Some("CommitEditor > Input")),
        KeyBinding::new("secondary-o", OpenRepository, Some("Spor")),
        KeyBinding::new("secondary-shift-n", NewBranch, Some("Spor")),
        KeyBinding::new("secondary-shift-s", Sync, Some("Spor")),
        KeyBinding::new("secondary-q", Quit, Some("Spor")),
        KeyBinding::new("secondary-,", EditTheme, Some("Spor")),
        KeyBinding::new("secondary-p", OpenPalette, Some("Spor")),
        KeyBinding::new("secondary-/", ShowShortcuts, Some("Spor")),
        KeyBinding::new("down", OlderCommit, Some("SporHistory")),
        KeyBinding::new("j", OlderCommit, Some("SporHistory")),
        KeyBinding::new("up", NewerCommit, Some("SporHistory")),
        KeyBinding::new("k", NewerCommit, Some("SporHistory")),
        KeyBinding::new("enter", CheckoutSelected, Some("SporHistory")),
    ]);
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Changes,
    History,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DockPanel {
    Git,
    Branches,
}

/// Which side of the center pane the dock sits on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DockSide {
    Left,
    Right,
}

/// Panel layout that survives restarts: whether the repository list is
/// shown, and the dock's side. Stored as `repos<TAB>1` / `dock<TAB>right`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Layout {
    pub show_repos: bool,
    pub dock_side: DockSide,
}

impl Layout {
    const SETTING: &'static str = "layout";

    fn saved() -> Self {
        let mut layout = Self {
            show_repos: true,
            dock_side: DockSide::Right,
        };
        for line in support::read_setting(Self::SETTING)
            .unwrap_or_default()
            .lines()
        {
            match line.split_once('\t') {
                Some(("repos", v)) => layout.show_repos = v != "0",
                Some(("dock", v)) => {
                    layout.dock_side = if v == "left" {
                        DockSide::Left
                    } else {
                        DockSide::Right
                    }
                }
                _ => {}
            }
        }
        layout
    }

    pub(crate) fn save(self) {
        let dock = match self.dock_side {
            DockSide::Left => "left",
            DockSide::Right => "right",
        };
        let repos = if self.show_repos { "1" } else { "0" };
        support::write_setting(Self::SETTING, &format!("repos\t{repos}\ndock\t{dock}\n"));
    }
}

/// Something ⌘Z can take back.
pub enum Undo {
    /// Put files' working-tree contents back (None: they didn't exist).
    Restore(Vec<(String, Option<Vec<u8>>)>),
    Stage(Vec<String>),
    Unstage(Vec<StatusEntry>),
    Uncommit {
        hash: String,
        message: String,
    },
    /// Move `branch` back to where it was before a merge.
    Merge {
        branch: String,
        before: String,
    },
}

/// The selected commit, loaded for the History detail pane.
pub struct CommitInfo {
    pub hash: String,
    pub details: CommitDetails,
}

/// A deferred call on the view, for menu items built outside it.
pub(crate) type SporAction = std::rc::Rc<dyn Fn(&mut Spor, &mut Window, &mut Context<Spor>)>;

/// Cancel + primary action buttons for a dialog; the action runs `on_ok`.
fn dialog_footer(id: &'static str, action: &'static str) -> impl gpui_kit::IntoElement {
    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
    DialogFooter::new()
        .gap_2()
        .child(DialogClose::new().child(Button::new("dialog-cancel").label("Cancel").outline()))
        .child(DialogAction::new().child(Button::new(id).label(action).primary()))
}

/// Marker type identifying the undo notification, so a newer one replaces it.
struct UndoNote;

/// Marker type for the "new commits to pull" notification.
struct PullNote;

pub struct Spor {
    pub(crate) repo: Option<Snapshot>,
    pub(crate) open_error: Option<String>,
    pub(crate) workspaces: Workspaces,
    pub(crate) tab: Tab,
    pub(crate) dock: Option<DockPanel>,
    pub(crate) layout: Layout,
    pub(crate) history_options: HistoryOptions,
    pub(crate) selected: Option<usize>,
    pub(crate) commit: Option<CommitInfo>,
    pub(crate) commit_diff: Entity<DiffState>,
    pub(crate) changes_diff: Entity<DiffState>,
    pub(crate) message: Entity<TextareaState>,
    pub(crate) branch_name: Entity<InputState>,
    workspace_name: Entity<InputState>,
    pub(crate) history_scroll: UniformListScrollHandle,
    pub(crate) history_focus: FocusHandle,
    /// Holds focus when nothing else does, so app shortcuts always fire.
    pub(crate) root_focus: FocusHandle,
    pub(crate) diff_mode: DiffMode,
    pub(crate) job: Option<&'static str>,
    pub(crate) prs: HashMap<String, PrInfo>,
    /// Every PR, for the ⌘P search, and the repository it belongs to.
    pub(crate) pr_search: PrSearch,
    pub(crate) pr_search_root: Option<PathBuf>,
    /// Branch and upstream counts of the workspace's other repositories,
    /// for the repository sidebar.
    pub(crate) repo_heads: HashMap<PathBuf, RepoHead>,
    /// The last background fetch: which repository, and when.
    last_fetch: Option<(PathBuf, Instant)>,
    pub(crate) appearance: Appearance,
    undo: Vec<(String, Undo)>,
    fingerprint: Option<String>,
    _job_task: Task<()>,
    _pr_task: Task<()>,
    pub(crate) _pr_search_task: Task<()>,
    _heads_task: Task<()>,
    _fetch_task: Task<()>,
    _commit_task: Task<()>,
    _poll_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Spor {
    pub fn new(initial: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = Appearance::saved();
        theme::apply(appearance, window, cx);

        let diff_mode = DiffMode::Unified;
        let commit_diff = cx.new(|cx| DiffState::new([], cx).with_mode(diff_mode));
        let changes_diff = cx.new(|cx| DiffState::new([], cx).with_mode(diff_mode));
        let message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Commit message")
                .auto_grow(3, 10)
        });
        let branch_name = cx.new(|cx| InputState::new(window, cx).placeholder("Branch name"));
        let workspace_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("Work, Side projects, …"));
        let history_focus = cx.focus_handle().tab_stop(true);
        let root_focus = cx.focus_handle();
        root_focus.focus(window, cx);

        let subscriptions = vec![
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.appearance == Appearance::System {
                    theme::apply(Appearance::System, window, cx);
                }
            }),
            cx.observe(&message, |_, _, cx| cx.notify()),
        ];

        let mut this = Self {
            repo: None,
            open_error: None,
            workspaces: Workspaces::load(),
            tab: Tab::Changes,
            dock: Some(DockPanel::Git),
            layout: Layout::saved(),
            history_options: HistoryOptions::saved(),
            selected: None,
            commit: None,
            commit_diff,
            changes_diff,
            message,
            branch_name,
            workspace_name,
            history_scroll: UniformListScrollHandle::new(),
            history_focus,
            root_focus,
            diff_mode,
            job: None,
            prs: HashMap::new(),
            pr_search: PrSearch::Idle,
            pr_search_root: None,
            repo_heads: HashMap::new(),
            last_fetch: None,
            appearance,
            undo: Vec::new(),
            fingerprint: None,
            _job_task: Task::ready(()),
            _pr_task: Task::ready(()),
            _pr_search_task: Task::ready(()),
            _heads_task: Task::ready(()),
            _fetch_task: Task::ready(()),
            _commit_task: Task::ready(()),
            _poll_task: Task::ready(()),
            _subscriptions: subscriptions,
        };
        let initial = initial.or_else(|| this.workspaces.current().landing().cloned());
        if let Some(dir) = initial {
            this.open_repo(&dir, window, cx);
        }
        this.start_polling(window, cx);
        this.start_auto_fetch(window, cx);
        this.refresh_repo_heads(cx);
        this
    }

    // ── Notifications ────────────────────────────────────────────────────────

    pub(crate) fn info(&self, text: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::info(text), cx);
    }

    pub(crate) fn ok(&self, text: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(text), cx);
    }

    pub(crate) fn error(&self, text: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(text), cx);
    }

    /// Record an undoable action and say what happened, with an Undo button.
    fn did(&mut self, text: String, undo: Undo, window: &mut Window, cx: &mut Context<Self>) {
        self.undo.push((text.clone(), undo));
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
        let spor = cx.entity().downgrade();
        window.remove_notification::<UndoNote>(cx);
        window.push_notification(
            Notification::success(text)
                .id::<UndoNote>()
                .action(move |_, _, _| {
                    let spor = spor.clone();
                    Button::new("undo").label("Undo").small().ghost().on_click(
                        move |_, window, cx| {
                            window.remove_notification::<UndoNote>(cx);
                            let _ = spor.update(cx, |this, cx| this.undo_last(window, cx));
                        },
                    )
                }),
            cx,
        );
    }

    // ── Opening ──────────────────────────────────────────────────────────────

    pub(crate) fn open_repo(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let opened = support::repo_root(dir).and_then(|root| {
            // Every git call in spor::git runs in the working directory.
            std::env::set_current_dir(&root).map_err(|e| e.to_string())?;
            Snapshot::load(&root)
        });
        match opened {
            Ok(snapshot) => {
                support::remember_repo(&snapshot.root);
                self.workspaces.opened(&snapshot.root);
                self.workspaces.save();
                window.set_window_title(&format!("{} — Spor", snapshot.name));
                let dirty = !snapshot.changes.is_empty();
                let head = snapshot.head_row();
                self.fingerprint = repo::fingerprint(&snapshot.root);
                self.repo = Some(snapshot);
                self.open_error = None;
                self.undo.clear();
                self.commit = None;
                self.selected = None;
                self.prs.clear();
                self.message
                    .update(cx, |state, cx| state.set_value("", window, cx));
                self.tab = if dirty { Tab::Changes } else { Tab::History };
                self.load_changes_diff(cx);
                if let Some(head) = head {
                    self.select_commit(head, cx);
                }
                self.fetch_prs(cx);
                self.refresh_repo_heads(cx);
            }
            Err(e) => self.open_error = Some(e),
        }
        cx.notify();
    }

    pub(crate) fn close_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.repo = None;
        self.commit = None;
        self.selected = None;
        self.undo.clear();
        self.fingerprint = None;
        window.set_window_title("Spor");
        self.refresh_repo_heads(cx);
        cx.notify();
    }

    pub(crate) fn pick_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ =
                        this.update_in(cx, |this, window, cx| this.open_repo(&path, window, cx));
                }
            }
        })
        .detach();
    }

    // ── Workspaces ───────────────────────────────────────────────────────────

    /// Switch to workspace `ix` and reopen the repository it was last on.
    pub(crate) fn switch_workspace(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ix >= self.workspaces.list.len() {
            return;
        }
        if ix == self.workspaces.active && self.repo.is_some() {
            return;
        }
        self.workspaces.active = ix;
        self.workspaces.save();
        self.open_error = None;
        self.land(window, cx);
    }

    /// Open the active workspace's landing repository, or its empty page.
    fn land(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.workspaces.current().landing().cloned() {
            Some(path) => self.open_repo(&path, window, cx),
            None => self.close_repo(window, cx),
        }
    }

    /// Ask for a name, then create a workspace and switch to it.
    pub(crate) fn new_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_workspace_dialog(None, window, cx);
    }

    pub(crate) fn rename_workspace(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name_workspace_dialog(Some(ix), window, cx);
    }

    fn name_workspace_dialog(
        &mut self,
        rename: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = rename
            .and_then(|ix| self.workspaces.list.get(ix))
            .map(|ws| ws.name.clone())
            .unwrap_or_default();
        let input = self.workspace_name.clone();
        input.update(cx, |state, cx| state.set_value(current, window, cx));
        let spor = cx.entity().downgrade();
        let (title, action) = match rename {
            Some(_) => ("Rename Workspace", "Rename"),
            None => ("New Workspace", "Create Workspace"),
        };
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let input_for_ok = input.clone();
            input.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title(title)
                .w(gpui_kit::px(400.))
                .child(
                    gpui_kit::component::v_flex()
                        .gap_2()
                        .child(
                            gpui_kit::div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Group repositories you work on together, like work and side projects."),
                        )
                        .child(gpui_kit::component::input::Input::new(&input)),
                )
                .footer(dialog_footer("name-workspace", action))
                .on_ok(move |_, window, cx| {
                    let name = input_for_ok.read(cx).value().trim().replace('\t', " ");
                    if name.is_empty() {
                        return false;
                    }
                    let _ = spor.update(cx, |this, cx| match rename {
                        Some(ix) => {
                            if let Some(ws) = this.workspaces.list.get_mut(ix) {
                                ws.name = name;
                            }
                            this.workspaces.save();
                            cx.notify();
                        }
                        None => {
                            let ix = this.workspaces.create(&name);
                            this.switch_workspace(ix, window, cx);
                            this.info(
                                format!("Created “{name}” — open a repository to add it"),
                                window,
                                cx,
                            );
                        }
                    });
                    true
                })
        });
    }

    /// Delete a workspace after confirming. Its repositories stay on disk.
    pub(crate) fn delete_workspace(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.workspaces.list.get(ix) else {
            return;
        };
        if self.workspaces.list.len() <= 1 {
            self.info("Spor needs at least one workspace", window, cx);
            return;
        }
        let name = ws.name.clone();
        let count = ws.repos.len();
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let spor = spor.clone();
            dialog
                .title(format!("Delete “{name}”?"))
                .w(gpui_kit::px(400.))
                .child(
                    gpui_kit::div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(match count {
                            0 => "It has no repositories.".to_string(),
                            1 => "Its repository will be removed from Spor but stays on disk."
                                .to_string(),
                            n => format!(
                                "Its {n} repositories will be removed from Spor but stay on disk."
                            ),
                        }),
                )
                .footer(dialog_footer("delete-workspace", "Delete Workspace"))
                .on_ok(move |_, window, cx| {
                    let _ = spor.update(cx, |this, cx| {
                        let was_active = this.workspaces.active == ix;
                        if this.workspaces.delete(ix) {
                            this.workspaces.save();
                            if was_active {
                                this.land(window, cx);
                            }
                            cx.notify();
                        }
                    });
                    true
                })
        });
    }

    /// Move the open repository to another workspace, and follow it there.
    pub(crate) fn move_repo_to(&mut self, to: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        self.workspaces.move_repo(&root, to);
        self.workspaces.opened(&root);
        self.workspaces.save();
        let name = self.workspaces.current().name.clone();
        self.ok(format!("Moved to {name}"), window, cx);
        cx.notify();
    }

    /// Take a repository out of the current workspace (it stays on disk).
    pub(crate) fn remove_repo(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.workspaces.remove_repo(root);
        self.workspaces.save();
        let was_open = self.repo.as_ref().is_some_and(|r| r.root == root);
        if was_open {
            self.land(window, cx);
        }
        cx.notify();
    }

    // ── Loading ──────────────────────────────────────────────────────────────

    /// Re-read the repository, keeping the selected commit if it still exists.
    pub(crate) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        match Snapshot::load(&root) {
            Ok(snapshot) => {
                let keep = self.commit.as_ref().map(|c| c.hash.clone());
                self.fingerprint = repo::fingerprint(&root);
                if let Some(old) = &self.repo {
                    self.notice_new_commits(&old.tracking, &snapshot.tracking, window, cx);
                }
                let selected = keep
                    .and_then(|h| snapshot.rows.iter().position(|r| r.commit.hash == h))
                    .or_else(|| snapshot.head_row());
                self.repo = Some(snapshot);
                // File contents can change without the change list
                // changing, so the diff is always reloaded.
                self.load_changes_diff(cx);
                self.selected = None;
                if let Some(i) = selected {
                    self.select_commit(i, cx);
                }
            }
            Err(e) => self.error(format!("Couldn't read the repository: {e}"), window, cx),
        }
        cx.notify();
    }

    fn load_changes_diff(&mut self, cx: &mut Context<Self>) {
        let files = self
            .repo
            .as_ref()
            .map(|r| repo::uncommitted_files(&r.changes))
            .unwrap_or_default();
        self.changes_diff
            .update(cx, |state, cx| state.set_files(files, cx));
    }

    /// Watch the repository for changes made outside the app (an editor
    /// saving, a terminal commit) and reload when anything moves.
    fn start_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut tick = 0u32;
        self._poll_task = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            tick += 1;
            // Other repositories change too (a terminal checkout, a pull).
            if tick.is_multiple_of(15)
                && this
                    .update(cx, |this, cx| this.refresh_repo_heads(cx))
                    .is_err()
            {
                break;
            }
            let Ok(Some(root)) = this.read_with(cx, |this, _| {
                (this.job.is_none()).then(|| this.repo.as_ref().map(|r| r.root.clone()))?
            }) else {
                continue;
            };
            let fresh = cx
                .background_spawn(async move { repo::fingerprint(&root) })
                .await;
            let alive = this.update_in(cx, |this, window, cx| {
                if fresh.is_some() && fresh != this.fingerprint {
                    this.reload(window, cx);
                }
            });
            if alive.is_err() {
                break;
            }
        });
    }

    /// Fetch the open repository quietly now and then, so behind counts are
    /// real and the app can say when there's something to pull. Failures
    /// (offline, no credentials helper) stay silent; Sync reports them.
    fn start_auto_fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._fetch_task = cx.spawn_in(window, async move |this, cx| loop {
            let Ok(due) = this.read_with(cx, |this, _| {
                let repo = this.repo.as_ref().filter(|r| !r.remotes.is_empty())?;
                let fresh = this.last_fetch.as_ref().is_some_and(|(root, at)| {
                    *root == repo.root && at.elapsed() < AUTO_FETCH_EVERY
                });
                (this.job.is_none() && !fresh).then(|| repo.root.clone())
            }) else {
                break;
            };
            if let Some(root) = due {
                let fetched = root.clone();
                let _ = this.update(cx, |this, _| {
                    this.last_fetch = Some((fetched, Instant::now()))
                });
                let _ = cx
                    .background_spawn(async move {
                        support::git_network(&root, &["fetch", "--all", "--prune"])
                    })
                    .await;
                // The poll loop sees the moved refs and reloads.
            }
            cx.background_executor()
                .timer(Duration::from_secs(10))
                .await;
        });
    }

    /// Say so when the branch's upstream gained commits, with a Pull button.
    fn notice_new_commits(
        &self,
        old: &git::TrackingInfo,
        new: &git::TrackingInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if new.branch.is_none() || old.branch != new.branch || new.behind <= old.behind {
            return;
        }
        let upstream = new.upstream.clone().unwrap_or_else(|| "the remote".into());
        let branch = new.branch.clone().unwrap_or_default();
        let text = if new.ahead > 0 {
            format!(
                "{upstream} has {} you don't — {branch} has diverged, so merge or rebase",
                plural(new.behind, "new commit")
            )
        } else {
            format!(
                "{upstream} has {} to pull",
                plural(new.behind, "new commit")
            )
        };
        let spor = cx.entity().downgrade();
        let can_pull = new.ahead == 0;
        window.remove_notification::<PullNote>(cx);
        window.push_notification(
            Notification::info(text)
                .id::<PullNote>()
                .when(can_pull, |note| {
                    note.action(move |_, _, _| {
                        let spor = spor.clone();
                        Button::new("pull-now")
                            .label("Pull")
                            .small()
                            .ghost()
                            .on_click(move |_, window, cx| {
                                window.remove_notification::<PullNote>(cx);
                                let _ = spor.update(cx, |this, cx| this.pull(window, cx));
                            })
                    })
                }),
            cx,
        );
    }

    /// Read the branch of every repository in the active workspace, in the
    /// background, for the repository sidebar.
    pub(crate) fn refresh_repo_heads(&mut self, cx: &mut Context<Self>) {
        let roots = self.workspaces.current().repos.clone();
        self._heads_task = cx.spawn(async move |this, cx| {
            let heads = cx
                .background_spawn(async move {
                    roots
                        .into_iter()
                        .filter_map(|root| repo::head(&root).map(|h| (root, h)))
                        .collect::<HashMap<_, _>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.repo_heads != heads {
                    this.repo_heads = heads;
                    cx.notify();
                }
            });
        });
    }

    fn fetch_prs(&mut self, cx: &mut Context<Self>) {
        self._pr_task = cx.spawn(async move |this, cx| {
            let prs = cx
                .background_spawn(async { remote::fetch_prs().unwrap_or_default() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.prs = prs
                    .into_iter()
                    .map(|p| (p.head_branch.clone(), p))
                    .collect();
                cx.notify();
            });
        });
    }

    // ── History ──────────────────────────────────────────────────────────────

    pub(crate) fn select_commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.repo.as_ref().and_then(|r| r.rows.get(ix)) else {
            return;
        };
        if self.selected == Some(ix) {
            return;
        }
        self.selected = Some(ix);
        self.history_scroll
            .scroll_to_item(ix, ScrollStrategy::Center);
        let hash = row.commit.hash.clone();
        let parents = row.commit.parents.clone();
        if self.commit.as_ref().is_some_and(|c| c.hash == hash) {
            cx.notify();
            return;
        }
        self._commit_task = cx.spawn(async move |this, cx| {
            let (details, files) = cx
                .background_spawn({
                    let hash = hash.clone();
                    async move {
                        (
                            git::commit_details(&hash).unwrap_or_default(),
                            repo::commit_files(&hash, &parents),
                        )
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.commit = Some(CommitInfo { hash, details });
                this.commit_diff
                    .update(cx, |state, cx| state.set_files(files, cx));
                cx.notify();
            });
        });
        cx.notify();
    }

    pub(crate) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else { return };
        if repo.rows.is_empty() {
            return;
        }
        let next = self
            .selected
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(repo.rows.len() - 1);
        self.select_commit(next, cx);
    }

    /// Show a branch or tag's tip in History.
    pub(crate) fn reveal_ref(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self
            .repo
            .as_ref()
            .and_then(|r| r.ref_rows.get(key).copied())
        else {
            self.info(
                format!("{key} is older than the loaded history"),
                window,
                cx,
            );
            return;
        };
        self.tab = Tab::History;
        self.selected = None;
        self.select_commit(i, cx);
        self.history_focus.focus(window, cx);
    }

    // ── Working tree ─────────────────────────────────────────────────────────

    /// Tick or untick a file: stage everything in it, or unstage it.
    pub(crate) fn toggle_change(
        &mut self,
        change: &Change,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if change.included() {
            let staged = change.staged_entries();
            if let Err(e) = staged.iter().try_for_each(git::unstage) {
                return self.error(format!("Couldn't unstage: {}", first_line(&e)), window, cx);
            }
            self.undo.push((
                format!("unstage {}", change.path),
                Undo::Stage(vec![change.path.clone()]),
            ));
        } else {
            if let Err(e) = git::stage(&change.path) {
                return self.error(format!("Couldn't stage: {}", first_line(&e)), window, cx);
            }
            self.undo.push((
                format!("stage {}", change.path),
                Undo::Unstage(vec![StatusEntry {
                    status: git::FileStatus::Staged,
                    path: change.path.clone(),
                    orig_path: change.orig_path.clone(),
                }]),
            ));
        }
        self.reload(window, cx);
    }

    /// Scroll Uncommitted Changes to a file's diff.
    pub(crate) fn show_change(&mut self, path: &str, cx: &mut Context<Self>) {
        self.tab = Tab::Changes;
        self.changes_diff
            .update(cx, |state, cx| state.scroll_to_file(path, cx));
        cx.notify();
    }

    /// Stage or unstage every changed file.
    pub(crate) fn set_all_staged(
        &mut self,
        stage: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else { return };
        let changes = repo.changes.clone();
        let result = if stage {
            let paths: Vec<String> = changes
                .iter()
                .filter(|c| !c.included())
                .map(|c| c.path.clone())
                .collect();
            let r = paths.iter().try_for_each(|p| git::stage(p));
            if r.is_ok() && !paths.is_empty() {
                let entries = paths
                    .into_iter()
                    .map(|path| StatusEntry {
                        status: git::FileStatus::Staged,
                        path,
                        orig_path: None,
                    })
                    .collect();
                self.undo.push(("stage all".into(), Undo::Unstage(entries)));
            }
            r
        } else {
            let entries: Vec<StatusEntry> =
                changes.iter().flat_map(|c| c.staged_entries()).collect();
            let r = entries.iter().try_for_each(git::unstage);
            if r.is_ok() && !entries.is_empty() {
                let paths = entries.iter().map(|e| e.path.clone()).collect();
                self.undo.push(("unstage all".into(), Undo::Stage(paths)));
            }
            r
        };
        if let Err(e) = result {
            self.error(format!("Couldn't stage: {}", first_line(&e)), window, cx);
        }
        self.reload(window, cx);
    }

    /// Throw away unstaged changes — immediately, but undoably: the current
    /// contents are kept in memory so ⌘Z can put them back.
    pub(crate) fn discard(
        &mut self,
        changes: &[Change],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut restore = Vec::new();
        for change in changes.iter().filter(|c| c.unstaged) {
            let before = std::fs::read(&change.path).ok();
            let result = if change.untracked {
                git::remove_untracked(&change.path)
            } else {
                git::discard_worktree(&change.path)
            };
            match result {
                Ok(()) => restore.push((change.path.clone(), before)),
                Err(e) => {
                    self.error(format!("Couldn't discard: {}", first_line(&e)), window, cx);
                    break;
                }
            }
        }
        if !restore.is_empty() {
            let text = match restore.as_slice() {
                [(path, _)] => format!(
                    "Discarded changes to {}",
                    path.rsplit('/').next().unwrap_or(path)
                ),
                many => format!("Discarded changes to {}", plural(many.len(), "file")),
            };
            self.did(text, Undo::Restore(restore), window, cx);
        }
        self.reload(window, cx);
    }

    pub(crate) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_string();
        let staged = self.repo.as_ref().map_or(0, |r| r.staged_count());
        if message.is_empty() || staged == 0 {
            return;
        }
        match git::commit(&message) {
            Ok(()) => {
                let hash = git::head_sha().unwrap_or_default();
                self.message
                    .update(cx, |state, cx| state.set_value("", window, cx));
                let summary = first_line(&message).to_string();
                self.did(
                    format!("Committed “{summary}”"),
                    Undo::Uncommit { hash, message },
                    window,
                    cx,
                );
                self.reload(window, cx);
            }
            Err(e) => self.error(format!("Commit failed: {}", first_line(&e)), window, cx),
        }
    }

    /// Take back the most recent undoable action.
    pub(crate) fn undo_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((label, action)) = self.undo.pop() else {
            return self.info("Nothing to undo", window, cx);
        };
        let done = match &action {
            Undo::Restore(files) if files.len() == 1 => {
                format!(
                    "Restored {}",
                    files[0].0.rsplit('/').next().unwrap_or(&files[0].0)
                )
            }
            Undo::Restore(files) => format!("Restored {}", plural(files.len(), "file")),
            Undo::Uncommit { .. } => "Commit undone — its changes are staged again".into(),
            Undo::Merge { branch, .. } => format!("Merge into {branch} undone"),
            _ => format!("Undid {label}"),
        };
        let result = match action {
            Undo::Restore(files) => {
                files
                    .into_iter()
                    .try_for_each(|(path, contents)| match contents {
                        Some(bytes) => {
                            if let Some(dir) = Path::new(&path).parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            std::fs::write(&path, bytes).map_err(|e| e.to_string())
                        }
                        None => std::fs::remove_file(&path).map_err(|e| e.to_string()),
                    })
            }
            Undo::Stage(paths) => paths.iter().try_for_each(|p| git::stage(p)),
            Undo::Unstage(entries) => entries.iter().try_for_each(git::unstage),
            Undo::Uncommit { hash, message } => {
                if git::head_sha().ok().as_deref() != Some(hash.as_str()) {
                    Err("the branch has moved on since that commit".into())
                } else {
                    git::uncommit().map(|()| {
                        self.message
                            .update(cx, |state, cx| state.set_value(message, window, cx));
                        self.tab = Tab::Changes;
                        self.dock = Some(DockPanel::Git);
                    })
                }
            }
            Undo::Merge { branch, before } => {
                let current = self.repo.as_ref().and_then(|r| r.tracking.branch.clone());
                if current.as_deref() != Some(branch.as_str()) {
                    Err(format!("switch back to {branch} first"))
                } else {
                    git::reset_keep(&before)
                }
            }
        };
        match result {
            Ok(()) => {
                self.info(done, window, cx);
                self.reload(window, cx);
            }
            Err(e) => self.error(format!("Couldn't undo: {}", first_line(&e)), window, cx),
        }
    }

    // ── Branches & stashes ───────────────────────────────────────────────────

    pub(crate) fn checkout(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Checking out `origin/x` lands on a local `x` tracking it rather than
        // a detached HEAD.
        let target = self
            .repo
            .as_ref()
            .and_then(|r| r.without_remote(name))
            .unwrap_or(name)
            .to_string();
        match git::checkout_branch(&target) {
            Ok(()) => {
                self.ok(format!("Switched to {target}"), window, cx);
                self.reload(window, cx);
            }
            Err(e) if git::is_worktree_conflict(&e) => self.offer_stash(target, window, cx),
            Err(e) => self.error(format!("Couldn't switch: {}", first_line(&e)), window, cx),
        }
    }

    /// Check out the branch at the selected commit, preferring a local branch.
    pub(crate) fn checkout_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else { return };
        let Some(row) = self.selected.and_then(|i| repo.rows.get(i)) else {
            return;
        };
        let refs: Vec<&String> = row
            .commit
            .refs
            .iter()
            .filter(|r| !r.starts_with("tag:") && !r.ends_with("/HEAD"))
            .collect();
        let pick = refs
            .iter()
            .find(|r| repo.without_remote(r).is_none())
            .or(refs.first())
            .map(|r| r.to_string());
        match pick {
            Some(name) if row.commit.head_ref.as_deref() == Some(name.as_str()) => {}
            Some(name) => self.checkout(&name, window, cx),
            None => self.info("No branch points here", window, cx),
        }
    }

    fn offer_stash(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let spor = spor.clone();
            let target = target.clone();
            dialog
                .title("Stash your changes?")
                .child(format!(
                    "Switching to {target} would overwrite uncommitted changes. Stash them \
                     first — they'll be waiting under Stashes in the branch list."
                ))
                .footer(dialog_footer("stash", "Stash and Switch"))
                .on_ok(move |_, window, cx| {
                    let target = target.clone();
                    let _ = spor.update(cx, |this, cx| this.stash_and_switch(&target, window, cx));
                    true
                })
        });
    }

    fn stash_and_switch(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        match git::stash_push().and_then(|_| git::checkout_branch(target)) {
            Ok(()) => {
                self.ok(
                    format!("Stashed your changes and switched to {target}"),
                    window,
                    cx,
                );
                self.reload(window, cx);
            }
            Err(e) => self.error(format!("Couldn't switch: {}", first_line(&e)), window, cx),
        }
    }

    pub(crate) fn stash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match git::stash_push() {
            Ok(()) => {
                self.ok("Changes stashed", window, cx);
                self.reload(window, cx);
            }
            Err(e) => self.error(format!("Couldn't stash: {}", first_line(&e)), window, cx),
        }
    }

    pub(crate) fn apply_stash(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match git::stash_pop_at(index) {
            Ok(()) => {
                self.ok("Stashed changes restored", window, cx);
                self.tab = Tab::Changes;
                self.reload(window, cx);
            }
            Err(e) => self.error(
                format!("Couldn't apply the stash: {}", first_line(&e)),
                window,
                cx,
            ),
        }
    }

    /// Ask for a name and create a branch at `sha` (HEAD when None).
    pub(crate) fn new_branch(
        &mut self,
        sha: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else { return };
        let row = match &sha {
            Some(sha) => repo.rows.iter().find(|r| &r.commit.hash == sha),
            None => repo.head_row().and_then(|i| repo.rows.get(i)),
        };
        let Some(row) = row else { return };
        let sha = row.commit.hash.clone();
        let from = format!("{} {}", row.commit.short, row.commit.subject);
        let input = self.branch_name.clone();
        input.update(cx, |state, cx| state.set_value("", window, cx));
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let sha = sha.clone();
            let input_for_ok = input.clone();
            input.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title("New Branch")
                .w(gpui_kit::px(440.))
                .child(
                    gpui_kit::component::v_flex()
                        .gap_2()
                        .child(
                            gpui_kit::div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("From {from}")),
                        )
                        .child(gpui_kit::component::input::Input::new(&input)),
                )
                .footer(dialog_footer("create-branch", "Create Branch"))
                .on_ok(move |_, window, cx| {
                    let name = input_for_ok.read(cx).value().trim().to_string();
                    if name.is_empty() || name.contains(char::is_whitespace) {
                        return false;
                    }
                    let sha = sha.clone();
                    let _ = spor.update(cx, |this, cx| this.create_branch(&name, &sha, window, cx));
                    true
                })
        });
    }

    fn create_branch(
        &mut self,
        name: &str,
        sha: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match git::create_branch_at(name, sha) {
            Ok(()) => {
                self.ok(format!("Created and switched to {name}"), window, cx);
                self.reload(window, cx);
            }
            Err(e) => self.error(
                format!("Couldn't create the branch: {}", first_line(&e)),
                window,
                cx,
            ),
        }
    }

    // ── Network ──────────────────────────────────────────────────────────────

    /// What the sync button would do right now, as its label.
    pub(crate) fn sync_label(&self) -> String {
        let Some(repo) = &self.repo else {
            return "Fetch".into();
        };
        let t = &repo.tracking;
        match (&t.branch, &t.upstream, t.ahead, t.behind) {
            (Some(_), None, _, _) if !repo.remotes.is_empty() => "Publish".into(),
            (_, _, a, b) if a > 0 && b > 0 => format!("Pull {b} · Push {a}"),
            (_, _, 0, b) if b > 0 => format!("Pull {b}"),
            (_, _, a, 0) if a > 0 => format!("Push {a}"),
            _ => "Fetch".into(),
        }
    }

    /// One button for the remote: publish a new branch, pull what's
    /// waiting, push what's ready, or just fetch.
    pub(crate) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else { return };
        let Some(remote) = repo.default_remote() else {
            return self.info("This repository has no remote", window, cx);
        };
        let branch = repo.tracking.branch.clone();
        let has_upstream = repo.tracking.upstream.is_some();
        let root = repo.root.clone();
        self.run_job("Sync", window, cx, move || {
            let git = |args: &[&str]| support::git_network(&root, args);
            git(&["fetch", "--all", "--prune"])?;
            let Some(branch) = branch else {
                return Ok("Fetched".into());
            };
            if !has_upstream {
                git(&["push", "--set-upstream", &remote, &branch])?;
                return Ok(format!("Published {branch} to {remote}"));
            }
            let counts = git(&["rev-list", "--left-right", "--count", "HEAD...@{u}"])?;
            let mut it = counts
                .split_whitespace()
                .map(|n| n.parse::<usize>().unwrap_or(0));
            let (ahead, behind) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
            if ahead > 0 && behind > 0 {
                return Err(format!(
                    "{branch} and its upstream have diverged ({ahead} local, {behind} remote). \
                     Merge or rebase first."
                ));
            }
            if behind > 0 {
                git(&["pull", "--ff-only"])?;
                return Ok(format!("Pulled {}", plural(behind, "commit")));
            }
            if ahead > 0 {
                git(&["push"])?;
                return Ok(format!("Pushed {}", plural(ahead, "commit")));
            }
            Ok("Everything is up to date".into())
        });
    }

    pub(crate) fn fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        self.run_job("Fetch", window, cx, move || {
            support::git_network(&root, &["fetch", "--all", "--prune"]).map(|_| "Fetched".into())
        });
    }

    pub(crate) fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        self.run_job("Pull", window, cx, move || {
            support::git_network(&root, &["pull", "--ff-only"]).map(|_| "Pulled".into())
        });
    }

    pub(crate) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        match git::push_args() {
            Ok(args) => self.run_job("Push", window, cx, move || {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                support::git_network(&root, &args).map(|_| "Pushed".into())
            }),
            Err(e) => self.error(format!("Push failed: {}", first_line(&e)), window, cx),
        }
    }

    /// Run network work in the background. There's no terminal to prompt in,
    /// so credentials come from a helper (the keychain, ssh-agent) and git
    /// fails rather than hanging.
    fn run_job(
        &mut self,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        self.job = Some(label);
        cx.notify();
        self._job_task = cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { work() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.job = None;
                match result {
                    Ok(msg) => {
                        // Once commits are pushed, taking one back would
                        // rewrite published history.
                        if matches!(label, "Sync" | "Push") {
                            this.undo.retain(|(_, u)| {
                                !matches!(u, Undo::Uncommit { .. } | Undo::Merge { .. })
                            });
                            window.remove_notification::<UndoNote>(cx);
                        }
                        this.ok(msg, window, cx);
                    }
                    Err(e) => this.error(format!("{label} failed: {e}"), window, cx),
                }
                this.reload(window, cx);
                this.fetch_prs(cx);
            });
        });
    }

    // ── Drag and drop ────────────────────────────────────────────────────────

    /// A branch label was dropped on another branch's row: offer to merge
    /// it there or open a pull request between the two.
    pub(crate) fn branch_dropped(
        &mut self,
        source: &str,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else { return };
        let local = |name: &str| repo.without_remote(name).unwrap_or(name).to_string();
        let (source_name, target_name) = (local(source), local(target));
        if source_name == target_name {
            return;
        }
        let current = repo.tracking.branch.as_deref() == Some(target_name.as_str());
        let can_pr = !repo.remotes.is_empty();
        let source = source.to_string();
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            let option = |id: &'static str,
                          path: &'static str,
                          title: String,
                          hint: String,
                          run: SporAction| {
                let spor = spor.clone();
                gpui_kit::component::h_flex()
                    .id(id)
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_3()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .cursor_pointer()
                    .hover(|this| this.bg(cx.theme().list_hover))
                    .child(gpui_kit::component::Icon::empty().path(path).small())
                    .child(
                        gpui_kit::component::v_flex()
                            .child(gpui_kit::div().text_sm().child(title))
                            .child(gpui_kit::div().text_xs().text_color(muted).child(hint)),
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let run = run.clone();
                        let _ = spor.update(cx, |this, cx| run(this, window, cx));
                    })
            };
            let (merge_src, merge_dst) = (source.clone(), target_name.clone());
            let (pr_src, pr_dst) = (source_name.clone(), target_name.clone());
            dialog
                .title(format!("{source_name} → {target_name}"))
                .w(gpui_kit::px(440.))
                .child(
                    gpui_kit::component::v_flex()
                        .gap_2()
                        .child(option(
                            "drop-merge",
                            icon::GIT_MERGE,
                            format!("Merge {source_name} into {target_name}"),
                            if current {
                                "Into the checked-out branch".into()
                            } else {
                                format!("Checks out {target_name} first")
                            },
                            std::rc::Rc::new(move |this, w, cx| {
                                this.merge_branch(&merge_src, &merge_dst, w, cx)
                            }),
                        ))
                        .when(can_pr, |this| {
                            this.child(option(
                                "drop-pr",
                                icon::GIT_PULL_REQUEST,
                                "Open Pull Request".into(),
                                format!("Propose merging {pr_src} into {pr_dst}"),
                                std::rc::Rc::new(move |this, w, cx| {
                                    this.open_pull_request_between(&pr_src, &pr_dst, w, cx)
                                }),
                            ))
                        }),
                )
        });
    }

    /// Merge `source` into `target`, checking `target` out first if needed.
    pub(crate) fn merge_branch(
        &mut self,
        source: &str,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else { return };
        if repo.tracking.branch.as_deref() != Some(target) {
            if !repo.changes.is_empty() {
                return self.info(
                    format!("Commit or stash your changes before merging into {target}"),
                    window,
                    cx,
                );
            }
            if let Err(e) = git::checkout_branch(target) {
                self.reload(window, cx);
                return self.error(format!("Couldn't switch: {}", first_line(&e)), window, cx);
            }
        }
        let before = git::head_sha().unwrap_or_default();
        let shown = repo.without_remote(source).unwrap_or(source).to_string();
        match git::merge(source) {
            Ok(()) => {
                let undo = Undo::Merge {
                    branch: target.to_string(),
                    before,
                };
                self.did(format!("Merged {shown} into {target}"), undo, window, cx);
                self.tab = Tab::History;
            }
            Err(_) if git::merge_in_progress() => {
                self.tab = Tab::Changes;
                let spor = cx.entity().downgrade();
                window.push_notification(
                    Notification::warning(format!(
                        "Merging {shown} into {target} hit conflicts — resolve them and commit, or abort"
                    ))
                    .autohide(false)
                    .action(move |_, _, _| {
                        let spor = spor.clone();
                        Button::new("abort-merge").label("Abort Merge").small().ghost().on_click(
                            move |_, window, cx| {
                                let _ = spor.update(cx, |this, cx| {
                                    match git::merge_abort() {
                                        Ok(()) => this.info("Merge aborted", window, cx),
                                        Err(e) => this.error(
                                            format!("Couldn't abort: {}", first_line(&e)),
                                            window,
                                            cx,
                                        ),
                                    }
                                    this.reload(window, cx);
                                });
                            },
                        )
                    }),
                    cx,
                );
            }
            Err(e) => self.error(format!("Couldn't merge: {}", first_line(&e)), window, cx),
        }
        self.reload(window, cx);
    }

    /// Open the host's page for a pull request from `head` into `base`.
    pub(crate) fn open_pull_request_between(
        &mut self,
        head: &str,
        base: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else { return };
        if !repo.is_published(head) {
            return self.info(
                format!("Publish {head} first — check it out and Sync"),
                window,
                cx,
            );
        }
        let Some(info) = remote::detect() else {
            return self.error("Couldn't detect the remote host", window, cx);
        };
        cx.open_url(&remote::compare_url(&info, base, head));
    }

    pub(crate) fn open_pull_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else { return };
        let Some(head) = repo.tracking.branch.clone() else {
            return self.info("Switch to a branch first", window, cx);
        };
        let Some(info) = remote::detect() else {
            return self.error("Couldn't detect the remote host", window, cx);
        };
        let base = git::default_base_branch().unwrap_or_else(|| "main".into());
        if base == head {
            return self.info(
                format!("You're on {base} — switch to a feature branch first"),
                window,
                cx,
            );
        }
        if repo.tracking.upstream.is_none() {
            return self.info("Publish this branch first", window, cx);
        }
        let url = match self.prs.get(&head) {
            Some(pr) => format!("{}/pull/{}", info.web_url, pr.number),
            None => remote::compare_url(&info, &base, &head),
        };
        cx.open_url(&url);
    }

    pub(crate) fn copy(&self, text: String, what: &str, window: &mut Window, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.info(format!("{what} copied"), window, cx);
    }

    pub(crate) fn reveal_in_finder(&self, path: &str) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        let full = root.join(path);
        let _ = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&full)
                .spawn()
        } else {
            let dir = full.parent().map(Path::to_path_buf).unwrap_or(root);
            std::process::Command::new("xdg-open").arg(dir).spawn()
        };
    }

    pub(crate) fn set_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        self.diff_mode = mode;
        for diff in [&self.changes_diff, &self.commit_diff] {
            diff.update(cx, |state, cx| state.set_mode(mode, cx));
        }
        cx.notify();
    }

    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance = appearance;
        theme::set(appearance, window, cx);
        cx.notify();
    }

    /// Open the theme editor in a sheet. Its Dark / Light switch previews
    /// that variant; closing it goes back to the chosen appearance.
    pub(crate) fn open_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| super::theme_editor::ThemeEditor::new(window, cx));
        let spor = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, _, _| {
            let spor = spor.clone();
            sheet
                .title("Theme")
                .size(gpui_kit::px(380.))
                .overlay(false)
                .child(editor.clone())
                .on_close(move |_, window, cx| {
                    let _ = spor.update(cx, |this, cx| theme::apply(this.appearance, window, cx));
                })
        });
    }

    pub(crate) fn toggle_repositories(&mut self, cx: &mut Context<Self>) {
        self.layout.show_repos = !self.layout.show_repos;
        self.layout.save();
        cx.notify();
    }

    /// Move the dock to the other side of the center pane, showing it if
    /// it was hidden.
    pub(crate) fn toggle_dock_side(&mut self, cx: &mut Context<Self>) {
        self.layout.dock_side = match self.layout.dock_side {
            DockSide::Left => DockSide::Right,
            DockSide::Right => DockSide::Left,
        };
        self.layout.save();
        self.dock.get_or_insert(DockPanel::Git);
        cx.notify();
    }

    pub(crate) fn weak(cx: &Context<Self>) -> WeakEntity<Self> {
        cx.entity().downgrade()
    }
}
