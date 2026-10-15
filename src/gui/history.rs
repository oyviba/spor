//! The History tab: a dense graph table on top (graph, branches and tags,
//! subject, author, date, SHA — columns and sizes set from the options
//! menu) and the selected commit underneath, its header and every changed
//! file in one diff. Dragging a branch label onto another branch's row
//! offers to merge it or open a pull request.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::graph::{self, GraphStyle, MainLane};
use super::history_options::{Density, HistoryOptions, LaneWidth};
use super::support::{plural, relative_time};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::Diff,
    h_flex,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    resizable::{resizable_panel, v_resizable},
    scroll::Scrollbar,
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Icon, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    canvas, div, prelude::FluentBuilder as _, px, uniform_list, AnyElement, App, AppContext as _,
    ClickEvent, Context, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use spor::graph::GraphRow;
use spor::remote::{ChecksState, PrInfo};
use std::collections::HashMap;
use std::rc::Rc;

const AUTHOR_W: f32 = 150.0;
const DATE_W: f32 = 110.0;
const SHA_W: f32 = 70.0;
const BRANCH_W: f32 = 190.0;

/// One label on a commit row: a branch (local, on a remote, or both) or a tag.
#[derive(Clone)]
struct RefTag {
    /// Shown text: the branch name without `origin/`, or the tag name.
    name: String,
    /// The ref to check out, merge or drag: the local branch when there is
    /// one, else the remote-tracking ref.
    target: String,
    local: bool,
    remote: bool,
    tag: bool,
    head: bool,
    default: bool,
    pr: Option<String>,
}

/// What a branch label carries while it's dragged onto another branch.
#[derive(Clone)]
pub(crate) struct DraggedBranch {
    pub name: String,
    pub target: String,
}

/// The label that follows the pointer during a drag.
struct DragPreview(SharedString);

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .px_2()
            .py_0p5()
            .gap_1()
            .rounded(px(4.))
            .bg(theme.primary)
            .text_color(theme.primary_foreground)
            .text_xs()
            .font_medium()
            .shadow_md()
            .child(Icon::empty().path(icon::GIT_BRANCH).xsmall())
            .child(self.0.clone())
    }
}

/// A local branch and its remote twin on the same commit share one label,
/// marked with a laptop and a cloud; the current branch comes first, then
/// the default branch, then the rest, with tags last.
fn ref_tags(
    row: &GraphRow,
    remotes: &[String],
    prs: &HashMap<String, PrInfo>,
    default_branch: Option<&str>,
    opts: &HistoryOptions,
) -> Vec<RefTag> {
    let refs = &row.commit.refs;
    let split_remote = |r: &str| -> Option<(String, String)> {
        let (remote, rest) = r.split_once('/')?;
        remotes
            .iter()
            .any(|x| x == remote)
            .then(|| (remote.to_string(), rest.to_string()))
    };
    let pr_mark = |branch: &str| {
        prs.get(branch).map(|pr| {
            let mark = match pr.checks {
                ChecksState::Passing => " ✓",
                ChecksState::Failing => " ✗",
                ChecksState::Pending => " ●",
                ChecksState::None => "",
            };
            format!("#{}{mark}", pr.number)
        })
    };
    let mut tags = Vec::new();
    for r in refs {
        if let Some(t) = r.strip_prefix("tag:") {
            if opts.tags {
                let name = t.trim().to_string();
                tags.push(RefTag {
                    target: name.clone(),
                    name,
                    local: false,
                    remote: false,
                    tag: true,
                    head: false,
                    default: false,
                    pr: None,
                });
            }
            continue;
        }
        let (name, local, remote) = match split_remote(r) {
            Some((remote, rest)) => {
                if rest == "HEAD" || refs.contains(&rest) || !opts.remote_branches {
                    continue;
                }
                // Other remotes than origin keep their prefix, so a fork's
                // branch isn't mistaken for origin's.
                let name = if remote == "origin" { rest } else { r.clone() };
                (name, false, true)
            }
            None => {
                let on_remote = refs
                    .iter()
                    .any(|o| split_remote(o).is_some_and(|(_, rest)| rest == *r));
                (r.clone(), true, on_remote)
            }
        };
        let branch = split_remote(r).map_or_else(|| r.clone(), |(_, rest)| rest);
        tags.push(RefTag {
            pr: pr_mark(&branch),
            default: default_branch == Some(branch.as_str()),
            head: row.commit.head_ref.as_deref() == Some(r.as_str()),
            name,
            target: r.clone(),
            local,
            remote,
            tag: false,
        });
    }
    tags.sort_by_key(|t| (!t.head, !t.default, t.tag));
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
        let opts = self.history_options;
        let rows = Rc::new(repo.rows.clone());
        let remotes = Rc::new(repo.remotes.clone());
        let prs = Rc::new(self.prs.clone());
        let default_branch = Rc::new(repo.default_branch.clone());
        // Below its tip, lane 0 is the default branch's first-parent chain.
        let main_tip = opts
            .bold_default
            .then(|| {
                let name = repo.default_branch.as_ref()?;
                repo.ref_rows
                    .get(name)
                    .or_else(|| repo.ref_rows.get(&format!("origin/{name}")))
                    .copied()
                    .filter(|&ix| repo.rows[ix].lane == 0)
            })
            .flatten();
        let row_h = opts.density.row_height();
        let graph_w = graph::width(repo.max_lanes, opts.lane_width.px());
        let selected = self.selected;
        let spor = Self::weak(cx);
        let style = GraphStyle {
            background: theme.background,
            head: theme.primary,
            dark: theme.is_dark(),
            lane_width: opts.lane_width.px(),
        };
        let selected_style = GraphStyle {
            background: theme.list_active,
            ..style
        };

        let header = h_flex()
            .relative()
            .h(px(26.))
            .flex_none()
            .pr_3()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().w(graph_w).flex_none())
            .when(opts.branch_column, |this| {
                this.child(div().w(px(BRANCH_W)).flex_none().child("Branch / Tag"))
            })
            .child(div().flex_1().child("Description"))
            .when(opts.author, |this| {
                this.child(div().w(px(AUTHOR_W)).flex_none().child("Author"))
            })
            .when(opts.date, |this| {
                this.child(div().w(px(DATE_W)).flex_none().child("Date"))
            })
            .when(opts.sha, |this| {
                this.child(div().w(px(SHA_W)).flex_none().child("Commit"))
            })
            .child(
                div()
                    .absolute()
                    .right_1()
                    .top(px(3.))
                    .child(self.history_options_menu(cx)),
            );

        let list = uniform_list("commit-rows", rows.len(), move |range, _, cx| {
            let theme = cx.theme().clone();
            range
                .map(|ix| {
                    let row = rows[ix].clone();
                    let is_selected = selected == Some(ix);
                    let tags = ref_tags(&row, &remotes, &prs, default_branch.as_deref(), &opts);
                    // Where a dragged branch lands: the row's first branch.
                    let drop_target = tags.iter().find(|t| !t.tag).cloned();
                    let graph_style = if is_selected { selected_style } else { style };
                    let main = main_tip.map_or(MainLane::default(), |tip| MainLane {
                        top: ix > tip,
                        bottom: ix >= tip,
                    });
                    let graph_row = row.clone();
                    let click_spor = spor.clone();
                    let menu_spor = spor.clone();
                    let drop_spor = spor.clone();
                    let menu_row = row.clone();
                    let hash = row.commit.hash.clone();
                    let labels = if opts.branch_column {
                        let more = tags.len().saturating_sub(1);
                        let names: Vec<String> = tags.iter().map(|t| t.name.clone()).collect();
                        h_flex()
                            .w(px(BRANCH_W))
                            .flex_none()
                            .pr_2()
                            .gap_1()
                            .overflow_hidden()
                            .children(
                                tags.into_iter()
                                    .next()
                                    .map(|t| render_tag(t, &hash, 0, true, &theme)),
                            )
                            .when(more > 0, |this| {
                                this.child(
                                    div()
                                        .id(SharedString::from(format!("more-refs-{hash}")))
                                        .flex_none()
                                        .px_1()
                                        .rounded(px(3.))
                                        .bg(theme.muted)
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("+{more}"))
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(names.join("\n")).build(window, cx)
                                        }),
                                )
                            })
                            .into_any_element()
                    } else {
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .mr_1()
                            .children(
                                tags.into_iter()
                                    .enumerate()
                                    .map(|(i, t)| render_tag(t, &hash, i, false, &theme)),
                            )
                            .into_any_element()
                    };
                    let description = div()
                        .min_w_0()
                        .truncate()
                        .ml_1()
                        .child(row.commit.subject.clone());
                    h_flex()
                        .id(SharedString::from(row.commit.hash.clone()))
                        .h(px(row_h))
                        .w_full()
                        .pr_3()
                        .text_sm()
                        .when(is_selected, |this| this.bg(theme.list_active))
                        .when(!is_selected, |this| {
                            this.hover(|this| this.bg(theme.list_hover))
                        })
                        .when_some(drop_target, |this, target| {
                            this.drag_over::<DraggedBranch>(|style, _, _, cx| {
                                style.bg(cx.theme().drop_target)
                            })
                            .on_drop(
                                move |dragged: &DraggedBranch,
                                      window: &mut Window,
                                      cx: &mut App| {
                                    let _ = drop_spor.update(cx, |this, cx| {
                                        this.branch_dropped(
                                            &dragged.target,
                                            &target.target,
                                            window,
                                            cx,
                                        )
                                    });
                                },
                            )
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
                                    graph::paint_row(bounds, &graph_row, &graph_style, main, window)
                                },
                            )
                            .w(graph_w)
                            .h_full()
                            .flex_none(),
                        )
                        .map(|this| {
                            if opts.branch_column {
                                this.child(labels)
                                    .child(h_flex().flex_1().min_w_0().child(description))
                            } else {
                                this.child(
                                    h_flex().flex_1().min_w_0().child(labels).child(description),
                                )
                            }
                        })
                        .when(opts.author, |this| {
                            this.child(
                                div()
                                    .w(px(AUTHOR_W))
                                    .flex_none()
                                    .truncate()
                                    .pr_2()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(row.commit.author.clone()),
                            )
                        })
                        .when(opts.date, |this| {
                            this.child(
                                div()
                                    .w(px(DATE_W))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(relative_time(row.commit.timestamp)),
                            )
                        })
                        .when(opts.sha, |this| {
                            this.child(
                                div()
                                    .w(px(SHA_W))
                                    .flex_none()
                                    .text_xs()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_color(theme.muted_foreground)
                                    .child(row.commit.short.clone()),
                            )
                        })
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

    /// The options button at the end of the History header.
    fn history_options_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let spor = Self::weak(cx);
        let opts = self.history_options;
        Button::new("history-options")
            .ghost()
            .xsmall()
            .icon(Icon::empty().path(icon::SETTINGS))
            .tooltip("Graph Options")
            .dropdown_menu(move |menu, window, cx| {
                type Set = fn(&mut HistoryOptions);
                let toggle = |label: &'static str, on: bool, set: Set| {
                    let spor = spor.clone();
                    PopupMenuItem::new(label)
                        .checked(on)
                        .on_click(move |_, _, cx| {
                            let _ =
                                spor.update(cx, |this, cx| this.update_history_options(set, cx));
                        })
                };
                let density_spor = spor.clone();
                let lanes_spor = spor.clone();
                menu.min_w(px(220.))
                    .label("Columns")
                    .item(toggle("Branch / Tag Column", opts.branch_column, |o| {
                        o.branch_column = !o.branch_column
                    }))
                    .item(toggle("Author", opts.author, |o| o.author = !o.author))
                    .item(toggle("Date", opts.date, |o| o.date = !o.date))
                    .item(toggle("Commit", opts.sha, |o| o.sha = !o.sha))
                    .separator()
                    .label("Labels")
                    .item(toggle("Remote-Only Branches", opts.remote_branches, |o| {
                        o.remote_branches = !o.remote_branches
                    }))
                    .item(toggle("Tags", opts.tags, |o| o.tags = !o.tags))
                    .separator()
                    .label("Graph")
                    .item(toggle("Bold Default Branch", opts.bold_default, |o| {
                        o.bold_default = !o.bold_default
                    }))
                    .submenu("Row Height", window, cx, move |mut menu, _, _| {
                        for (density, label) in Density::ALL {
                            let spor = density_spor.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(opts.density == density)
                                    .on_click(move |_, _, cx| {
                                        let _ = spor.update(cx, |this, cx| {
                                            this.history_options.density = density;
                                            this.update_history_options(|_| {}, cx)
                                        });
                                    }),
                            );
                        }
                        menu
                    })
                    .submenu("Lane Width", window, cx, move |mut menu, _, _| {
                        for (width, label) in LaneWidth::ALL {
                            let spor = lanes_spor.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(opts.lane_width == width)
                                    .on_click(move |_, _, cx| {
                                        let _ = spor.update(cx, |this, cx| {
                                            this.history_options.lane_width = width;
                                            this.update_history_options(|_| {}, cx)
                                        });
                                    }),
                            );
                        }
                        menu
                    })
                    .separator()
                    .item({
                        let spor = spor.clone();
                        PopupMenuItem::new("Reset to Defaults").on_click(move |_, _, cx| {
                            let _ = spor.update(cx, |this, cx| {
                                this.history_options = HistoryOptions::default();
                                this.update_history_options(|_| {}, cx)
                            });
                        })
                    })
            })
    }

    fn update_history_options(&mut self, set: fn(&mut HistoryOptions), cx: &mut Context<Self>) {
        set(&mut self.history_options);
        self.history_options.save();
        cx.notify();
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

/// A ref label. Branches show where they live — a laptop for local, a
/// cloud for a remote — and can be dragged onto another branch's row.
fn render_tag(
    tag: RefTag,
    hash: &str,
    ix: usize,
    shrink: bool,
    theme: &gpui_kit::component::Theme,
) -> AnyElement {
    let remote_only = tag.remote && !tag.local;
    let (fg, bg, border): (Hsla, Hsla, Hsla) = if tag.default && !remote_only {
        (theme.primary_foreground, theme.primary, theme.primary)
    } else if tag.default {
        (theme.primary, gpui_kit::transparent_black(), theme.primary)
    } else if tag.head {
        (
            theme.primary,
            theme.primary.opacity(0.15),
            theme.primary.opacity(0.6),
        )
    } else if remote_only {
        (
            theme.muted_foreground,
            gpui_kit::transparent_black(),
            theme.border,
        )
    } else {
        (theme.foreground, theme.muted, theme.border)
    };
    let icon_color = fg.opacity(0.75);
    let mut label = h_flex()
        .id(SharedString::from(format!("ref-{hash}-{ix}")))
        .flex_none()
        .when(shrink, |this| this.flex_shrink(1.).min_w_0())
        .gap_1()
        .px_1p5()
        .rounded(px(3.))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_xs()
        .text_color(fg)
        .when(tag.default || tag.head, |this| this.font_semibold())
        // A ring marks the checked-out branch even when it's the default.
        .when(tag.head && tag.default, |this| {
            this.border_2().border_color(theme.foreground.opacity(0.5))
        })
        .when(tag.tag, |this| {
            this.child(
                Icon::empty()
                    .path(icon::TAG)
                    .xsmall()
                    .text_color(icon_color),
            )
        })
        .when(tag.local, |this| {
            this.child(
                Icon::empty()
                    .path(icon::LAPTOP)
                    .xsmall()
                    .text_color(icon_color),
            )
        })
        .when(tag.remote, |this| {
            this.child(
                Icon::empty()
                    .path(icon::CLOUD)
                    .xsmall()
                    .text_color(icon_color),
            )
        })
        .child(div().min_w_0().truncate().child(tag.name.clone()))
        .when_some(tag.pr.clone(), |this, pr| {
            this.child(div().flex_none().opacity(0.8).child(pr))
        });
    if !tag.tag {
        let dragged = DraggedBranch {
            name: tag.name.clone(),
            target: tag.target.clone(),
        };
        label = label.cursor_grab().on_drag(dragged, |d, _, _, cx| {
            let name = SharedString::from(d.name.clone());
            cx.new(|_| DragPreview(name))
        });
    }
    label.into_any_element()
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
