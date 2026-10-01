//! Look and feel: bundled fonts (Inter, JetBrains Mono, Phosphor icons), a
//! color palette per light/dark mode, and the egui style built from it.
//! Everything that picks a color asks [`palette`] so both modes stay in sync.

use eframe::egui::{
    self, epaint::Shadow, style::ScrollStyle, Color32, CornerRadius, FontData, FontDefinitions,
    FontFamily, FontId, Margin, Stroke, TextStyle, Theme,
};
use std::sync::Arc;

pub use egui_phosphor::regular as icon;

/// Family name for the semibold weight (headings, emphasis).
pub const SEMIBOLD: &str = "semibold";

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Main content background (commit list, diff).
    pub bg: Color32,
    /// Sidebar and toolbar.
    pub chrome: Color32,
    /// Inspector and other secondary surfaces.
    pub surface: Color32,
    /// Cards, inputs, buttons.
    pub raised: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub selection: Color32,
    pub green: Color32,
    pub red: Color32,
    pub yellow: Color32,
    pub blue: Color32,
    pub purple: Color32,
    pub add_bg: Color32,
    pub del_bg: Color32,
    pub add_gutter: Color32,
    pub del_gutter: Color32,
    pub hunk_bg: Color32,
    pub head: Color32,
}

const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x17, 0x18, 0x1d),
    chrome: Color32::from_rgb(0x1d, 0x1e, 0x24),
    surface: Color32::from_rgb(0x1b, 0x1c, 0x22),
    raised: Color32::from_rgb(0x26, 0x27, 0x2f),
    hover: Color32::from_rgb(0x24, 0x25, 0x2d),
    border: Color32::from_rgb(0x2c, 0x2e, 0x37),
    text: Color32::from_rgb(0xe4, 0xe5, 0xea),
    muted: Color32::from_rgb(0x9a, 0x9c, 0xa8),
    faint: Color32::from_rgb(0x67, 0x69, 0x75),
    accent: Color32::from_rgb(0x6d, 0x8d, 0xff),
    on_accent: Color32::WHITE,
    selection: Color32::from_rgb(0x26, 0x31, 0x52),
    green: Color32::from_rgb(0x4f, 0xc2, 0x86),
    red: Color32::from_rgb(0xf0, 0x6a, 0x6a),
    yellow: Color32::from_rgb(0xe8, 0xb4, 0x4c),
    blue: Color32::from_rgb(0x6d, 0x8d, 0xff),
    purple: Color32::from_rgb(0xb3, 0x8a, 0xf5),
    add_bg: Color32::from_rgb(0x19, 0x2c, 0x24),
    del_bg: Color32::from_rgb(0x33, 0x1d, 0x21),
    add_gutter: Color32::from_rgb(0x1f, 0x3a, 0x2d),
    del_gutter: Color32::from_rgb(0x45, 0x23, 0x28),
    hunk_bg: Color32::from_rgb(0x1e, 0x22, 0x33),
    head: Color32::from_rgb(0xff, 0xc8, 0x3d),
};

const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0xf4, 0xf4, 0xf6),
    surface: Color32::from_rgb(0xf9, 0xf9, 0xfb),
    raised: Color32::from_rgb(0xff, 0xff, 0xff),
    hover: Color32::from_rgb(0xec, 0xed, 0xf1),
    border: Color32::from_rgb(0xe0, 0xe1, 0xe6),
    text: Color32::from_rgb(0x1d, 0x1e, 0x24),
    muted: Color32::from_rgb(0x5f, 0x61, 0x6d),
    faint: Color32::from_rgb(0x96, 0x98, 0xa3),
    accent: Color32::from_rgb(0x3d, 0x63, 0xe8),
    on_accent: Color32::WHITE,
    selection: Color32::from_rgb(0xdf, 0xe7, 0xff),
    green: Color32::from_rgb(0x1f, 0x9d, 0x5c),
    red: Color32::from_rgb(0xd6, 0x3b, 0x3b),
    yellow: Color32::from_rgb(0xb7, 0x7c, 0x0b),
    blue: Color32::from_rgb(0x3d, 0x63, 0xe8),
    purple: Color32::from_rgb(0x84, 0x52, 0xd9),
    add_bg: Color32::from_rgb(0xe8, 0xf7, 0xee),
    del_bg: Color32::from_rgb(0xfd, 0xec, 0xec),
    add_gutter: Color32::from_rgb(0xcf, 0xef, 0xdc),
    del_gutter: Color32::from_rgb(0xf9, 0xd4, 0xd4),
    hunk_bg: Color32::from_rgb(0xee, 0xf1, 0xfc),
    head: Color32::from_rgb(0xd9, 0x92, 0x00),
};

pub fn palette(ctx: &egui::Context) -> Palette {
    match ctx.theme() {
        Theme::Dark => DARK,
        Theme::Light => LIGHT,
    }
}

/// Make a lane/branch color readable on this mode's background: the graph
/// palette is tuned for dark, so darken it on light backgrounds.
pub fn adapt(c: Color32, p: &Palette) -> Color32 {
    if p.dark {
        c
    } else {
        Color32::from_rgb(
            (c.r() as f32 * 0.72) as u8,
            (c.g() as f32 * 0.72) as u8,
            (c.b() as f32 * 0.72) as u8,
        )
    }
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(load_preference());
    for (theme, p) in [(Theme::Dark, DARK), (Theme::Light, LIGHT)] {
        ctx.style_mut_of(theme, |style| apply(style, &p));
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut fonts,
        "inter",
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        &mut fonts,
        "inter-semibold",
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        &mut fonts,
        "jetbrains-mono",
        include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    );

    // Our faces first; egui's defaults stay behind them as fallbacks for
    // symbols and emoji.
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "inter".into());
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "jetbrains-mono".into());
    let mut semibold = vec!["inter-semibold".to_string()];
    semibold.extend(
        fonts.families[&FontFamily::Proportional]
            .iter()
            .skip(1)
            .cloned(),
    );
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);

    // Icons right after our text faces and ahead of egui's bundled
    // emoji-icon font, which also uses the Private Use Area. The bundled Inter
    // is subset without its own PUA glyphs (see assets/fonts/README.md) so it
    // can't shadow Phosphor's codepoints.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    for family in [
        FontFamily::Proportional,
        FontFamily::Name(SEMIBOLD.into()),
        FontFamily::Monospace,
    ] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.retain(|f| f != "phosphor");
            list.insert(1, "phosphor".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn preference_file() -> Option<std::path::PathBuf> {
    super::config_dir().map(|d| d.join("appearance"))
}

/// Saved System/Light/Dark choice; `SPOR_THEME=light|dark` overrides it.
fn load_preference() -> egui::ThemePreference {
    let saved = std::env::var("SPOR_THEME")
        .ok()
        .or_else(|| std::fs::read_to_string(preference_file()?).ok())
        .unwrap_or_default();
    match saved.trim() {
        "light" => egui::ThemePreference::Light,
        "dark" => egui::ThemePreference::Dark,
        _ => egui::ThemePreference::System,
    }
}

pub fn set_preference(ctx: &egui::Context, pref: egui::ThemePreference) {
    ctx.set_theme(pref);
    let name = match pref {
        egui::ThemePreference::Light => "light",
        egui::ThemePreference::Dark => "dark",
        egui::ThemePreference::System => "system",
    };
    if let Some(file) = preference_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, name);
    }
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

fn apply(style: &mut egui::Style, p: &Palette) {
    use FontFamily::{Monospace, Proportional};
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Body, FontId::new(13.5, Proportional)),
        (TextStyle::Button, FontId::new(13.5, Proportional)),
        (TextStyle::Heading, semibold(17.0)),
        (TextStyle::Monospace, FontId::new(12.5, Monospace)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 5.0);
    s.interact_size.y = 26.0;
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(16);
    s.scroll = ScrollStyle {
        bar_width: 8.0,
        floating_width: 8.0,
        floating_allocated_width: 0.0,
        ..ScrollStyle::floating()
    };

    let v = &mut style.visuals;
    v.dark_mode = p.dark;
    v.override_text_color = None;
    v.panel_fill = p.bg;
    v.window_fill = p.raised;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    let shadow_alpha = if p.dark { 110 } else { 40 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.extreme_bg_color = if p.dark {
        Color32::from_rgb(0x13, 0x14, 0x18)
    } else {
        Color32::WHITE
    };
    v.text_edit_bg_color = Some(v.extreme_bg_color);
    v.faint_bg_color = p.hover;
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.yellow;
    v.error_fg_color = p.red;
    v.selection.bg_fill = p.accent.gamma_multiply(if p.dark { 0.45 } else { 0.3 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = p.raised;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border.lerp_to_gamma(p.muted, 0.35));
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.selection;
    w.active.weak_bg_fill = p.selection;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open = w.hovered;
}
