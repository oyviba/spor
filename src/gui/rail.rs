//! The workspace rail, like Slack's: one tile per workspace down the left
//! edge, the active one marked, ⌘1–⌘9 to jump, + to add one.

use super::app::*;
use super::workspaces::COLORS;
use gpui_kit::component::{
    menu::{ContextMenuExt as _, PopupMenuItem},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, hsla, prelude::FluentBuilder as _, px, AnyElement, Context, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};

pub const RAIL_WIDTH: f32 = 56.;
const TILE: f32 = 36.;

/// Tile color for a workspace's color index.
fn tile_color(color: usize) -> Hsla {
    const HUES: [f32; COLORS] = [212., 152., 266., 24., 340., 186., 44., 296.];
    hsla(HUES[color % COLORS] / 360., 0.55, 0.45, 1.)
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
                        .rounded(px(8.))
                        .bg(tile_color(ws.color))
                        .text_color(gpui_kit::white())
                        .text_sm()
                        .font_semibold()
                        .cursor_pointer()
                        .when(selected, |this| {
                            this.border_2().border_color(theme.foreground.opacity(0.85))
                        })
                        .when(!selected, |this| {
                            this.opacity(0.7).hover(|this| this.opacity(1.))
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
                            menu.item(PopupMenuItem::new("Rename…").on_click(
                                move |_, window, cx| {
                                    let _ = rename.update(cx, |this, cx| {
                                        this.rename_workspace(ix, window, cx)
                                    });
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

        v_flex()
            .flex_none()
            .w(px(RAIL_WIDTH))
            .h_full()
            .py_2()
            .gap_2()
            .items_center()
            .bg(theme.title_bar)
            .border_r_1()
            .border_color(theme.border)
            .children(tiles)
            .child(
                div()
                    .id("new-workspace")
                    .size(px(TILE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .border_1()
                    .border_dashed()
                    .border_color(theme.muted_foreground.opacity(0.5))
                    .text_color(theme.muted_foreground)
                    .cursor_pointer()
                    .hover(|this| this.bg(theme.list_hover).text_color(theme.foreground))
                    .child(Icon::new(IconName::Plus).small())
                    .tooltip(|window, cx| {
                        Tooltip::new("New Workspace…")
                            .action(&NewWorkspace, Some("Spor"))
                            .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.new_workspace(window, cx))),
            )
            .into_any_element()
    }
}
