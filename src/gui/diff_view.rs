//! Diff viewer: one file at a time, with old/new line-number gutters, tinted
//! added/removed lines and hunk headers. Rows are virtualized, so huge diffs
//! scroll as smoothly as small ones.

use super::theme::{self, icon, Palette};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Ui, Vec2};
use spor::diff::{ChangeKind, DiffLine, FileDiff, LineKind};

enum Row {
    Hunk { range: String, section: String },
    Line(DiffLine),
}

/// A parsed file prepared for display.
pub struct DiffDoc {
    pub file: FileDiff,
    rows: Vec<Row>,
    max_chars: usize,
    max_line_no: u32,
}

impl DiffDoc {
    pub fn new(file: FileDiff) -> Self {
        let mut rows = Vec::new();
        let mut max_chars = 0;
        let mut max_line_no = 0;
        for h in &file.hunks {
            rows.push(Row::Hunk {
                range: h.range.clone(),
                section: h.section.clone(),
            });
            for l in &h.lines {
                let mut l = l.clone();
                l.text = l.text.replace('\t', "    ");
                max_chars = max_chars.max(l.text.chars().count());
                max_line_no = max_line_no
                    .max(l.old_no.unwrap_or(0))
                    .max(l.new_no.unwrap_or(0));
                rows.push(Row::Line(l));
            }
        }
        Self {
            file,
            rows,
            max_chars,
            max_line_no,
        }
    }
}

pub fn change_icon(change: ChangeKind, p: &Palette) -> (&'static str, egui::Color32) {
    match change {
        ChangeKind::Added => (icon::FILE_PLUS, p.green),
        ChangeKind::Deleted => (icon::FILE_MINUS, p.red),
        ChangeKind::Modified => (icon::PENCIL_SIMPLE, p.yellow),
        ChangeKind::Renamed => (icon::ARROW_RIGHT, p.purple),
    }
}

/// Header strip: change icon, path (old → new for renames), +/- counts.
/// `actions` adds buttons on the right.
pub fn header(ui: &mut Ui, file: &FileDiff, actions: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        let (glyph, color) = change_icon(file.change, &p);
        ui.label(egui::RichText::new(glyph).size(15.0).color(color));
        let path = match (&file.old_path, &file.new_path) {
            (Some(o), Some(n)) if o != n => format!("{o}  →  {n}"),
            _ => file.path().to_string(),
        };
        ui.add(egui::Label::new(egui::RichText::new(path).font(theme::semibold(13.0))).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            actions(ui);
            if file.deletions > 0 {
                ui.label(
                    egui::RichText::new(format!("−{}", file.deletions))
                        .monospace()
                        .color(p.red),
                );
            }
            if file.additions > 0 {
                ui.label(
                    egui::RichText::new(format!("+{}", file.additions))
                        .monospace()
                        .color(p.green),
                );
            }
        });
    });
}

/// Centered placeholder for an empty pane.
pub fn placeholder(ui: &mut Ui, glyph: &str, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.centered_and_justified(|ui| {
        ui.label(
            egui::RichText::new(format!("{glyph}\n\n{text}"))
                .size(13.0)
                .color(p.faint),
        );
    });
}

pub fn body(ui: &mut Ui, doc: &DiffDoc) {
    let p = theme::palette(ui.ctx());
    if doc.file.binary {
        return placeholder(ui, icon::FILE, "Binary file — no text diff");
    }
    if doc.rows.is_empty() {
        let msg = match doc.file.change {
            ChangeKind::Renamed => "Renamed without changes",
            _ => "No content changes",
        };
        return placeholder(ui, icon::FILE_DASHED, msg);
    }

    let mono = FontId::monospace(12.5);
    let num_font = FontId::monospace(11.5);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&mono, 'M'));
    let num_w = (doc.max_line_no.max(1).to_string().len() as f32) * char_w * 0.95 + 16.0;
    let gutter = num_w * 2.0;
    let text_x = gutter + 26.0;
    let content_w = text_x + doc.max_chars as f32 * char_w + 24.0;
    let row_h = 20.0;

    egui::ScrollArea::both()
        .id_salt("diff_body")
        .auto_shrink(false)
        .show_rows(ui, row_h, doc.rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width().max(content_w);
            for row in &doc.rows[range] {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
                let painter = ui.painter_at(rect);
                let mid = rect.center().y;
                match row {
                    Row::Hunk { range, section } => {
                        painter.rect_filled(rect, 0.0, p.hunk_bg);
                        let g = painter.text(
                            Pos2::new(rect.left() + text_x, mid),
                            Align2::LEFT_CENTER,
                            range,
                            num_font.clone(),
                            p.accent.gamma_multiply(0.85),
                        );
                        if !section.is_empty() {
                            painter.text(
                                Pos2::new(g.right() + 12.0, mid),
                                Align2::LEFT_CENTER,
                                section,
                                num_font.clone(),
                                p.muted,
                            );
                        }
                    }
                    Row::Line(l) => {
                        let (bg, gutter_bg, marker, marker_color) = match l.kind {
                            LineKind::Added => (Some(p.add_bg), Some(p.add_gutter), "+", p.green),
                            LineKind::Removed => (Some(p.del_bg), Some(p.del_gutter), "−", p.red),
                            _ => (None, None, "", p.faint),
                        };
                        if let Some(bg) = bg {
                            painter.rect_filled(rect, 0.0, bg);
                        }
                        let g_rect = Rect::from_min_size(rect.min, Vec2::new(gutter, row_h));
                        if let Some(gb) = gutter_bg {
                            painter.rect_filled(g_rect, 0.0, gb);
                        }
                        for (n, x) in [(l.old_no, num_w), (l.new_no, gutter)] {
                            if let Some(n) = n {
                                painter.text(
                                    Pos2::new(rect.left() + x - 8.0, mid),
                                    Align2::RIGHT_CENTER,
                                    n.to_string(),
                                    num_font.clone(),
                                    p.faint,
                                );
                            }
                        }
                        if l.kind == LineKind::NoNewline {
                            painter.text(
                                Pos2::new(rect.left() + text_x, mid),
                                Align2::LEFT_CENTER,
                                format!(
                                    "{} No newline at end of file",
                                    icon::ARROW_COUNTER_CLOCKWISE
                                ),
                                FontId::proportional(11.5),
                                p.faint,
                            );
                            continue;
                        }
                        painter.text(
                            Pos2::new(rect.left() + gutter + 10.0, mid),
                            Align2::LEFT_CENTER,
                            marker,
                            mono.clone(),
                            marker_color,
                        );
                        painter.text(
                            Pos2::new(rect.left() + text_x, mid),
                            Align2::LEFT_CENTER,
                            &l.text,
                            mono.clone(),
                            p.text,
                        );
                    }
                }
            }
        });
}
