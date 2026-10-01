//! Small painted building blocks shared by the views.

use super::theme::{self, icon, Palette};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    Ui, Vec2,
};
use std::time::{Duration, Instant};

/// A circle with the author's initials, colored by a hash of their name so
/// the same person always gets the same color.
pub fn avatar(ui: &mut Ui, name: &str, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_avatar(ui.painter(), rect.center(), name, size);
    resp
}

pub fn paint_avatar(painter: &egui::Painter, center: Pos2, name: &str, size: f32) {
    let hue = (fnv(name) % 360) as f32 / 360.0;
    let fill: Color32 = egui::ecolor::Hsva::new(hue, 0.45, 0.62, 1.0).into();
    painter.circle_filled(center, size / 2.0, fill);
    painter.text(
        center,
        Align2::CENTER_CENTER,
        initials(name),
        theme::semibold(size * 0.42),
        Color32::WHITE,
    );
}

fn initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter(|w| !w.is_empty());
    let first = words.next().and_then(|w| w.chars().next());
    let last = words.next_back().and_then(|w| w.chars().next());
    [first, last]
        .into_iter()
        .flatten()
        .flat_map(char::to_uppercase)
        .collect()
}

pub fn fnv(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// A small capsule label (branch or tag on a commit row). Returns the right
/// edge.
pub fn tag(
    painter: &egui::Painter,
    x: f32,
    mid: f32,
    text: &str,
    fg: Color32,
    fill: Color32,
) -> f32 {
    let g = painter.layout_no_wrap(text.to_string(), FontId::proportional(10.5), fg);
    let rect = Rect::from_min_size(Pos2::new(x, mid - 8.0), Vec2::new(g.size().x + 10.0, 16.0));
    painter.rect_filled(rect, CornerRadius::same(4), fill);
    let h = g.size().y;
    painter.galley(Pos2::new(x + 5.0, mid - h / 2.0), g, fg);
    rect.right()
}

pub fn tag_width(painter: &egui::Painter, text: &str) -> f32 {
    painter
        .layout_no_wrap(text.to_string(), FontId::proportional(10.5), Color32::WHITE)
        .size()
        .x
        + 10.0
}

/// Borderless toolbar button: just an SF-Symbols-style glyph that gets a
/// rounded highlight on hover, like macOS toolbar items.
pub fn toolbar_button(ui: &mut Ui, glyph: &str, tooltip: &str, enabled: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 28.0), sense);
    if enabled && resp.hovered() {
        let fill = if resp.is_pointer_button_down_on() {
            p.raised.lerp_to_gamma(p.text, 0.12)
        } else {
            p.hover.lerp_to_gamma(p.text, 0.05)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(17.0),
        if enabled { p.muted } else { p.faint },
    );
    resp.on_hover_text(tooltip)
}

/// Spinning arc, for work in progress.
pub fn spinner(painter: &egui::Painter, center: Pos2, r: f32, color: Color32, t: f32) {
    let start = t * 6.0;
    let points: Vec<Pos2> = (0..=24)
        .map(|i| {
            let a = start + i as f32 / 24.0 * std::f32::consts::PI * 1.5;
            center + Vec2::angled(a) * r
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.8, color)));
}

#[derive(Clone, Copy, PartialEq)]
pub enum Check {
    Off,
    On,
    Mixed,
}

/// A macOS-style checkbox painted in `rect` (14pt square, accent when on).
pub fn paint_checkbox(painter: &egui::Painter, center: Pos2, state: Check, p: &Palette) {
    let rect = Rect::from_center_size(center, Vec2::splat(14.0));
    match state {
        Check::Off => {
            painter.rect_filled(rect, CornerRadius::same(4), p.bg);
            painter.rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(1.0, p.muted.gamma_multiply(0.7)),
                StrokeKind::Inside,
            );
        }
        Check::On | Check::Mixed => {
            painter.rect_filled(rect, CornerRadius::same(4), p.accent);
            let stroke = Stroke::new(1.8, Color32::WHITE);
            if state == Check::On {
                let c = rect.center();
                painter.line_segment([c + Vec2::new(-3.5, 0.2), c + Vec2::new(-1.0, 2.8)], stroke);
                painter.line_segment([c + Vec2::new(-1.0, 2.8), c + Vec2::new(3.8, -2.8)], stroke);
            } else {
                let c = rect.center();
                painter.line_segment([c + Vec2::new(-3.5, 0.0), c + Vec2::new(3.5, 0.0)], stroke);
            }
        }
    }
}

/// A segmented control. Returns true when the selection changed.
pub fn segmented(ui: &mut Ui, options: &[&str], selected: &mut usize) -> bool {
    let p = theme::palette(ui.ctx());
    let font = FontId::proportional(11.5);
    let widths: Vec<f32> = options
        .iter()
        .map(|o| {
            ui.painter()
                .layout_no_wrap(o.to_string(), font.clone(), p.text)
                .size()
                .x
                + 20.0
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 22.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let track = if p.dark {
        Color32::from_rgb(0x32, 0x32, 0x35)
    } else {
        Color32::from_rgb(0xe4, 0xe4, 0xe7)
    };
    painter.rect_filled(rect, CornerRadius::same(6), track);
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, (opt, w)) in options.iter().zip(&widths).enumerate() {
        let seg = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(*w, 18.0));
        let resp = ui.interact(seg, ui.id().with(("seg", i, *opt)), Sense::click());
        if i == *selected {
            let fill = if p.dark {
                Color32::from_rgb(0x5a, 0x5a, 0x5e)
            } else {
                Color32::WHITE
            };
            painter.rect_filled(seg, CornerRadius::same(5), fill);
        } else if resp.hovered() {
            painter.rect_filled(
                seg,
                CornerRadius::same(5),
                track.lerp_to_gamma(p.text, 0.06),
            );
        }
        painter.text(
            seg.center(),
            Align2::CENTER_CENTER,
            *opt,
            font.clone(),
            p.text,
        );
        if resp.clicked() && i != *selected {
            *selected = i;
            changed = true;
        }
        x += w;
    }
    changed
}

/// A flat clickable row, for the sidebar: rounded neutral highlight when
/// selected.
pub fn list_row(
    ui: &mut Ui,
    selected: bool,
    height: f32,
    paint: impl FnOnce(&egui::Painter, Rect, &Palette),
) -> Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, CornerRadius::same(6), p.sidebar_sel);
    }
    paint(&painter, rect, &p);
    resp
}

/// Sidebar group caption ("Branches", "Tags"): small, semibold, secondary.
pub fn section_header(ui: &mut Ui, title: &str) {
    let p = theme::palette(ui.ctx());
    ui.label(
        egui::RichText::new(title)
            .font(theme::semibold(11.0))
            .color(p.faint),
    );
}

/// Accent-filled default button.
pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let btn = egui::Button::new(
        egui::RichText::new(text)
            .font(theme::semibold(13.0))
            .color(if enabled { p.on_accent } else { p.faint }),
    )
    .fill(if enabled { p.accent } else { p.raised })
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(6))
    .min_size(Vec2::new(72.0, 24.0));
    ui.add_enabled(enabled, btn)
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(72.0, 24.0)))
}

// ── Toasts ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
    /// Shows an Undo button that takes back the latest undoable action.
    pub undoable: bool,
}

impl Toast {
    fn ttl(&self) -> Duration {
        match (self.kind, self.undoable) {
            (ToastKind::Error, _) => Duration::from_secs(9),
            (_, true) => Duration::from_secs(8),
            _ => Duration::from_secs(4),
        }
    }
}

/// Notifications stacked in the bottom-right corner, clear of the commit
/// composer. Each is measured first and
/// pinned at a fixed position (an auto-sized, anchored area drifts while it
/// settles, which makes buttons inside it hard to click). Plain toasts
/// dismiss on click; undoable ones carry an Undo button. Returns true when
/// Undo was pressed.
pub fn show_toasts(ctx: &egui::Context, toasts: &mut Vec<Toast>) -> bool {
    toasts.retain(|t| t.born.elapsed() < t.ttl());
    if toasts.is_empty() {
        return false;
    }
    let p = theme::palette(ctx);
    let screen = ctx.content_rect();
    let fill = ctx.global_style().visuals.window_fill;
    let shadow = ctx.global_style().visuals.popup_shadow;
    let mut dismiss = None;
    let mut undo = false;
    let mut bottom = screen.bottom() - 20.0;

    for (i, t) in toasts.iter().enumerate().rev() {
        let (glyph, color) = match t.kind {
            ToastKind::Info => (icon::INFO, p.muted),
            ToastKind::Success => (icon::CHECK_CIRCLE, p.green),
            ToastKind::Error => (icon::WARNING_CIRCLE, p.red),
        };
        let galley =
            ctx.fonts_mut(|f| f.layout(t.text.clone(), FontId::proportional(13.0), p.text, 440.0));
        let undo_w = if t.undoable { 52.0 } else { 0.0 };
        let size = Vec2::new(
            14.0 + 22.0 + galley.size().x + undo_w + 14.0,
            (galley.size().y + 18.0).max(36.0),
        );
        let rect = Rect::from_min_size(
            Pos2::new(screen.right() - 20.0 - size.x, bottom - size.y),
            size,
        );
        bottom = rect.top() - 8.0;

        egui::Area::new(egui::Id::new(("toast", i)))
            .fixed_pos(rect.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let painter = ui.painter();
                painter.add(shadow.as_shape(rect, CornerRadius::same(12)));
                painter.rect_filled(rect, CornerRadius::same(12), fill);
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(12),
                    Stroke::new(1.0, p.border),
                    StrokeKind::Inside,
                );
                painter.text(
                    Pos2::new(rect.left() + 22.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    glyph,
                    FontId::proportional(15.0),
                    color,
                );
                let gs = galley.size();
                painter.galley(
                    Pos2::new(rect.left() + 36.0, rect.center().y - gs.y / 2.0),
                    galley.clone(),
                    p.text,
                );
                let whole = ui.interact(rect, ui.id().with("toast_bg"), Sense::click());
                if t.undoable {
                    let ur = Rect::from_min_max(
                        Pos2::new(rect.right() - 14.0 - undo_w + 8.0, rect.top()),
                        Pos2::new(rect.right() - 6.0, rect.bottom()),
                    );
                    let b = ui
                        .interact(ur, ui.id().with("toast_undo"), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.painter().text(
                        ur.center(),
                        Align2::CENTER_CENTER,
                        "Undo",
                        theme::semibold(12.5),
                        if b.hovered() {
                            p.accent.lerp_to_gamma(p.text, 0.2)
                        } else {
                            p.accent
                        },
                    );
                    if b.clicked() {
                        undo = true;
                    }
                } else if whole.clicked() {
                    dismiss = Some(i);
                }
                ui.allocate_rect(rect, Sense::hover());
            });
    }
    if let Some(i) = dismiss {
        toasts.remove(i);
    }
    ctx.request_repaint_after(Duration::from_millis(250));
    undo
}

/// Lay out one line of text, cut with "…" if it's wider than `max_width`.
pub fn ellipsized(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: max_width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

/// Paint `text` left-aligned at `x`, vertically centered on `mid`, ellipsized
/// to end before `right`. Returns the right edge of what was drawn.
pub fn text_until(
    painter: &egui::Painter,
    x: f32,
    mid: f32,
    right: f32,
    text: &str,
    font: FontId,
    color: Color32,
) -> f32 {
    if right - x < 8.0 {
        return x;
    }
    let g = ellipsized(painter, text, font, color, right - x);
    let (w, h) = (g.size().x, g.size().y);
    painter.galley(Pos2::new(x, mid - h / 2.0), g, color);
    x + w
}

/// "3 hours ago" style, no timezone needed.
pub fn relative_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let dt = (now - timestamp).max(0);
    let plural = |n: i64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    match dt {
        0..=59 => "just now".into(),
        60..=3599 => plural(dt / 60, "minute"),
        3600..=86_399 => plural(dt / 3600, "hour"),
        86_400..=172_799 => "yesterday".into(),
        172_800..=604_799 => plural(dt / 86_400, "day"),
        604_800..=2_591_999 => plural(dt / 604_800, "week"),
        2_592_000..=31_535_999 => plural(dt / 2_592_000, "month"),
        _ => plural(dt / 31_536_000, "year"),
    }
}

/// Split `dir/sub/file.rs` into (`file.rs`, `dir/sub/`).
pub fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[i + 1..], &path[..=i]),
        None => (path, ""),
    }
}
