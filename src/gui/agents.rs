//! Agent tasks in the app: the Agents dock panel, the New Agent Task dialog,
//! and a task's page — the agent's transcript, what it changed against its
//! base, feedback for it, and ways to land the work, open a pull request,
//! or throw it away.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::support::{self, first_line, plural, relative_time};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    diff::{Diff, DiffEvent, DiffFile, DiffLineRange, DiffSide, DiffState},
    h_flex,
    input::{Input, InputState, Textarea, TextareaState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    StyledExt as _, WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, AppContext as _, Context, Entity,
    Focusable as _, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task as GpuiTask, Window,
};
use spor::agent::{self, AgentKind, Entry, Permissions, Run, RunState, Runs};
use spor::tasks::{LandMode, Task, TaskState, TaskStats, Tasks};
use std::collections::HashMap;
use std::rc::Rc;

/// Which agent new tasks start, as picked last time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgentChoice {
    Claude,
    Command,
    /// Create the task; you run an agent in its folder yourself.
    Manual,
}

impl AgentChoice {
    fn label(self) -> &'static str {
        match self {
            AgentChoice::Claude => "Claude Code",
            AgentChoice::Command => "Custom command",
            AgentChoice::Manual => "None — I'll run one myself",
        }
    }

    fn setting(self) -> &'static str {
        match self {
            AgentChoice::Claude => "claude",
            AgentChoice::Command => "command",
            AgentChoice::Manual => "manual",
        }
    }
}

/// A task as the panel and its page show it.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskView {
    pub task: Task,
    pub stats: TaskStats,
    /// Its agent's latest run, if one was started from spor.
    pub run: Option<Run>,
    /// What its committed work would conflict with: its base, or another
    /// task's branch, and the files.
    pub conflicts: Vec<(String, Vec<String>)>,
}

impl TaskView {
    fn running(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.state == RunState::Running)
    }
}

/// Which half of a task's page is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskPane {
    Transcript,
    Changes,
}

/// A review comment on lines of the task's diff, sent with the next
/// feedback.
#[derive(Clone, Debug, PartialEq)]
pub struct LineComment {
    pub place: String,
    pub text: String,
}

/// Merge-tree results, keyed by the two commits merged.
type Radar = HashMap<(String, String), Vec<String>>;

/// What the Agents panel and the task page show.
pub struct Agents {
    pub list: Vec<TaskView>,
    /// The task whose page is open.
    pub open: Option<String>,
    pub pane: TaskPane,
    pub transcript: Vec<Entry>,
    pub diff: Entity<DiffState>,
    /// What the diff was loaded for, so it only reloads when work moved.
    diff_for: Option<(String, TaskStats)>,
    /// Lines last selected in the task's diff, to comment on.
    selection: Option<DiffLineRange>,
    pub comments: Vec<LineComment>,
    pub prompt: Entity<TextareaState>,
    pub feedback: Entity<TextareaState>,
    note: Entity<InputState>,
    pub choice: AgentChoice,
    pub allow_commands: bool,
    pub command: String,
    radar: Radar,
    /// Runs that ended since the last look, to announce.
    ended: Vec<(String, RunState)>,
    _load: GpuiTask<()>,
    _diff: GpuiTask<()>,
    _subscriptions: Vec<Subscription>,
}

impl Agents {
    pub fn new(window: &mut Window, cx: &mut Context<Spor>) -> Self {
        let diff = cx.new(|cx| DiffState::new([], cx));
        let subscriptions = vec![cx.subscribe(&diff, |this, _, event: &DiffEvent, cx| {
            if let DiffEvent::SelectionChanged(Some(range)) | DiffEvent::SelectionEnded(range) =
                event
            {
                this.agents.selection = Some(range.clone());
                cx.notify();
            }
        })];
        let choice = match support::read_setting("agent").as_deref() {
            Some("command") => AgentChoice::Command,
            Some("manual") => AgentChoice::Manual,
            _ => AgentChoice::Claude,
        };
        Self {
            list: Vec::new(),
            open: None,
            pane: TaskPane::Transcript,
            transcript: Vec::new(),
            diff,
            diff_for: None,
            selection: None,
            comments: Vec::new(),
            prompt: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("What should the agent do? e.g. Fix the login redirect on Safari")
                    .auto_grow(4, 12)
            }),
            feedback: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Feedback for the agent — what to change, what to try next")
                    .auto_grow(2, 8)
            }),
            note: cx.new(|cx| InputState::new(window, cx)),
            choice,
            allow_commands: support::read_setting("agent-commands").as_deref() == Some("1"),
            command: support::read_setting("agent-command").unwrap_or_default(),
            radar: Radar::new(),
            ended: Vec::new(),
            _load: GpuiTask::ready(()),
            _diff: GpuiTask::ready(()),
            _subscriptions: subscriptions,
        }
    }

    pub fn open_task(&self) -> Option<&TaskView> {
        let id = self.open.as_ref()?;
        self.list.iter().find(|v| &v.task.id == id)
    }

    fn get(&self, id: &str) -> Option<TaskView> {
        self.list.iter().find(|v| v.task.id == id).cloned()
    }

    /// Is any agent working? Then spor keeps watching even with the panel
    /// closed, to say when it's done.
    pub fn any_running(&self) -> bool {
        self.list.iter().any(TaskView::running)
    }

    fn kind(&self) -> Option<AgentKind> {
        match self.choice {
            AgentChoice::Claude => Some(AgentKind::Claude),
            AgentChoice::Command => Some(AgentKind::Command(self.command.clone())),
            AgentChoice::Manual => None,
        }
    }

    fn save_settings(&self) {
        support::write_setting("agent", self.choice.setting());
        support::write_setting(
            "agent-commands",
            if self.allow_commands { "1" } else { "0" },
        );
        support::write_setting("agent-command", &self.command);
    }
}

/// A short state for a task, and its color.
fn status(view: &TaskView, cx: &App) -> (&'static str, Hsla) {
    let theme = cx.theme();
    let (task, stats) = (&view.task, &view.stats);
    match (&task.state, view.run.as_ref().map(|r| &r.state)) {
        (TaskState::Landed(_), _) => ("Landed", theme.muted_foreground),
        _ if stats.missing => ("Folder missing", theme.red),
        (_, Some(RunState::Running)) => ("Agent working", theme.blue),
        (_, Some(RunState::Failed(_))) => ("Agent failed", theme.red),
        (_, Some(RunState::Stopped)) if stats.is_empty() => ("Stopped", theme.muted_foreground),
        _ if !view.conflicts.is_empty() => ("Conflicts", theme.red),
        _ if stats.is_empty() => ("Waiting for work", theme.muted_foreground),
        (_, Some(RunState::Finished)) => ("Ready for review", theme.green),
        _ if stats.uncommitted > 0 => ("In progress", theme.yellow),
        _ => ("Ready to land", theme.green),
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

/// "conflicts with fix-login (a.rs, b.rs)".
fn conflict_text(conflicts: &[(String, Vec<String>)]) -> String {
    conflicts
        .iter()
        .map(|(with, files)| {
            let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
            let more = files.len().saturating_sub(3);
            let more = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            format!("conflicts with {with} in {}{more}", shown.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Read every task, its agent's latest run, and the conflicts its work
/// would hit. Runs in the background.
fn load(git: spor::git::Repo, mut radar: Radar) -> (Vec<TaskView>, Radar) {
    let Ok(tasks) = Tasks::open(&git) else {
        return (Vec::new(), radar);
    };
    let mut list: Vec<TaskView> = tasks
        .list()
        .into_iter()
        .map(|task| TaskView {
            stats: tasks.stats(&task),
            run: Runs::of(&tasks, &task).latest(),
            conflicts: Vec::new(),
            task,
        })
        .collect();
    // Trial merges of committed work, against the base and each other.
    // Each pair of commits is merged once and remembered.
    let tips: Vec<Option<String>> = list
        .iter()
        .map(|v| {
            (v.task.state == TaskState::Open && v.stats.commits > 0)
                .then(|| git.resolve(&v.task.branch).ok())
                .flatten()
        })
        .collect();
    let mut fresh = Radar::new();
    let mut check = |a: &str, b: &str| -> Vec<String> {
        let key = (a.to_string(), b.to_string());
        let files = radar
            .remove(&key)
            .unwrap_or_else(|| git.merge_conflicts(a, b).unwrap_or_default());
        fresh.insert(key, files.clone());
        files
    };
    for i in 0..list.len() {
        let Some(tip) = tips[i].clone() else { continue };
        let base = list[i].task.base.clone();
        if let Ok(base_tip) = git.resolve(&base) {
            let files = check(&base_tip, &tip);
            if !files.is_empty() {
                list[i].conflicts.push((base, files));
            }
        }
        for j in 0..list.len() {
            let Some(other) = tips[j].clone().filter(|_| j != i) else {
                continue;
            };
            let (a, b) = if i < j {
                (&tip, &other)
            } else {
                (&other, &tip)
            };
            let files = check(a, b);
            if !files.is_empty() {
                let with = list[j].task.id.clone();
                list[i].conflicts.push((with, files));
            }
        }
    }
    (list, fresh)
}

impl Spor {
    fn tasks(&self) -> Result<Tasks, String> {
        Tasks::open(&self.git())
    }

    /// Re-read the tasks, their agents and their conflicts, in the
    /// background.
    pub(crate) fn refresh_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            self.agents.list.clear();
            return;
        };
        let git = self.git();
        let radar = std::mem::take(&mut self.agents.radar);
        let open = self.agents.open_task().cloned();
        let tasks = self.tasks().ok();
        self.agents._load = cx.spawn(async move |this, cx| {
            let ((list, radar), transcript) = cx
                .background_spawn(async move {
                    let loaded = load(git, radar);
                    let transcript = match (open, tasks) {
                        (Some(v), Some(tasks)) => Some(Runs::of(&tasks, &v.task).transcript()),
                        _ => None,
                    };
                    (loaded, transcript)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.agents.radar = radar;
                if this.repo.as_ref().map(|r| &r.root) != Some(&root) {
                    return;
                }
                if let Some(t) = transcript {
                    if this.agents.transcript != t {
                        this.agents.transcript = t;
                        cx.notify();
                    }
                }
                if this.agents.list != list {
                    for new in &list {
                        let was_running = this
                            .agents
                            .list
                            .iter()
                            .any(|old| old.task.id == new.task.id && old.running());
                        if let Some(run) = new.run.as_ref().filter(|_| was_running) {
                            if run.state != RunState::Running {
                                this.agents
                                    .ended
                                    .push((new.task.id.clone(), run.state.clone()));
                            }
                        }
                    }
                    this.agents.list = list;
                    this.load_task_diff(cx);
                    cx.notify();
                }
            });
        });
    }

    /// Say which agents finished, failed or stopped since the last look.
    pub(crate) fn announce_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (id, state) in std::mem::take(&mut self.agents.ended) {
            let text = match &state {
                RunState::Finished => format!("{id} is ready for review"),
                RunState::Failed(code) => format!("The agent on {id} failed (exit code {code})"),
                _ => format!("The agent on {id} stopped"),
            };
            let note = match state {
                RunState::Finished => Notification::success(text),
                RunState::Failed(_) => Notification::error(text),
                _ => Notification::info(text),
            };
            let spor = cx.entity().downgrade();
            window.push_notification(
                note.action(move |_, _, _| {
                    let (spor, id) = (spor.clone(), id.clone());
                    Button::new("open-finished-task")
                        .label("Open")
                        .small()
                        .ghost()
                        .on_click(move |_, _, cx| {
                            let _ = spor.update(cx, |this, cx| this.open_task_page(&id, cx));
                        })
                }),
                cx,
            );
        }
    }

    /// Load the open task's changes, unless they're already showing.
    fn load_task_diff(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.agents.open_task().cloned() else {
            return;
        };
        let key = (view.task.id.clone(), view.stats);
        if self.agents.diff_for.as_ref() == Some(&key) {
            return;
        }
        self.agents.diff_for = Some(key);
        let Ok(tasks) = self.tasks() else { return };
        let task = view.task;
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
                this.agents.selection = None;
                cx.notify();
            });
        });
    }

    pub(crate) fn show_agents(&mut self, cx: &mut Context<Self>) {
        self.set_dock(DockPanel::Agents);
        self.refresh_tasks(cx);
        cx.notify();
    }

    pub(crate) fn open_task_page(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.agents.open.as_deref() != Some(id) {
            self.agents.comments.clear();
            self.agents.transcript.clear();
        }
        self.agents.open = Some(id.to_string());
        self.agents.diff_for = None;
        self.agents
            .diff
            .update(cx, |state, cx| state.set_files([], cx));
        // A task an agent worked on opens on what it said; one worked on
        // by hand, on its changes.
        let has_run = self.agents.get(id).is_some_and(|v| v.run.is_some());
        self.agents.pane = if has_run {
            TaskPane::Transcript
        } else {
            TaskPane::Changes
        };
        self.set_tab(Tab::Task);
        self.load_task_diff(cx);
        self.refresh_tasks(cx);
        cx.notify();
    }

    pub(crate) fn close_task_page(&mut self, cx: &mut Context<Self>) {
        self.agents.open = None;
        if self.tab == Tab::Task {
            self.set_tab(Tab::History);
        }
        cx.notify();
    }

    // ── Creating and running ─────────────────────────────────────────────

    /// The branch a new task starts from: the checked-out one, else the
    /// default branch.
    fn task_base(&self) -> Option<String> {
        let repo = self.repo.as_ref()?;
        repo.tracking
            .branch
            .clone()
            .or_else(|| repo.default_branch.clone())
    }

    fn set_agent_choice(
        &mut self,
        choice: AgentChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agents.choice = choice;
        self.agents.save_settings();
        if choice == AgentChoice::Command && self.agents.command.trim().is_empty() {
            self.edit_agent_command(window, cx);
        }
        window.refresh();
        cx.notify();
    }

    pub(crate) fn new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = self.task_base() else {
            return self.info("Make a first commit before starting a task", window, cx);
        };
        let prompt = self.agents.prompt.clone();
        prompt.update(cx, |state, cx| state.set_value("", window, cx));
        let spor = cx.entity().downgrade();
        // Focus the prompt once; refocusing on every render would fight
        // the agent picker and checkbox for focus.
        let focused = Rc::new(std::cell::Cell::new(false));
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let base = base.clone();
            let prompt_ok = prompt.clone();
            if !focused.replace(true) {
                prompt.read(cx).focus_handle(cx).focus(window, cx);
            }
            let (choice, allow) = spor
                .upgrade()
                .map(|s| {
                    let a = &s.read(cx).agents;
                    (a.choice, a.allow_commands)
                })
                .unwrap_or((AgentChoice::Manual, false));
            let menu_spor = spor.clone();
            let check_spor = spor.clone();
            let muted = cx.theme().muted_foreground;
            dialog
                .title("New Agent Task")
                .w(px(560.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(Textarea::new(&prompt))
                        .child(
                            h_flex()
                                .gap_2()
                                .text_sm()
                                .child(div().text_color(muted).child("Agent"))
                                .child(
                                    Button::new("task-agent")
                                        .outline()
                                        .small()
                                        .label(choice.label())
                                        .dropdown_caret(true)
                                        .dropdown_menu(move |mut menu, _, _| {
                                            for c in [
                                                AgentChoice::Claude,
                                                AgentChoice::Command,
                                                AgentChoice::Manual,
                                            ] {
                                                let spor = menu_spor.clone();
                                                menu = menu.item(
                                                    PopupMenuItem::new(c.label())
                                                        .checked(c == choice)
                                                        .on_click(move |_, window, cx| {
                                                            let _ = spor.update(cx, |this, cx| {
                                                                this.set_agent_choice(c, window, cx)
                                                            });
                                                        }),
                                                );
                                            }
                                            let spor = menu_spor.clone();
                                            menu.separator().item(
                                                PopupMenuItem::new("Edit Custom Command…")
                                                    .on_click(move |_, window, cx| {
                                                        let _ = spor.update(cx, |this, cx| {
                                                            this.edit_agent_command(window, cx)
                                                        });
                                                    }),
                                            )
                                        }),
                                )
                                .when(choice == AgentChoice::Claude, |this| {
                                    this.child(
                                        Checkbox::new("task-allow-commands")
                                            .label("Let it run shell commands")
                                            .checked(allow)
                                            .small()
                                            .on_click(move |checked: &bool, window, cx| {
                                                let checked = *checked;
                                                let _ = check_spor.update(cx, |this, cx| {
                                                    this.agents.allow_commands = checked;
                                                    this.agents.save_settings();
                                                    window.refresh();
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                }),
                        )
                        .child(div().text_xs().text_color(muted).child(match choice {
                            AgentChoice::Claude if allow => format!(
                                "Claude Code starts in its own branch from {base} and its own \
                                 folder, so your checkout isn't touched. It may edit files and \
                                 run commands there — never git push."
                            ),
                            AgentChoice::Claude => format!(
                                "Claude Code starts in its own branch from {base} and its own \
                                 folder, so your checkout isn't touched. It may edit files; \
                                 commands it wants to run are refused."
                            ),
                            AgentChoice::Command => format!(
                                "Runs your command in its own branch from {base} and its own \
                                 folder, so your checkout isn't touched."
                            ),
                            AgentChoice::Manual => format!(
                                "It gets its own branch from {base} and its own folder. Run an \
                                 agent there, then review and land the result here."
                            ),
                        })),
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
                            DialogAction::new().child(
                                Button::new("task-create")
                                    .label(if choice == AgentChoice::Manual {
                                        "Create Task"
                                    } else {
                                        "Start Agent"
                                    })
                                    .primary(),
                            ),
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

    /// Ask for the command custom agents run, with `{prompt}` for the prompt.
    pub(crate) fn edit_agent_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.agents.note.clone();
        let current = self.agents.command.clone();
        note.update(cx, |state, cx| {
            state.set_value(current, window, cx);
            state.set_placeholder("codex exec {prompt}", window, cx);
        });
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let note_ok = note.clone();
            note.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title("Custom Agent Command")
                .w(px(500.))
                .child(
                    v_flex().gap_2().child(Input::new(&note)).child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "Runs in the task's folder with sh. {prompt} becomes the task, \
                                 quoted; $SPOR_PROMPT holds it too. Its output becomes the \
                                 transcript.",
                            ),
                    ),
                )
                .footer({
                    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            DialogClose::new()
                                .child(Button::new("cmd-cancel").label("Cancel").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("cmd-save").label("Save").primary()),
                        )
                })
                .on_ok(move |_, window, cx| {
                    let command = note_ok.read(cx).value().trim().to_string();
                    let _ = spor.update(cx, |this, cx| {
                        this.agents.command = command;
                        this.agents.save_settings();
                        window.refresh();
                        cx.notify();
                    });
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
        let start = self.agents.kind();
        if let Some(e) = start.as_ref().and_then(agent_missing) {
            return self.error(e, window, cx);
        }
        cx.spawn_in(window, async move |this, cx| {
            let created = cx
                .background_spawn(async move { tasks.create(&prompt, &base) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match created {
                Ok(task) => {
                    this.dock = Some(DockPanel::Agents);
                    this.open_task_page(&task.id, cx);
                    if start.is_some() {
                        this.start_agent(&task, task.prompt.clone(), false, window, cx);
                    } else {
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
                    this.reload(window, cx);
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

    /// Start the chosen agent on a task: with its prompt, or with feedback
    /// (`follow_up`), continuing the agent's session when it has one.
    fn start_agent(
        &mut self,
        task: &Task,
        prompt: String,
        follow_up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(tasks) = self.tasks() else { return };
        let runs = Runs::of(&tasks, task);
        // Follow-ups go to the agent that did the work, whatever's picked now.
        let previous = runs.latest().map(|r| r.kind);
        let kind = match (follow_up, previous, self.agents.kind()) {
            (true, Some(k), _) | (_, _, Some(k)) => k,
            _ => return self.info("Pick an agent in New Agent Task first", window, cx),
        };
        if let Some(e) = agent_missing(&kind) {
            return self.error(e, window, cx);
        }
        let session = follow_up.then(|| runs.session()).flatten();
        // A custom command, or Claude without a session to resume, needs the
        // whole task again, not just the feedback.
        let prompt = if follow_up && session.is_none() {
            format!(
                "{}\n\nFeedback on the work so far:\n{}",
                task.prompt.trim(),
                prompt
            )
        } else {
            prompt
        };
        let permissions = Permissions::commands(self.agents.allow_commands);
        match runs.start(task, &kind, &prompt, session.as_deref(), permissions) {
            Ok(_) => {
                self.agents.pane = TaskPane::Transcript;
                self.ok(
                    format!("{} is working on {}", kind.label(), task.id),
                    window,
                    cx,
                );
            }
            Err(e) => self.error(first_line(&e).to_string(), window, cx),
        }
        self.refresh_tasks(cx);
    }

    fn start_agent_on(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(v) = self.agents.get(id) {
            let prompt = v.task.prompt.clone();
            self.start_agent(&v.task, prompt, false, window, cx);
        }
    }

    /// Send the feedback box and any line comments to the task's agent.
    fn send_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.agents.open_task().cloned() else {
            return;
        };
        let text = self.agents.feedback.read(cx).value().trim().to_string();
        let mut message = text.clone();
        if !self.agents.comments.is_empty() {
            let notes: Vec<String> = self
                .agents
                .comments
                .iter()
                .map(|c| format!("- {}: {}", c.place, c.text))
                .collect();
            if !message.is_empty() {
                message.push_str("\n\n");
            }
            message.push_str("Comments on the changes:\n");
            message.push_str(&notes.join("\n"));
        }
        if message.is_empty() {
            return;
        }
        self.start_agent(&view.task, message, true, window, cx);
        if view.run.is_some() || self.agents.kind().is_some() {
            self.agents
                .feedback
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.agents.comments.clear();
        }
    }

    /// Ask the agent to bring its branch up to date with the base and
    /// sort out the conflicts.
    fn ask_to_rebase(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.agents.get(id) else {
            return;
        };
        let files: Vec<String> = view
            .conflicts
            .iter()
            .filter(|(with, _)| *with == view.task.base)
            .flat_map(|(_, f)| f.clone())
            .collect();
        let message = format!(
            "{base} has moved on and this branch now conflicts with it in {files}. \
             Rebase onto {base} (git rebase {base}), resolve the conflicts keeping both \
             sides' intent, make sure it still builds and the tests pass, and commit.",
            base = view.task.base,
            files = files.join(", ")
        );
        self.start_agent(&view.task, message, true, window, cx);
    }

    fn stop_agent(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.agents.get(id) else {
            return;
        };
        let result = self.tasks().and_then(|t| Runs::of(&t, &view.task).stop());
        if let Err(e) = result {
            self.error(e, window, cx);
        }
        self.refresh_tasks(cx);
    }

    /// Note a comment on the lines selected in the task's diff, to send
    /// with the next feedback.
    fn comment_on_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.agents.selection.clone() else {
            return self.info("Select lines in the diff first", window, cx);
        };
        let side = |s: DiffSide| if s == DiffSide::Original { "was " } else { "" };
        let place = if range.start() == range.end() {
            format!("{}:{}{}", range.path(), side(range.side()), range.start())
        } else {
            format!(
                "{}:{}{}–{}",
                range.path(),
                side(range.side()),
                range.start().min(range.end()),
                range.start().max(range.end())
            )
        };
        let note = self.agents.note.clone();
        note.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder("What should change here?", window, cx);
        });
        let spor = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let spor = spor.clone();
            let place = place.clone();
            let note_ok = note.clone();
            note.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .title(format!("Comment on {place}"))
                .w(px(500.))
                .child(Input::new(&note))
                .footer({
                    use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
                    DialogFooter::new()
                        .gap_2()
                        .child(
                            DialogClose::new()
                                .child(Button::new("comment-cancel").label("Cancel").outline()),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("comment-add").label("Add Comment").primary()),
                        )
                })
                .on_ok(move |_, window, cx| {
                    let text = note_ok.read(cx).value().trim().to_string();
                    if text.is_empty() {
                        return false;
                    }
                    let place = place.clone();
                    let _ = spor.update(cx, |this, cx| {
                        this.agents.comments.push(LineComment { place, text });
                        this.agents.pane = TaskPane::Transcript;
                        window.refresh();
                        cx.notify();
                    });
                    true
                })
        });
    }

    // ── Landing and discarding ───────────────────────────────────────────

    pub(crate) fn land_task(
        &mut self,
        id: &str,
        mode: LandMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.agents.get(id) else {
            return;
        };
        if view.running() {
            return self.info(
                format!(
                    "The agent is still working on {} — stop it or wait",
                    view.task.id
                ),
                window,
                cx,
            );
        }
        let (task, stats) = (view.task, view.stats);
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
                            "{} in its folder {} committed. They're committed first and land \
                             with the rest.",
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
                self.set_tab(Tab::History);
            }
            Err(e) => self.error(first_line(&e).to_string(), window, cx),
        }
        self.reload(window, cx);
        self.refresh_tasks(cx);
    }

    pub(crate) fn discard_task(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.agents.get(id) else {
            return;
        };
        if view.running() {
            return self.info(
                format!(
                    "The agent is still working on {} — stop it first",
                    view.task.id
                ),
                window,
                cx,
            );
        }
        let (task, stats) = (view.task, view.stats);
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
                            "Its folder has {} that aren't committed. They're deleted with it \
                             and can't be brought back; Undo restores the commits only.",
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

    /// Remove a landed task from the list, transcript and all.
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
        let Some(view) = self.agents.get(id) else {
            return;
        };
        let (task, stats) = (view.task, view.stats);
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
            support::git_network(&worktree, &["push", "--set-upstream", &remote, &branch])
                .map(|_| format!("Published {branch} — opening a pull request"))
        });
    }

    /// Open the task's folder in a terminal (macOS) or file manager.
    pub(crate) fn open_task_folder(&self, id: &str) {
        let Some(view) = self.agents.get(id) else {
            return;
        };
        let _ = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
                .args(["-a", "Terminal"])
                .arg(&view.task.worktree)
                .spawn()
        } else {
            std::process::Command::new("xdg-open")
                .arg(&view.task.worktree)
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
            .partition(|v| v.task.state == TaskState::Open);
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
            for view in items {
                let is_selected = selected.as_deref() == Some(view.task.id.as_str());
                list = list.child(render_task_row(view, is_selected, spor.clone(), cx));
            }
        }
        let menu_spor = spor.clone();
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .h(px(44.))
                    .flex_none()
                    .px_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_semibold().child("Agents"))
                    .child(div().flex_1())
                    .child(
                        Button::new("new-task")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("New Agent Task… (⌘⇧T)")
                            .on_click(cx.listener(|this, _, w, cx| this.new_task(w, cx))),
                    )
                    .child(
                        Button::new("agents-menu")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(icon::ELLIPSIS))
                            .dropdown_menu(move |menu, _, _| {
                                let spor = menu_spor.clone();
                                menu.item(PopupMenuItem::new("Custom Agent Command…").on_click(
                                    move |_, window, cx| {
                                        let _ = spor.update(cx, |this, cx| {
                                            this.edit_agent_command(window, cx)
                                        });
                                    },
                                ))
                            }),
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
        let Some(view) = self.agents.open_task().cloned() else {
            return empty_state(
                icon::BOT,
                "Task not found",
                "It may have been landed or discarded",
                cx,
            );
        };
        let (task, stats) = (view.task.clone(), view.stats.clone());
        let id = task.id.clone();
        let (label, color) = status(&view, cx);
        let running = view.running();
        let here = self.repo.as_ref().and_then(|r| r.tracking.branch.clone());
        let on_base = here.as_deref() == Some(task.base.as_str());
        let can_land = stats.commits + stats.uncommitted > 0 && !stats.missing && !running;
        let has_remote = self.repo.as_ref().is_some_and(|r| !r.remotes.is_empty());
        let can_start = view.run.is_none() && self.agents.choice != AgentChoice::Manual;
        let base_conflict = view.conflicts.iter().any(|(with, _)| *with == task.base);
        let spor = Self::weak(cx);
        let command = task.agent_command();
        let (land_id, discard_id, pr_id, folder_id, stop_id, start_id, rebase_id) = (
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
        );
        let copy_command = command.clone();
        let pane = self.agents.pane;
        let run_line = view.run.as_ref().map(|run| {
            let mut parts = vec![run.kind.label().to_string()];
            match &run.state {
                RunState::Running => parts.push(format!("started {}", relative_time(run.started))),
                RunState::Finished => parts.push("finished".into()),
                RunState::Failed(code) => parts.push(format!("failed ({code})")),
                RunState::Stopped => parts.push("stopped".into()),
            }
            if let Some(cost) = run.cost_usd {
                parts.push(format!("${cost:.2}"));
            }
            parts.join(" · ")
        });
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
                    .when_some(run_line, |this, line| this.child("·").child(line)),
            )
            .when(!view.conflicts.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(theme.red)
                        .child(conflict_text(&view.conflicts))
                        .when(base_conflict && view.run.is_some() && !running, |this| {
                            this.child(
                                Button::new("task-ask-rebase")
                                    .outline()
                                    .small()
                                    .label("Ask Agent to Rebase")
                                    .tooltip("The agent rebases onto the base and resolves the conflicts")
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.ask_to_rebase(&rebase_id, w, cx)
                                    })),
                            )
                        }),
                )
            })
            .when(!view.conflicts.is_empty() || stats.base_moved, |this| this)
            .when(stats.base_moved && !base_conflict, |this| {
                this.child(div().text_xs().text_color(theme.yellow).child(format!(
                    "{} has moved on — landing rebases first",
                    task.base
                )))
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
                            .tooltip(if running {
                                "The agent is still working"
                            } else if !on_base {
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
                                .disabled(stats.commits == 0 || running)
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.task_pull_request(&pr_id, w, cx)
                                })),
                        )
                    })
                    .when(running, |this| {
                        this.child(
                            Button::new("task-stop")
                                .ghost()
                                .small()
                                .label("Stop Agent")
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.stop_agent(&stop_id, w, cx)
                                })),
                        )
                    })
                    .when(can_start, |this| {
                        this.child(
                            Button::new("task-start")
                                .ghost()
                                .small()
                                .label(format!("Start {}", self.agents.choice.label()))
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.start_agent_on(&start_id, w, cx)
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("task-copy-command")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(icon::COPY))
                            .tooltip(format!("Copy “{command}”"))
                            .on_click(cx.listener(move |this, _, w, cx| {
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
                            .disabled(running)
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.discard_task(&discard_id, w, cx)
                            })),
                    ),
            );
        let tabs = h_flex()
            .flex_none()
            .h(px(40.))
            .px_3()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("pane-transcript")
                    .ghost()
                    .small()
                    .label(if self.agents.comments.is_empty() {
                        "Transcript".to_string()
                    } else {
                        format!("Transcript · {} to send", plural(self.agents.comments.len(), "comment"))
                    })
                    .selected(pane == TaskPane::Transcript)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.agents.pane = TaskPane::Transcript;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("pane-changes")
                    .ghost()
                    .small()
                    .label(if stats.files > 0 {
                        format!("Changes · {}", plural(stats.files, "file"))
                    } else {
                        "Changes".to_string()
                    })
                    .selected(pane == TaskPane::Changes)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.agents.pane = TaskPane::Changes;
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .when(pane == TaskPane::Changes && !stats.is_empty(), |this| {
                this.child(
                    Button::new("comment-lines")
                        .outline()
                        .small()
                        .label("Comment on Lines…")
                        .disabled(self.agents.selection.is_none())
                        .tooltip("Select lines in the diff, then say what should change; it's sent with your next feedback")
                        .on_click(cx.listener(|this, _, w, cx| this.comment_on_selection(w, cx))),
                )
            });
        let body = match pane {
            TaskPane::Changes if stats.is_empty() => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .p_6()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if running {
                            "No changes yet — the agent is working."
                        } else {
                            "No changes yet. Start an agent, or run one in the task's folder — for Claude Code:"
                        }),
                )
                .when(!running, |this| {
                    this.child(
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
                })
                .into_any_element(),
            TaskPane::Changes => div()
                .flex_1()
                .min_h_0()
                .child(Diff::new(&self.agents.diff).line_number(true))
                .into_any_element(),
            TaskPane::Transcript => self.render_transcript(&view, cx),
        };
        v_flex()
            .size_full()
            .child(header)
            .child(tabs)
            .child(body)
            .into_any_element()
    }

    fn render_transcript(&mut self, view: &TaskView, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let running = view.running();
        const SHOWN: usize = 600;
        let entries = &self.agents.transcript;
        let skip = entries.len().saturating_sub(SHOWN);
        let mut list = v_flex().p_3().gap_2();
        if entries.is_empty() {
            list = list.child(
                div()
                    .py_6()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .text_center()
                    .child(if view.run.is_some() {
                        "Waiting for the agent's first words…"
                    } else {
                        "No agent has run here from spor yet. Start one above, or run one in the task's folder."
                    }),
            );
        }
        if skip > 0 {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("… {skip} earlier entries")),
            );
        }
        // Paths inside the task's folder read better relative to it.
        let folder = format!("{}/", view.task.worktree.display());
        for entry in &entries[skip..] {
            let entry = match entry {
                Entry::Tool { name, detail } => Entry::Tool {
                    name: name.clone(),
                    detail: detail.replace(&folder, ""),
                },
                other => other.clone(),
            };
            list = list.child(render_entry(&entry, cx));
        }
        if running {
            list = list.child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.blue)
                    .child(Icon::empty().path(icon::BOT).xsmall())
                    .child("Working…"),
            );
        }
        let comments = self.agents.comments.clone();
        let feedback_empty = self.agents.feedback.read(cx).value().trim().is_empty();
        let can_send = !running && (!feedback_empty || !comments.is_empty());
        let composer = v_flex()
            .flex_none()
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .children(comments.iter().enumerate().map(|(ix, c)| {
                h_flex()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .flex_none()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.muted_foreground)
                            .child(c.place.clone()),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(c.text.clone()))
                    .child(
                        Button::new(("remove-comment", ix))
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if ix < this.agents.comments.len() {
                                    this.agents.comments.remove(ix);
                                }
                                cx.notify();
                            })),
                    )
            }))
            .child(Textarea::new(&self.agents.feedback))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(if running {
                                "Send feedback once the agent has finished"
                            } else if view.run.as_ref().is_some_and(|r| r.session.is_some()) {
                                "Continues the agent's conversation"
                            } else {
                                "Starts the agent again with the task and your feedback"
                            }),
                    )
                    .child(
                        Button::new("send-feedback")
                            .primary()
                            .small()
                            .label("Send to Agent")
                            .disabled(!can_send)
                            .on_click(cx.listener(|this, _, w, cx| this.send_feedback(w, cx))),
                    ),
            );
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("transcript")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(composer)
            .into_any_element()
    }
}

/// Is the agent there to run? A message saying how to get it, if not.
fn agent_missing(kind: &AgentKind) -> Option<String> {
    match kind {
        AgentKind::Claude if !agent::available("claude") => Some(
            "Claude Code isn't installed, or isn't on the PATH — install it, or pick a custom command"
                .into(),
        ),
        AgentKind::Command(c) if c.trim().is_empty() => {
            Some("Set the custom agent command first (Agents panel menu)".into())
        }
        _ => None,
    }
}

fn render_entry(entry: &Entry, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mono = theme.mono_font_family.clone();
    match entry {
        Entry::Prompt(text) => v_flex()
            .mt_2()
            .px_3()
            .py_2()
            .gap_1()
            .rounded(theme.radius)
            .bg(theme.muted)
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(theme.muted_foreground)
                    .child("You"),
            )
            .child(div().text_sm().child(text.clone()))
            .into_any_element(),
        Entry::Text(text) => div().text_sm().child(text.clone()).into_any_element(),
        Entry::Tool { name, detail } => h_flex()
            .w_full()
            .gap_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().flex_none().font_medium().child(name.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(mono)
                    .child(detail.clone()),
            )
            .into_any_element(),
        Entry::ToolError(text) => div()
            .text_xs()
            .text_color(theme.red)
            .child(text.clone())
            .into_any_element(),
        Entry::Done {
            ok,
            summary,
            cost_usd,
            turns,
            seconds,
        } => {
            let mut facts = vec![if *ok { "Done" } else { "Ended" }.to_string()];
            if let Some(s) = seconds {
                facts.push(spor::github::duration(*s as i64));
            }
            if let Some(t) = turns {
                facts.push(plural(*t as usize, "turn"));
            }
            if let Some(c) = cost_usd {
                facts.push(format!("${c:.2}"));
            }
            let color = if *ok { theme.green } else { theme.red };
            v_flex()
                .gap_1()
                .pl_2()
                .border_l_2()
                .border_color(color)
                .child(div().text_xs().text_color(color).child(facts.join(" · ")))
                .when(!summary.is_empty() && summary != "Finished", |this| {
                    this.child(div().text_sm().child(summary.clone()))
                })
                .into_any_element()
        }
        Entry::Output(line) => div()
            .text_xs()
            .font_family(mono)
            .text_color(theme.muted_foreground)
            .child(line.clone())
            .into_any_element(),
        // Only agents run under the host ask or get steered; the GPUI app
        // doesn't run them that way yet, so these show plainly.
        Entry::Ask { tool, detail, .. } => div()
            .text_xs()
            .text_color(theme.yellow)
            .child(format!("Asked to use {tool}: {detail}"))
            .into_any_element(),
        Entry::Answer {
            allow,
            message,
            always,
            ..
        } => div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(if *allow && *always {
                "Allowed for this session".to_string()
            } else if *allow {
                "Allowed".to_string()
            } else if message.is_empty() {
                "Denied".to_string()
            } else {
                format!("Denied: {message}")
            })
            .into_any_element(),
        Entry::Steer(text) => div()
            .text_sm()
            .child(format!("You: {text}"))
            .into_any_element(),
        Entry::Setup { ok, code } => div()
            .text_xs()
            .text_color(match ok {
                Some(false) => theme.red,
                _ => theme.muted_foreground,
            })
            .child(match (ok, code) {
                (None, _) => "Setting up the folder…".to_string(),
                (Some(true), _) => "Folder set up".to_string(),
                (Some(false), c) => format!("Setup failed (exit {})", c.unwrap_or(1)),
            })
            .into_any_element(),
    }
}

fn render_task_row(
    view: TaskView,
    selected: bool,
    spor: gpui_kit::WeakEntity<Spor>,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let (label, color) = status(&view, cx);
    let (task, stats) = (view.task.clone(), view.stats.clone());
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
    let running = view.running();
    let menu_view = view.clone();
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
                )
                .when(!view.conflicts.is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(theme.red)
                            .truncate()
                            .child(conflict_text(&view.conflicts)),
                    )
                }),
        )
        .when(
            !landed && !running && stats.last_commit > 0 && stats.commits > 0,
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
            let t = menu_view.task.clone();
            if t.state != TaskState::Open {
                let id = t.id.clone();
                return menu.item(act(
                    "Remove from List",
                    Rc::new(move |this, w, cx| this.forget_task(&id, w, cx)),
                ));
            }
            let (open, squash, merge, discard, folder, stop, path, command) = (
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.id.clone(),
                t.worktree.to_string_lossy().into_owned(),
                t.agent_command(),
            );
            let worktree = t.worktree.clone();
            let menu = menu.item(act(
                "Open Task",
                Rc::new(move |this, _, cx| this.open_task_page(&open, cx)),
            ));
            let menu = if menu_view.running() {
                menu.item(act(
                    "Stop Agent",
                    Rc::new(move |this, w, cx| this.stop_agent(&stop, w, cx)),
                ))
            } else {
                menu
            };
            menu.item(act(
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
