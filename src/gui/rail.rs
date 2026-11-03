//! The navigation rail down the left edge: the workspaces on top, like
//! Slack's (⌘1–⌘9 to jump), then one labeled button per section — Changes,
//! History, Pull Requests, Actions, Agents — and the settings at the bottom.
//! A section opens its panel in the sidebar and its view in the center;
//! clicking the open one again hides the sidebar.

use super::app::*;
use super::assets::icon;
use super::theme::Appearance;
use super::workspaces::COLORS;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::DiffMode,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Icon, Selectable as _, StyledExt as _,
};
use gpui_kit::{
    div, hsla, prelude::FluentBuilder as _, px, Action, Anchor, AnyElement, Context, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};

pub const RAIL_WIDTH: f32 = 72.;
const TILE: f32 = 36.;
const ITEM_WIDTH: f32 = 60.;

/// Tile color for a workspace's color index.
fn tile_color(color: usize) -> Hsla {
    const HUES: [f32; COLORS] = [212., 152., 266., 24., 340., 186., 44., 296.];
    hsla(HUES[color % COLORS] / 360., 0.55, 0.45, 1.)
}

/// One section of the rail.
struct Section {
    panel: DockPanel,
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    tip: &'static str,
    action: Box<dyn Action>,
}

fn sections() -> [Section; 5] {
    [
        Section {
            panel: DockPanel::Git,
            id: "nav-changes",
            icon: icon::FILE_DIFF,
            label: "Changes",
            tip: "Uncommitted changes and the commit",
            action: Box::new(ShowChanges),
        },
        Section {
            panel: DockPanel::Branches,
            id: "nav-history",
            icon: icon::CLOCK,
            label: "History",
            tip: "The commit graph, branches and tags",
            action: Box::new(ShowHistory),
        },
        Section {
            panel: DockPanel::PullRequests,
            id: "nav-pulls",
            icon: icon::GIT_PULL_REQUEST,
            label: "Pulls",
            tip: "Pull requests",
            action: Box::new(ShowPullRequests),
        },
        Section {
            panel: DockPanel::Actions,
            id: "nav-actions",
            icon: icon::ACTIONS,
            label: "Actions",
            tip: "GitHub Actions runs",
            action: Box::new(ShowActions),
        },
        Section {
            panel: DockPanel::Agents,
            id: "nav-agents",
            icon: icon::BOT,
            label: "Agents",
            tip: "Agent tasks, each on its own branch",
            action: Box::new(ShowAgents),
        },
    ]
}

impl Spor {
    pub(crate) fn render_rail(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let spor = Self::weak(cx);
        let active = self.workspaces.active;
        let only_one = self.workspaces.list.len() <= 1;

        let tiles = self.workspaces.list.iter().enumerate().map(|(ix, ws)| {
            let selected = ix == active;
            let name: SharedString = ws.name.clone().into();
            let menu_spor = spor.clone();
            div()
                .relative()
                .w_full()
                .flex()
                .justify_center()
                // Slack's active marker: a pill on the rail's left edge.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(if selected { 4. } else { 14. }))
                        .w(px(3.))
                        .h(px(if selected { TILE - 8. } else { 8. }))
                        .rounded_r(px(2.))
                        .bg(theme.foreground)
                        .when(!selected, |this| this.opacity(0.)),
                )
                .child(
                    div()
                        .id(("workspace", ix))
                        .size(px(TILE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(10.))
                        .bg(tile_color(ws.color))
                        .text_color(gpui_kit::white())
                        .text_sm()
                        .font_semibold()
                        .cursor_pointer()
                        .when(!selected, |this| {
                            this.opacity(0.55).hover(|this| this.opacity(1.))
                        })
                        .child(ws.initials())
                        .tooltip(move |window, cx| {
                            let tip = Tooltip::new(name.clone());
                            match workspace_action(ix) {
                                Some(action) => tip.action(action.as_ref(), Some("Spor")),
                                None => tip,
                            }
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.switch_workspace(ix, window, cx)
                        }))
                        .context_menu(move |menu, _, _| {
                            let rename = menu_spor.clone();
                            let delete = menu_spor.clone();
                            let create = menu_spor.clone();
                            menu.item(PopupMenuItem::new("Rename…").on_click(
                                move |_, window, cx| {
                                    let _ = rename.update(cx, |this, cx| {
                                        this.rename_workspace(ix, window, cx)
                                    });
                                },
                            ))
                            .item(PopupMenuItem::new("New Workspace…").on_click(
                                move |_, window, cx| {
                                    let _ = create
                                        .update(cx, |this, cx| this.new_workspace(window, cx));
                                },
                            ))
                            .separator()
                            .item(
                                PopupMenuItem::new("Delete Workspace…")
                                    .disabled(only_one)
                                    .on_click(move |_, window, cx| {
                                        let _ = delete.update(cx, |this, cx| {
                                            this.delete_workspace(ix, window, cx)
                                        });
                                    }),
                            )
                        }),
                )
        });

        let section = self.section();
        let sidebar_open = self.dock.is_some();
        let changes = self.repo.as_ref().map_or(0, |r| r.changes.len());
        let nav = self.repo.is_some().then(|| {
            v_flex()
                .w_full()
                .items_center()
                .gap_1()
                .children(sections().into_iter().map(|s| {
                    let selected = s.panel == section;
                    let badge = (s.panel == DockPanel::Git && changes > 0).then_some(changes);
                    let panel = s.panel;
                    let (label, tip, action) = (s.label, s.tip, s.action);
                    let fg = if selected {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    };
                    let id = s.id;
                    v_flex()
                        .id(id)
                        .debug_selector(move || id.into())
                        .relative()
                        .w(px(ITEM_WIDTH))
                        .pt(px(8.))
                        .pb(px(6.))
                        .gap(px(4.))
                        .items_center()
                        .rounded(px(10.))
                        .cursor_pointer()
                        .text_color(fg)
                        .when(selected, |this| {
                            // Strong while its panel is open; a hint of it
                            // when the sidebar is hidden.
                            this.bg(if sidebar_open {
                                theme.primary.opacity(0.14)
                            } else {
                                theme.foreground.opacity(0.06)
                            })
                        })
                        .when(!selected, |this| {
                            this.hover(|this| {
                                this.bg(theme.foreground.opacity(0.06))
                                    .text_color(theme.foreground)
                            })
                        })
                        .child(
                            div()
                                .relative()
                                .child(
                                    Icon::empty()
                                        .path(s.icon)
                                        .size(px(20.))
                                        .text_color(if selected { theme.primary } else { fg }),
                                )
                                .when_some(badge, |this, n| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top(px(-6.))
                                            .left(px(12.))
                                            .h(px(16.))
                                            .min_w(px(16.))
                                            .px(px(4.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .bg(theme.primary)
                                            .text_color(theme.primary_foreground)
                                            .text_size(px(10.))
                                            .font_semibold()
                                            .child(if n > 99 {
                                                "99+".into()
                                            } else {
                                                n.to_string()
                                            }),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(13.))
                                .when(selected, |this| this.font_semibold())
                                .when(!selected, |this| this.font_medium())
                                .child(label),
                        )
                        .tooltip(move |window, cx| {
                            Tooltip::new(tip)
                                .action(action.as_ref(), Some("Spor"))
                                .build(window, cx)
                        })
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.go_to(panel, window, cx)),
                        )
                }))
        });

        let show_repos = self.layout.show_repos;
        v_flex()
            .id("nav-rail")
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .pt_3()
            .pb_3()
            .gap_2()
            .items_center()
            .bg(theme.title_bar)
            .border_r_1()
            .border_color(theme.border)
            .children(tiles)
            .when_some(nav, |this, nav| {
                this.child(div().my_1().w(px(28.)).h(px(1.)).bg(theme.border))
                    .child(nav)
            })
            .child(div().flex_1())
            .when(self.repo.is_some(), |this| {
                this.child(
                    Button::new("toggle-repositories")
                        .ghost()
                        .size(px(40.))
                        .child(Icon::empty().path(icon::FOLDER_GIT).size(px(20.)))
                        .selected(show_repos)
                        .tooltip_with_action(
                            "Repositories in this workspace",
                            &ToggleRepositories,
                            Some("Spor"),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_repositories(cx))),
                )
            })
            .child(self.settings_menu(cx))
            .into_any_element()
    }

    /// The gear at the foot of the rail: appearance, layout and help.
    fn settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let spor = Self::weak(cx);
        let appearance = self.appearance;
        let diff_mode = self.diff_mode;
        let dock_side = self.layout.dock_side;
        let has_repo = self.repo.is_some();
        Button::new("settings")
            .ghost()
            .size(px(40.))
            .child(Icon::empty().path(icon::SETTINGS).size(px(20.)))
            .tooltip("Settings")
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, window, cx| {
                let item = |label: &'static str, f: SporAction| {
                    let spor = spor.clone();
                    PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let f = f.clone();
                        let _ = spor.update(cx, |this, cx| f(this, window, cx));
                    })
                };
                let theme_spor = spor.clone();
                menu = menu.min_w(px(240.)).label("Appearance");
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
                                let _ = spor
                                    .update(cx, |this, cx| this.set_appearance(mode, window, cx));
                            }),
                    );
                }
                menu = menu.item(item(
                    "Edit Theme…",
                    std::rc::Rc::new(|this, w, cx| this.open_theme_editor(w, cx)),
                ));
                if has_repo {
                    menu = menu.separator().label("Layout");
                    for (mode, label) in [
                        (DiffMode::Unified, "Unified Diffs"),
                        (DiffMode::Split, "Side-by-Side Diffs"),
                    ] {
                        let spor = spor.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(match diff_mode {
                                    DiffMode::Unified => mode == DiffMode::Unified,
                                    _ => mode == DiffMode::Split,
                                })
                                .on_click(move |_, _, cx| {
                                    let _ =
                                        spor.update(cx, |this, cx| this.set_diff_mode(mode, cx));
                                }),
                        );
                    }
                    menu = menu.item(item(
                        match dock_side {
                            DockSide::Left => "Sidebar on the Right",
                            DockSide::Right => "Sidebar on the Left",
                        },
                        std::rc::Rc::new(|this, _, cx| this.toggle_dock_side(cx)),
                    ));
                }
                let _ = (window, cx);
                menu.separator()
                    .item(item(
                        "New Workspace…",
                        std::rc::Rc::new(|this, w, cx| this.new_workspace(w, cx)),
                    ))
                    .item(item(
                        "Command Palette… (⌘P)",
                        std::rc::Rc::new(|this, w, cx| this.open_palette(w, cx)),
                    ))
                    .item(item(
                        "Keyboard Shortcuts (⌘/)",
                        std::rc::Rc::new(|this, w, cx| this.open_shortcuts(w, cx)),
                    ))
            })
    }
}
