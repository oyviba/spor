//! Agent tasks in the app: the Agents dock panel, the New Agent Task dialog,
//! and a task's page — what it changed against its base, with ways to land
//! it, open a pull request, or throw it away. Spor doesn't run agents yet;
//! each task has a folder of its own to run one in.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::support::{first_line, plural, relative_time};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::{Diff, DiffFile, DiffState},
    h_flex,
    input::{Textarea, TextareaState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
    WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, AppContext as _, Context, Entity,
    Focusable as _, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task as GpuiTask, Window,
};
use spor::tasks::{LandMode, Task, TaskState, TaskStats, Tasks};
use std::rc::Rc;

/// What the Agents panel and the task page show.
pub struct Agents {
    pub list: Vec<(Task, TaskStats)>,
    /// The task whose page is open.
    pub open: Option<String>,
    pub diff: Entity<DiffState>,
    /// What the diff was loaded for, so it only reloads when work moved.
    diff_for: Option<(String, TaskStats)>,
    pub prompt: Entity<TextareaState>,
    _load: GpuiTask<()>,
    _diff: GpuiTask<()>,
}

impl Agents {
    pub fn new(window: &mut Window, cx: &mut Context<Spor>) -> Self {
        Self {
            list: Vec::new(),
            open: None,
            diff: cx.new(|cx| DiffState::new([], cx)),
            diff_for: None,
            prompt: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("What should the agent do? e.g. Fix the login redirect on Safari")
                    .auto_grow(4, 12)
            }),
            _load: GpuiTask::ready(()),
            _diff: GpuiTask::ready(()),
        }
    }

    pub fn open_task(&self) -> Option<&(Task, TaskStats)> {
        let id = self.open.as_ref()?;
        self.list.iter().find(|(t, _)| &t.id == id)
    }

    fn get(&self, id: &str) -> Option<(Task, TaskStats)> {
        self.list.iter().find(|(t, _)| t.id == id).cloned()
    }
}

/// A one-word state for a task, and its color.
fn status(task: &Task, stats: &TaskStats, cx: &App) -> (&'static str, Hsla) {
    let theme = cx.theme();
    match &task.state {
        TaskState::Landed(_) => ("Landed", theme.muted_foreground),
        TaskState::Open if stats.missing => ("Folder missing", theme.red),
        TaskState::Open if stats.is_empty() => ("Waiting for work", theme.muted_foreground),
        TaskState::Open if stats.uncommitted > 0 => ("In progress", theme.yellow),
        TaskState::Open => ("Ready to land", theme.green),
    }
}

/// "2 commits · 1 uncommitted · +40 −3".
fn summary(stats: &TaskStats) -> String {
    let mut parts = Vec::new();
    if stats.commits > 0 {
        parts.push(plural(stats.commits, "commit"));
    }
    if stats.uncommitted > 0 {
        parts.push(format!("{} uncommitted", stats.uncommitted));
    }
    if stats.files > 0 {
        parts.push(format!("+{} −{}", stats.additions, stats.deletions));
    }
    parts.join(" · ")
}

impl Spor {
    fn tasks(&self) -> Result<Tasks, String> {
        Tasks::open(&self.git())
    }

    /// Re-read the tasks and what each has done, in the background.
    pub(crate) fn refresh_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            self.agents.list.clear();
            return;
        };
        let git = self.git();
        self.agents._load = cx.spawn(async move |this, cx| {
            let list = cx
                .background_spawn(async move {
                    let Ok(tasks) = Tasks::open(&git) else {
                        return Vec::new();
                    };
                    tasks
                        .list()
                        .into_iter()
                        .map(|task| {
                            let stats = tasks.stats(&task);
                            (task, stats)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.repo.as_ref().map(|r| &r.root) != Some(&root) {
                    return;
                }
                if this.agents.list != list {
                    this.agents.list = list;
                    this.load_task_diff(cx);
                    cx.notify();
                }
            });
        });
    }

    /// Load the open task's changes, unless they're already showing.
    fn load_task_diff(&mut self, cx: &mut Context<Self>) {
        let Some((task, stats)) = self.agents.open_task().cloned() else {
            return;
        };
        let key = (task.id.clone(), stats);
        if self.agents.diff_for.as_ref() == Some(&key) {
            return;
        }
        self.agents.diff_for = Some(key);
        let Ok(tasks) = self.tasks() else { return };
        self.agents._diff = cx.spawn(async move |this, cx| {
            let files = cx
                .background_spawn(async move {
                    tasks
                        .patch(&task)
                        .ok()
                        .and_then(|p| DiffFile::parse(&p).ok())
                        .unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.agents
                    .diff
                    .update(cx, |state, cx| state.set_files(files, cx));
                cx.notify();
            });
        });
    }

    pub(crate) fn show_agents(&mut self, cx: &mut Context<Self>) {
        self.dock = Some(DockPanel::Agents);
        self.refresh_tasks(cx);
        cx.notify();
    }

    pub(crate) fn open_task_page(&mut self, id: &str, cx: &mut Context<Self>) {
        self.agents.open = Some(id.to_string());
        self.agents.diff_for = None;
        self.agents
            .diff
            .update(cx, |state, cx| state.set_files([], cx));
        self.tab = Tab::Task;
        self.load_task_diff(cx);
        cx.notify();
    }

    pub(crate) fn close_task_page(&mut self, cx: &mut Context<Self>) {
        self.agents.open = None;
        if self.tab == Tab::Task {
            self.tab = Tab::History;
        }
        cx.notify();
    }

    // ── Creating ─────────────────────────────────────────────────────────

    /// The branch a new task starts from: the checked-out one, else the
    /// default branch.
    fn task_base(&self) -> Option<String> {
        let repo = self.repo.as_ref()?;
        repo.tracking
            .branch
            .clone()
            .or_else(|| repo.default_branch.clone())
    }

    pub(crate) fn new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = self.task_base() else {
            return self.info("Make a first commit before starting a task", window, cx);
        };
        let prompt = self.agents.prompt.clone();
        prompt.update(cx, |state, cx| state.set_value("", window, cx));
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let base = base.clone();
            let prompt_ok = prompt.clone();
            prompt.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title("New Agent Task")
                .w(px(560.))
                .child(
                    v_flex().gap_2().child(Textarea::new(&prompt)).child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "It gets its own branch from {base} and its own folder, so \
                                     your checkout isn't touched. Run an agent there, then \
                                     review and land the result here."
                            )),
                    ),
                )
                .footer({
                    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            DialogClose::new()
                                .child(Button::new("task-cancel").label("Cancel").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("task-create").label("Create Task").primary()),
                        )
                })
                .on_ok(move |_, window, cx| {
                    let text = prompt_ok.read(cx).value().trim().to_string();
                    if text.is_empty() {
                        return false;
                    }
                    let base = base.clone();
                    let _ = spor.update(cx, |this, cx| this.create_task(text, base, window, cx));
                    true
                })
        });
    }

    fn create_task(
        &mut self,
        prompt: String,
        base: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tasks = match self.tasks() {
            Ok(t) => t,
            Err(e) => return self.error(first_line(&e).to_string(), window, cx),
        };
        cx.spawn_in(window, async move |this, cx| {
            let created = cx
                .background_spawn(async move { tasks.create(&prompt, &base) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match created {
                Ok(task) => {
                    this.dock = Some(DockPanel::Agents);
                    this.open_task_page(&task.id, cx);
                    this.refresh_tasks(cx);
                    this.reload(window, cx);
                    let command = task.agent_command();
                    let spor = cx.entity().downgrade();
                    window.push_notification(
                        Notification::success(format!(
                            "Created {} — run your agent in its folder",
                            task.id
                        ))
                        .action(move |_, _, _| {
                            let (spor, command) = (spor.clone(), command.clone());
                            Button::new("copy-agent-command")
                                .label("Copy Command")
                                .small()
                                .ghost()
                                .on_click(move |_, window, cx| {
                                    let command = command.clone();
                                    let _ = spor.update(cx, |this, cx| {
                                        this.copy(command, "Command", window, cx)
                                    });
                                })
                        }),
                        cx,
                    );
                }
                Err(e) => this.error(
                    format!("Couldn't create the task: {}", first_line(&e)),
                    window,
                    cx,
                ),
            });
        })
        .detach();
    }

    // ── Landing and discarding ───────────────────────────────────────────

    pub(crate) fn land_task(
        &mut self,
        id: &str,
        mode: LandMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((task, stats)) = self.agents.get(id) else {
            return;
        };
        let here = self.repo.as_ref().and_then(|r| r.tracking.branch.clone());
        if here.as_deref() != Some(task.base.as_str()) {
            return self.info(
                format!("Check out {} to land {} on it", task.base, task.id),
                window,
                cx,
            );
        }
        if stats.uncommitted == 0 {
            return self.do_land(task, mode, false, window, cx);
        }
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let spor = spor.clone();
            let task = task.clone();
            dialog
                .title(format!("Land {} with its uncommitted changes?", task.id))
                .w(px(460.))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{} in its folder {} committed. They're committed first and land with \
                     the rest. If the agent is still working, wait for it to finish.",
                            plural(stats.uncommitted, "file"),
                            if stats.uncommitted == 1 {
                                "isn't"
                            } else {
                                "aren't"
                            }
                        )),
                )
                .footer({
                    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            DialogClose::new()
                                .child(Button::new("land-cancel").label("Cancel").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("land-all").label("Commit and Land").primary()),
                        )
                })
                .on_ok(move |_, window, cx| {
                    let task = task.clone();
                    let _ = spor.update(cx, |this, cx| this.do_land(task, mode, true, window, cx));
                    true
                })
        });
    }

    fn do_land(
        &mut self,
        task: Task,
        mode: LandMode,
        commit_uncommitted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .tasks()
            .and_then(|t| t.land(&task, mode, commit_uncommitted));
        match result {
            Ok(undo) => {
                if self.agents.open.as_deref() == Some(task.id.as_str()) {
                    self.close_task_page(cx);
                }
                self.did(
                    format!("Landed {} on {}", task.id, task.base),
                    Undo::Task(Box::new(undo)),
                    window,
                    cx,
                );
                self.tab = Tab::History;
            }
            Err(e) => self.error(first_line(&e).to_string(), window, cx),
        }
        self.reload(window, cx);
        self.refresh_tasks(cx);
    }

    pub(crate) fn discard_task(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((task, stats)) = self.agents.get(id) else {
            return;
        };
        if stats.uncommitted == 0 {
            return self.do_discard(task, window, cx);
        }
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let spor = spor.clone();
            let task = task.clone();
            dialog
                .title(format!("Discard {}?", task.id))
                .w(px(440.))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "Its folder has {} that aren't committed. They're deleted with it and \
                     can't be brought back; Undo restores the commits only.",
                            plural(stats.uncommitted, "changed file")
                        )),
                )
                .footer({
                    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            DialogClose::new()
                                .child(Button::new("discard-cancel").label("Cancel").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("discard-ok").label("Discard").primary()),
                        )
                })
                .on_ok(move |_, window, cx| {
                    let task = task.clone();
                    let _ = spor.update(cx, |this, cx| this.do_discard(task, window, cx));
                    true
                })
        });
    }

    fn do_discard(&mut self, task: Task, window: &mut Window, cx: &mut Context<Self>) {
        match self.tasks().and_then(|t| t.discard(&task)) {
            Ok(undo) => {
                if self.agents.open.as_deref() == Some(task.id.as_str()) {
                    self.close_task_page(cx);
                }
                self.did(
                    format!("Discarded {}", task.id),
                    Undo::Task(Box::new(undo)),
                    window,
                    cx,
                );
            }
            Err(e) => self.error(format!("Couldn't discard: {}", first_line(&e)), window, cx),
        }
        self.reload(window, cx);
        self.refresh_tasks(cx);
    }

    /// Remove a landed task from the list.
    pub(crate) fn forget_task(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.tasks().and_then(|t| t.forget(id)) {
            self.error(first_line(&e).to_string(), window, cx);
        }
        self.refresh_tasks(cx);
    }

    /// Publish the task's branch and open a pull request for it on the host.
    pub(crate) fn task_pull_request(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((task, stats)) = self.agents.get(id) else {
            return;
        };
        if let Some(running) = self.job {
            return self.info(
                format!("{running} is still running — try again when it's done"),
                window,
                cx,
            );
        }
        let Some(repo) = &self.repo else { return };
        let Some(remote) = repo.default_remote() else {
            return self.info("This repository has no remote", window, cx);
        };
        if stats.uncommitted > 0 {
            return self.info(
                format!(
                    "{} has uncommitted changes — commit them in its folder first",
                    task.id
                ),
                window,
                cx,
            );
        }
        let Some(info) = spor::remote::detect_in(&repo.root) else {
            return self.error("Couldn't detect the remote host", window, cx);
        };
        let url = spor::remote::compare_url(&info, &task.base, &task.branch);
        let worktree = task.worktree.clone();
        let branch = task.branch.clone();
        self.after_job = Some(Rc::new(move |_, _, cx| cx.open_url(&url)));
        self.run_job("Publish", window, cx, move || {
            super::support::git_network(&worktree, &["push", "--set-upstream", &remote, &branch])
                .map(|_| format!("Published {branch} — opening a pull request"))
        });
    }

    /// Open the task's folder in a terminal (macOS) or file manager.
    pub(crate) fn open_task_folder(&self, id: &str) {
        let Some((task, _)) = self.agents.get(id) else {
            return;
        };
        let _ = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .args(["-a", "Terminal"])
                .arg(&task.worktree)
                .spawn()
        } else {
            std::process::Command::new("xdg-open")
                .arg(&task.worktree)
                .spawn()
        };
    }

    // ── Rendering ────────────────────────────────────────────────────────

    pub(crate) fn render_agents_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let spor = Self::weak(cx);
        let (open, landed): (Vec<_>, Vec<_>) = self
            .agents
            .list
            .iter()
            .cloned()
            .partition(|(t, _)| t.state == TaskState::Open);
        let selected = (self.tab == Tab::Task)
            .then(|| self.agents.open.clone())
            .flatten();
        let mut list = v_flex().py_1();
        if open.is_empty() && landed.is_empty() {
            list = list.child(
                v_flex()
                    .px_4()
                    .py_6()
                    .gap_2()
                    .items_center()
                    .text_center()
                    .child(
                        Icon::empty()
                            .path(icon::BOT)
                            .large()
                            .text_color(theme.muted_foreground.opacity(0.5)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("No agent tasks"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .child(
                                "Give an agent its own branch and folder, so several can work \
                                 at once without touching your checkout.",
                            ),
                    )
                    .child(
                        Button::new("first-task")
                            .primary()
                            .small()
                            .label("New Agent Task…")
                            .on_click(cx.listener(|this, _, w, cx| this.new_task(w, cx))),
                    ),
            );
        }
        for (title, items) in [("Open", open), ("Landed", landed)] {
            if items.is_empty() {
                continue;
            }
            list = list.child(
                h_flex()
                    .h(px(26.))
                    .px_3()
                    .text_xs()
                    .font_medium()
                    .text_color(theme.muted_foreground)
                    .child(title)
                    .child(div().flex_1())
                    .child(div().opacity(0.7).child(items.len().to_string())),
            );
            for (task, stats) in items {
                let is_selected = selected.as_deref() == Some(task.id.as_str());
                list = list.child(render_task_row(task, stats, is_selected, spor.clone(), cx));
            }
        }
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .h(px(36.))
                    .flex_none()
                    .px_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_medium().child("Agents"))
                    .child(div().flex_1())
                    .child(
                        Button::new("new-task")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New Agent Task… (⌘⇧T)")
                            .on_click(cx.listener(|this, _, w, cx| this.new_task(w, cx))),
                    ),
            )
            .child(
                div()
                    .id("task-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }

    pub(crate) fn render_task_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some((task, stats)) = self.agents.open_task().cloned() else {
            return empty_state(
                icon::BOT,
                "Task not found",
                "It may have been landed or discarded",
                cx,
            );
        };
        let id = task.id.clone();
        let (label, color) = status(&task, &stats, cx);
        let here = self.repo.as_ref().and_then(|r| r.tracking.branch.clone());
        let on_base = here.as_deref() == Some(task.base.as_str());
        let can_land = stats.commits + stats.uncommitted > 0 && !stats.missing;
        let has_remote = self.repo.as_ref().is_some_and(|r| !r.remotes.is_empty());
        let spor = Self::weak(cx);
        let command = task.agent_command();
        let (land_id, discard_id, pr_id, folder_id, copy_id) =
            (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
        let copy_command = command.clone();
        let header = v_flex()
            .flex_none()
            .px_4()
            .py_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(Icon::empty().path(icon::BOT).small().text_color(color))
                    .child(
                        div()
                            .text_lg()
                            .font_medium()
                            .min_w_0()
                            .truncate()
                            .child(task.title()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_1p5()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(color.opacity(0.5))
                            .text_xs()
                            .text_color(color)
                            .child(label),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .font_family(theme.mono_font_family.clone())
                            .child(task.branch.clone()),
                    )
                    .child(format!("from {}", task.base))
                    .child("·")
                    .child(format!("started {}", relative_time(task.created)))
                    .when(!summary(&stats).is_empty(), |this| {
                        this.child("·").child(summary(&stats))
                    })
                    .when(stats.base_moved, |this| {
                        this.child("·")
                            .child(div().text_color(theme.yellow).child(format!(
                                "{} has moved on — landing rebases first",
                                task.base
                            )))
                    }),
            )
            .when(task.prompt.trim().lines().count() > 1, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(task.prompt.trim().to_string()),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("task-land")
                            .primary()
                            .small()
                            .label("Land on ".to_string() + &task.base)
                            .disabled(!can_land)
                            .tooltip(if !on_base {
                                "Check out the base branch first"
                            } else {
                                "One commit with the task as its message (⌘Z undoes)"
                            })
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.land_task(&land_id, LandMode::Squash, w, cx)
                            })),
                    )
                    .child({
                        let spor = spor.clone();
                        let id = id.clone();
                        Button::new("task-land-more")
                            .ghost()
                            .small()
                            .icon(IconName::ChevronDown)
                            .disabled(!can_land)
                            .dropdown_menu(move |menu, _, _| {
                                let (spor, id) = (spor.clone(), id.clone());
                                menu.item(PopupMenuItem::new("Land with a Merge Commit").on_click(
                                    move |_, window, cx| {
                                        let _ = spor.update(cx, |this, cx| {
                                            this.land_task(&id, LandMode::Merge, window, cx)
                                        });
                                    },
                                ))
                            })
                    })
                    .when(has_remote, |this| {
                        this.child(
                            Button::new("task-pr")
                                .ghost()
                                .small()
                                .icon(Icon::empty().path(icon::GIT_PULL_REQUEST))
                                .label("Open Pull Request")
                                .disabled(stats.commits == 0)
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.task_pull_request(&pr_id, w, cx)
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("task-copy-command")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(icon::COPY))
                            .label("Copy Agent Command")
                            .tooltip(command.clone())
                            .on_click(cx.listener(move |this, _, w, cx| {
                                let _ = &copy_id;
                                this.copy(copy_command.clone(), "Command", w, cx)
                            })),
                    )
                    .child(
                        Button::new("task-folder")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(icon::FOLDER_GIT))
                            .tooltip(if cfg!(target_os = "macos") {
                                "Open its folder in Terminal"
                            } else {
                                "Open its folder"
                            })
                            .on_click(
                                cx.listener(move |this, _, _, _| this.open_task_folder(&folder_id)),
                            ),
                    )
                    .child(
                        Button::new("task-discard")
                            .ghost()
                            .small()
                            .label("Discard")
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.discard_task(&discard_id, w, cx)
                            })),
                    ),
            );
        let body = if stats.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .p_6()
                .child(
                    div().text_sm().text_color(theme.muted_foreground).child(
                        "No changes yet. Run an agent in the task's folder — for Claude Code:",
                    ),
                )
                .child(
                    div()
                        .max_w(px(640.))
                        .px_3()
                        .py_2()
                        .rounded(theme.radius)
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .child(command),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground.opacity(0.8))
                        .child(
                            "Its commits show up in History as they're made, and its changes here.",
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .child(Diff::new(&self.agents.diff).line_number(true))
                .into_any_element()
        };
        v_flex()
            .size_full()
            .child(header)
            .child(body)
            .into_any_element()
    }
}

fn render_task_row(
    task: Task,
    stats: TaskStats,
    selected: bool,
    spor: gpui_kit::WeakEntity<Spor>,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let (label, color) = status(&task, &stats, cx);
    let landed = task.state != TaskState::Open;
    let id = task.id.clone();
    let click = {
        let spor = spor.clone();
        let id = id.clone();
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = spor.update(cx, |this, cx| this.open_task_page(&id, cx));
        }
    };
    let detail = match (&task.state, summary(&stats)) {
        (TaskState::Landed(sha), _) => format!("landed as {}", &sha[..sha.len().min(7)]),
        (_, s) if s.is_empty() => task.branch.clone(),
        (_, s) => s,
    };
    let menu_task = task.clone();
    h_flex()
        .id(SharedString::from(format!("task-{}", task.id)))
        .mx_1()
        .px_2()
        .py_1p5()
        .gap_2()
        .items_start()
        .rounded(theme.radius)
        .when(selected, |this| this.bg(theme.list_active))
        .when(!selected, |this| {
            this.hover(|this| this.bg(theme.list_hover))
        })
        .when(landed, |this| this.opacity(0.6))
        .when(!landed, |this| this.on_click(click))
        .child(
            div()
                .mt(px(5.))
                .flex_none()
                .size(px(8.))
                .rounded_full()
                .bg(color),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().text_sm().truncate().child(task.title()))
                .child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(div().text_color(color).child(label))
                        .child("·")
                        .child(div().min_w_0().truncate().child(detail)),
                ),
        )
        .when(
            !landed && stats.last_commit > 0 && stats.commits > 0,
            |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(relative_time(stats.last_commit)),
                )
            },
        )
        .context_menu(move |menu, _, _| {
            let act = |label: &'static str, f: SporAction| {
                let spor = spor.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let f = f.clone();
                    let _ = spor.update(cx, |this, cx| f(this, window, cx));
                })
            };
            let t = menu_task.clone();
            if t.state != TaskState::Open {
                let id = t.id.clone();
                return menu.item(act(
                    "Remove from List",
                    Rc::new(move |this, w, cx| this.forget_task(&id, w, cx)),
                ));
            }
            let (open, squash, merge, discard, folder, path, command) = (
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.worktree.to_string_lossy().into_owned(),
                t.agent_command(),
            );
            let worktree = t.worktree.clone();
            menu.item(act(
                "Open Task",
                Rc::new(move |this, _, cx| this.open_task_page(&open, cx)),
            ))
            .item(act(
                "Copy Agent Command",
                Rc::new(move |this, w, cx| this.copy(command.clone(), "Command", w, cx)),
            ))
            .item(act(
                "Copy Folder Path",
                Rc::new(move |this, w, cx| this.copy(path.clone(), "Path", w, cx)),
            ))
            .item(act(
                if cfg!(target_os = "macos") {
                    "Open Folder in Terminal"
                } else {
                    "Open Folder"
                },
                Rc::new(move |this, _, _| this.open_task_folder(&folder)),
            ))
            .item(act(
                "Open Folder as Repository",
                Rc::new(move |this, w, cx| this.open_repo(&worktree, w, cx)),
            ))
            .separator()
            .item(act(
                "Land (Squash)",
                Rc::new(move |this, w, cx| this.land_task(&squash, LandMode::Squash, w, cx)),
            ))
            .item(act(
                "Land with a Merge Commit",
                Rc::new(move |this, w, cx| this.land_task(&merge, LandMode::Merge, w, cx)),
            ))
            .separator()
            .item(act(
                "Discard Task…",
                Rc::new(move |this, w, cx| this.discard_task(&discard, w, cx)),
            ))
        })
        .into_any_element()
}
