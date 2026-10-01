//! The top bar: repository and branch switchers on the left, the main git
//! actions in the middle, pull request + refresh on the right.

use super::theme::{self, icon};
use super::widgets::{self, tool_button};
use super::{Sel, SporApp};
use eframe::egui::{self, Align2, CornerRadius, FontId, Key, Pos2, Sense, Ui, Vec2};

/// A chrome-style dropdown trigger: caption over value, with a caret.
fn switcher(ui: &mut Ui, glyph: &str, caption: &str, value: &str, extra: &str) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let value_g = ui
        .painter()
        .layout_no_wrap(value.to_string(), theme::semibold(13.5), p.text);
    let extra_g =
        ui.painter()
            .layout_no_wrap(extra.to_string(), FontId::proportional(12.0), p.muted);
    let w = (value_g.size().x + extra_g.size().x + 64.0).clamp(140.0, 300.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::click());
    let painter = ui.painter_at(rect);
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    painter.text(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        p.muted,
    );
    let x = rect.left() + 34.0;
    painter.text(
        Pos2::new(x, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        caption,
        FontId::proportional(10.5),
        p.faint,
    );
    let vy = rect.top() + 29.0;
    let clip = egui::Rect::from_min_max(
        Pos2::new(x, rect.top()),
        Pos2::new(rect.right() - 22.0, rect.bottom()),
    );
    let vw = value_g.size().x;
    let vh = value_g.size().y;
    painter
        .with_clip_rect(clip)
        .galley(Pos2::new(x, vy - vh / 2.0), value_g, p.text);
    if !extra.is_empty() {
        let eh = extra_g.size().y;
        painter.with_clip_rect(clip).galley(
            Pos2::new(x + vw + 8.0, vy - eh / 2.0),
            extra_g,
            p.muted,
        );
    }
    painter.text(
        Pos2::new(rect.right() - 12.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon::CARET_DOWN,
        FontId::proportional(12.0),
        p.faint,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl SporApp {
    pub(super) fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let Some(repo) = &self.repo else { return };
        let busy = self.job.as_ref().map(|j| j.label);
        let (ahead, behind) = (repo.tracking.ahead, repo.tracking.behind);
        let has_wip = !repo.status.is_empty();
        let stashes = repo.stashes;
        let branch_label = match (&repo.tracking.branch, repo.tracking.detached) {
            (Some(b), _) => b.clone(),
            (None, true) => "Detached HEAD".into(),
            (None, false) => "—".into(),
        };
        let sync = match (&repo.tracking.upstream, ahead, behind) {
            (None, _, _) => "not published".to_string(),
            (Some(_), 0, 0) => String::new(),
            (Some(_), a, b) => {
                let mut s = String::new();
                if a > 0 {
                    s.push_str(&format!("{} {a}", icon::ARROW_UP));
                }
                if b > 0 {
                    s.push_str(&format!(" {} {b}", icon::ARROW_DOWN));
                }
                s.trim().to_string()
            }
        };
        let repo_name = repo.name.clone();

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(6.0);

            // Repository menu.
            let resp = switcher(ui, icon::FOLDER_SIMPLE, "REPOSITORY", &repo_name, "");
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(260.0);
                if ui.button(format!("{}  Open…", icon::FOLDER_OPEN)).clicked() {
                    self.pick_repo();
                }
                let others: Vec<_> = self
                    .recent
                    .iter()
                    .filter(|p| self.repo.as_ref().is_none_or(|r| &r.root != *p))
                    .cloned()
                    .collect();
                if !others.is_empty() {
                    ui.separator();
                    widgets::section_header(ui, "Recent", None);
                    for path in others {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if ui
                            .button(format!("{}  {name}", icon::CLOCK))
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            self.open_repo(&path);
                        }
                    }
                }
                ui.separator();
                if ui
                    .button(format!("{}  Close repository", icon::X))
                    .clicked()
                {
                    self.close_repo();
                }
                ui.separator();
                widgets::section_header(ui, "Appearance", None);
                let current = ui.ctx().options(|o| o.theme_preference);
                ui.horizontal(|ui| {
                    for (pref, glyph, label) in [
                        (egui::ThemePreference::System, icon::MONITOR, "System"),
                        (egui::ThemePreference::Light, icon::SUN, "Light"),
                        (egui::ThemePreference::Dark, icon::MOON, "Dark"),
                    ] {
                        if ui
                            .selectable_label(current == pref, format!("{glyph} {label}"))
                            .clicked()
                        {
                            theme::set_preference(ui.ctx(), pref);
                        }
                    }
                });
            });

            ui.add_space(4.0);
            let resp = switcher(ui, icon::GIT_BRANCH, "BRANCH", &branch_label, &sync);
            egui::Popup::menu(&resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.branch_menu(ui));

            // Centered action group.
            let group_w = 6.0 * 58.0 + 5.0 * 2.0;
            let space = (ui.available_width() - group_w) / 2.0 - 150.0;
            ui.add_space(space.max(12.0));

            let enabled = busy.is_none();
            let is = |l: &str| busy == Some(l);
            if tool_button(
                ui,
                icon::CLOUD_ARROW_DOWN,
                "Fetch",
                None,
                enabled,
                is("Fetch"),
            )
            .on_hover_text("Fetch all remotes")
            .clicked()
            {
                self.fetch();
            }
            let badge = (behind > 0).then(|| behind.to_string());
            if tool_button(
                ui,
                icon::DOWNLOAD_SIMPLE,
                "Pull",
                badge,
                enabled,
                is("Pull"),
            )
            .on_hover_text("Pull (fast-forward only)")
            .clicked()
            {
                self.pull();
            }
            let badge = (ahead > 0).then(|| ahead.to_string());
            if tool_button(ui, icon::UPLOAD_SIMPLE, "Push", badge, enabled, is("Push"))
                .on_hover_text("Push to the upstream branch")
                .clicked()
            {
                self.request_push();
            }
            if tool_button(ui, icon::GIT_BRANCH, "Branch", None, true, false)
                .on_hover_text("Create a branch at the selected commit")
                .clicked()
            {
                self.branch_from_selection();
            }
            if tool_button(ui, icon::STACK, "Stash", None, has_wip, false)
                .on_hover_text("Stash uncommitted changes")
                .clicked()
            {
                self.stash();
            }
            let badge = (stashes > 0).then(|| stashes.to_string());
            if tool_button(
                ui,
                icon::ARROW_COUNTER_CLOCKWISE,
                "Pop",
                badge,
                stashes > 0,
                false,
            )
            .on_hover_text("Apply and drop the latest stash")
            .clicked()
            {
                self.stash_pop();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if tool_button(ui, icon::ARROWS_CLOCKWISE, "Refresh", None, true, false)
                    .on_hover_text("Reload (⌘R)")
                    .clicked()
                {
                    self.refresh();
                    self.needs_pr_fetch = true;
                }
                if tool_button(
                    ui,
                    icon::GIT_PULL_REQUEST,
                    "Pull request",
                    None,
                    true,
                    false,
                )
                .on_hover_text("Open this branch's pull request, or start one")
                .clicked()
                {
                    self.open_pull_request(&ctx);
                }
            });
        });
    }

    fn branch_from_selection(&mut self) {
        let Some(repo) = &self.repo else { return };
        let row = match self.sel {
            Sel::Commit(i) => repo.rows.get(i),
            Sel::Wip => repo.head_row().and_then(|i| repo.rows.get(i)),
        };
        if let Some(row) = row {
            let label = format!("{} {}", row.commit.short, row.commit.subject);
            self.new_branch_dialog(row.commit.hash.clone(), label);
        }
    }

    /// Filterable branch list inside the branch switcher popup.
    fn branch_menu(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        ui.set_width(320.0);
        let filter = ui.add(
            egui::TextEdit::singleline(&mut self.branch_filter)
                .hint_text(format!("{}  Switch to branch…", icon::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 6)),
        );
        if !filter.has_focus() && !filter.lost_focus() {
            filter.request_focus();
        }
        let q = self.branch_filter.to_lowercase();
        let Some(repo) = &self.repo else { return };
        let matches: Vec<(String, bool, bool)> = repo
            .branches
            .iter()
            .filter(|b| q.is_empty() || b.name.to_lowercase().contains(&q))
            .map(|b| (b.name.clone(), b.is_current, b.is_remote))
            .collect();
        let mut chosen = None;
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            chosen = matches
                .iter()
                .find(|(_, cur, _)| !cur)
                .map(|(n, _, _)| n.clone());
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(380.0)
            .show(ui, |ui| {
                for remote in [false, true] {
                    let group: Vec<_> = matches.iter().filter(|(_, _, r)| *r == remote).collect();
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    widgets::section_header(ui, if remote { "Remote" } else { "Local" }, None);
                    for (name, current, _) in group {
                        let resp = widgets::list_row(ui, *current, 28.0, |painter, rect, p| {
                            let glyph = if remote {
                                icon::CLOUD
                            } else {
                                icon::GIT_BRANCH
                            };
                            let color = if *current { p.head } else { p.muted };
                            painter.text(
                                Pos2::new(rect.left() + 16.0, rect.center().y),
                                Align2::CENTER_CENTER,
                                glyph,
                                FontId::proportional(14.0),
                                color,
                            );
                            painter.text(
                                Pos2::new(rect.left() + 32.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                name,
                                if *current {
                                    theme::semibold(13.0)
                                } else {
                                    FontId::proportional(13.0)
                                },
                                p.text,
                            );
                            if *current {
                                painter.text(
                                    Pos2::new(rect.right() - 10.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    icon::CHECK,
                                    FontId::proportional(13.0),
                                    p.accent,
                                );
                            }
                        });
                        if resp.clicked() && !current {
                            chosen = Some(name.clone());
                        }
                    }
                }
                if matches.is_empty() {
                    ui.label(egui::RichText::new("No matching branches").color(p.faint));
                }
            });
        if let Some(name) = chosen {
            self.branch_filter.clear();
            ui.close();
            self.checkout(&name);
        }
    }
}
