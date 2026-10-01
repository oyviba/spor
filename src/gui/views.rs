//! Top-level screens: the welcome screen, and the workspace that arranges
//! toolbar, sidebar, commit list, diff pane and inspector.

use super::diff_view;
use super::graph_view::{self, Columns, RepoMeta, ROW_HEIGHT};
use super::theme::{self, icon};
use super::widgets::{self, list_row};
use super::{Sel, SporApp};
use eframe::egui::{
    self, Align2, CornerRadius, FontId, Frame, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui,
    Vec2,
};

enum RowAction {
    Checkout(String),
    NewBranch(String, String),
    Copy(String, &'static str),
}

fn logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("spor_logo");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    let tex = ctx.load_texture("spor_logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

impl SporApp {
    pub(super) fn welcome(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let mut open = None;
        let mut pick = false;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                let width = 420.0;
                let top = (ui.available_height() * 0.16).max(24.0);
                ui.add_space(top);
                ui.vertical_centered(|ui| {
                    ui.set_max_width(width);
                    if let Some(tex) = logo(ui.ctx()) {
                        ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(104.0)));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Spor").font(theme::semibold(30.0)));
                    ui.label(
                        RichText::new("Follow the track of every branch")
                            .size(14.0)
                            .color(p.muted),
                    );
                    ui.add_space(24.0);
                    ui.scope(|ui| {
                        ui.spacing_mut().button_padding = Vec2::new(22.0, 9.0);
                        if widgets::primary_button(
                            ui,
                            &format!("{}  Open repository…", icon::FOLDER_OPEN),
                            true,
                        )
                        .clicked()
                        {
                            pick = true;
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("or drop a folder anywhere in this window  ·  ⌘O")
                            .size(12.0)
                            .color(p.faint),
                    );
                    if let Some(e) = &self.open_error {
                        ui.add_space(16.0);
                        Frame::new()
                            .fill(p.red.gamma_multiply(0.12))
                            .stroke(Stroke::new(1.0, p.red.gamma_multiply(0.5)))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{}  {e}", icon::WARNING)).color(p.red),
                                );
                            });
                    }
                });

                if !self.recent.is_empty() {
                    ui.add_space(36.0);
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(width);
                        Frame::new()
                            .fill(p.surface)
                            .stroke(Stroke::new(1.0, p.border))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(width - 20.0);
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(6.0);
                                    widgets::section_header(ui, "Recent repositories", None);
                                });
                                ui.add_space(4.0);
                                for path in &self.recent {
                                    let name = path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| path.display().to_string());
                                    let full = path.display().to_string();
                                    let resp = list_row(ui, false, 44.0, |painter, rect, p| {
                                        painter.text(
                                            Pos2::new(rect.left() + 22.0, rect.center().y),
                                            Align2::CENTER_CENTER,
                                            icon::FOLDER_SIMPLE,
                                            FontId::proportional(18.0),
                                            p.accent,
                                        );
                                        painter.text(
                                            Pos2::new(rect.left() + 44.0, rect.top() + 14.0),
                                            Align2::LEFT_CENTER,
                                            &name,
                                            theme::semibold(13.5),
                                            p.text,
                                        );
                                        widgets::text_until(
                                            painter,
                                            rect.left() + 44.0,
                                            rect.top() + 31.0,
                                            rect.right() - 10.0,
                                            &full,
                                            FontId::proportional(11.5),
                                            p.faint,
                                        );
                                    });
                                    if resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        open = Some(path.clone());
                                    }
                                }
                            });
                    });
                }
            });
        if pick {
            self.pick_repo();
        }
        if let Some(path) = open {
            self.open_repo(&path);
        }
    }

    pub(super) fn workspace(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());

        egui::Panel::top("toolbar")
            .exact_size(58.0)
            .frame(
                Frame::new()
                    .fill(p.chrome)
                    .inner_margin(Margin::symmetric(4, 6)),
            )
            .show(ui, |ui| self.toolbar(ui));

        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(250.0)
            .min_size(190.0)
            .max_size(420.0)
            .frame(
                Frame::new()
                    .fill(p.chrome)
                    .inner_margin(Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| self.sidebar(ui));

        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(400.0)
            .min_size(320.0)
            .max_size(640.0)
            .frame(
                Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(18, 0)),
            )
            .show(ui, |ui| self.inspector(ui));

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| {
                let h = ui.available_height();
                egui::Panel::bottom("diff")
                    .resizable(true)
                    .default_size((h * 0.45).max(160.0))
                    .min_size(120.0)
                    .max_size(h - 140.0)
                    .frame(Frame::new().fill(p.bg))
                    .show(ui, |ui| {
                        // Claim the whole panel: it remembers its content's
                        // height, so anything less would shrink it.
                        ui.set_min_height(ui.available_height());
                        self.diff_pane(ui);
                    });
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.bg))
                    .show(ui, |ui| self.commit_list(ui));
            });
    }

    fn diff_pane(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(doc) = &self.diff else {
            let msg = match self.sel {
                Sel::Wip if !self.has_wip() => "Working tree clean",
                _ => "Select a file to see its changes",
            };
            return diff_view::placeholder(ui, icon::FILE_TEXT, msg);
        };
        let wip_entry = match (self.sel, self.file_sel) {
            (Sel::Wip, Some(i)) => self.repo.as_ref().and_then(|r| r.status.get(i)).cloned(),
            _ => None,
        };
        let mut toggle = false;
        Frame::new()
            .fill(p.chrome)
            .inner_margin(Margin::symmetric(14, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                diff_view::header(ui, &doc.file, |ui| {
                    if let Some(e) = &wip_entry {
                        let label = if e.status.is_staged() {
                            format!("{}  Unstage file", icon::MINUS)
                        } else {
                            format!("{}  Stage file", icon::PLUS)
                        };
                        if ui.button(label).clicked() {
                            toggle = true;
                        }
                    }
                });
            });
        let r = ui.cursor();
        ui.painter()
            .hline(r.x_range(), r.top(), Stroke::new(1.0, p.border));
        diff_view::body(ui, doc);
        if toggle {
            if let Some(e) = wip_entry {
                self.toggle_stage(&e);
            }
        }
    }

    fn commit_list(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let Some(repo) = &self.repo else { return };
        let meta = RepoMeta {
            remotes: &repo.remotes,
            prs: &repo.prs,
        };
        let cols = Columns::new(ui.available_width(), repo.max_lanes);
        let wip = !repo.status.is_empty();
        let total = repo.rows.len() + wip as usize;
        let head_lane = repo
            .head_row()
            .and_then(|i| repo.rows.get(i))
            .map_or(0, |r| r.lane);
        let wip_summary = {
            let staged = repo.status.iter().filter(|e| e.status.is_staged()).count();
            let n = repo.status.len();
            let files = if n == 1 {
                "1 file".to_string()
            } else {
                format!("{n} files")
            };
            if staged > 0 {
                format!("{files} · {staged} staged")
            } else {
                files
            }
        };

        // Column header strip.
        let (hdr, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
        ui.painter().rect_filled(hdr, 0.0, p.bg);
        cols.paint_header(ui.painter(), hdr, &p);
        ui.painter().hline(
            hdr.x_range(),
            hdr.bottom() - 0.5,
            Stroke::new(1.0, p.border),
        );

        let sel_pos = match self.sel {
            Sel::Wip => 0,
            Sel::Commit(i) => i + wip as usize,
        };
        let mut area = egui::ScrollArea::vertical()
            .id_salt("commits")
            .auto_shrink(false);
        if std::mem::take(&mut self.scroll_to_sel) {
            // Keep the selection in view with a row of margin.
            let viewport = ui.available_height();
            let y = sel_pos as f32 * ROW_HEIGHT;
            let offset = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(egui::Id::new("commits_offset")))
                .unwrap_or(0.0);
            if y < offset + ROW_HEIGHT {
                area = area.vertical_scroll_offset((y - ROW_HEIGHT * 2.0).max(0.0));
            } else if y + ROW_HEIGHT * 3.0 > offset + viewport {
                area = area.vertical_scroll_offset(y + ROW_HEIGHT * 3.0 - viewport);
            }
        }

        let mut clicked = None;
        let mut double = false;
        let mut action = None;
        let output = area.show_rows(ui, ROW_HEIGHT, total, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for pos in range {
                let (rect, resp) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), ROW_HEIGHT),
                    Sense::click(),
                );
                let painter = ui.painter_at(rect);
                let selected = pos == sel_pos;
                if selected {
                    painter.rect_filled(rect, 0.0, p.selection);
                    painter.rect_filled(
                        Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
                        0.0,
                        p.accent,
                    );
                } else if resp.hovered() {
                    painter.rect_filled(rect, 0.0, p.hover);
                }
                if resp.clicked() || resp.secondary_clicked() {
                    clicked = Some(pos);
                }
                if resp.double_clicked() {
                    double = true;
                }

                if wip && pos == 0 {
                    graph_view::paint_wip_row(&painter, rect, head_lane, &wip_summary, &cols, &p);
                    continue;
                }
                let row = &repo.rows[pos - wip as usize];
                graph_view::paint_row(&painter, rect, row, &meta, &cols, &p, selected);

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
                        if ui
                            .button(format!("{}  Check out {r}", icon::ARROW_RIGHT))
                            .clicked()
                        {
                            action = Some(RowAction::Checkout(r.clone()));
                        }
                    }
                    if ui
                        .button(format!("{}  New branch here…", icon::GIT_BRANCH))
                        .clicked()
                    {
                        action = Some(RowAction::NewBranch(
                            row.commit.hash.clone(),
                            format!("{} {}", row.commit.short, row.commit.subject),
                        ));
                    }
                    ui.separator();
                    if ui.button(format!("{}  Copy SHA", icon::COPY)).clicked() {
                        action = Some(RowAction::Copy(
                            row.commit.hash.clone(),
                            "Copied commit SHA",
                        ));
                    }
                    if ui.button(format!("{}  Copy message", icon::COPY)).clicked() {
                        action = Some(RowAction::Copy(
                            row.commit.subject.clone(),
                            "Copied commit message",
                        ));
                    }
                });
            }
        });
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("commits_offset"), output.state.offset.y));

        if let Some(pos) = clicked {
            let sel = if wip && pos == 0 {
                Sel::Wip
            } else {
                Sel::Commit(pos - wip as usize)
            };
            if sel != self.sel {
                self.select(sel);
            }
        }
        if double {
            self.checkout_selected();
        }
        match action {
            Some(RowAction::Checkout(name)) => self.checkout(&name),
            Some(RowAction::NewBranch(sha, label)) => self.new_branch_dialog(sha, label),
            Some(RowAction::Copy(text, msg)) => {
                ui.ctx().copy_text(text);
                self.info(msg);
            }
            None => {}
        }
    }
}
