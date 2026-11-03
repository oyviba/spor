//! The Uncommitted Changes tab: every changed file in one scrolling diff,
//! like Zed's project diff. The git panel's file list scrolls this view.

use super::actions::ChangesMode;
use super::app::*;
use super::assets::icon;
use super::support::plural;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::Diff,
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
    StyledExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, AnyElement, Context, IntoElement, ParentElement as _,
    Styled as _, Window,
};
use spor::git::{ConflictSide, LineAction};

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
        if self.conflict_path.is_some() {
            return self.render_conflict(cx);
        }
        let theme = cx.theme().clone();
        let mode = self.changes_mode;
        let has_changes = self.repo.as_ref().is_some_and(|r| !r.changes.is_empty());
        let files = self.changes_diff.read(cx).files();
        if !has_changes {
            return empty_state(
                icon::LIST_CHECKS,
                "No uncommitted changes",
                "Edits to tracked files and new files show up here",
                cx,
            );
        }
        let selection =
            self.line_selection.is_some() || self.changes_diff.read(cx).selected_lines().is_some();
        let mode_button = |id: &'static str, label: &'static str, m: ChangesMode| {
            Button::new(id)
                .ghost()
                .small()
                .label(label)
                .selected(mode == m)
                .on_click(cx.listener(move |this, _, _, cx| this.set_changes_mode(m, cx)))
        };
        let line_button = |id: &'static str, label: &'static str, action: LineAction| {
            Button::new(id)
                .outline()
                .small()
                .label(label)
                .disabled(!selection)
                .tooltip(if selection {
                    "Applies to the changed lines you selected"
                } else {
                    "Drag over line numbers to select lines"
                })
                .on_click(
                    cx.listener(move |this, _, w, cx| this.apply_selected_lines(action, w, cx)),
                )
        };
        let toolbar = h_flex()
            .flex_none()
            .gap_1()
            .child(mode_button("changes-all", "All", ChangesMode::All))
            .child(mode_button(
                "changes-unstaged",
                "Unstaged",
                ChangesMode::Unstaged,
            ))
            .child(mode_button("changes-staged", "Staged", ChangesMode::Staged))
            .child(div().w(gpui_kit::px(8.)))
            .when(mode == ChangesMode::Unstaged, |this| {
                this.child(line_button("stage-lines", "Stage Lines", LineAction::Stage))
                    .child(line_button(
                        "discard-lines",
                        "Discard Lines",
                        LineAction::Discard,
                    ))
            })
            .when(mode == ChangesMode::Staged, |this| {
                this.child(line_button(
                    "unstage-lines",
                    "Unstage Lines",
                    LineAction::Unstage,
                ))
            })
            .when(mode == ChangesMode::All && selection, |this| {
                this.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child("Switch to Unstaged or Staged to act on lines"),
                )
            });
        if files.is_empty() {
            return v_flex()
                .size_full()
                .child(
                    h_flex()
                        .h(gpui_kit::px(40.))
                        .flex_none()
                        .px_3()
                        .border_b_1()
                        .border_color(theme.border)
                        .text_xs()
                        .child(toolbar),
                )
                .child(empty_state(
                    icon::LIST_CHECKS,
                    if mode == ChangesMode::Staged {
                        "Nothing staged"
                    } else {
                        "Nothing unstaged"
                    },
                    "Click a file in the git panel, or select lines here, to move it across",
                    cx,
                ))
                .into_any_element();
        }
        let (adds, dels) = files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions(), d + f.deletions()));
        let count = files.len();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(gpui_kit::px(40.))
                    .flex_none()
                    .px_3()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(toolbar)
                    .child(div().flex_1())
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

impl Spor {
    /// One conflicted file with a choice for each conflict, and ways to
    /// settle the whole file at once.
    fn render_conflict(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(path) = self.conflict_path.clone() else {
            return div().into_any_element();
        };
        let state = self.conflict_diff.read(cx);
        let total = state.files().first().map_or(0, |f| f.conflicts().len());
        let resolved = (0..total)
            .filter(|&ix| state.conflict_resolution(&path, ix).is_some())
            .count();
        let ready = state.resolved_text(&path).is_some();
        let (ours, theirs) = (path.clone(), path.clone());
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(gpui_kit::px(40.))
                    .flex_none()
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_xs()
                    .child(
                        Icon::empty()
                            .path(icon::GIT_MERGE)
                            .small()
                            .text_color(theme.warning),
                    )
                    .child(div().text_sm().font_medium().child(path.clone()))
                    .child(div().text_color(theme.muted_foreground).child(format!(
                        "{resolved} of {} resolved",
                        plural(total, "conflict")
                    )))
                    .child(div().flex_1())
                    .child(
                        Button::new("conflict-ours")
                            .ghost()
                            .small()
                            .label("Use Current")
                            .tooltip("Keep the checked-out branch's version of the whole file")
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.resolve_file(&ours, ConflictSide::Ours, w, cx)
                            })),
                    )
                    .child(
                        Button::new("conflict-theirs")
                            .ghost()
                            .small()
                            .label("Use Incoming")
                            .tooltip("Keep the incoming version of the whole file")
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.resolve_file(&theirs, ConflictSide::Theirs, w, cx)
                            })),
                    )
                    .child(
                        Button::new("conflict-close")
                            .ghost()
                            .small()
                            .label("Close")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.conflict_path = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("conflict-save")
                            .primary()
                            .small()
                            .label("Save and Mark Resolved")
                            .disabled(!ready)
                            .tooltip(if ready {
                                "Write the file with your choices and stage it"
                            } else {
                                "Pick a side for every conflict first"
                            })
                            .on_click(cx.listener(|this, _, w, cx| this.save_resolution(w, cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(Diff::new(&self.conflict_diff).line_number(true)),
            )
            .into_any_element()
    }
}
