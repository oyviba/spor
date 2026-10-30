//! The window frame: title bar, the workspace rail and repository list, the
//! dock + center split (the dock on either side), and the status bar.

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
    v_flex, ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, TitleBar,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, Context, ExternalPaths, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window,
};
use remote_badge::pr_label;
use std::path::PathBuf;

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
            .on_action(cx.listener(|this, _: &ToggleDockSide, _, cx| this.toggle_dock_side(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleRepositories, _, cx| this.toggle_repositories(cx)),
            )
            .on_action(cx.listener(|this, _: &ShowGitPanel, _, cx| {
                this.dock = Some(DockPanel::Git);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowBranches, _, cx| {
                this.dock = Some(DockPanel::Branches);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SearchBranches, window, cx| {
                this.dock = Some(DockPanel::Branches);
                this.branch_filter
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
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
            .on_action(cx.listener(|this, _: &ShowPullRequests, _, cx| this.show_pulls(cx)))
            .on_action(cx.listener(|this, _: &ShowActions, _, cx| this.show_runs(cx)))
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
                    _ => None,
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
            .on_action(
                cx.listener(|this, _: &EditTheme, window, cx| this.open_theme_editor(window, cx)),
            )
            .on_action(|_: &Quit, _, cx| cx.quit())
            .on_action(
                cx.listener(|this, _: &NewWorkspace, window, cx| this.new_workspace(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowShortcuts, window, cx| this.open_shortcuts(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Fetch, window, cx| this.fetch(window, cx)))
            .on_action(cx.listener(|this, _: &Pull, window, cx| this.pull(window, cx)))
            .on_action(cx.listener(|this, _: &Push, window, cx| this.push(window, cx)))
            .on_action(cx.listener(|this, _: &OpenPullRequest, window, cx| {
                this.open_pull_request(window, cx)
            }))
            .on_action(cx.listener(|this, _: &StashChanges, window, cx| this.stash(window, cx)));
        macro_rules! workspace_keys {
            ($root:expr, $($action:ident => $ix:expr),*) => {
                $root$(.on_action(cx.listener(|this, _: &$action, window, cx| {
                    this.switch_workspace($ix, window, cx)
                })))*
            };
        }
        let root = workspace_keys!(root,
            Workspace1 => 0, Workspace2 => 1, Workspace3 => 2, Workspace4 => 3, Workspace5 => 4,
            Workspace6 => 5, Workspace7 => 6, Workspace8 => 7, Workspace9 => 8)
        .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
            if let Some(path) = paths.paths().first() {
                this.open_repo(&path.clone(), window, cx);
            }
        }));

        let rail = self.render_rail(window, cx);
        let repos = self
            .layout
            .show_repos
            .then(|| self.render_repos_panel(window, cx));
        let main = if self.repo.is_some() {
            self.render_workspace(window, cx)
        } else {
            self.render_welcome(window, cx)
        };
        root.child(self.render_title_bar(window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(rail)
                    .children(repos)
                    .child(div().flex_1().min_w_0().h_full().child(main)),
            )
            .when(self.repo.is_some(), |this| {
                this.child(self.render_status_bar(window, cx))
            })
    }
}

impl Spor {
    fn render_title_bar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let spor = Self::weak(cx);
        let Some(repo) = &self.repo else {
            return TitleBar::new()
                .child(
                    div()
                        .px_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.workspaces.current().name.clone()),
                )
                .into_any_element();
        };
        let name: SharedString = repo.name.clone().into();
        let branch: SharedString = repo
            .tracking
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into())
            .into();
        let workspace = self.workspaces.current();
        let workspace_name = workspace.name.clone();
        let repos: Vec<(PathBuf, bool)> = workspace
            .repos
            .iter()
            .map(|p| (p.clone(), *p == repo.root))
            .collect();
        let others: Vec<(usize, String)> = self
            .workspaces
            .list
            .iter()
            .enumerate()
            .filter(|(ix, _)| *ix != self.workspaces.active)
            .map(|(ix, ws)| (ix, ws.name.clone()))
            .collect();
        let root = repo.root.clone();
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
        let behind = repo.tracking.behind;
        let upstream = repo
            .tracking
            .upstream
            .clone()
            .unwrap_or_else(|| "the remote".into());
        let sync_label = self.sync_label();
        let job = self.job;
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
                    menu = menu
                        .scrollable(true)
                        .max_h(px(520.))
                        .min_w(px(240.))
                        .label(workspace_name.clone());
                    for (path, current) in &repos {
                        let spor = spor.clone();
                        let target = path.clone();
                        let label = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        menu = menu.item(PopupMenuItem::new(label).checked(*current).on_click(
                            move |_, window, cx| {
                                let _ =
                                    spor.update(cx, |this, cx| this.open_repo(&target, window, cx));
                            },
                        ));
                    }
                    let open = spor.clone();
                    menu = menu.separator().item(
                        PopupMenuItem::new(format!("Add Repository to {workspace_name}…"))
                            .on_click(move |_, window, cx| {
                                let _ = open.update(cx, |this, cx| this.pick_repo(window, cx));
                            }),
                    );
                    if !others.is_empty() {
                        let move_spor = spor.clone();
                        let others = others.clone();
                        menu = menu.submenu(
                            "Move Repository To",
                            window,
                            cx,
                            move |mut menu, _, _| {
                                for (ix, name) in &others {
                                    let spor = move_spor.clone();
                                    let ix = *ix;
                                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                                        move |_, window, cx| {
                                            let _ = spor.update(cx, |this, cx| {
                                                this.move_repo_to(ix, window, cx)
                                            });
                                        },
                                    ));
                                }
                                menu
                            },
                        );
                    }
                    let remove = spor.clone();
                    let root = root.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("Remove from {workspace_name}")).on_click(
                            move |_, window, cx| {
                                let _ = remove
                                    .update(cx, |this, cx| this.remove_repo(&root, window, cx));
                            },
                        ),
                    );
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
                            let editor = theme_spor.clone();
                            menu.separator()
                                .item(PopupMenuItem::new("Edit Theme…").on_click(
                                    move |_, window, cx| {
                                        let _ = editor.update(cx, |this, cx| {
                                            this.open_theme_editor(window, cx)
                                        });
                                    },
                                ))
                        },
                    );
                    let palette = spor.clone();
                    let shortcuts = spor.clone();
                    menu = menu
                        .separator()
                        .item(PopupMenuItem::new("Search Pull Requests… (⌘P)").on_click(
                            move |_, window, cx| {
                                let _ =
                                    palette.update(cx, |this, cx| this.open_palette(window, cx));
                            },
                        ))
                        .item(PopupMenuItem::new("Keyboard Shortcuts (⌘/)").on_click(
                            move |_, window, cx| {
                                let _ = shortcuts
                                    .update(cx, |this, cx| this.open_shortcuts(window, cx));
                            },
                        ));
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
                    // Hard to miss: the branch is behind its upstream.
                    .when(behind > 0, |this| {
                        this.child(
                            Button::new("title-pull")
                                .info()
                                .small()
                                .icon(IconName::ArrowDown)
                                .label(sync_label)
                                .loading(job.is_some())
                                .tooltip(format!(
                                    "{upstream} has {} you don't have yet (⌘⇧S)",
                                    plural(behind, "commit")
                                ))
                                .on_click(cx.listener(|this, _, window, cx| this.sync(window, cx))),
                        )
                    })
                    .when_some(pr, |this, pr| {
                        let spor = spor.clone();
                        let number = pr.number;
                        this.child(
                            Button::new("pull-request")
                                .ghost()
                                .small()
                                .icon(Icon::empty().path(icon::GIT_PULL_REQUEST))
                                .label(pr_label(&pr))
                                .tooltip("Show this branch's pull request")
                                .on_click(move |_, _, cx| {
                                    let _ = spor.update(cx, |this, cx| this.show_pull(number, cx));
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
        let dock_panel = |content| {
            resizable_panel()
                .size(px(320.))
                .size_range(px(240.)..px(560.))
                .child(content)
        };
        let center = resizable_panel()
            .size_range(px(420.)..px(4000.))
            .child(center);
        // Separate ids so each side remembers its own dock width.
        match self.layout.dock_side {
            DockSide::Left => h_resizable("spor-workspace")
                .when_some(dock_content, |this, content| {
                    this.child(dock_panel(content))
                })
                .child(center),
            DockSide::Right => h_resizable("spor-workspace-right")
                .child(center)
                .when_some(dock_content, |this, content| {
                    this.child(dock_panel(content))
                }),
        }
        .into_any_element()
    }

    fn render_center(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let changes = self.repo.as_ref().map_or(0, |r| r.changes.len());
        let selected = match self.tab {
            Tab::Changes => 0,
            Tab::History => 1,
            Tab::PullRequests => 2,
            Tab::Actions => 3,
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
            .child(
                TabItem::new()
                    .label("Pull Requests")
                    .prefix(tab_icon(icon::GIT_PULL_REQUEST)),
            )
            .child(
                TabItem::new()
                    .label("Actions")
                    .prefix(tab_icon(icon::ACTIONS)),
            )
            .selected_index(selected)
            .on_click(cx.listener(|this, ix: &usize, window, cx| {
                match ix {
                    0 => this.tab = Tab::Changes,
                    1 => {
                        this.tab = Tab::History;
                        this.history_focus.focus(window, cx);
                    }
                    2 => this.show_pulls(cx),
                    _ => this.show_runs(cx),
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
                Tab::PullRequests => self.render_pulls(window, cx),
                Tab::Actions => self.render_runs(window, cx),
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
        let ahead = (t.ahead > 0).then(|| format!("↑{}", t.ahead));
        let behind = (t.behind > 0).then(|| format!("↓{} to pull", t.behind));
        let blue = cx.theme().blue;
        let branch = t.branch.clone().unwrap_or_else(|| "detached".into());
        let commits = plural(repo.rows.len(), "commit");
        let mode = self.diff_mode;
        let job = self.job;
        let show_repos = self.layout.show_repos;
        let dock_side = self.layout.dock_side;

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
                    .child(
                        Button::new("toggle-repositories")
                            .ghost()
                            .xsmall()
                            .icon(Icon::empty().path(icon::FOLDER_GIT))
                            .selected(show_repos)
                            .tooltip("Repositories (⌘⇧E)")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_repositories(cx))),
                    )
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
                        Button::new("dock-side")
                            .ghost()
                            .xsmall()
                            .icon(match dock_side {
                                DockSide::Left => IconName::PanelLeft,
                                DockSide::Right => IconName::PanelRight,
                            })
                            .tooltip(match dock_side {
                                DockSide::Left => "Move Panel to the Right",
                                DockSide::Right => "Move Panel to the Left",
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_dock_side(cx))),
                    )
                    .child(
                        h_flex()
                            .ml_2()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .child(Icon::empty().path(icon::GIT_BRANCH).xsmall())
                            .child(branch)
                            .children(ahead),
                    )
                    .when_some(behind, |this, behind| {
                        this.child(
                            Button::new("status-pull")
                                .ghost()
                                .xsmall()
                                .label(behind)
                                .text_color(blue)
                                .tooltip("Pull the new commits (⌘⇧S)")
                                .on_click(cx.listener(|this, _, window, cx| this.sync(window, cx))),
                        )
                    }),
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
