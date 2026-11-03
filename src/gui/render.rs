//! The window frame: title bar, the navigation rail and repository list,
//! and the sidebar + center split (the sidebar on either side). The rail
//! picks a section; its panel fills the sidebar and its view the center.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::repos_panel::{REPOS_MAX_WIDTH, REPOS_MIN_WIDTH, REPOS_WIDTH};
use super::support::plural;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::DiffMode,
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar,
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
                    None => Some(this.tab.panel()),
                };
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleDockSide, _, cx| this.toggle_dock_side(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleRepositories, _, cx| this.toggle_repositories(cx)),
            )
            .on_action(cx.listener(|this, _: &ShowGitPanel, _, cx| {
                this.set_dock(DockPanel::Git);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowBranches, _, cx| {
                this.set_dock(DockPanel::Branches);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SearchBranches, window, cx| {
                this.set_dock(DockPanel::Branches);
                this.branch_filter
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowChanges, _, cx| {
                this.set_tab(Tab::Changes);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                this.set_tab(Tab::History);
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
            .on_action(
                cx.listener(|this, _: &StashChanges, window, cx| {
                    this.stash_with_message(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SearchCommits, window, cx| this.focus_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &PullRebase, window, cx| this.pull_rebase(window, cx)))
            .on_action(cx.listener(|this, _: &NewTask, window, cx| this.new_task(window, cx)))
            .on_action(cx.listener(|this, _: &ShowAgents, _, cx| this.show_agents(cx)))
            .on_action(cx.listener(|this, _: &ContinueOperation, window, cx| {
                this.continue_operation(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &AbortOperation, window, cx| {
                    this.abort_operation(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ToggleAmend, window, cx| this.toggle_amend(window, cx)),
            )
            .on_action(cx.listener(|this, _: &NewTag, window, cx| {
                if let Some(sha) = this.selected_sha() {
                    this.new_tag(sha, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CherryPickSelected, window, cx| {
                if let Some(sha) = this.selected_sha() {
                    this.cherry_pick(&sha, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &RevertSelected, window, cx| {
                if let Some(sha) = this.selected_sha() {
                    this.revert(&sha, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &StageLines, window, cx| {
                this.apply_selected_lines(spor::git::LineAction::Stage, window, cx)
            }))
            .on_action(cx.listener(|this, _: &UnstageLines, window, cx| {
                this.apply_selected_lines(spor::git::LineAction::Unstage, window, cx)
            }))
            .on_action(cx.listener(|this, _: &DiscardLines, window, cx| {
                this.apply_selected_lines(spor::git::LineAction::Discard, window, cx)
            }));
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
        // The repository list gets a draggable edge; the rail stays fixed.
        let main = div()
            .size_full()
            .bg(cx.theme().background)
            .child(main)
            .into_any_element();
        let main = match repos {
            Some(repos) => h_resizable("spor-repos")
                .child(
                    resizable_panel()
                        .size(px(REPOS_WIDTH))
                        .size_range(px(REPOS_MIN_WIDTH)..px(REPOS_MAX_WIDTH))
                        .child(repos),
                )
                .child(resizable_panel().child(main))
                .into_any_element(),
            None => main,
        };
        root.child(self.render_title_bar(window, cx)).child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(rail)
                .child(div().flex_1().min_w_0().h_full().child(main)),
        )
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
        let behind = repo.tracking.behind;
        let ahead = repo.tracking.ahead;
        let has_remote = !repo.remotes.is_empty();
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
                    })
                    // One button for the remote, always in the same place;
                    // it stands out when there's something to pull or push.
                    .when(has_remote, |this| {
                        let sync = Button::new("title-sync");
                        let sync = if behind > 0 || ahead > 0 {
                            sync.primary()
                        } else {
                            sync.outline()
                        };
                        this.child(
                            sync.small()
                                .px_3()
                                .icon(if behind > 0 {
                                    Icon::new(IconName::ArrowDown)
                                } else if ahead > 0 {
                                    Icon::new(IconName::ArrowUp)
                                } else {
                                    Icon::empty().path(icon::SYNC)
                                })
                                .label(match job {
                                    Some(job) => format!("{job}ing…"),
                                    None => sync_label,
                                })
                                .loading(job.is_some())
                                .tooltip(if behind > 0 {
                                    format!(
                                        "{upstream} has {} you don't have yet (⌘⇧S)",
                                        plural(behind, "commit")
                                    )
                                } else {
                                    "Fetch, then pull or push as needed (⌘⇧S)".to_string()
                                })
                                .on_click(cx.listener(|this, _, window, cx| this.sync(window, cx))),
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
            DockPanel::PullRequests => self.render_pull_panel(cx),
            DockPanel::Actions => self.render_run_panel(cx),
            DockPanel::Agents => self.render_agents_panel(cx),
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
        let section = self.section();
        let theme = cx.theme().clone();
        // A page opened in a section gets a slim bar to close it by.
        let page = match self.tab {
            _ if self.tab.panel() != section => None,
            Tab::Pull => self.hub.pull_detail.ready().map_or_else(
                || self.hub.pull.map(|n| format!("#{n}")),
                |d| Some(format!("#{} {}", d.pull.number, d.pull.title)),
            ),
            Tab::Run => self.hub.run_detail.ready().map_or_else(
                || Some("Run".to_string()),
                |d| Some(format!("{} #{}", d.run.workflow, d.run.number)),
            ),
            Tab::Task => self.agents.open_task().map(|v| v.task.title().to_string()),
            _ => None,
        }
        .map(|label| {
            let (path, parent) = match self.tab {
                Tab::Pull => (icon::GIT_PULL_REQUEST, "Pull Requests"),
                Tab::Run => (icon::ACTIONS, "Actions"),
                _ => (icon::BOT, "Agents"),
            };
            h_flex()
                .h(px(40.))
                .flex_none()
                .pl_4()
                .pr_2()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .text_sm()
                .child(
                    Icon::empty()
                        .path(path)
                        .small()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(parent),
                )
                .child(div().text_color(theme.muted_foreground).child("/"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_medium()
                        .child(label),
                )
                .child(
                    Button::new("close-page")
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Close")
                        .on_click(cx.listener(|this, _, _, cx| match this.tab {
                            Tab::Task => this.close_task_page(cx),
                            _ => this.close_page(cx),
                        })),
                )
        });
        let banner = self.render_operation_banner(cx);
        let content = if self.tab.panel() != section {
            // The section's list is open, but nothing from it is yet.
            match section {
                DockPanel::PullRequests => empty_state(
                    icon::GIT_PULL_REQUEST,
                    "No pull request open",
                    "Pick one from the list to see its checks, files and conversation",
                    cx,
                ),
                DockPanel::Actions => empty_state(
                    icon::ACTIONS,
                    "No run open",
                    "Pick a run from the list to follow its jobs and logs",
                    cx,
                ),
                _ => empty_state(
                    icon::BOT,
                    "No task open",
                    "Pick a task from the list, or start a new one (⌘⇧T)",
                    cx,
                ),
            }
        } else {
            match self.tab {
                Tab::Changes => self.render_changes(window, cx),
                Tab::History => self.render_history(window, cx),
                Tab::Pull => self.render_pull_page(window, cx),
                Tab::Run => self.render_run_page(window, cx),
                Tab::Rebase => self.render_rebase(window, cx),
                Tab::Task => self.render_task_page(cx),
            }
        };
        v_flex()
            .size_full()
            .min_w_0()
            .children(page)
            .children(banner)
            .child(div().flex_1().min_h_0().child(content))
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
