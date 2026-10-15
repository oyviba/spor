//! The window frame: title bar, the dock + center split, and the status bar.

use super::app::*;
use super::assets::icon;
use super::support::plural;
use super::theme::Appearance;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::DiffMode,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    spinner::Spinner,
    status_bar::StatusBar,
    tab::{Tab as TabItem, TabBar},
    v_flex, ActiveTheme as _, Icon, Selectable as _, Sizable as _, TitleBar,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, Context, ExternalPaths,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window,
};
use remote_badge::pr_label;

impl Render for Spor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let root = v_flex()
            .id("spor")
            .key_context("Spor")
            .track_focus(&self.root_focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .on_action(cx.listener(|this, _: &ToggleDock, _, cx| {
                this.dock = match this.dock {
                    Some(_) => None,
                    None => Some(DockPanel::Git),
                };
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowGitPanel, _, cx| {
                this.dock = Some(DockPanel::Git);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowBranches, _, cx| {
                this.dock = Some(DockPanel::Branches);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowChanges, _, cx| {
                this.tab = Tab::Changes;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                this.tab = Tab::History;
                this.history_focus.focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Refresh, window, cx| this.reload(window, cx)))
            .on_action(cx.listener(|this, _: &UndoLast, window, cx| this.undo_last(window, cx)))
            .on_action(cx.listener(|this, _: &Commit, window, cx| this.commit(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenRepository, window, cx| this.pick_repo(window, cx)),
            )
            .on_action(cx.listener(|this, _: &NewBranch, window, cx| {
                let sha = match this.tab {
                    Tab::History => this
                        .selected
                        .and_then(|i| this.repo.as_ref()?.rows.get(i))
                        .map(|r| r.commit.hash.clone()),
                    Tab::Changes => None,
                };
                this.new_branch(sha, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Sync, window, cx| this.sync(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleDiffMode, _, cx| {
                let next = match this.diff_mode {
                    DiffMode::Unified => DiffMode::Split,
                    _ => DiffMode::Unified,
                };
                this.set_diff_mode(next, cx);
            }))
            .on_action(cx.listener(|this, _: &OlderCommit, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &NewerCommit, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &CheckoutSelected, window, cx| {
                this.checkout_selected(window, cx)
            }))
            .on_action(|_: &Quit, _, cx| cx.quit())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.paths().first() {
                    this.open_repo(&path.clone(), window, cx);
                }
            }));

        root.child(self.render_title_bar(window, cx))
            .child(div().flex_1().min_h_0().child(if self.repo.is_some() {
                self.render_workspace(window, cx)
            } else {
                self.render_welcome(window, cx)
            }))
            .when(self.repo.is_some(), |this| {
                this.child(self.render_status_bar(window, cx))
            })
    }
}

impl Spor {
    fn render_title_bar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let spor = Self::weak(cx);
        let Some(repo) = &self.repo else {
            return TitleBar::new().into_any_element();
        };
        let name: SharedString = repo.name.clone().into();
        let branch: SharedString = repo
            .tracking
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into())
            .into();
        let recent: Vec<_> = self
            .recent
            .iter()
            .filter(|p| **p != repo.root)
            .cloned()
            .collect();
        let locals: Vec<(String, bool)> = repo
            .branches
            .iter()
            .filter(|b| !b.is_remote)
            .map(|b| (b.name.clone(), b.is_current))
            .collect();
        let remotes: Vec<String> = repo
            .branches
            .iter()
            .filter(|b| b.is_remote)
            .map(|b| b.name.clone())
            .collect();
        let appearance = self.appearance;
        let pr = repo
            .tracking
            .branch
            .as_ref()
            .and_then(|b| self.prs.get(b))
            .cloned();

        let repo_menu = {
            let spor = spor.clone();
            Button::new("repo-menu")
                .ghost()
                .small()
                .label(name)
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, window, cx| {
                    let open = spor.clone();
                    menu = menu.item(PopupMenuItem::new("Open Repository…").on_click(
                        move |_, window, cx| {
                            let _ = open.update(cx, |this, cx| this.pick_repo(window, cx));
                        },
                    ));
                    if !recent.is_empty() {
                        menu = menu.separator().label("Recent");
                        for path in &recent {
                            let spor = spor.clone();
                            let path = path.clone();
                            let label = super::support::tildify(&path);
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, window, cx| {
                                    let _ = spor
                                        .update(cx, |this, cx| this.open_repo(&path, window, cx));
                                },
                            ));
                        }
                    }
                    let theme_spor = spor.clone();
                    menu = menu.separator().submenu(
                        "Appearance",
                        window,
                        cx,
                        move |mut menu, _, _| {
                            for (mode, label) in [
                                (Appearance::System, "Match System"),
                                (Appearance::Light, "Light"),
                                (Appearance::Dark, "Dark"),
                            ] {
                                let spor = theme_spor.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(label)
                                        .checked(appearance == mode)
                                        .on_click(move |_, window, cx| {
                                            let _ = spor.update(cx, |this, cx| {
                                                this.set_appearance(mode, window, cx)
                                            });
                                        }),
                                );
                            }
                            menu
                        },
                    );
                    let close = spor.clone();
                    menu.separator()
                        .item(PopupMenuItem::new("Close Repository").on_click(
                            move |_, window, cx| {
                                let _ = close.update(cx, |this, cx| this.close_repo(window, cx));
                            },
                        ))
                })
        };

        let branch_menu = {
            let spor = spor.clone();
            Button::new("branch-menu")
                .ghost()
                .small()
                .icon(Icon::empty().path(icon::GIT_BRANCH))
                .label(branch)
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, window, cx| {
                    menu = menu
                        .scrollable(true)
                        .max_h(px(480.))
                        .min_w(px(260.))
                        .label("Branches");
                    for (name, current) in &locals {
                        let spor = spor.clone();
                        let target = name.clone();
                        menu =
                            menu.item(PopupMenuItem::new(name.clone()).checked(*current).on_click(
                                move |_, window, cx| {
                                    let _ = spor
                                        .update(cx, |this, cx| this.checkout(&target, window, cx));
                                },
                            ));
                    }
                    if !remotes.is_empty() {
                        let remote_spor = spor.clone();
                        let remotes = remotes.clone();
                        menu = menu.separator().submenu(
                            "Remote Branches",
                            window,
                            cx,
                            move |mut menu, _, _| {
                                for name in &remotes {
                                    let spor = remote_spor.clone();
                                    let target = name.clone();
                                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                                        move |_, window, cx| {
                                            let _ = spor.update(cx, |this, cx| {
                                                this.checkout(&target, window, cx)
                                            });
                                        },
                                    ));
                                }
                                menu.scrollable(true).max_h(px(400.))
                            },
                        );
                    }
                    let create = spor.clone();
                    menu.separator()
                        .item(
                            PopupMenuItem::new("New Branch…").on_click(move |_, window, cx| {
                                let _ =
                                    create.update(cx, |this, cx| this.new_branch(None, window, cx));
                            }),
                        )
                })
        };

        TitleBar::new()
            .child(
                h_flex()
                    .size_full()
                    .gap_1()
                    .pr_2()
                    .child(repo_menu)
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .text_sm()
                            .child("/"),
                    )
                    .child(branch_menu)
                    .child(div().flex_1())
                    .when_some(pr, |this, pr| {
                        let spor = spor.clone();
                        this.child(
                            Button::new("pull-request")
                                .ghost()
                                .small()
                                .icon(Icon::empty().path(icon::GIT_PULL_REQUEST))
                                .label(pr_label(&pr))
                                .tooltip("Open this branch's pull request")
                                .on_click(move |_, window, cx| {
                                    let _ = spor
                                        .update(cx, |this, cx| this.open_pull_request(window, cx));
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let dock = self.dock;
        let center = self.render_center(window, cx);
        let dock_content = dock.map(|panel| match panel {
            DockPanel::Git => self.render_git_panel(window, cx),
            DockPanel::Branches => self.render_branches(window, cx),
        });
        h_resizable("spor-workspace")
            .when_some(dock_content, |this, content| {
                this.child(
                    resizable_panel()
                        .size(px(320.))
                        .size_range(px(240.)..px(560.))
                        .child(content),
                )
            })
            .child(
                resizable_panel()
                    .size_range(px(420.)..px(4000.))
                    .child(center),
            )
            .into_any_element()
    }

    fn render_center(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let changes = self.repo.as_ref().map_or(0, |r| r.changes.len());
        let selected = match self.tab {
            Tab::Changes => 0,
            Tab::History => 1,
        };
        let changes_label = if changes > 0 {
            format!("Uncommitted Changes ({changes})")
        } else {
            "Uncommitted Changes".to_string()
        };
        let tab_icon = |path: &'static str| {
            div().pl_3().child(
                Icon::empty()
                    .path(path)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
        };
        let tabs = TabBar::new("center-tabs")
            .child(
                TabItem::new()
                    .label(changes_label)
                    .prefix(tab_icon(icon::FILE_DIFF)),
            )
            .child(
                TabItem::new()
                    .label("History")
                    .prefix(tab_icon(icon::CLOCK)),
            )
            .selected_index(selected)
            .on_click(cx.listener(|this, ix: &usize, window, cx| {
                this.tab = if *ix == 0 { Tab::Changes } else { Tab::History };
                if this.tab == Tab::History {
                    this.history_focus.focus(window, cx);
                }
                cx.notify();
            }));
        v_flex()
            .size_full()
            .min_w_0()
            .child(tabs)
            .child(div().flex_1().min_h_0().child(match self.tab {
                Tab::Changes => self.render_changes(window, cx),
                Tab::History => self.render_history(window, cx),
            }))
            .into_any_element()
    }

    fn render_status_bar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(repo) = &self.repo else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let dock = self.dock;
        let t = &repo.tracking;
        let mut sync = Vec::new();
        if t.ahead > 0 {
            sync.push(format!("↑{}", t.ahead));
        }
        if t.behind > 0 {
            sync.push(format!("↓{}", t.behind));
        }
        let branch = t.branch.clone().unwrap_or_else(|| "detached".into());
        let commits = plural(repo.rows.len(), "commit");
        let mode = self.diff_mode;
        let job = self.job;

        let dock_button =
            |id: &'static str, path: &'static str, panel: DockPanel, tip: &'static str| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(Icon::empty().path(path))
                    .selected(dock == Some(panel))
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dock = if this.dock == Some(panel) {
                            None
                        } else {
                            Some(panel)
                        };
                        cx.notify();
                    }))
            };

        StatusBar::new()
            .left(
                h_flex()
                    .gap_1()
                    .child(dock_button(
                        "dock-git",
                        icon::GIT_COMPARE,
                        DockPanel::Git,
                        "Git Panel",
                    ))
                    .child(dock_button(
                        "dock-branches",
                        icon::GIT_BRANCH,
                        DockPanel::Branches,
                        "Branches",
                    ))
                    .child(
                        h_flex()
                            .ml_2()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .child(Icon::empty().path(icon::GIT_BRANCH).xsmall())
                            .child(branch)
                            .when(!sync.is_empty(), |this| this.child(sync.join(" "))),
                    ),
            )
            .right(
                h_flex()
                    .gap_3()
                    .text_xs()
                    .text_color(muted)
                    .when_some(job, |this, job| {
                        this.child(
                            h_flex()
                                .gap_1()
                                .child(Spinner::new().xsmall())
                                .child(format!("{job}ing…")),
                        )
                    })
                    .child(commits)
                    .child(
                        Button::new("diff-mode")
                            .ghost()
                            .xsmall()
                            .label(match mode {
                                DiffMode::Unified => "Unified",
                                _ => "Split",
                            })
                            .tooltip("Toggle unified / side-by-side diffs")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let next = match this.diff_mode {
                                    DiffMode::Unified => DiffMode::Split,
                                    _ => DiffMode::Unified,
                                };
                                this.set_diff_mode(next, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

/// Small helpers for PR badges.
pub mod remote_badge {
    use spor::remote::{ChecksState, PrInfo, ReviewState};

    pub fn pr_label(pr: &PrInfo) -> String {
        let checks = match pr.checks {
            ChecksState::Passing => " ✓",
            ChecksState::Failing => " ✗",
            ChecksState::Pending => " ●",
            ChecksState::None => "",
        };
        let review = match (pr.draft, pr.review) {
            (true, _) => " · draft",
            (_, ReviewState::Approved) => " · approved",
            (_, ReviewState::ChangesRequested) => " · changes requested",
            _ => "",
        };
        format!("#{}{checks}{review}", pr.number)
    }
}
