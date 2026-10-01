//! Top-level screens: the welcome window and the workspace layout.

use super::theme::{self, icon};
use super::titlebar;
use super::widgets::text_until;
use super::{SporApp, View};
use eframe::egui::{
    self, Align2, CornerRadius, FontId, Frame, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui,
    Vec2,
};

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
    /// Xcode-style welcome window: app identity and actions on the left,
    /// recent repositories on the right.
    pub(super) fn welcome(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let mut open = None;
        let mut pick = false;

        // Let the window be dragged by its empty background.
        let bg = ui.interact(ui.max_rect(), ui.id().with("welcome_bg"), Sense::drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.chrome))
            .show(ui, |ui| {
                let size = Vec2::new(760.0, 440.0);
                let card = Rect::from_center_size(ui.max_rect().center(), size);
                let left = Rect::from_min_size(card.min, Vec2::new(440.0, size.y));
                let right = Rect::from_min_max(Pos2::new(left.right(), card.top()), card.max);
                let painter = ui.painter();
                painter.rect_filled(card, CornerRadius::same(12), p.bg);
                painter.rect_filled(
                    right,
                    CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: 12,
                        se: 12,
                    },
                    p.sidebar,
                );
                painter.rect_stroke(
                    card,
                    CornerRadius::same(12),
                    Stroke::new(1.0, p.border),
                    egui::StrokeKind::Inside,
                );

                // Left: identity and actions.
                let mut l = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left.shrink2(Vec2::new(40.0, 36.0)))
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                );
                l.add_space(10.0);
                if let Some(tex) = logo(l.ctx()) {
                    l.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(120.0)));
                }
                l.label(RichText::new("Spor").font(theme::semibold(30.0)));
                l.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(12.0)
                        .color(p.muted),
                );
                l.add_space(28.0);
                if action_row(&mut l, icon::FOLDER_OPEN, "Open Existing Repository…", "⌘O")
                    .clicked()
                {
                    pick = true;
                }
                l.add_space(2.0);
                l.label(
                    RichText::new("You can also drop a folder onto this window.")
                        .size(11.5)
                        .color(p.faint),
                );
                if let Some(e) = &self.open_error {
                    l.add_space(14.0);
                    l.label(RichText::new(format!("{}  {e}", icon::WARNING_CIRCLE)).color(p.red));
                }

                // Right: recent repositories.
                let mut r = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right.shrink2(Vec2::new(10.0, 12.0)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                if self.recent.is_empty() {
                    let c = right.center();
                    r.painter().text(
                        c,
                        Align2::CENTER_CENTER,
                        "No Recent Repositories",
                        FontId::proportional(13.0),
                        p.faint,
                    );
                }
                r.spacing_mut().item_spacing.y = 2.0;
                for path in &self.recent {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let parent = path
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    let home = std::env::var("HOME").unwrap_or_default();
                    let parent = if !home.is_empty() && parent.starts_with(&home) {
                        format!("~{}", &parent[home.len()..])
                    } else {
                        parent
                    };
                    let (rect, resp) =
                        r.allocate_exact_size(Vec2::new(r.available_width(), 46.0), Sense::click());
                    let painter = r.painter();
                    if resp.hovered() {
                        painter.rect_filled(rect, CornerRadius::same(6), p.sidebar_sel);
                    }
                    painter.text(
                        Pos2::new(rect.left() + 22.0, rect.center().y),
                        Align2::CENTER_CENTER,
                        icon::FOLDER_SIMPLE,
                        FontId::proportional(22.0),
                        p.accent,
                    );
                    text_until(
                        painter,
                        rect.left() + 44.0,
                        rect.top() + 15.0,
                        rect.right() - 8.0,
                        &name,
                        theme::semibold(13.0),
                        p.text,
                    );
                    text_until(
                        painter,
                        rect.left() + 44.0,
                        rect.top() + 32.0,
                        rect.right() - 8.0,
                        &parent,
                        FontId::proportional(11.0),
                        p.muted,
                    );
                    if resp
                        .on_hover_text(path.display().to_string())
                        .double_clicked()
                    {
                        open = Some(path.clone());
                    }
                }
                if !self.recent.is_empty() {
                    r.add_space(6.0);
                    r.label(
                        RichText::new("Double-click to open")
                            .size(11.0)
                            .color(p.faint),
                    );
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

        egui::Panel::top("titlebar")
            .exact_size(titlebar::HEIGHT)
            .frame(Frame::new().fill(p.chrome))
            .show(ui, |ui| self.titlebar(ui));

        if self.sidebar_open {
            let resp = egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(220.0)
                .min_size(180.0)
                .max_size(360.0)
                .frame(
                    Frame::new()
                        .fill(p.sidebar)
                        .inner_margin(Margin::symmetric(8, 0)),
                )
                .show(ui, |ui| self.sidebar(ui));
            self.sidebar_w = resp.response.rect.width();
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg))
            .show(ui, |ui| match self.view {
                View::Changes => self.changes_view(ui),
                View::History => self.history_view(ui),
            });
    }
}

/// A large, quiet action row on the welcome screen.
fn action_row(ui: &mut Ui, glyph: &str, label: &str, shortcut: &str) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(300.0, 40.0), Sense::click());
    let fill = if resp.hovered() {
        p.raised.lerp_to_gamma(p.text, 0.06)
    } else {
        p.raised
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    if !p.dark {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        p.accent,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::semibold(13.0),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 14.0, rect.center().y),
        Align2::RIGHT_CENTER,
        shortcut,
        FontId::proportional(12.0),
        p.faint,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
