//! The History tab: a dense graph table on top (graph, subject with ref
//! tags, author, date, SHA) and the selected commit underneath — its header
//! and every changed file in one diff.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::graph::{self, GraphColors};
use super::support::{plural, relative_time};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::Diff,
    h_flex,
    menu::{ContextMenuExt as _, PopupMenuItem},
    resizable::{resizable_panel, v_resizable},
    scroll::Scrollbar,
    v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    canvas, div, prelude::FluentBuilder as _, px, uniform_list, AnyElement, App, ClickEvent,
    Context, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use spor::graph::GraphRow;
use spor::remote::{ChecksState, PrInfo};
use std::collections::HashMap;
use std::rc::Rc;

const ROW_HEIGHT: f32 = 26.0;
const AUTHOR_W: f32 = 150.0;
const DATE_W: f32 = 110.0;
const SHA_W: f32 = 70.0;

/// One ref label on a commit row.
struct RefTag {
    text: String,
    head: bool,
    remote_only: bool,
}

/// A local branch and its remote twin on the same commit share one tag.
fn ref_tags(row: &GraphRow, remotes: &[String], prs: &HashMap<String, PrInfo>) -> Vec<RefTag> {
    let refs = &row.commit.refs;
    let remote_rest = |r: &str| -> Option<String> {
        let (remote, rest) = r.split_once('/')?;
        remotes
            .iter()
            .any(|x| x == remote)
            .then(|| rest.to_string())
    };
    let mut tags = Vec::new();
    for r in refs {
        if let Some(t) = r.strip_prefix("tag:") {
            tags.push(RefTag {
                text: t.trim().to_string(),
                head: false,
                remote_only: false,
            });
            continue;
        }
        let rest = remote_rest(r);
        if let Some(rest) = &rest {
            if rest == "HEAD" || refs.iter().any(|o| o == rest) {
                continue;
            }
        }
        let mut text = r.clone();
        if let Some(pr) = prs.get(rest.as_deref().unwrap_or(r)) {
            let mark = match pr.checks {
                ChecksState::Passing => " ✓",
                ChecksState::Failing => " ✗",
                ChecksState::Pending => " ●",
                ChecksState::None => "",
            };
            text.push_str(&format!(" #{}{mark}", pr.number));
        }
        let head = row.commit.head_ref.as_deref() == Some(r.as_str());
        let tag = RefTag {
            text,
            head,
            remote_only: rest.is_some(),
        };
        if head {
            tags.insert(0, tag);
        } else {
            tags.push(tag);
        }
    }
    tags
}

impl Spor {
    pub(crate) fn render_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(repo) = &self.repo else {
            return div().into_any_element();
        };
        if repo.rows.is_empty() {
            return empty_state(
                icon::GIT_COMMIT,
                "No commits yet",
                "Make the first commit from the git panel",
                cx,
            );
        }
        let list = self.render_commit_list(window, cx);
        let detail = self.render_commit_detail(window, cx);
        v_resizable("history-split")
            .child(
                resizable_panel()
                    .size(px(320.))
                    .size_range(px(120.)..px(4000.))
                    .child(list),
            )
            .child(
                resizable_panel()
                    .size_range(px(160.)..px(4000.))
                    .child(detail),
            )
            .into_any_element()
    }

    fn render_commit_list(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(repo) = &self.repo else {
            return div().into_any_element();
        };
        let rows = Rc::new(repo.rows.clone());
        let remotes = Rc::new(repo.remotes.clone());
        let prs = Rc::new(self.prs.clone());
        let graph_w = graph::width(repo.max_lanes);
        let selected = self.selected;
        let spor = Self::weak(cx);
        let colors = GraphColors {
            background: theme.background,
            head: theme.primary,
            dark: theme.is_dark(),
        };
        let selected_colors = GraphColors {
            background: theme.list_active,
            ..colors
        };

        let header = h_flex()
            .h(px(26.))
            .flex_none()
            .pr_3()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().w(graph_w).flex_none())
            .child(div().flex_1().child("Description"))
            .child(div().w(px(AUTHOR_W)).flex_none().child("Author"))
            .child(div().w(px(DATE_W)).flex_none().child("Date"))
            .child(div().w(px(SHA_W)).flex_none().child("Commit"));

        let list = uniform_list("commit-rows", rows.len(), move |range, _, cx| {
            let theme = cx.theme().clone();
            range
                .map(|ix| {
                    let row = rows[ix].clone();
                    let is_selected = selected == Some(ix);
                    let tags = ref_tags(&row, &remotes, &prs);
                    let graph_colors = if is_selected { selected_colors } else { colors };
                    let graph_row = row.clone();
                    let click_spor = spor.clone();
                    let menu_spor = spor.clone();
                    let menu_row = row.clone();
                    h_flex()
                        .id(SharedString::from(row.commit.hash.clone()))
                        .h(px(ROW_HEIGHT))
                        .w_full()
                        .pr_3()
                        .text_sm()
                        .when(is_selected, |this| this.bg(theme.list_active))
                        .when(!is_selected, |this| {
                            this.hover(|this| this.bg(theme.list_hover))
                        })
                        .on_click(
                            move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
                                let double = event.click_count() >= 2;
                                let _ = click_spor.update(cx, |this, cx| {
                                    this.select_commit(ix, cx);
                                    this.history_focus.focus(window, cx);
                                    if double {
                                        this.checkout_selected(window, cx);
                                    }
                                });
                            },
                        )
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, (), window, _| {
                                    graph::paint_row(bounds, &graph_row, &graph_colors, window)
                                },
                            )
                            .w(graph_w)
                            .h_full()
                            .flex_none(),
                        )
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .children(tags.into_iter().map(|tag| render_tag(tag, &theme)))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .ml_1()
                                        .child(row.commit.subject.clone()),
                                ),
                        )
                        .child(
                            div()
                                .w(px(AUTHOR_W))
                                .flex_none()
                                .truncate()
                                .pr_2()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(row.commit.author.clone()),
                        )
                        .child(
                            div()
                                .w(px(DATE_W))
                                .flex_none()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(relative_time(row.commit.timestamp)),
                        )
                        .child(
                            div()
                                .w(px(SHA_W))
                                .flex_none()
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .child(row.commit.short.clone()),
                        )
                        .context_menu(move |menu, _, _| {
                            commit_menu(menu, &menu_row, menu_spor.clone())
                        })
                        .into_any_element()
                })
                .collect()
        })
        .size_full()
        .track_scroll(&self.history_scroll);

        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("commit-list")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .track_focus(&self.history_focus)
                    .key_context("SporHistory")
                    .child(list)
                    .child(Scrollbar::vertical(&self.history_scroll)),
            )
            .into_any_element()
    }

    fn render_commit_detail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (Some(repo), Some(info)) = (&self.repo, &self.commit) else {
            return empty_state(icon::GIT_COMMIT, "No commit selected", "", cx);
        };
        let Some(row) = repo.rows.iter().find(|r| r.commit.hash == info.hash) else {
            return div().into_any_element();
        };
        let details = &info.details;
        let mut lines = details.message.lines();
        let subject = lines.next().unwrap_or(&row.commit.subject).to_string();
        let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
        let files = self.commit_diff.read(cx).files();
        let (adds, dels) = files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions(), d + f.deletions()));
        let file_count = files.len();
        let hash = info.hash.clone();
        let short: SharedString = hash[..hash.len().min(10)].to_string().into();
        let parents: Vec<(usize, String)> = row
            .commit
            .parents
            .iter()
            .filter_map(|p| {
                repo.rows
                    .iter()
                    .position(|r| &r.commit.hash == p)
                    .map(|i| (i, p[..p.len().min(7)].to_string()))
            })
            .collect();
        let author = details.author.clone();
        let initials: String = author
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect();

        let header = v_flex()
            .flex_none()
            .px_4()
            .py_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(div().text_base().font_medium().child(subject))
            .when(!body.is_empty(), |this| {
                this.child(
                    div()
                        .id("commit-body")
                        .max_h(px(120.))
                        .overflow_y_scroll()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .whitespace_normal()
                        .child(body),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .size(px(18.))
                            .rounded_full()
                            .bg(avatar_color(&author))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(gpui_kit::white())
                            .text_size(px(9.))
                            .font_medium()
                            .child(initials),
                    )
                    .child(div().text_color(theme.foreground).child(author))
                    .child("·")
                    .child(details.date.clone())
                    .child("·")
                    .child(
                        Button::new("copy-sha")
                            .ghost()
                            .xsmall()
                            .label(short)
                            .icon(Icon::empty().path(icon::COPY))
                            .tooltip("Copy commit SHA")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.copy(hash.clone(), "Commit SHA", window, cx)
                            })),
                    )
                    .children(parents.into_iter().map(|(ix, short)| {
                        Button::new(SharedString::from(format!("parent-{short}")))
                            .ghost()
                            .xsmall()
                            .label(format!("parent {short}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.select_commit(ix, cx)))
                    }))
                    .child(div().flex_1())
                    .child(plural(file_count, "file"))
                    .child(div().text_color(theme.green).child(format!("+{adds}")))
                    .child(div().text_color(theme.red).child(format!("−{dels}"))),
            );

        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(Diff::new(&self.commit_diff).line_number(true)),
            )
            .into_any_element()
    }
}

fn render_tag(tag: RefTag, theme: &gpui_kit::component::Theme) -> AnyElement {
    let (fg, bg, border): (Hsla, Hsla, Hsla) = if tag.head {
        (
            theme.primary,
            theme.primary.opacity(0.15),
            theme.primary.opacity(0.5),
        )
    } else if tag.remote_only {
        (
            theme.muted_foreground,
            gpui_kit::transparent_black(),
            theme.border,
        )
    } else {
        (theme.foreground, theme.muted, theme.border)
    };
    div()
        .flex_none()
        .px_1p5()
        .rounded(px(3.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_xs()
        .text_color(fg)
        .child(tag.text)
        .into_any_element()
}

/// A stable, pleasant color per author name.
fn avatar_color(name: &str) -> Hsla {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    gpui_kit::hsla((h % 360) as f32 / 360.0, 0.45, 0.5, 1.0)
}

fn commit_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    row: &GraphRow,
    spor: gpui_kit::WeakEntity<Spor>,
) -> gpui_kit::component::menu::PopupMenu {
    type Act = Rc<dyn Fn(&mut Spor, &mut Window, &mut Context<Spor>)>;
    let act = |label: String, f: Act| {
        let spor = spor.clone();
        PopupMenuItem::new(label).on_click(move |_, window, cx| {
            let f = f.clone();
            let _ = spor.update(cx, |this, cx| f(this, window, cx));
        })
    };
    let mut menu = menu;
    for r in row
        .commit
        .refs
        .iter()
        .filter(|r| !r.starts_with("tag:") && !r.ends_with("/HEAD"))
    {
        if row.commit.head_ref.as_deref() == Some(r.as_str()) {
            continue;
        }
        let name = r.clone();
        menu = menu.item(act(
            format!("Check Out {r}"),
            Rc::new(move |this, w, cx| this.checkout(&name, w, cx)),
        ));
    }
    let (hash, sha, subject) = (
        row.commit.hash.clone(),
        row.commit.hash.clone(),
        row.commit.subject.clone(),
    );
    menu.item(act(
        "New Branch from Here…".into(),
        Rc::new(move |this, w, cx| this.new_branch(Some(hash.clone()), w, cx)),
    ))
    .separator()
    .item(act(
        "Copy Commit SHA".into(),
        Rc::new(move |this, w, cx| this.copy(sha.clone(), "Commit SHA", w, cx)),
    ))
    .item(act(
        "Copy Message".into(),
        Rc::new(move |this, w, cx| this.copy(subject.clone(), "Message", w, cx)),
    ))
}
