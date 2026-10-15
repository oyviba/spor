//! The Uncommitted Changes tab: every changed file in one scrolling diff,
//! like Zed's project diff. The git panel's file list scrolls this view.

use super::app::*;
use super::assets::icon;
use super::support::plural;
use gpui_kit::component::{
    diff::Diff, h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _,
};
use gpui_kit::{div, AnyElement, Context, IntoElement, ParentElement as _, Styled as _, Window};

/// Centered message for an empty pane.
pub(crate) fn empty_state(
    path: &'static str,
    title: &'static str,
    hint: &'static str,
    cx: &gpui_kit::App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            Icon::empty()
                .path(path)
                .large()
                .text_color(theme.muted_foreground.opacity(0.5)),
        )
        .child(
            div()
                .text_sm()
                .font_medium()
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground.opacity(0.7))
                .child(hint),
        )
        .into_any_element()
}

impl Spor {
    pub(crate) fn render_changes(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let files = self.changes_diff.read(cx).files();
        if files.is_empty() {
            return empty_state(
                icon::LIST_CHECKS,
                "No uncommitted changes",
                "Edits to tracked files and new files show up here",
                cx,
            );
        }
        let (adds, dels) = files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions(), d + f.deletions()));
        let count = files.len();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(gpui_kit::px(32.))
                    .flex_none()
                    .px_3()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(plural(count, "file"))
                    .child(div().text_color(theme.green).child(format!("+{adds}")))
                    .child(div().text_color(theme.red).child(format!("−{dels}"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(Diff::new(&self.changes_diff).line_number(true)),
            )
            .into_any_element()
    }
}
