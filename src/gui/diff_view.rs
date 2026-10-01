//! Diff viewer: one file at a time, unified or side by side (like Xcode's
//! comparison view), with line numbers and softly tinted changes. Rows are
//! virtualized, so huge diffs scroll as smoothly as small ones.

use super::theme::{self, icon, Palette};
use super::widgets;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Ui, Vec2};
use spor::diff::{ChangeKind, DiffLine, FileDiff, LineKind};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DiffMode {
    Unified,
    Split,
}

enum Row {
    Hunk { range: String, section: String },
    Line(DiffLine),
}

enum SplitRow {
    Hunk { range: String, section: String },
    Pair(Option<DiffLine>, Option<DiffLine>),
}

/// A parsed file prepared for display in either mode.
pub struct DiffDoc {
    pub file: FileDiff,
    rows: Vec<Row>,
    split: Vec<SplitRow>,
    max_chars: usize,
    max_line_no: u32,
}

impl DiffDoc {
    pub fn new(file: FileDiff) -> Self {
        let mut rows = Vec::new();
        let mut split = Vec::new();
        let mut max_chars = 0;
        let mut max_line_no = 0;
        for h in &file.hunks {
            rows.push(Row::Hunk {
                range: h.range.clone(),
                section: h.section.clone(),
            });
            split.push(SplitRow::Hunk {
                range: h.range.clone(),
                section: h.section.clone(),
            });
            // Removed lines followed by added lines become side-by-side
            // pairs; context lines appear on both sides.
            let (mut removed, mut added) = (Vec::new(), Vec::new());
            let flush = |removed: &mut Vec<DiffLine>,
                         added: &mut Vec<DiffLine>,
                         split: &mut Vec<SplitRow>| {
                let n = removed.len().max(added.len());
                let mut r = removed.drain(..);
                let mut a = added.drain(..);
                for _ in 0..n {
                    split.push(SplitRow::Pair(r.next(), a.next()));
                }
            };
            for l in &h.lines {
                let mut l = l.clone();
                l.text = l.text.replace('\t', "    ");
                max_chars = max_chars.max(l.text.chars().count());
                max_line_no = max_line_no
                    .max(l.old_no.unwrap_or(0))
                    .max(l.new_no.unwrap_or(0));
                match l.kind {
                    LineKind::Removed => {
                        if !added.is_empty() {
                            flush(&mut removed, &mut added, &mut split);
                        }
                        removed.push(l.clone());
                    }
                    LineKind::Added => added.push(l.clone()),
                    LineKind::Context => {
                        flush(&mut removed, &mut added, &mut split);
                        split.push(SplitRow::Pair(Some(l.clone()), Some(l.clone())));
                    }
                    LineKind::NoNewline => {}
                }
                rows.push(Row::Line(l));
            }
            flush(&mut removed, &mut added, &mut split);
        }
        Self {
            file,
            rows,
            split,
            max_chars,
            max_line_no,
        }
    }
}

/// Xcode-style status letter for a change.
pub fn change_badge(change: ChangeKind, p: &Palette) -> (&'static str, Color32) {
    match change {
        ChangeKind::Added => ("A", p.green),
        ChangeKind::Deleted => ("D", p.red),
        ChangeKind::Modified => ("M", p.yellow),
        ChangeKind::Renamed => ("R", p.purple),
    }
}

/// Paint a status letter in a small rounded square, centered on `center`.
pub fn paint_badge(painter: &egui::Painter, center: Pos2, letter: &str, color: Color32) {
    let rect = Rect::from_center_size(center, Vec2::splat(15.0));
    painter.rect_filled(rect, CornerRadius::same(4), color.gamma_multiply(0.18));
    painter.text(
        center,
        Align2::CENTER_CENTER,
        letter,
        theme::semibold(10.0),
        color,
    );
}

/// Header: file name, folder, change counts, the Unified/Side-by-side
/// switch, and `actions` (e.g. Discard) on the right.
pub fn header(ui: &mut Ui, file: &FileDiff, mode: &mut DiffMode, actions: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.set_min_height(38.0);
        let (name, dir) = widgets::split_path(file.path());
        ui.label(egui::RichText::new(name).font(theme::semibold(13.0)));
        let mut sub = dir.trim_end_matches('/').to_string();
        if let (Some(o), Some(n)) = (&file.old_path, &file.new_path) {
            if o != n {
                sub = format!("renamed from {o}");
            }
        }
        if !sub.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(sub).size(12.0).color(p.muted)).truncate());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            actions(ui);
            let mut idx = match mode {
                DiffMode::Unified => 0,
                DiffMode::Split => 1,
            };
            if widgets::segmented(ui, &["Unified", "Side by Side"], &mut idx) {
                *mode = if idx == 0 {
                    DiffMode::Unified
                } else {
                    DiffMode::Split
                };
            }
            ui.add_space(8.0);
            if file.deletions > 0 {
                ui.label(
                    egui::RichText::new(format!("−{}", file.deletions))
                        .size(12.0)
                        .color(p.red),
                );
            }
            if file.additions > 0 {
                ui.label(
                    egui::RichText::new(format!("+{}", file.additions))
                        .size(12.0)
                        .color(p.green),
                );
            }
        });
    });
}

/// Centered empty state: a large faint symbol, a title and a hint.
pub fn empty_state(ui: &mut Ui, glyph: &str, title: &str, hint: &str) {
    let p = theme::palette(ui.ctx());
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    let c = rect.center();
    painter.text(
        c - Vec2::new(0.0, 34.0),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(44.0),
        p.faint,
    );
    painter.text(
        c + Vec2::new(0.0, 8.0),
        Align2::CENTER_CENTER,
        title,
        theme::semibold(15.0),
        p.muted,
    );
    if !hint.is_empty() {
        painter.text(
            c + Vec2::new(0.0, 30.0),
            Align2::CENTER_CENTER,
            hint,
            FontId::proportional(12.5),
            p.faint,
        );
    }
    ui.allocate_rect(rect, Sense::hover());
}

pub fn body(ui: &mut Ui, doc: &DiffDoc, mode: DiffMode) {
    if doc.file.binary {
        return empty_state(ui, icon::FILE, "Binary File", "No text changes to show");
    }
    if doc.rows.is_empty() {
        let title = match doc.file.change {
            ChangeKind::Renamed => "Renamed",
            _ => "No Content Changes",
        };
        return empty_state(ui, icon::FILE_DASHED, title, "");
    }
    match mode {
        DiffMode::Unified => unified(ui, doc),
        DiffMode::Split => side_by_side(ui, doc),
    }
}

struct Metrics {
    mono: FontId,
    num: FontId,
    char_w: f32,
    num_w: f32,
    row_h: f32,
}

fn metrics(ui: &Ui, doc: &DiffDoc) -> Metrics {
    let mono = FontId::monospace(12.0);
    let char_w = ui.ctx().fonts_mut(|f| f.glyph_width(&mono, 'M'));
    let digits = doc.max_line_no.max(1).to_string().len() as f32;
    Metrics {
        num: FontId::monospace(11.0),
        num_w: digits * char_w * 0.95 + 18.0,
        mono,
        char_w,
        row_h: 19.0,
    }
}

fn paint_hunk(
    painter: &egui::Painter,
    rect: Rect,
    x: f32,
    range: &str,
    section: &str,
    m: &Metrics,
    p: &Palette,
) {
    painter.rect_filled(rect, 0.0, p.hunk_bg);
    let mid = rect.center().y;
    let g = painter.text(
        Pos2::new(x, mid),
        Align2::LEFT_CENTER,
        range,
        m.num.clone(),
        p.faint,
    );
    if !section.is_empty() {
        painter.text(
            Pos2::new(g.right() + 10.0, mid),
            Align2::LEFT_CENTER,
            section,
            m.num.clone(),
            p.muted,
        );
    }
}

fn unified(ui: &mut Ui, doc: &DiffDoc) {
    let p = theme::palette(ui.ctx());
    let m = metrics(ui, doc);
    let gutter = m.num_w * 2.0;
    let text_x = gutter + 22.0;
    let content_w = text_x + doc.max_chars as f32 * m.char_w + 24.0;

    egui::ScrollArea::both()
        .id_salt("diff_unified")
        .auto_shrink(false)
        .show_rows(ui, m.row_h, doc.rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width().max(content_w);
            for row in &doc.rows[range] {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, m.row_h), Sense::hover());
                let painter = ui.painter_at(rect);
                let mid = rect.center().y;
                match row {
                    Row::Hunk { range, section } => {
                        paint_hunk(&painter, rect, rect.left() + text_x, range, section, &m, &p)
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
                        if let Some(gb) = gutter_bg {
                            painter.rect_filled(
                                Rect::from_min_size(rect.min, Vec2::new(gutter, m.row_h)),
                                0.0,
                                gb,
                            );
                        }
                        for (n, x) in [(l.old_no, m.num_w), (l.new_no, gutter)] {
                            if let Some(n) = n {
                                painter.text(
                                    Pos2::new(rect.left() + x - 8.0, mid),
                                    Align2::RIGHT_CENTER,
                                    n.to_string(),
                                    m.num.clone(),
                                    p.faint,
                                );
                            }
                        }
                        if l.kind == LineKind::NoNewline {
                            painter.text(
                                Pos2::new(rect.left() + text_x, mid),
                                Align2::LEFT_CENTER,
                                "No newline at end of file",
                                FontId::proportional(11.0),
                                p.faint,
                            );
                            continue;
                        }
                        painter.text(
                            Pos2::new(rect.left() + gutter + 8.0, mid),
                            Align2::LEFT_CENTER,
                            marker,
                            m.mono.clone(),
                            marker_color,
                        );
                        painter.text(
                            Pos2::new(rect.left() + text_x, mid),
                            Align2::LEFT_CENTER,
                            &l.text,
                            m.mono.clone(),
                            p.text,
                        );
                    }
                }
            }
        });
}

fn side_by_side(ui: &mut Ui, doc: &DiffDoc) {
    let p = theme::palette(ui.ctx());
    let m = metrics(ui, doc);

    egui::ScrollArea::vertical()
        .id_salt("diff_split")
        .auto_shrink(false)
        .show_rows(ui, m.row_h, doc.split.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            let half = (width / 2.0).floor();
            for row in &doc.split[range] {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, m.row_h), Sense::hover());
                let painter = ui.painter_at(rect);
                match row {
                    SplitRow::Hunk { range, section } => paint_hunk(
                        &painter,
                        rect,
                        rect.left() + m.num_w + 10.0,
                        range,
                        section,
                        &m,
                        &p,
                    ),
                    SplitRow::Pair(left, right) => {
                        let l_rect = Rect::from_min_size(rect.min, Vec2::new(half, m.row_h));
                        let r_rect =
                            Rect::from_min_max(Pos2::new(rect.left() + half, rect.top()), rect.max);
                        side(&painter, l_rect, left.as_ref(), true, &m, &p);
                        side(&painter, r_rect, right.as_ref(), false, &m, &p);
                        painter.vline(
                            r_rect.left(),
                            rect.y_range(),
                            egui::Stroke::new(1.0, p.border),
                        );
                    }
                }
            }
        });
}

/// One half of a side-by-side row. An absent line is drawn as a hatched
/// placeholder so the two sides stay aligned.
fn side(
    painter: &egui::Painter,
    rect: Rect,
    line: Option<&DiffLine>,
    old: bool,
    m: &Metrics,
    p: &Palette,
) {
    let mid = rect.center().y;
    let Some(l) = line else {
        painter.rect_filled(rect, 0.0, p.surface);
        return;
    };
    let changed = l.kind != LineKind::Context;
    let (bg, gutter_bg) = match (changed, old) {
        (true, true) => (Some(p.del_bg), Some(p.del_gutter)),
        (true, false) => (Some(p.add_bg), Some(p.add_gutter)),
        _ => (None, None),
    };
    if let Some(bg) = bg {
        painter.rect_filled(rect, 0.0, bg);
    }
    if let Some(gb) = gutter_bg {
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(m.num_w, rect.height())),
            0.0,
            gb,
        );
    }
    let n = if old { l.old_no } else { l.new_no };
    if let Some(n) = n {
        painter.text(
            Pos2::new(rect.left() + m.num_w - 8.0, mid),
            Align2::RIGHT_CENTER,
            n.to_string(),
            m.num.clone(),
            p.faint,
        );
    }
    let clip = Rect::from_min_max(Pos2::new(rect.left() + m.num_w, rect.top()), rect.max);
    painter
        .with_clip_rect(clip.intersect(painter.clip_rect()))
        .text(
            Pos2::new(rect.left() + m.num_w + 10.0, mid),
            Align2::LEFT_CENTER,
            &l.text,
            m.mono.clone(),
            p.text,
        );
}
