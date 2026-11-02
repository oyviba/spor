//! ⌘P: one search field over the repository's pull requests (picking one
//! opens it in the Pull Requests tab) and the app's commands (with their
//! shortcuts), and the table of those commands.
//!
//! Both are driven by [`SHORTCUTS`], so a new action shows up in the palette
//! and the shortcut list from one entry. Its keys are its defaults unless
//! the user assigned others (`keymap`).

use super::app::*;
use super::assets::icon;
use gpui_kit::component::{
    command::{Command, CommandGroup, CommandItem, CommandState},
    h_flex,
    kbd::Kbd,
    ActiveTheme as _, Disableable as _, Icon, IndexPath, Sizable as _, WindowExt as _,
};
use gpui_kit::{
    div, px, Action, AnyElement, App, AppContext as _, AsKeystroke, Context, Entity, FocusHandle,
    Focusable as _, Hsla, IntoElement, KeyContext, ParentElement as _, Render, Styled as _,
    Subscription, WeakEntity, Window,
};
use spor::remote::{PrState, PrSummary};
use std::rc::Rc;

/// One entry in the palette and the shortcut list.
pub struct Shortcut {
    pub group: &'static str,
    pub label: &'static str,
    pub action: fn() -> Box<dyn Action>,
    /// Where the binding applies.
    pub context: &'static str,
    /// The keys it has until the user assigns others (see `keymap`).
    pub default_keys: &'static [&'static str],
    /// A second context the same keys apply in.
    pub also_in: Option<&'static str>,
    /// Shown as "first – last", for a numbered run like ⌘1–⌘9. Such a run
    /// is bound in `app::init` and can't be reassigned.
    pub through: Option<fn() -> Box<dyn Action>>,
    /// Offered in the palette. Movement keys only mean something in place.
    pub in_palette: bool,
}

impl Shortcut {
    /// Can the user give it other keys?
    pub fn assignable(&self) -> bool {
        self.through.is_none()
    }

    /// The action's registered name, which the saved keymap is keyed by.
    pub fn name(&self) -> &'static str {
        (self.action)().name()
    }
}

const fn cmd(
    group: &'static str,
    label: &'static str,
    default_keys: &'static [&'static str],
    action: fn() -> Box<dyn Action>,
) -> Shortcut {
    Shortcut {
        group,
        label,
        action,
        context: "Spor",
        default_keys,
        also_in: None,
        through: None,
        in_palette: true,
    }
}

const fn in_history(
    label: &'static str,
    default_keys: &'static [&'static str],
    action: fn() -> Box<dyn Action>,
) -> Shortcut {
    Shortcut {
        group: "History",
        label,
        action,
        context: "SporHistory",
        default_keys,
        also_in: None,
        through: None,
        in_palette: false,
    }
}

pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        in_palette: false,
        ..cmd(
            "General",
            "Search Pull Requests and Commands",
            &["secondary-p"],
            || Box::new(OpenPalette),
        )
    },
    cmd("General", "Keyboard Shortcuts", &["secondary-/"], || {
        Box::new(ShowShortcuts)
    }),
    cmd("General", "Open Repository…", &["secondary-o"], || {
        Box::new(OpenRepository)
    }),
    cmd(
        "General",
        "New Workspace…",
        &["secondary-shift-w"],
        || Box::new(NewWorkspace),
    ),
    Shortcut {
        through: Some(|| Box::new(Workspace9)),
        in_palette: false,
        ..cmd("General", "Switch to Workspace 1–9", &[], || {
            Box::new(Workspace1)
        })
    },
    cmd("General", "Edit Theme…", &["secondary-,"], || {
        Box::new(EditTheme)
    }),
    cmd("General", "Quit Spor", &["secondary-q"], || Box::new(Quit)),
    Shortcut {
        // The commit message editor binds ⌘↩ itself; this wins there.
        also_in: Some("CommitEditor > Input"),
        ..cmd("Git", "Commit", &["secondary-enter"], || Box::new(Commit))
    },
    cmd("Git", "Undo", &["secondary-z"], || Box::new(UndoLast)),
    cmd(
        "Git",
        "Sync (Fetch, then Pull or Push)",
        &["secondary-shift-s"],
        || Box::new(Sync),
    ),
    cmd("Git", "Fetch", &[], || Box::new(Fetch)),
    cmd("Git", "Pull", &[], || Box::new(Pull)),
    cmd("Git", "Push", &[], || Box::new(Push)),
    cmd("Git", "New Branch…", &["secondary-shift-n"], || {
        Box::new(NewBranch)
    }),
    cmd("Git", "Open Pull Request for This Branch", &[], || {
        Box::new(OpenPullRequest)
    }),
    cmd("Git", "Stash All Changes", &[], || Box::new(StashChanges)),
    cmd("Git", "Refresh", &["secondary-r"], || Box::new(Refresh)),
    cmd(
        "View",
        "Show Uncommitted Changes",
        &["secondary-shift-c"],
        || Box::new(ShowChanges),
    ),
    cmd("View", "Show History", &["secondary-shift-h"], || {
        Box::new(ShowHistory)
    }),
    cmd("View", "Show Pull Requests", &["secondary-shift-p"], || {
        Box::new(ShowPullRequests)
    }),
    cmd("View", "Show Actions", &["secondary-shift-a"], || {
        Box::new(ShowActions)
    }),
    cmd("View", "Show Git Panel", &["secondary-shift-g"], || {
        Box::new(ShowGitPanel)
    }),
    cmd("View", "Show Branches", &["secondary-shift-b"], || {
        Box::new(ShowBranches)
    }),
    cmd("View", "Search Branches", &["secondary-shift-f"], || {
        Box::new(SearchBranches)
    }),
    cmd("View", "Toggle Dock", &["secondary-b"], || {
        Box::new(ToggleDock)
    }),
    cmd("View", "Move Dock to the Other Side", &[], || {
        Box::new(ToggleDockSide)
    }),
    cmd(
        "View",
        "Toggle Repositories",
        &["secondary-shift-e"],
        || Box::new(ToggleRepositories),
    ),
    cmd("View", "Toggle Unified / Split Diff", &[], || {
        Box::new(ToggleDiffMode)
    }),
    in_history("Select Older Commit", &["down", "j"], || {
        Box::new(OlderCommit)
    }),
    in_history("Select Newer Commit", &["up", "k"], || {
        Box::new(NewerCommit)
    }),
    in_history("Check Out Selected Commit's Branch", &["enter"], || {
        Box::new(CheckoutSelected)
    }),
];

/// Every key bound to `action` where `context` applies, deduplicated.
pub(crate) fn keys(action: &dyn Action, context: &str, window: &Window) -> Vec<Kbd> {
    let Ok(context) = KeyContext::parse(context) else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    window
        .bindings_for_action_in_context(action, context)
        .iter()
        .filter_map(|b| b.keystrokes().first().map(|k| k.as_keystroke().clone()))
        .filter(|k| {
            let new = !seen.contains(k);
            seen.push(k.clone());
            new
        })
        .map(Kbd::new)
        .collect()
}

/// The keys for a shortcut row: one or more alternatives, or a range.
pub(crate) fn render_keys(shortcut: &Shortcut, window: &Window, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let first = keys(&*(shortcut.action)(), shortcut.context, window);
    let row = h_flex().flex_none().gap_1().text_xs().text_color(muted);
    match shortcut.through {
        Some(last) => {
            let last = keys(&*last(), shortcut.context, window);
            row.children(first.into_iter().take(1))
                .child("–")
                .children(last.into_iter().take(1))
        }
        None => {
            let mut row = row;
            for (i, kbd) in first.into_iter().enumerate() {
                if i > 0 {
                    row = row.child("or");
                }
                row = row.child(kbd);
            }
            row
        }
    }
    .into_any_element()
}

/// The repository's PRs as the palette last loaded them.
#[derive(Clone)]
pub enum PrSearch {
    Idle,
    Loading,
    Ready(Rc<Vec<PrSummary>>),
    Failed(String),
}

/// What a palette row does when picked.
#[derive(Clone)]
enum Pick {
    Pull(u64),
    Run(usize),
    Nothing,
}

pub struct Palette {
    state: Entity<CommandState>,
    spor: WeakEntity<Spor>,
    root_focus: FocusHandle,
    query: String,
    /// The first PR is highlighted once, when the list first shows up.
    highlighted_first: bool,
    _subscriptions: Vec<Subscription>,
}

impl Palette {
    /// Built while `spor` is being updated, so it must not read it.
    fn new(
        spor: &Entity<Spor>,
        root_focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.new(|cx| CommandState::new(window, cx));
        Self {
            state,
            root_focus,
            spor: spor.downgrade(),
            query: String::new(),
            highlighted_first: false,
            // Re-render when the PR list arrives.
            _subscriptions: vec![cx.observe(spor, |_, _, cx| cx.notify())],
        }
    }
}

fn pr_color(state: PrState, cx: &App) -> Hsla {
    let theme = cx.theme();
    match state {
        PrState::Open => theme.green,
        PrState::Draft => theme.muted_foreground,
        PrState::Merged => theme.magenta,
        PrState::Closed => theme.red,
    }
}

fn pr_state_label(state: PrState) -> &'static str {
    match state {
        PrState::Open => "Open",
        PrState::Draft => "Draft",
        PrState::Merged => "Merged",
        PrState::Closed => "Closed",
    }
}

fn pr_item(pr: &PrSummary) -> CommandItem {
    let pr = pr.clone();
    CommandItem::new()
        .label(format!("#{} {}", pr.number, pr.title))
        .child(move |_, cx| {
            let theme = cx.theme();
            let color = pr_color(pr.state, cx);
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(
                    Icon::empty()
                        .path(icon::GIT_PULL_REQUEST)
                        .small()
                        .text_color(color),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(format!("#{}", pr.number)),
                )
                .child(div().flex_1().min_w_0().truncate().child(pr.title.clone()))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(200.))
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{} · {}", pr.head_branch, pr.author)),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(48.))
                        .text_xs()
                        .text_color(color)
                        .child(pr_state_label(pr.state)),
                )
        })
}

fn note_item(text: String) -> CommandItem {
    CommandItem::new()
        .label(text.clone())
        .disabled(true)
        .child(move |_, cx| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.clone())
        })
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let search = self
            .spor
            .upgrade()
            .map(|spor| spor.read(cx).pr_search.clone())
            .unwrap_or(PrSearch::Idle);
        let query = self.query.trim().to_lowercase();

        // Group 0: pull requests. With no query, the open ones; typing
        // searches every state.
        let mut groups: Vec<(CommandGroup, Vec<Pick>)> = Vec::new();
        let mut prs = CommandGroup::new().label(if query.is_empty() {
            "Open Pull Requests"
        } else {
            "Pull Requests"
        });
        let mut pr_picks = Vec::new();
        match &search {
            PrSearch::Idle | PrSearch::Loading => {
                prs = prs.item(note_item("Loading pull requests…".into()));
                pr_picks.push(Pick::Nothing);
            }
            PrSearch::Failed(e) => {
                prs = prs.item(note_item(e.clone()));
                pr_picks.push(Pick::Nothing);
            }
            PrSearch::Ready(list) => {
                let shown = list
                    .iter()
                    .filter(|pr| {
                        if query.is_empty() {
                            matches!(pr.state, PrState::Open | PrState::Draft)
                        } else {
                            pr.matches(&query)
                        }
                    })
                    .take(100);
                for pr in shown {
                    prs = prs.item(pr_item(pr));
                    pr_picks.push(Pick::Pull(pr.number));
                }
                if pr_picks.is_empty() && query.is_empty() {
                    prs = prs.item(note_item(
                        "No open pull requests — type to search closed and merged ones".into(),
                    ));
                    pr_picks.push(Pick::Nothing);
                }
            }
        }
        // The list arrives after the palette opens; start on its first PR
        // rather than wherever the highlight fell meanwhile.
        if !self.highlighted_first && matches!(pr_picks.first(), Some(Pick::Pull(_))) {
            self.highlighted_first = true;
            let state = self.state.clone();
            window.defer(cx, move |window, cx| {
                state.update(cx, |state, cx| {
                    state.set_selected_index(Some(IndexPath::new(0)), window, cx)
                })
            });
        }
        if !pr_picks.is_empty() {
            groups.push((prs, pr_picks));
        }

        // Group 1: commands, each with its shortcut.
        let mut commands = CommandGroup::new().label("Commands");
        let mut command_picks = Vec::new();
        for (ix, shortcut) in SHORTCUTS.iter().enumerate() {
            let label = shortcut.label.to_lowercase();
            let group = shortcut.group.to_lowercase();
            if !shortcut.in_palette
                || !query
                    .split_whitespace()
                    .all(|w| label.contains(w) || group.contains(w))
            {
                continue;
            }
            commands = commands.item(CommandItem::new().label(shortcut.label).child(
                move |window, cx| {
                    let shortcut = &SHORTCUTS[ix];
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .child(div().flex_1().min_w_0().truncate().child(shortcut.label))
                        .child(render_keys(shortcut, window, cx))
                },
            ));
            command_picks.push(Pick::Run(ix));
        }
        if !command_picks.is_empty() {
            groups.push((commands, command_picks));
        }

        let picks: Rc<Vec<Vec<Pick>>> = Rc::new(groups.iter().map(|(_, p)| p.clone()).collect());
        let palette = cx.entity().downgrade();
        let root_focus = self.root_focus.clone();
        let spor = self.spor.clone();
        let mut command = Command::new(&self.state)
            .filterable(false)
            .bordered(false)
            .max_h(px(440.))
            .placeholder(
                "Search pull requests by number, title, branch or author, or run a command…",
            )
            .on_query(move |query, _, cx| {
                let query = query.to_string();
                let _ = palette.update(cx, |this, cx| {
                    this.query = query;
                    cx.notify();
                });
            })
            .on_confirm(move |ix, window, cx| {
                let Some(pick) = picks.get(ix.section).and_then(|g| g.get(ix.row)) else {
                    return;
                };
                match pick.clone() {
                    Pick::Nothing => {}
                    Pick::Pull(number) => {
                        window.close_dialog(cx);
                        let _ = spor.update(cx, |this, cx| this.show_pull(number, cx));
                    }
                    Pick::Run(ix) => {
                        window.close_dialog(cx);
                        // The dialog isn't inside the Spor view, so the
                        // action is sent to the view's own focus handle.
                        let action = (SHORTCUTS[ix].action)();
                        root_focus.dispatch_action(&*action, window, cx);
                    }
                }
            });
        for (group, _) in groups {
            command = command.group(group);
        }
        command
    }
}

impl Spor {
    /// ⌘P: search the repository's pull requests, or run a command.
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_pr_search(cx);
        let spor = cx.entity();
        let root_focus = self.root_focus.clone();
        let palette = cx.new(|cx| Palette::new(&spor, root_focus, window, cx));
        let state = palette.read(cx).state.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            state.read(cx).focus_handle(cx).focus(window, cx);
            dialog
                .w(px(680.))
                .margin_top(px(72.))
                .p_0()
                .close_button(false)
                .child(palette.clone())
        });
    }

    /// Fetch the PR list for the palette in the background. The last list
    /// stays up while a fresh one loads, so reopening is instant.
    fn load_pr_search(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            self.pr_search =
                PrSearch::Failed("Open a repository to search its pull requests".into());
            return;
        };
        if self.pr_search_root.as_ref() != Some(&root) {
            self.pr_search = PrSearch::Loading;
            self.pr_search_root = Some(root.clone());
        } else if matches!(self.pr_search, PrSearch::Idle | PrSearch::Failed(_)) {
            self.pr_search = PrSearch::Loading;
        }
        self._pr_search_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let root = root.clone();
                    async move { spor::remote::list_prs_for_search(&root) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.pr_search_root.as_ref() != Some(&root) {
                    return;
                }
                this.pr_search = match result {
                    Ok(list) => PrSearch::Ready(Rc::new(list)),
                    Err(_) if matches!(this.pr_search, PrSearch::Ready(_)) => {
                        this.pr_search.clone()
                    }
                    Err(e) => PrSearch::Failed(e),
                };
                cx.notify();
            });
        });
    }
}
