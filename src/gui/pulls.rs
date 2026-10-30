//! The Pull Requests tab: the repository's pull requests on the left
//! (Open, Yours, Review Requested, Closed), the selected one on the right —
//! its header and actions (check out, approve, merge, close), then its
//! conversation, its checks, and its files.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::hub::{now, Load, PullTab};
use super::support::{plural, relative_time};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    diff::Diff,
    h_flex,
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    spinner::Spinner,
    tab::{Tab as TabItem, TabBar},
    tag::Tag,
    text::TextView,
    v_flex, ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
    StyledExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, Context, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use spor::github::{
    self, Check, Comment, MergeMethod, Mergeable, Pull, PullDetail, PullFilter, Status, Verdict,
};
use spor::remote::{ChecksState, PrState, ReviewState};

/// A check, run, job or step's status as a colored circle.
pub(crate) fn status_icon(status: Status, cx: &App) -> Icon {
    let theme = cx.theme();
    let (path, color) = match status {
        Status::Success => (icon::CHECK_PASSED, theme.green),
        Status::Failure => (icon::CHECK_FAILED, theme.red),
        Status::Running => (icon::CHECK_RUNNING, theme.yellow),
        Status::Queued => (icon::CHECK_QUEUED, theme.yellow),
        Status::Cancelled => (icon::CHECK_CANCELLED, theme.muted_foreground),
        Status::Skipped | Status::Neutral => (icon::CHECK_SKIPPED, theme.muted_foreground),
    };
    Icon::empty().path(path).small().text_color(color)
}

fn checks_status(checks: ChecksState) -> Option<Status> {
    match checks {
        ChecksState::Passing => Some(Status::Success),
        ChecksState::Failing => Some(Status::Failure),
        ChecksState::Pending => Some(Status::Running),
        ChecksState::None => None,
    }
}

fn pr_color(state: PrState, cx: &App) -> Hsla {
    let theme = cx.theme();
    match state {
        PrState::Open => theme.green,
        PrState::Draft => theme.muted_foreground,
        PrState::Merged => theme.magenta,
        PrState::Closed => theme.red,
    }
}

fn pr_icon(state: PrState, cx: &App) -> Icon {
    let path = match state {
        PrState::Open => icon::GIT_PULL_REQUEST,
        PrState::Draft => icon::PR_DRAFT,
        PrState::Merged => icon::GIT_MERGE,
        PrState::Closed => icon::PR_CLOSED,
    };
    Icon::empty()
        .path(path)
        .small()
        .text_color(pr_color(state, cx))
}

fn state_tag(state: PrState) -> Tag {
    let (tag, label) = match state {
        PrState::Open => (Tag::success(), "Open"),
        PrState::Draft => (Tag::secondary(), "Draft"),
        PrState::Merged => (Tag::primary(), "Merged"),
        PrState::Closed => (Tag::danger(), "Closed"),
    };
    tag.small().child(label)
}

/// A spinner or a reason, centered, for a pane that has nothing to show yet.
pub(crate) fn pending_state(loading: Option<&str>, error: Option<&str>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .px_6()
        .text_sm()
        .text_color(theme.muted_foreground)
        .when_some(loading, |this, text| {
            this.child(Spinner::new().small()).child(text.to_string())
        })
        .when_some(error, |this, text| {
            this.child(
                Icon::empty()
                    .path(icon::GITHUB)
                    .large()
                    .text_color(theme.muted_foreground.opacity(0.5)),
            )
            .child(div().text_center().child(text.to_string()))
        })
        .into_any_element()
}

/// Initials in a colored circle.
fn avatar(name: &str) -> impl IntoElement {
    let hue = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
        % 360;
    div()
        .flex_none()
        .size(px(20.))
        .rounded_full()
        .bg(gpui_kit::hsla(hue as f32 / 360., 0.45, 0.5, 1.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .text_size(px(9.))
        .font_medium()
        .child(
            name.chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default(),
        )
}

impl Spor {
    pub(crate) fn render_pulls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list = self.render_pull_list(cx);
        let detail = self.render_pull_page(window, cx);
        h_resizable("pulls-split")
            .child(
                resizable_panel()
                    .size(px(380.))
                    .size_range(px(260.)..px(640.))
                    .child(list),
            )
            .child(
                resizable_panel()
                    .size_range(px(360.)..px(4000.))
                    .child(detail),
            )
            .into_any_element()
    }

    fn render_pull_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let filter = self.hub.pull_filter;
        let selected = self.hub.pull;
        let refreshing = matches!(self.hub.pulls, Load::Loading);

        let header = h_flex()
            .h(px(36.))
            .flex_none()
            .px_2()
            .gap_0p5()
            .border_b_1()
            .border_color(theme.border)
            .children(PullFilter::ALL.into_iter().map(|f| {
                Button::new(SharedString::from(format!("pull-filter-{}", f.label())))
                    .ghost()
                    .xsmall()
                    .label(f.label())
                    .selected(f == filter)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_pull_filter(f, cx)))
            }))
            .child(div().flex_1())
            .child(
                Button::new("pulls-refresh")
                    .ghost()
                    .xsmall()
                    .icon(Icon::empty().path(icon::REFRESH))
                    .loading(refreshing)
                    .tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.load_pulls(cx);
                        if let Some(n) = this.hub.pull {
                            this.select_pull_again(n, cx);
                        }
                    })),
            );

        let body = match &self.hub.pulls {
            Load::Idle | Load::Loading => pending_state(Some("Loading pull requests…"), None, cx),
            Load::Failed(e) => pending_state(None, Some(e), cx),
            Load::Ready(list) if list.is_empty() => empty_state(
                icon::GIT_PULL_REQUEST,
                "No pull requests here",
                "Try another filter above",
                cx,
            ),
            Load::Ready(list) => {
                let rows: Vec<AnyElement> = list
                    .iter()
                    .map(|pull| self.render_pull_row(pull, selected == Some(pull.number), cx))
                    .collect();
                div()
                    .id("pull-list")
                    .size_full()
                    .overflow_y_scroll()
                    .children(rows)
                    .into_any_element()
            }
        };

        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn render_pull_row(&self, pull: &Pull, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let number = pull.number;
        let review = match (pull.state, pull.review) {
            (PrState::Open, ReviewState::Approved) => Some(("Approved", theme.green)),
            (PrState::Open, ReviewState::ChangesRequested) => {
                Some(("Changes requested", theme.red))
            }
            _ => None,
        };
        let checks = checks_status(pull.checks).map(|s| status_icon(s, cx));
        div()
            .id(SharedString::from(format!("pull-{number}")))
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .cursor_pointer()
            .when(selected, |this| this.bg(theme.list_active))
            .when(!selected, |this| this.hover(|s| s.bg(theme.list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.select_pull(number, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(div().pt(px(2.)).child(pr_icon(pull.state, cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_sm().truncate().child(pull.title.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(format!(
                                        "#{number} · {} · {} · {}",
                                        pull.head,
                                        pull.author,
                                        relative_time(pull.updated)
                                    )),
                            )
                            .when_some(review, |this, (label, color)| {
                                this.child(div().text_xs().text_color(color).child(label))
                            }),
                    )
                    .children(checks.map(|icon| div().pt(px(2.)).child(icon))),
            )
            .into_any_element()
    }

    /// Reload the selected PR's page even though it's already showing.
    fn select_pull_again(&mut self, number: u64, cx: &mut Context<Self>) {
        self.hub.pull = None;
        self.select_pull(number, cx);
    }

    fn render_pull_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let detail = match &self.hub.pull_detail {
            Load::Idle => {
                return empty_state(
                    icon::GIT_PULL_REQUEST,
                    "No pull request selected",
                    "Pick one from the list",
                    cx,
                )
            }
            Load::Loading => return pending_state(Some("Loading pull request…"), None, cx),
            Load::Failed(e) => return pending_state(None, Some(&e.clone()), cx),
            Load::Ready(d) => d.clone(),
        };
        let header = self.render_pull_header(&detail, cx);
        let tab = self.hub.pull_tab;
        let failing = detail
            .checks
            .iter()
            .filter(|c| c.status == Status::Failure)
            .count();
        let checks_label = if failing > 0 {
            format!("Checks ({failing} failing)")
        } else {
            format!("Checks ({})", detail.checks.len())
        };
        let tabs = TabBar::new("pull-tabs")
            .child(TabItem::new().label(format!("Conversation ({})", detail.timeline.len())))
            .child(TabItem::new().label(checks_label))
            .child(TabItem::new().label(format!("Files ({})", detail.changed_files)))
            .selected_index(match tab {
                PullTab::Conversation => 0,
                PullTab::Checks => 1,
                PullTab::Files => 2,
            })
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                let tab = match ix {
                    0 => PullTab::Conversation,
                    1 => PullTab::Checks,
                    _ => PullTab::Files,
                };
                this.set_pull_tab(tab, cx);
            }));
        let body = match tab {
            PullTab::Conversation => self.render_conversation(&detail, window, cx),
            PullTab::Checks => self.render_checks(&detail, cx),
            PullTab::Files => self.render_pull_files(cx),
        };
        v_flex()
            .size_full()
            .child(header)
            .child(tabs)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn render_pull_header(&self, detail: &PullDetail, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let pull = &detail.pull;
        let number = pull.number;
        let url = pull.url.clone();
        let busy = self.hub.busy;
        let open = matches!(pull.state, PrState::Open | PrState::Draft);
        let checked_out = self
            .repo
            .as_ref()
            .and_then(|r| r.tracking.branch.as_deref())
            == Some(pull.head.as_str());
        let conflicting = detail.mergeable == Mergeable::Conflicting;
        let spor = Self::weak(cx);

        let merge = (pull.state == PrState::Open).then(|| {
            Button::new("pull-merge")
                .primary()
                .small()
                .label("Merge")
                .icon(Icon::empty().path(icon::GIT_MERGE))
                .dropdown_caret(true)
                .loading(busy == Some("Merging"))
                .disabled(conflicting || busy.is_some())
                .tooltip(if conflicting {
                    "This branch has conflicts that must be resolved first"
                } else {
                    "Merge on GitHub"
                })
                .dropdown_menu(move |mut menu, _, _| {
                    for method in MergeMethod::ALL {
                        let spor = spor.clone();
                        menu = menu.item(PopupMenuItem::new(method.label()).on_click(
                            move |_, window, cx| {
                                let _ = spor.update(cx, |this, cx| {
                                    this.merge_pull(number, method, window, cx)
                                });
                            },
                        ));
                    }
                    menu
                })
        });

        let summary = match pull.state {
            PrState::Merged => format!(
                "{} merged into {} from {}",
                pull.author, pull.base, pull.head
            ),
            _ => format!(
                "{} wants to merge {} into {} from {}",
                pull.author,
                plural(detail.commits, "commit"),
                pull.base,
                pull.head
            ),
        };

        v_flex()
            .flex_none()
            .px_4()
            .pt_3()
            .pb_2()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_medium()
                            .truncate()
                            .child(pull.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_lg()
                            .text_color(theme.muted_foreground)
                            .child(format!("#{number}")),
                    )
                    .child(
                        Button::new("pull-web")
                            .ghost()
                            .small()
                            .icon(Icon::empty().path(icon::EXTERNAL_LINK))
                            .tooltip("Open on GitHub")
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(state_tag(pull.state))
                    .child(div().min_w_0().truncate().child(summary))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_color(theme.green)
                            .child(format!("+{}", pull.additions)),
                    )
                    .child(
                        div()
                            .text_color(theme.red)
                            .child(format!("−{}", pull.deletions)),
                    ),
            )
            .when(open && conflicting, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(theme.red)
                        .child("This branch has conflicts with the base branch"),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(merge)
                    .when(pull.state == PrState::Draft, |this| {
                        this.child(
                            Button::new("pull-ready")
                                .primary()
                                .small()
                                .label("Ready for Review")
                                .loading(busy == Some("Updating"))
                                .disabled(busy.is_some())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.mark_ready(number, window, cx)
                                })),
                        )
                    })
                    .when(open && pull.review != ReviewState::Approved, |this| {
                        this.child(
                            Button::new("pull-approve")
                                .outline()
                                .small()
                                .label("Approve")
                                .loading(busy == Some("Approving"))
                                .disabled(busy.is_some())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.approve_pull(number, window, cx)
                                })),
                        )
                    })
                    .when(open && !checked_out, |this| {
                        this.child(
                            Button::new("pull-checkout")
                                .outline()
                                .small()
                                .label("Check Out")
                                .icon(Icon::empty().path(icon::GIT_BRANCH))
                                .loading(busy == Some("Checking out"))
                                .disabled(busy.is_some())
                                .tooltip(format!("Switch to {}", pull.head))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.checkout_pull(number, window, cx)
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .when(pull.state != PrState::Merged, |this| {
                        this.child(
                            Button::new("pull-close")
                                .ghost()
                                .small()
                                .label(if open { "Close" } else { "Reopen" })
                                .loading(matches!(busy, Some("Closing" | "Reopening")))
                                .disabled(busy.is_some())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_pull_open(number, !open, window, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_conversation(
        &self,
        detail: &PullDetail,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let number = detail.pull.number;
        let busy = self.hub.busy;
        let can_comment = !self.hub.comment.read(cx).value().trim().is_empty();

        let description = comment_card(
            &format!("pull-body-{number}"),
            &Comment {
                author: detail.pull.author.clone(),
                body: detail.body.clone(),
                at: 0,
                verdict: None,
            },
            "No description provided.",
            cx,
        );
        let timeline: Vec<AnyElement> = detail
            .timeline
            .iter()
            .enumerate()
            .map(|(ix, c)| comment_card(&format!("pull-{number}-comment-{ix}"), c, "", cx))
            .collect();

        div()
            .id(SharedString::from(format!("conversation-{number}")))
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .child(description)
                    .children(timeline)
                    .child(
                        v_flex()
                            .gap_2()
                            .pt_2()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(Textarea::new(&self.hub.comment))
                            .child(
                                h_flex().child(div().flex_1()).child(
                                    Button::new("pull-comment")
                                        .primary()
                                        .small()
                                        .label("Comment")
                                        .icon(Icon::empty().path(icon::COMMENT))
                                        .loading(busy == Some("Commenting"))
                                        .disabled(!can_comment || busy.is_some())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.comment_on_pull(window, cx)
                                        })),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_checks(&self, detail: &PullDetail, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if detail.checks.is_empty() {
            return empty_state(
                icon::LIST_CHECKS,
                "No checks",
                "Nothing reported a status for this pull request's last commit",
                cx,
            );
        }
        let count =
            |pred: fn(Status) -> bool| detail.checks.iter().filter(|c| pred(c.status)).count();
        let failing = count(|s| s == Status::Failure);
        let running = count(Status::active);
        let passed = count(|s| s == Status::Success);
        let mut summary = Vec::new();
        if failing > 0 {
            summary.push(format!("{failing} failing"));
        }
        if running > 0 {
            summary.push(format!("{running} in progress"));
        }
        if passed > 0 {
            summary.push(format!("{passed} passed"));
        }
        let skipped = detail.checks.len() - failing - running - passed;
        if skipped > 0 {
            summary.push(format!("{skipped} skipped or cancelled"));
        }
        let now = now();
        let rows: Vec<AnyElement> = detail
            .checks
            .iter()
            .enumerate()
            .map(|(ix, check)| self.render_check_row(ix, check, now, cx))
            .collect();
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(summary.join(" · ")),
            )
            .child(
                div()
                    .id("check-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_check_row(
        &self,
        ix: usize,
        check: &Check,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let in_app = check.run_id.is_some();
        // A commit status has a start but no end; only running checks
        // count up to now.
        let end = if check.completed > 0 {
            Some(check.completed)
        } else {
            check.status.active().then_some(now)
        };
        let took = end
            .filter(|_| check.started > 0)
            .map(|end| github::duration(end - check.started));
        let open = check.clone();
        h_flex()
            .id(SharedString::from(format!("check-{ix}")))
            .px_4()
            .py_1p5()
            .gap_2()
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_hover))
            .on_click(cx.listener(move |this, _, _, cx| this.open_check(&open, cx)))
            .child(status_icon(check.status, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .text_sm()
                    .when(!check.workflow.is_empty(), |this| {
                        this.child(
                            div()
                                .flex_none()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} /", check.workflow)),
                        )
                    })
                    .child(div().truncate().child(check.name.clone())),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(match (&took, check.status.active()) {
                        (Some(t), true) => format!("{} · {t}", check.status.label()),
                        (Some(t), false) => t.clone(),
                        (None, _) => check.status.label().to_string(),
                    }),
            )
            .child(
                Icon::empty()
                    .path(if in_app {
                        icon::ACTIONS
                    } else {
                        icon::EXTERNAL_LINK
                    })
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .into_any_element()
    }

    fn render_pull_files(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.hub.pull_diff_load {
            Load::Idle | Load::Loading => pending_state(Some("Loading the diff…"), None, cx),
            Load::Failed(e) => pending_state(None, Some(e), cx),
            Load::Ready(_) if self.hub.pull_diff.read(cx).files().is_empty() => {
                empty_state(icon::FILE_DIFF, "No changes", "", cx)
            }
            Load::Ready(_) => Diff::new(&self.hub.pull_diff)
                .line_number(true)
                .into_any_element(),
        }
    }
}

/// A comment, review or the PR's description: who, when, what they said.
fn comment_card(id: &str, comment: &Comment, empty: &'static str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let verdict = comment.verdict.and_then(|v| match v {
        Verdict::Approved => Some(Tag::success().small().child("Approved")),
        Verdict::ChangesRequested => Some(Tag::danger().small().child("Changes requested")),
        Verdict::Dismissed => Some(Tag::secondary().small().child("Dismissed")),
        Verdict::Commented => None,
    });
    let body = comment.body.trim();
    // A review with nothing to say is just its verdict.
    let bare = body.is_empty() && comment.verdict.is_some();
    v_flex()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(
            h_flex()
                .px_3()
                .py_1p5()
                .gap_2()
                .bg(theme.muted.opacity(0.5))
                .when(!bare, |this| this.border_b_1().border_color(theme.border))
                .text_xs()
                .child(avatar(&comment.author))
                .child(
                    div()
                        .font_medium()
                        .text_color(theme.foreground)
                        .child(comment.author.clone()),
                )
                .children(verdict)
                .when(comment.at > 0, |this| {
                    this.child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(relative_time(comment.at)),
                    )
                }),
        )
        .when(!bare, |this| {
            this.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .when(body.is_empty(), |this| {
                        this.text_color(theme.muted_foreground).child(empty)
                    })
                    .when(!body.is_empty(), |this| {
                        this.child(
                            TextView::markdown(
                                SharedString::from(id.to_string()),
                                body.to_string(),
                            )
                            .selectable(true),
                        )
                    }),
            )
        })
        .into_any_element()
}
