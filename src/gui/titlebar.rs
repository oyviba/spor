//! The unified title bar: on macOS the traffic lights sit in it (the window
//! draws under a transparent title bar), followed by the sidebar toggle, the
//! repository title with its branch as a clickable subtitle, and on the right
//! a New Branch button and a single Sync button with a menu for the
//! individual git operations. Dragging the bar moves the window.

use super::theme::{self, icon};
use super::widgets::{self, toolbar_button};
use super::{SporApp, View};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui,
    Vec2,
};

pub const HEIGHT: f32 = 52.0;

/// Room for the close/minimize/zoom buttons drawn by macOS.
fn traffic_lights_width() -> f32 {
    if cfg!(target_os = "macos") {
        76.0
    } else {
        10.0
    }
}

impl SporApp {
    pub(super) fn titlebar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let full = ui.max_rect();

        // Background first, so every control drawn later sits on top of it.
        let bg = ui.interact(full, ui.id().with("titlebar_bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        // The sidebar's material continues up into the title bar.
        if self.sidebar_open {
            let side = Rect::from_min_size(full.min, Vec2::new(self.sidebar_w, full.height()));
            ui.painter().rect_filled(side, 0.0, p.sidebar);
            ui.painter()
                .vline(side.right(), side.y_range(), Stroke::new(1.0, p.border));
        }

        let Some(repo) = &self.repo else { return };
        let name = repo.name.clone();
        let branch = repo
            .tracking
            .branch
            .clone()
            .unwrap_or_else(|| "Detached HEAD".into());
        let status = sync_status(repo);
        let busy = self.job.as_ref().map(|j| j.label);

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(traffic_lights_width());
            if toolbar_button(ui, icon::SIDEBAR_SIMPLE, "Show/hide sidebar", true).clicked() {
                self.sidebar_open = !self.sidebar_open;
            }
            // Title column starts where the content does.
            let content_x = if self.sidebar_open {
                full.left() + self.sidebar_w + 16.0
            } else {
                ui.cursor().left() + 10.0
            };
            let pad = (content_x - ui.cursor().left()).max(8.0);
            ui.add_space(pad);

            // Title and subtitle, like a document window: repo, then branch.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(8.0);
                let title = ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(format!("{name}  {}", icon::CARET_DOWN))
                                .font(theme::semibold(13.5))
                                .color(p.text),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                egui::Popup::menu(&title).show(|ui| self.repo_menu(ui));

                let sub = ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(format!("{branch}{status}  {}", icon::CARET_DOWN))
                                .size(11.5)
                                .color(p.muted),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_text("Switch branch")
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                egui::Popup::menu(&sub)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| self.branch_menu(ui));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                let resp = self.sync_button(ui, busy, &status);
                egui::Popup::menu(&resp.1).show(|ui| self.more_menu(ui, &ctx));
                if resp.0.clicked() {
                    self.sync();
                }
                ui.add_space(6.0);
                if toolbar_button(ui, icon::GIT_BRANCH, "New Branch…", true).clicked() {
                    self.new_branch_here();
                }
                if self.view == View::Changes
                    && toolbar_button(ui, icon::ARCHIVE_BOX, "Stash Changes", self.has_changes())
                        .clicked()
                {
                    self.stash();
                }
            });
        });
    }

    fn has_changes(&self) -> bool {
        self.repo.as_ref().is_some_and(|r| !r.changes.is_empty())
    }

    /// A rounded button reading "Sync" (or what it's doing), with a chevron
    /// segment that opens the menu of individual operations. Returns the
    /// main and chevron responses.
    fn sync_button(
        &self,
        ui: &mut Ui,
        busy: Option<&'static str>,
        status: &str,
    ) -> (egui::Response, egui::Response) {
        let p = theme::palette(ui.ctx());
        let label = match busy {
            Some("Sync") => "Syncing…",
            Some("Fetch") => "Fetching…",
            Some("Pull") => "Pulling…",
            Some("Push") => "Pushing…",
            _ => "Sync",
        };
        let font = FontId::proportional(13.0);
        let tw = ui
            .painter()
            .layout_no_wrap(label.to_string(), font.clone(), p.text)
            .size()
            .x;
        let main_w = tw + 40.0;
        let chev_w = 22.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(main_w + chev_w, 28.0), Sense::hover());
        let main_rect = Rect::from_min_size(rect.min, Vec2::new(main_w, 28.0));
        let chev_rect = Rect::from_min_max(Pos2::new(main_rect.right(), rect.top()), rect.max);
        let enabled = busy.is_none();
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let main = ui.interact(main_rect, ui.id().with("sync_main"), sense);
        let chev = ui.interact(chev_rect, ui.id().with("sync_chev"), Sense::click());

        let painter = ui.painter();
        let base = p.raised;
        painter.rect_filled(rect, CornerRadius::same(7), base);
        painter.rect_stroke(
            rect,
            CornerRadius::same(7),
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
        for (r, resp, radius) in [
            (
                main_rect,
                &main,
                CornerRadius {
                    nw: 7,
                    sw: 7,
                    ne: 0,
                    se: 0,
                },
            ),
            (
                chev_rect,
                &chev,
                CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 7,
                    se: 7,
                },
            ),
        ] {
            if resp.hovered() && (enabled || r == chev_rect) {
                painter.rect_filled(r.shrink(1.0), radius, base.lerp_to_gamma(p.text, 0.08));
            }
        }
        painter.vline(
            chev_rect.left(),
            rect.y_range().shrink(6.0),
            Stroke::new(1.0, p.border),
        );

        let icon_c = Pos2::new(main_rect.left() + 15.0, rect.center().y);
        if busy.is_some() {
            let t = ui.input(|i| i.time) as f32;
            widgets::spinner(painter, icon_c, 6.0, p.accent, t);
            ui.ctx().request_repaint();
        } else {
            painter.text(
                icon_c,
                Align2::CENTER_CENTER,
                icon::ARROWS_CLOCKWISE,
                FontId::proportional(15.0),
                p.text,
            );
        }
        painter.text(
            Pos2::new(main_rect.left() + 28.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            font,
            if enabled { p.text } else { p.muted },
        );
        painter.text(
            chev_rect.center(),
            Align2::CENTER_CENTER,
            icon::CARET_DOWN,
            FontId::proportional(11.0),
            p.muted,
        );
        // A dot when there's something to send or receive.
        if !status.is_empty() && enabled {
            painter.circle_filled(
                Pos2::new(main_rect.left() + 21.0, rect.top() + 8.0),
                3.5,
                p.accent,
            );
        }
        let tip = if status.is_empty() {
            "Fetch, then pull and push as needed".to_string()
        } else {
            format!("Sync{status}")
        };
        (
            main.on_hover_text(tip.trim_start_matches(" · ")),
            chev.on_hover_text("More"),
        )
    }

    fn more_menu(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.set_min_width(220.0);
        let idle = self.job.is_none();
        if ui.add_enabled(idle, egui::Button::new("Fetch")).clicked() {
            self.fetch();
        }
        if ui.add_enabled(idle, egui::Button::new("Pull")).clicked() {
            self.pull();
        }
        if ui.add_enabled(idle, egui::Button::new("Push")).clicked() {
            self.push();
        }
        ui.separator();
        if ui.button("Open Pull Request").clicked() {
            self.open_pull_request(ctx);
        }
        if ui.button("Refresh").clicked() {
            self.refresh();
            self.needs_pr_fetch = true;
        }
    }

    fn repo_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(240.0);
        if ui.button("Open…").clicked() {
            self.pick_repo();
        }
        let others: Vec<_> = self
            .recent
            .iter()
            .filter(|p| self.repo.as_ref().is_none_or(|r| &r.root != *p))
            .cloned()
            .collect();
        if !others.is_empty() {
            ui.menu_button("Open Recent", |ui| {
                for path in others {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if ui
                        .button(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.open_repo(&path);
                    }
                }
            });
        }
        if let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) {
            if ui.button("Show in Finder").clicked() {
                let _ = if cfg!(target_os = "macos") {
                    std::process::Command::new("open").arg(&root).spawn()
                } else {
                    std::process::Command::new("xdg-open").arg(&root).spawn()
                };
            }
        }
        ui.separator();
        let current = ui.ctx().options(|o| o.theme_preference);
        ui.menu_button("Appearance", |ui| {
            for (pref, label) in [
                (egui::ThemePreference::System, "Use System Setting"),
                (egui::ThemePreference::Light, "Light"),
                (egui::ThemePreference::Dark, "Dark"),
            ] {
                let mark = if current == pref { icon::CHECK } else { "  " };
                if ui.button(format!("{mark}  {label}")).clicked() {
                    theme::set_preference(ui.ctx(), pref);
                }
            }
        });
        ui.separator();
        if ui.button("Close Repository").clicked() {
            self.close_repo();
        }
    }

    /// The branch popover: type to filter, click to switch.
    fn branch_menu(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        ui.set_width(300.0);
        let filter = ui.add(
            egui::TextEdit::singleline(&mut self.branch_filter)
                .hint_text(format!("{}  Switch to branch", icon::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 5)),
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
        let mut new_branch = false;
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            chosen = matches
                .iter()
                .find(|(_, cur, _)| !cur)
                .map(|(n, _, _)| n.clone());
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .max_height(340.0)
            .show(ui, |ui| {
                for remote in [false, true] {
                    let group: Vec<_> = matches.iter().filter(|(_, _, r)| *r == remote).collect();
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    widgets::section_header(ui, if remote { "Remote" } else { "Local" });
                    for (name, current, _) in group {
                        let (rect, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 24.0),
                            Sense::click(),
                        );
                        let hovered = resp.hovered();
                        let fg = if hovered { Color32::WHITE } else { p.text };
                        if hovered {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(5), p.accent);
                        }
                        if *current {
                            ui.painter().text(
                                Pos2::new(rect.left() + 12.0, rect.center().y),
                                Align2::CENTER_CENTER,
                                icon::CHECK,
                                FontId::proportional(12.0),
                                fg,
                            );
                        }
                        widgets::text_until(
                            ui.painter(),
                            rect.left() + 26.0,
                            rect.center().y,
                            rect.right() - 8.0,
                            name,
                            FontId::proportional(13.0),
                            fg,
                        );
                        if resp.clicked() && !current {
                            chosen = Some(name.clone());
                        }
                    }
                }
                if matches.is_empty() {
                    ui.label(egui::RichText::new("No matching branches").color(p.faint));
                }
            });
        ui.separator();
        if ui.button("New Branch…").clicked() {
            new_branch = true;
        }
        if let Some(name) = chosen {
            self.branch_filter.clear();
            ui.close();
            self.checkout(&name);
        } else if new_branch {
            ui.close();
            self.new_branch_here();
        }
    }
}

/// " · 2 to send, 1 to receive", " · not published", or "".
fn sync_status(repo: &super::Repo) -> String {
    let t = &repo.tracking;
    if t.branch.is_none() {
        return String::new();
    }
    if t.upstream.is_none() {
        return " · not published".into();
    }
    let mut parts = Vec::new();
    if t.ahead > 0 {
        parts.push(format!("{} to send", t.ahead));
    }
    if t.behind > 0 {
        parts.push(format!("{} to receive", t.behind));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" · {}", parts.join(", "))
    }
}
