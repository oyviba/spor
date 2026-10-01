//! The Changes view: changed files with checkboxes (ticked = included in the
//! next commit), the commit composer under them, and the selected file's
//! diff alongside. Discarding is immediate and undoable.

use super::diff_view::{self, paint_badge};
use super::theme::{self, icon};
use super::widgets::{self, paint_checkbox, split_path, text_until, Check};
use super::{Change, SporApp};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, Sense, Ui, Vec2,
};

enum Action {
    Select(String),
    Toggle(Change),
    All(bool),
    Discard(Change),
    Reveal(String),
    Copy(String),
}

const ROW_H: f32 = 28.0;

impl SporApp {
    pub(super) fn changes_view(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(repo) = &self.repo else { return };
        if repo.changes.is_empty() {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(p.bg))
                .show(ui, |ui| {
                    diff_view::empty_state(
                        ui,
                        icon::CHECK_CIRCLE,
                        "No Changes",
                        "Your files match the last commit.",
                    )
                });
            return;
        }
        let changes = repo.changes.clone();
        let branch = repo
            .tracking
            .branch
            .clone()
            .unwrap_or_else(|| "HEAD".into());
        let mut action = None;

        egui::Panel::left("changes_list")
            .resizable(true)
            .default_size(340.0)
            .min_size(260.0)
            .max_size(520.0)
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                egui::Panel::bottom("composer")
                    .frame(Frame::new().fill(p.surface).inner_margin(Margin::same(12)))
                    .show(ui, |ui| self.composer(ui, &changes, &branch));

                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.bg))
                    .show(ui, |ui| {
                        // Header: select-all checkbox and a count.
                        let included = changes.iter().filter(|c| c.included()).count();
                        let any = changes.iter().any(|c| c.staged);
                        let all_state = if included == changes.len() {
                            Check::On
                        } else if any {
                            Check::Mixed
                        } else {
                            Check::Off
                        };
                        let (hdr, hresp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 34.0),
                            Sense::click(),
                        );
                        paint_checkbox(
                            ui.painter(),
                            Pos2::new(hdr.left() + 22.0, hdr.center().y),
                            all_state,
                            &p,
                        );
                        let n = changes.len();
                        ui.painter().text(
                            Pos2::new(hdr.left() + 40.0, hdr.center().y),
                            Align2::LEFT_CENTER,
                            format!("{n} Changed File{}", if n == 1 { "" } else { "s" }),
                            theme::semibold(12.0),
                            p.muted,
                        );
                        if hresp.on_hover_text("Include or exclude all").clicked() {
                            action = Some(Action::All(all_state != Check::On));
                        }
                        ui.painter().hline(
                            hdr.x_range(),
                            hdr.bottom() - 0.5,
                            egui::Stroke::new(1.0, p.border),
                        );

                        egui::ScrollArea::vertical()
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add_space(4.0);
                                for c in &changes {
                                    if let Some(a) = self.change_row(ui, c) {
                                        action = Some(a);
                                    }
                                }
                            });
                    });
            });

        // Diff of the selected file.
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                let selected = changes
                    .iter()
                    .find(|c| Some(&c.path) == self.change_sel.as_ref())
                    .cloned();
                let Some(doc) = &self.diff else {
                    return diff_view::empty_state(ui, icon::FILE_TEXT, "No Selection", "");
                };
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        diff_view::header(ui, &doc.file, &mut self.diff_mode, |ui| {
                            if let Some(c) = &selected {
                                if c.unstaged
                                    && widgets::secondary_button(ui, "Discard Changes")
                                        .on_hover_text("Revert this file — ⌘Z undoes it")
                                        .clicked()
                                {
                                    action = Some(Action::Discard(c.clone()));
                                }
                            }
                        });
                    });
                let r = ui.cursor();
                ui.painter()
                    .hline(r.x_range(), r.top(), egui::Stroke::new(1.0, p.border));
                diff_view::body(ui, doc, self.diff_mode);
            });

        match action {
            Some(Action::Select(path)) => self.select_change(Some(path)),
            Some(Action::Toggle(c)) => {
                self.change_sel = Some(c.path.clone());
                self.toggle_change(&c);
            }
            Some(Action::All(include)) => self.set_all_included(include),
            Some(Action::Discard(c)) => self.discard(&c),
            Some(Action::Reveal(path)) => self.reveal_in_finder(&path),
            Some(Action::Copy(path)) => ui.ctx().copy_text(path),
            None => {}
        }
    }

    fn change_row(&self, ui: &mut Ui, c: &Change) -> Option<Action> {
        let p = theme::palette(ui.ctx());
        let selected = self.change_sel.as_ref() == Some(&c.path);
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        let row = rect.shrink2(Vec2::new(6.0, 1.0));
        let painter = ui.painter_at(rect);
        let (text, muted) = if selected {
            painter.rect_filled(row, CornerRadius::same(6), p.selection);
            (Color32::WHITE, Color32::from_white_alpha(190))
        } else {
            if resp.hovered() {
                painter.rect_filled(row, CornerRadius::same(6), p.hover);
            }
            (p.text, p.muted)
        };
        let mid = rect.center().y;

        // Checkbox with its own hit area.
        let check_c = Pos2::new(rect.left() + 22.0, mid);
        let check_rect = Rect::from_center_size(check_c, Vec2::splat(22.0));
        let check = ui.interact(check_rect, ui.id().with(("check", &c.path)), Sense::click());
        let state = if c.included() {
            Check::On
        } else if c.staged {
            Check::Mixed
        } else {
            Check::Off
        };
        paint_checkbox(&painter, check_c, state, &p);

        let (letter, color) = if c.untracked {
            ("?", p.muted)
        } else if c.deleted {
            ("D", p.red)
        } else if c.added {
            ("A", p.green)
        } else if c.orig_path.is_some() {
            ("R", p.purple)
        } else {
            ("M", p.yellow)
        };
        let badge_c = Pos2::new(rect.right() - 20.0, mid);
        if selected {
            painter.text(
                badge_c,
                Align2::CENTER_CENTER,
                letter,
                theme::semibold(10.5),
                Color32::WHITE,
            );
        } else {
            paint_badge(&painter, badge_c, letter, color);
        }

        let (name, dir) = split_path(&c.path);
        let right = rect.right() - 36.0;
        let end = text_until(
            &painter,
            rect.left() + 40.0,
            mid,
            right,
            name,
            FontId::proportional(13.0),
            text,
        );
        if !dir.is_empty() {
            text_until(
                &painter,
                end + 6.0,
                mid,
                right,
                dir.trim_end_matches('/'),
                FontId::proportional(11.5),
                muted,
            );
        }

        let mut action = None;
        if check.clicked() {
            action = Some(Action::Toggle(c.clone()));
        } else if resp.clicked() {
            action = Some(Action::Select(c.path.clone()));
        }
        resp.context_menu(|ui| {
            let label = if c.included() {
                "Exclude from Commit"
            } else {
                "Include in Commit"
            };
            if ui.button(label).clicked() {
                action = Some(Action::Toggle(c.clone()));
            }
            if c.unstaged && ui.button("Discard Changes").clicked() {
                action = Some(Action::Discard(c.clone()));
            }
            ui.separator();
            let reveal = if cfg!(target_os = "macos") {
                "Show in Finder"
            } else {
                "Show in File Manager"
            };
            if ui.button(reveal).clicked() {
                action = Some(Action::Reveal(c.path.clone()));
            }
            if ui.button("Copy Path").clicked() {
                action = Some(Action::Copy(c.path.clone()));
            }
        });
        action
    }

    fn composer(&mut self, ui: &mut Ui, changes: &[Change], branch: &str) {
        let p = theme::palette(ui.ctx());
        let included = changes.iter().filter(|c| c.staged).count();
        let summary = ui.add(
            egui::TextEdit::singleline(&mut self.summary)
                .hint_text("Summary")
                .font(theme::semibold(13.0))
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(8, 6)),
        );
        let len = self.summary.chars().count();
        if len > 50 {
            let color = if len > 72 { p.yellow } else { p.faint };
            ui.painter().text(
                Pos2::new(summary.rect.right() - 8.0, summary.rect.center().y),
                Align2::RIGHT_CENTER,
                len.to_string(),
                FontId::proportional(11.0),
                color,
            );
        }
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.description)
                .hint_text("Description")
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(8, 6)),
        );
        ui.add_space(8.0);
        let can = included > 0 && !self.summary.trim().is_empty();
        let mut clicked = false;
        ui.horizontal(|ui| {
            let note = if included == 0 {
                "Tick files to include them".to_string()
            } else {
                format!("To {branch}")
            };
            ui.add(
                egui::Label::new(egui::RichText::new(note).size(11.5).color(p.muted)).truncate(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = widgets::primary_button(ui, "Commit", can)
                    .on_hover_text("⌘↩")
                    .clicked();
            });
        });
        if can && clicked {
            self.commit();
        }
    }
}
