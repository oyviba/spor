//! The git panel: changed files split into Staged and Unstaged, and the
//! commit message editor with the remote and Commit buttons underneath.
//! Clicking a file moves it to the other list (and shows its diff), so a
//! commit takes exactly the files picked; the eye button only shows the diff.

use super::app::*;
use super::assets::icon;
use super::repo::Change;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Textarea,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, uniform_list, AnyElement, App, Context, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use std::rc::Rc;

#[derive(Clone)]
enum Row {
    Header(&'static str, usize),
    Entry(Change),
}

/// The color Zed gives a file name for its status.
fn status_color(change: &Change, cx: &App) -> Hsla {
    let theme = cx.theme();
    if change.untracked || change.added {
        theme.green
    } else if change.deleted {
        theme.red
    } else if change.orig_path.is_some() {
        theme.blue
    } else {
        theme.yellow
    }
}

/// A checkbox that can also show the mixed state of a partly staged file.
fn checkbox(state: Option<bool>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let on = state != Some(false);
    div()
        .flex_none()
        .size(px(14.))
        .rounded(px(3.))
        .border_1()
        .flex()
        .items_center()
        .justify_center()
        .when(on, |this| {
            this.bg(theme.primary).border_color(theme.primary)
        })
        .when(!on, |this| {
            this.border_color(theme.muted_foreground.opacity(0.6))
        })
        .when_some(
            match state {
                Some(true) => Some(IconName::Check),
                None => Some(IconName::Minus),
                Some(false) => None,
            },
            |this, name| {
                this.child(
                    Icon::new(name)
                        .size(px(11.))
                        .text_color(theme.primary_foreground),
                )
            },
        )
}

impl Spor {
    pub(crate) fn render_git_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(repo) = &self.repo else {
            return div().into_any_element();
        };
        let changes = repo.changes.clone();
        let all_staged = !changes.is_empty() && changes.iter().all(|c| c.included());
        let staged = repo.staged_count();
        let has_remote = !repo.remotes.is_empty();
        // A partly staged file stays under Unstaged, with a mixed checkbox:
        // clicking it stages the rest.
        let (staged_rows, unstaged_rows): (Vec<Change>, Vec<Change>) =
            changes.iter().cloned().partition(|c| c.included());
        let mut rows = Vec::new();
        if !staged_rows.is_empty() {
            rows.push(Row::Header("Staged", staged_rows.len()));
            rows.extend(staged_rows.into_iter().map(Row::Entry));
        }
        if !unstaged_rows.is_empty() {
            rows.push(Row::Header("Unstaged", unstaged_rows.len()));
            rows.extend(unstaged_rows.into_iter().map(Row::Entry));
        }
        let rows = Rc::new(rows);
        let spor = Self::weak(cx);
        let message_empty = self.message.read(cx).value().trim().is_empty();
        let job = self.job;
        let sync_label = self.sync_label();

        let header = h_flex()
            .h(px(36.))
            .flex_none()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(div().text_sm().font_medium().child("Changes"))
            .when(!changes.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(changes.len().to_string()),
                )
            })
            .child(div().flex_1())
            .when(!changes.is_empty(), |this| {
                this.child(
                    Button::new("stage-all")
                        .ghost()
                        .xsmall()
                        .label(if all_staged {
                            "Unstage All"
                        } else {
                            "Stage All"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_all_staged(!all_staged, window, cx)
                        })),
                )
            })
            .child({
                let spor = spor.clone();
                let discardable: Vec<Change> =
                    changes.iter().filter(|c| c.unstaged).cloned().collect();
                Button::new("git-panel-menu")
                    .ghost()
                    .xsmall()
                    .icon(Icon::empty().path(icon::ELLIPSIS))
                    .dropdown_menu(move |menu, _, _| {
                        let item = |label: &'static str, f: SporAction| {
                            let spor = spor.clone();
                            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                let f = f.clone();
                                let _ = spor.update(cx, |this, cx| f(this, window, cx));
                            })
                        };
                        let discard = discardable.clone();
                        menu.item(item("Fetch", Rc::new(|this, w, cx| this.fetch(w, cx))))
                            .item(item("Pull", Rc::new(|this, w, cx| this.pull(w, cx))))
                            .item(item("Push", Rc::new(|this, w, cx| this.push(w, cx))))
                            .separator()
                            .item(item(
                                "Open Pull Request",
                                Rc::new(|this, w, cx| this.open_pull_request(w, cx)),
                            ))
                            .separator()
                            .item(item(
                                "Stash All Changes",
                                Rc::new(|this, w, cx| this.stash(w, cx)),
                            ))
                            .item(item(
                                "Discard All Changes",
                                Rc::new(move |this, w, cx| this.discard(&discard, w, cx)),
                            ))
                            .separator()
                            .item(item("Refresh", Rc::new(|this, w, cx| this.reload(w, cx))))
                    })
            });

        let list = if rows.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No changes"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground.opacity(0.7))
                        .child("Your files match the last commit"),
                )
                .into_any_element()
        } else {
            let list_rows = rows.clone();
            let spor = spor.clone();
            div()
                .id("git-entries")
                .flex_1()
                .min_h_0()
                .child(
                    uniform_list("git-entry-list", rows.len(), move |range, _, cx| {
                        range
                            .map(|ix| render_row(&list_rows[ix], ix, spor.clone(), cx))
                            .collect()
                    })
                    .size_full()
                    .py_1(),
                )
                .into_any_element()
        };

        let can_commit = staged > 0 && !message_empty;
        let commit_label = match staged {
            0 => "Commit".to_string(),
            1 => "Commit 1 File".to_string(),
            n => format!("Commit {n} Files"),
        };
        let footer = v_flex()
            .flex_none()
            .p_2()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .key_context("CommitEditor")
                    .child(Textarea::new(&self.message)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(has_remote, |this| {
                        this.child(
                            Button::new("sync")
                                .ghost()
                                .small()
                                .icon(Icon::empty().path(icon::SYNC))
                                .label(sync_label)
                                .loading(job.is_some())
                                .disabled(job.is_some())
                                .tooltip("Fetch, then pull or push as needed (⌘⇧S)")
                                .on_click(cx.listener(|this, _, window, cx| this.sync(window, cx))),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("commit")
                            .primary()
                            .small()
                            .label(commit_label)
                            .disabled(!can_commit)
                            .tooltip(if staged == 0 {
                                "Click files to stage them for the commit"
                            } else if message_empty {
                                "Write a commit message first"
                            } else {
                                "Commit (⌘↩)"
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    ),
            );

        let _ = window;
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(header)
            .child(list)
            .child(footer)
            .into_any_element()
    }
}

fn render_row(row: &Row, ix: usize, spor: gpui_kit::WeakEntity<Spor>, cx: &mut App) -> AnyElement {
    let theme = cx.theme().clone();
    match row {
        Row::Header(title, count) => h_flex()
            .id(("git-header", ix))
            .h(px(26.))
            .px_3()
            .gap_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().font_medium().child(*title))
            .child(div().opacity(0.7).child(count.to_string()))
            .into_any_element(),
        Row::Entry(change) => {
            let (name, dir) = match change.path.rsplit_once('/') {
                Some((dir, name)) => (name.to_string(), dir.to_string()),
                None => (change.path.clone(), String::new()),
            };
            let state = if change.included() {
                Some(true)
            } else if change.staged {
                None
            } else {
                Some(false)
            };
            let color = status_color(change, cx);
            let path: SharedString = change.path.clone().into();
            let show = {
                let spor = spor.clone();
                let path = path.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                    cx.stop_propagation();
                    let _ = spor.update(cx, |this, cx| this.show_change(&path, cx));
                }
            };
            let stage_and_show = {
                let spor = spor.clone();
                let change = change.clone();
                move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                    let _ = spor.update(cx, |this, cx| {
                        this.toggle_change(&change, window, cx);
                        this.show_change(&change.path, cx);
                    });
                }
            };
            let group: SharedString = format!("git-row-{}", change.path).into();
            let menu_change = change.clone();
            let menu_spor = spor.clone();
            h_flex()
                .id(SharedString::from(format!("git-entry-{}", change.path)))
                .w_full()
                .h(px(26.))
                .px_3()
                .gap_2()
                .text_sm()
                .group(group.clone())
                .cursor_pointer()
                .hover(|this| this.bg(theme.list_hover))
                .on_click(stage_and_show)
                .child(checkbox(state, cx))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1p5()
                        .child(
                            div()
                                .flex_none()
                                .text_color(color)
                                .when(change.deleted, |this| this.line_through())
                                .child(name),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(dir),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .opacity(0.)
                        .group_hover(group, |this| this.opacity(1.))
                        .child(
                            Button::new(SharedString::from(format!("git-show-{}", change.path)))
                                .ghost()
                                .xsmall()
                                .icon(Icon::empty().path(icon::FILE_DIFF))
                                .tooltip("Show changes")
                                .on_click(show),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(color)
                        .child(change.letter()),
                )
                .context_menu(move |menu, _, _| {
                    let spor = menu_spor.clone();
                    let change = menu_change.clone();
                    let act = |label: String, f: SporAction| {
                        let spor = spor.clone();
                        PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            let f = f.clone();
                            let _ = spor.update(cx, |this, cx| f(this, window, cx));
                        })
                    };
                    let (c1, c2, c3) = (change.clone(), change.clone(), change.clone());
                    let path = change.path.clone();
                    let show_path = path.clone();
                    let mut menu = menu
                        .item(act(
                            if change.included() {
                                "Unstage".into()
                            } else {
                                "Stage".into()
                            },
                            Rc::new(move |this, w, cx| this.toggle_change(&c1, w, cx)),
                        ))
                        .item(act(
                            "Show Changes".into(),
                            Rc::new(move |this, _, cx| this.show_change(&show_path, cx)),
                        ));
                    if change.unstaged {
                        menu = menu.item(act(
                            "Discard Changes".into(),
                            Rc::new(move |this, w, cx| {
                                this.discard(std::slice::from_ref(&c2), w, cx)
                            }),
                        ));
                    }
                    let copy_path = path.clone();
                    menu.separator()
                        .item(act(
                            if cfg!(target_os = "macos") {
                                "Reveal in Finder".into()
                            } else {
                                "Reveal in File Manager".into()
                            },
                            Rc::new(move |this, _, _| this.reveal_in_finder(&c3.path)),
                        ))
                        .item(act(
                            "Copy Path".into(),
                            Rc::new(move |this, w, cx| this.copy(copy_path.clone(), "Path", w, cx)),
                        ))
                })
                .into_any_element()
        }
    }
}
