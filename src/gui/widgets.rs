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

/// One segment of a pill label: text in a color.
pub struct Seg {
    pub text: String,
    pub color: Color32,
}

impl Seg {
    pub fn new(text: impl Into<String>, color: Color32) -> Self {
        Self {
            text: text.into(),
            color,
        }
    }
}

/// A rounded label made of colored segments, painted at `x` and vertically
/// centered on `mid`. `solid` fills with `tint` (for HEAD); otherwise it's a
/// soft tint with a hairline border. Returns the right edge.
pub fn paint_pill(
    painter: &egui::Painter,
    x: f32,
    mid: f32,
    segs: &[Seg],
    tint: Color32,
    solid: bool,
) -> f32 {
    let font = FontId::proportional(11.5);
    let galleys: Vec<_> = segs
        .iter()
        .map(|s| painter.layout_no_wrap(s.text.clone(), font.clone(), s.color))
        .collect();
    let w: f32 = galleys.iter().map(|g| g.size().x).sum();
    let h = 18.0;
    let rect = Rect::from_min_size(Pos2::new(x, mid - h / 2.0), Vec2::new(w + 14.0, h));
    if solid {
        painter.rect_filled(rect, CornerRadius::same(9), tint);
    } else {
        painter.rect_filled(rect, CornerRadius::same(9), tint.gamma_multiply(0.16));
        painter.rect_stroke(
            rect,
            CornerRadius::same(9),
            Stroke::new(1.0, tint.gamma_multiply(0.45)),
            StrokeKind::Inside,
        );
    }
    let mut tx = x + 7.0;
    for g in galleys {
        let gw = g.size().x;
        let gh = g.size().y;
        painter.galley(Pos2::new(tx, mid - gh / 2.0), g, Color32::WHITE);
        tx += gw;
    }
    rect.right()
}

/// Width a pill will take, for layout decisions before painting.
pub fn pill_width(painter: &egui::Painter, segs: &[Seg]) -> f32 {
    let font = FontId::proportional(11.5);
    segs.iter()
        .map(|s| {
            painter
                .layout_no_wrap(s.text.clone(), font.clone(), s.color)
                .size()
                .x
        })
        .sum::<f32>()
        + 14.0
}

/// Toolbar action: icon over a small caption, with an optional count badge
/// and a spinner while `busy`.
pub fn tool_button(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    badge: Option<String>,
    enabled: bool,
    busy: bool,
) -> Response {
    let p = theme::palette(ui.ctx());
    let size = Vec2::new(58.0, 44.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter_at(rect.expand(4.0));
    if enabled && (resp.hovered() || busy) {
        let fill = if resp.is_pointer_button_down_on() {
            p.selection
        } else {
            p.hover
        };
        painter.rect_filled(rect, CornerRadius::same(8), fill);
    }
    let color = if enabled { p.text } else { p.faint };
    let icon_center = Pos2::new(rect.center().x, rect.top() + 15.0);
    if busy {
        let t = ui.input(|i| i.time) as f32;
        let r = 7.0;
        let start = t * 6.0;
        let points: Vec<Pos2> = (0..=20)
            .map(|i| {
                let a = start + i as f32 / 20.0 * std::f32::consts::PI * 1.5;
                icon_center + Vec2::angled(a) * r
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(2.0, p.accent)));
        ui.ctx().request_repaint();
    } else {
        painter.text(
            icon_center,
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(19.0),
            color,
        );
    }
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        if enabled { p.muted } else { p.faint },
    );
    if let Some(badge) = badge {
        let font = theme::semibold(10.0);
        let g = painter.layout_no_wrap(badge, font, p.on_accent);
        let bw = g.size().x + 8.0;
        let br = Rect::from_center_size(
            Pos2::new(icon_center.x + 13.0, icon_center.y - 8.0),
            Vec2::new(bw.max(15.0), 15.0),
        );
        painter.rect_filled(br, CornerRadius::same(8), p.accent);
        painter.galley(br.center() - g.size() / 2.0, g, p.on_accent);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A flat clickable row of text with icon, used for menus and sidebar items.
/// Returns the response for the whole row.
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
        painter.rect_filled(rect, CornerRadius::same(6), p.selection);
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), p.hover);
    }
    paint(&painter, rect, &p);
    resp
}

/// Uppercase small section caption with an optional right-aligned action.
pub fn section_header(ui: &mut Ui, title: &str, count: Option<usize>) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .font(theme::semibold(10.5))
                .color(p.faint),
        );
        if let Some(n) = count {
            ui.label(
                egui::RichText::new(n.to_string())
                    .font(theme::semibold(10.5))
                    .color(p.faint),
            );
        }
    });
}

/// Accent-filled primary button.
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
    .min_size(Vec2::new(0.0, 30.0));
    ui.add_enabled(enabled, btn)
}

/// Danger button for destructive confirmations.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    let p = theme::palette(ui.ctx());
    ui.add(
        egui::Button::new(
            egui::RichText::new(text)
                .font(theme::semibold(13.0))
                .color(Color32::WHITE),
        )
        .fill(p.red)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, 30.0)),
    )
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(0.0, 30.0)))
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
}

impl Toast {
    fn ttl(&self) -> Duration {
        match self.kind {
            ToastKind::Error => Duration::from_secs(9),
            _ => Duration::from_secs(4),
        }
    }
}

/// Stack of transient notifications in the bottom-right corner. Clicking a
/// toast dismisses it.
pub fn show_toasts(ctx: &egui::Context, toasts: &mut Vec<Toast>) {
    toasts.retain(|t| t.born.elapsed() < t.ttl());
    if toasts.is_empty() {
        return;
    }
    let p = theme::palette(ctx);
    let mut dismiss = None;
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -20.0))
        .order(egui::Order::Foreground)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                for (i, t) in toasts.iter().enumerate() {
                    let (glyph, color) = match t.kind {
                        ToastKind::Info => (icon::INFO, p.accent),
                        ToastKind::Success => (icon::CHECK_CIRCLE, p.green),
                        ToastKind::Error => (icon::WARNING, p.red),
                    };
                    let resp = egui::Frame::new()
                        .fill(p.raised)
                        .stroke(Stroke::new(1.0, p.border))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .shadow(ctx.global_style().visuals.popup_shadow)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(glyph)
                                        .font(FontId::proportional(16.0))
                                        .color(color),
                                );
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&t.text).color(p.text))
                                        .wrap(),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.clicked() {
                        dismiss = Some(i);
                    }
                    ui.add_space(6.0);
                }
            });
        });
    if let Some(i) = dismiss {
        toasts.remove(i);
    }
    ctx.request_repaint_after(Duration::from_millis(250));
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
