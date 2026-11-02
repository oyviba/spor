//! Actions: recent workflow runs in the dock (every branch, or just the
//! checked-out one), and the open run in the center — what it ran for,
//! re-run and cancel, its jobs with their steps, and the selected job's log.

use super::app::*;
use super::assets::icon;
use super::changes_view::empty_state;
use super::hub::{now, Load};
use super::pulls::{pending_state, status_icon};
use super::support::relative_time;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, uniform_list, AnyElement, Context,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use spor::github::{self, Job, LogKind, Run, RunDetail, Status};
use std::rc::Rc;

fn status_tag(status: Status) -> Tag {
    let tag = match status {
        Status::Success => Tag::success(),
        Status::Failure => Tag::danger(),
        Status::Queued | Status::Running => Tag::warning(),
        _ => Tag::secondary(),
    };
    tag.small().child(status.label())
}

/// How long a run took, or has been going.
fn run_took(run: &Run, now: i64) -> String {
    let end = if run.status.active() {
        now
    } else {
        run.updated
    };
    github::duration(end - run.created)
}

impl Spor {
    /// The dock panel: recent workflow runs on every branch or the
    /// checked-out one. Picking one opens it in the center.
    pub(crate) fn render_run_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let this_branch = self.hub.runs_this_branch;
        let branch = self.repo.as_ref().and_then(|r| r.tracking.branch.clone());
        let refreshing = matches!(self.hub.runs, Load::Loading);
        let selected = self.hub.run.filter(|_| self.tab == Tab::Run);
        let now = now();
        let spor = Self::weak(cx);

        let scope = match (&branch, this_branch) {
            (Some(b), true) => format!("Runs on {b}"),
            _ => "Runs on All Branches".to_string(),
        };
        let header = h_flex()
            .h(px(36.))
            .flex_none()
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("runs-scope")
                    .ghost()
                    .xsmall()
                    .label(scope)
                    .dropdown_caret(true)
                    .dropdown_menu(move |menu, _, _| {
                        let all = spor.clone();
                        let mine = spor.clone();
                        let menu = menu.item(
                            PopupMenuItem::new("All Branches")
                                .checked(!this_branch)
                                .on_click(move |_, _, cx| {
                                    let _ = all.update(cx, |this, cx| {
                                        this.set_runs_this_branch(false, cx)
                                    });
                                }),
                        );
                        match &branch {
                            Some(b) => menu.item(
                                PopupMenuItem::new(b.clone()).checked(this_branch).on_click(
                                    move |_, _, cx| {
                                        let _ = mine.update(cx, |this, cx| {
                                            this.set_runs_this_branch(true, cx)
                                        });
                                    },
                                ),
                            ),
                            None => menu,
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("runs-refresh")
                    .ghost()
                    .xsmall()
                    .icon(Icon::empty().path(icon::REFRESH))
                    .loading(refreshing)
                    .tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.load_runs(cx))),
            );

        let body = match &self.hub.runs {
            Load::Idle | Load::Loading => pending_state(Some("Loading workflow runs…"), None, cx),
            Load::Failed(e) => pending_state(None, Some(e), cx),
            Load::Ready(list) if list.is_empty() => empty_state(
                icon::ACTIONS,
                "No workflow runs",
                "Runs show up here once a GitHub Actions workflow runs",
                cx,
            ),
            Load::Ready(list) => {
                let rows: Vec<AnyElement> = list
                    .iter()
                    .map(|run| self.render_run_row(run, selected == Some(run.id), now, cx))
                    .collect();
                div()
                    .id("run-list")
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

    fn render_run_row(
        &self,
        run: &Run,
        selected: bool,
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let id = run.id;
        div()
            .id(SharedString::from(format!("run-{id}")))
            .px_3()
            .py_2()
            .cursor_pointer()
            .when(selected, |this| this.bg(theme.list_active))
            .when(!selected, |this| this.hover(|s| s.bg(theme.list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.show_run(id, None, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(div().pt(px(2.)).child(status_icon(run.status, cx)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_sm().truncate().child(run.title.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(format!(
                                        "{} #{} · {} · {}",
                                        run.workflow, run.number, run.branch, run.event
                                    )),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_none()
                            .items_end()
                            .gap_0p5()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(relative_time(run.created))
                            .child(run_took(run, now)),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_run_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = match &self.hub.run_detail {
            Load::Idle => return empty_state(icon::ACTIONS, "No run open", "", cx),
            Load::Loading => return pending_state(Some("Loading run…"), None, cx),
            Load::Failed(e) => return pending_state(None, Some(&e.clone()), cx),
            Load::Ready(d) => d.clone(),
        };
        let header = self.render_run_header(&detail, cx);
        let jobs = self.render_jobs(&detail, cx);
        let log = self.render_log(&detail, window, cx);
        v_flex()
            .size_full()
            .child(header)
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("run-jobs-split")
                        .child(
                            resizable_panel()
                                .size(px(260.))
                                .size_range(px(180.)..px(480.))
                                .child(jobs),
                        )
                        .child(resizable_panel().size_range(px(240.)..px(4000.)).child(log)),
                ),
            )
            .into_any_element()
    }

    fn render_run_header(&self, detail: &RunDetail, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let run = &detail.run;
        let id = run.id;
        let url = run.url.clone();
        let busy = self.hub.busy;
        let active = run.status.active();
        let failed = matches!(run.status, Status::Failure | Status::Cancelled);
        let sha: String = run.sha.chars().take(7).collect();
        let attempt = (run.attempt > 1).then(|| format!(" · attempt {}", run.attempt));

        v_flex()
            .flex_none()
            .px_4()
            .pt_3()
            .pb_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .when_some(self.hub.run_back, |this, number| {
                        this.child(
                            Button::new("run-back")
                                .ghost()
                                .small()
                                .icon(IconName::ArrowLeft)
                                .label(format!("#{number}"))
                                .tooltip("Back to the pull request")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.show_pull(number, cx)),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_medium()
                            .truncate()
                            .child(run.title.clone()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(status_tag(run.status))
                    .child(div().min_w_0().truncate().child(format!(
                        "{} #{}{} · {} · {sha} · {} · {} · took {}",
                        run.workflow,
                        run.number,
                        attempt.unwrap_or_default(),
                        run.branch,
                        run.event,
                        relative_time(run.created),
                        run_took(run, now()),
                    ))),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(failed, |this| {
                        this.child(
                            Button::new("run-rerun-failed")
                                .primary()
                                .small()
                                .label("Re-run Failed Jobs")
                                .icon(Icon::empty().path(icon::RERUN))
                                .loading(busy == Some("Re-running"))
                                .disabled(busy.is_some())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.rerun(id, true, window, cx)
                                })),
                        )
                    })
                    .when(!active, |this| {
                        this.child(
                            Button::new("run-rerun-all")
                                .outline()
                                .small()
                                .label("Re-run All Jobs")
                                .when(!failed, |b| b.icon(Icon::empty().path(icon::RERUN)))
                                .loading(!failed && busy == Some("Re-running"))
                                .disabled(busy.is_some())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.rerun(id, false, window, cx)
                                })),
                        )
                    })
                    .when(active, |this| {
                        this.child(
                            Button::new("run-cancel")
                                .outline()
                                .small()
                                .label("Cancel Run")
                                .icon(Icon::empty().path(icon::CHECK_CANCELLED))
                                .loading(busy == Some("Cancelling"))
                                .disabled(busy.is_some())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.cancel_run(id, window, cx)
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("run-web")
                            .ghost()
                            .small()
                            .label("Open on GitHub")
                            .icon(Icon::empty().path(icon::EXTERNAL_LINK))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            )
            .into_any_element()
    }

    /// The run's jobs; the selected one lists its steps underneath.
    fn render_jobs(&self, detail: &RunDetail, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if detail.jobs.is_empty() {
            return pending_state(None, Some("This run has no jobs yet"), cx);
        }
        let now = now();
        let selected = self.hub.job;
        let mut items: Vec<AnyElement> = Vec::new();
        for job in &detail.jobs {
            let id = job.id;
            let is_selected = selected == Some(id);
            items.push(
                h_flex()
                    .id(SharedString::from(format!("job-{id}")))
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .cursor_pointer()
                    .when(is_selected, |this| this.bg(theme.list_active))
                    .when(!is_selected, |this| this.hover(|s| s.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_job(id, cx)))
                    .child(status_icon(job.status, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .child(job.name.clone()),
                    )
                    .children(job.elapsed(now).map(|secs| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(github::duration(secs))
                    }))
                    .into_any_element(),
            );
            if is_selected {
                for step in &job.steps {
                    items.push(
                        h_flex()
                            .pl(px(34.))
                            .pr_3()
                            .py_0p5()
                            .gap_2()
                            .text_xs()
                            .text_color(if step.status == Status::Skipped {
                                theme.muted_foreground
                            } else {
                                theme.foreground
                            })
                            .child(status_icon(step.status, cx).xsmall())
                            .child(div().min_w_0().truncate().child(step.name.clone()))
                            .into_any_element(),
                    );
                }
            }
        }
        div()
            .id("job-list")
            .size_full()
            .py_1()
            .border_r_1()
            .border_color(theme.border)
            .overflow_y_scroll()
            .children(items)
            .into_any_element()
    }

    fn render_log(
        &self,
        detail: &RunDetail,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(job) = self
            .hub
            .job
            .and_then(|id| detail.jobs.iter().find(|j| j.id == id))
            .cloned()
        else {
            return pending_state(None, Some("Pick a job to see its log"), cx);
        };
        let url = job.url.clone();
        let header = h_flex()
            .h(px(32.))
            .flex_none()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(status_icon(job.status, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_medium()
                    .truncate()
                    .child(job.name.clone()),
            )
            .when(!url.is_empty(), |this| {
                this.child(
                    Button::new("job-web")
                        .ghost()
                        .xsmall()
                        .icon(Icon::empty().path(icon::EXTERNAL_LINK))
                        .tooltip("Open this job on GitHub")
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            });
        let body = if job.status.active() {
            live_job(&job, cx)
        } else {
            match &self.hub.log {
                Load::Idle | Load::Loading => pending_state(Some("Loading the log…"), None, cx),
                Load::Failed(e) => pending_state(None, Some(e), cx),
                Load::Ready(lines) => self.render_log_lines(lines.clone(), cx),
            }
        };
        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn render_log_lines(
        &self,
        lines: Rc<Vec<github::LogLine>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        if lines.is_empty() {
            return pending_state(None, Some("The log is empty"), cx);
        }
        let gutter = px(8. * (lines.len().to_string().len() as f32) + 16.);
        uniform_list("job-log", lines.len(), move |range, _, cx| {
            let theme = cx.theme().clone();
            range
                .map(|ix| {
                    let line = &lines[ix];
                    let color = match line.kind {
                        LogKind::Error => theme.red,
                        LogKind::Warning => theme.yellow,
                        LogKind::Notice => theme.blue,
                        LogKind::Command => theme.magenta,
                        LogKind::Group => theme.foreground,
                        LogKind::Plain => theme.foreground.opacity(0.85),
                    };
                    h_flex()
                        .w_full()
                        .h(px(18.))
                        .pr_3()
                        .when(line.kind == LogKind::Group, |this| {
                            this.bg(theme.muted.opacity(0.6)).font_medium()
                        })
                        .when(line.kind == LogKind::Error, |this| {
                            this.bg(theme.red.opacity(0.08))
                        })
                        .child(
                            div()
                                .flex_none()
                                .w(gutter)
                                .pr_2()
                                .text_right()
                                .text_color(theme.muted_foreground.opacity(0.6))
                                .child((ix + 1).to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(color)
                                .child(line.text.clone()),
                        )
                })
                .collect()
        })
        .track_scroll(&self.hub.log_scroll)
        .size_full()
        .py_1()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .into_any_element()
    }
}

/// A job still going: its steps so far, since GitHub serves the log only
/// once the job finishes.
fn live_job(job: &Job, cx: &gpui_kit::App) -> AnyElement {
    let theme = cx.theme();
    let current = job
        .steps
        .iter()
        .find(|s| s.status == Status::Running)
        .map(|s| s.name.clone());
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(status_icon(job.status, cx).large())
        .child(match current {
            Some(step) => format!("Running “{step}”"),
            None => job.status.label().to_string(),
        })
        .child(
            div()
                .text_xs()
                .child("The log shows up here when the job finishes"),
        )
        .into_any_element()
}
