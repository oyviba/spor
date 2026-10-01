//! The History view: the timeline on the left, and the selected commit on
//! the right — a Mail-style header (author, date, message), its changed
//! files, and the diff of the chosen file.

use super::diff_view::{self, change_badge, paint_badge};
use super::graph_view::{self, RepoMeta, ROW_HEIGHT};
use super::theme::{self, icon};
use super::widgets::{avatar, split_path, text_until};
use super::SporApp;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, RichText, Sense, Ui, Vec2,
};

enum RowAction {
    Checkout(String),
    NewBranch(String, String),
    Copy(String),
}

impl SporApp {
    pub(super) fn history_view(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        egui::Panel::left("history_list")
            .resizable(true)
            .default_size(470.0)
            .min_size(320.0)
            .max_size(760.0)
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| self.commit_list(ui));

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| self.commit_detail(ui));
    }

    fn commit_list(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(repo) = &self.repo else { return };
        let meta = RepoMeta {
            remotes: &repo.remotes,
            prs: &repo.prs,
        };
        let graph_w = graph_view::graph_width(repo.max_lanes);
        let total = repo.rows.len();

        let mut area = egui::ScrollArea::vertical()
            .id_salt("commits")
            .auto_shrink(false);
        if std::mem::take(&mut self.scroll_to_sel) {
            // Keep the selection in view with a row of margin.
            let viewport = ui.available_height();
            let y = self.commit_sel as f32 * ROW_HEIGHT;
            let offset = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(egui::Id::new("commits_offset")))
                .unwrap_or(0.0);
            if y < offset + ROW_HEIGHT {
                area = area.vertical_scroll_offset((y - ROW_HEIGHT).max(0.0));
            } else if y + ROW_HEIGHT * 2.0 > offset + viewport {
                area = area.vertical_scroll_offset(y + ROW_HEIGHT * 2.0 - viewport);
            }
        }

        let mut clicked = None;
        let mut double = false;
        let mut action = None;
        let output = area.show_rows(ui, ROW_HEIGHT, total, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                let (rect, resp) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), ROW_HEIGHT),
                    Sense::click(),
                );
                let painter = ui.painter_at(rect);
                let selected = i == self.commit_sel;
                let inset = rect.shrink2(Vec2::new(6.0, 1.0));
                if selected {
                    painter.rect_filled(inset, CornerRadius::same(7), p.selection);
                } else {
                    if resp.hovered() {
                        painter.rect_filled(inset, CornerRadius::same(7), p.hover);
                    }
                    // Hairline between rows, indented past the graph.
                    painter.hline(
                        (rect.left() + graph_w + 4.0)..=rect.right(),
                        rect.bottom() - 0.5,
                        egui::Stroke::new(1.0, p.border.gamma_multiply(0.6)),
                    );
                }
                let row = &repo.rows[i];
                graph_view::paint_row(&painter, rect, row, &meta, graph_w, &p, selected);

                if resp.clicked() || resp.secondary_clicked() {
                    clicked = Some(i);
                }
                if resp.double_clicked() {
                    double = true;
                }
                resp.context_menu(|ui| {
                    ui.set_min_width(220.0);
                    for r in row
                        .commit
                        .refs
                        .iter()
                        .filter(|r| !r.starts_with("tag:") && !r.ends_with("/HEAD"))
                    {
                        if row.commit.head_ref.as_deref() == Some(r.as_str()) {
                            continue;
                        }
                        if ui.button(format!("Check Out “{r}”")).clicked() {
                            action = Some(RowAction::Checkout(r.clone()));
                        }
                    }
                    if ui.button("New Branch from Here…").clicked() {
                        action = Some(RowAction::NewBranch(
                            row.commit.hash.clone(),
                            format!("{} — {}", row.commit.short, row.commit.subject),
                        ));
                    }
                    ui.separator();
                    if ui.button("Copy Commit ID").clicked() {
                        action = Some(RowAction::Copy(row.commit.hash.clone()));
                    }
                    if ui.button("Copy Message").clicked() {
                        action = Some(RowAction::Copy(row.commit.subject.clone()));
                    }
                });
            }
        });
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("commits_offset"), output.state.offset.y));

        if let Some(i) = clicked {
            if i != self.commit_sel {
                self.select_commit(i);
            }
        }
        if double {
            self.checkout_selected();
        }
        match action {
            Some(RowAction::Checkout(name)) => self.checkout(&name),
            Some(RowAction::NewBranch(sha, label)) => self.new_branch_dialog(sha, label),
            Some(RowAction::Copy(text)) => ui.ctx().copy_text(text),
            None => {}
        }
    }

    fn commit_detail(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        if self.inspected.is_none() {
            return diff_view::empty_state(ui, icon::GIT_COMMIT, "No Commit Selected", "");
        }
        let mut select_file = None;
        let mut jump_to = None;
        let mut copy = None;

        egui::Panel::top("commit_header")
            .resizable(true)
            .default_size(260.0)
            .min_size(120.0)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin {
                left: 20,
                right: 20,
                top: 16,
                bottom: 0,
            }))
            .show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                let Some(ins) = &self.inspected else { return };
                let Some(repo) = &self.repo else { return };
                let Some(row) = repo.rows.get(self.commit_sel) else {
                    return;
                };

                // Sender line, as in a Mail message header.
                ui.horizontal(|ui| {
                    avatar(ui, &ins.details.author, 38.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add_space(1.0);
                        ui.label(RichText::new(&ins.details.author).font(theme::semibold(13.5)));
                        ui.label(RichText::new(&ins.details.date).size(11.5).color(p.muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let short = &ins.hash[..ins.hash.len().min(8)];
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new(format!("{short} {}", icon::COPY))
                                        .monospace()
                                        .size(11.5)
                                        .color(p.muted),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_text("Copy commit ID")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            copy = Some(ins.hash.clone());
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                egui::ScrollArea::vertical()
                    .id_salt("commit_detail")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let mut lines = ins.details.message.lines();
                        let subject = lines.next().unwrap_or(&row.commit.subject).to_string();
                        let body = lines.collect::<Vec<_>>().join("\n");
                        ui.add(
                            egui::Label::new(RichText::new(subject).font(theme::semibold(16.0)))
                                .wrap(),
                        );
                        if !body.trim().is_empty() {
                            ui.add_space(2.0);
                            ui.add(
                                egui::Label::new(RichText::new(body.trim()).color(p.muted)).wrap(),
                            );
                        }
                        if !ins.details.committer.is_empty()
                            && ins.details.committer != ins.details.author
                        {
                            ui.label(
                                RichText::new(format!("Committed by {}", ins.details.committer))
                                    .size(11.5)
                                    .color(p.faint),
                            );
                        }
                        if !row.commit.parents.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let label = if row.commit.parents.len() > 1 {
                                    "Parents"
                                } else {
                                    "Parent"
                                };
                                ui.label(RichText::new(label).size(11.5).color(p.faint));
                                for parent in &row.commit.parents {
                                    let short = &parent[..parent.len().min(7)];
                                    if ui
                                        .add(
                                            egui::Label::new(
                                                RichText::new(short)
                                                    .monospace()
                                                    .size(11.5)
                                                    .color(p.accent),
                                            )
                                            .sense(Sense::click()),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        jump_to =
                                            repo.rows.iter().position(|r| &r.commit.hash == parent);
                                    }
                                }
                            });
                        }

                        ui.add_space(14.0);
                        let (adds, dels) = ins
                            .files
                            .iter()
                            .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
                        ui.horizontal(|ui| {
                            let n = ins.files.len();
                            ui.label(
                                RichText::new(format!(
                                    "{n} File{} Changed",
                                    if n == 1 { "" } else { "s" }
                                ))
                                .font(theme::semibold(11.5))
                                .color(p.muted),
                            );
                            ui.label(RichText::new(format!("+{adds}")).size(11.5).color(p.green));
                            ui.label(RichText::new(format!("−{dels}")).size(11.5).color(p.red));
                        });
                        ui.add_space(2.0);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, f) in ins.files.iter().enumerate() {
                            let selected = self.commit_file == Some(i);
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            let painter = ui.painter_at(rect);
                            let (text, muted) = if selected {
                                painter.rect_filled(rect, CornerRadius::same(6), p.selection);
                                (Color32::WHITE, Color32::from_white_alpha(190))
                            } else {
                                if resp.hovered() {
                                    painter.rect_filled(rect, CornerRadius::same(6), p.hover);
                                }
                                (p.text, p.muted)
                            };
                            let mid = rect.center().y;
                            let (letter, color) = change_badge(f.change, &p);
                            let bc = Pos2::new(rect.left() + 14.0, mid);
                            if selected {
                                painter.text(
                                    bc,
                                    Align2::CENTER_CENTER,
                                    letter,
                                    theme::semibold(10.5),
                                    text,
                                );
                            } else {
                                paint_badge(&painter, bc, letter, color);
                            }
                            let counts = if f.binary {
                                "binary".to_string()
                            } else {
                                format!("+{}  −{}", f.additions, f.deletions)
                            };
                            let cr = painter.text(
                                Pos2::new(rect.right() - 10.0, mid),
                                Align2::RIGHT_CENTER,
                                counts,
                                FontId::proportional(11.0),
                                muted,
                            );
                            let (name, dir) = split_path(f.path());
                            let end = text_until(
                                &painter,
                                rect.left() + 30.0,
                                mid,
                                cr.left() - 10.0,
                                name,
                                FontId::proportional(13.0),
                                text,
                            );
                            if !dir.is_empty() {
                                text_until(
                                    &painter,
                                    end + 6.0,
                                    mid,
                                    cr.left() - 10.0,
                                    dir.trim_end_matches('/'),
                                    FontId::proportional(11.5),
                                    muted,
                                );
                            }
                            if resp.clicked() {
                                select_file = Some(i);
                            }
                        }
                        ui.add_space(10.0);
                    });
            });

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                let Some(doc) = &self.diff else {
                    return diff_view::empty_state(ui, icon::FILE_TEXT, "No File Selected", "");
                };
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        diff_view::header(ui, &doc.file, &mut self.diff_mode, |_| {});
                    });
                let r = ui.cursor();
                ui.painter()
                    .hline(r.x_range(), r.top(), egui::Stroke::new(1.0, p.border));
                diff_view::body(ui, doc, self.diff_mode);
            });

        if let Some(h) = copy {
            ui.ctx().copy_text(h);
            self.info("Commit ID copied");
        }
        if let Some(i) = select_file {
            self.select_commit_file(i);
        }
        if let Some(i) = jump_to {
            self.select_commit(i);
            self.scroll_to_sel = true;
        }
    }
}
