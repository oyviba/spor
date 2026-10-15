//! The theme editor, opened in a sheet beside the window (⌘,): the main
//! colors of Spor Light or Spor Dark, a few preset palettes, corner radius
//! and font size. Every change applies at once — the window is the preview —
//! and is saved as an override on top of the shipped theme.

use super::theme::{self, Appearance, Overrides};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    h_flex, v_flex, ActiveTheme as _, Colorize as _, Disableable as _, Selectable as _,
    Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, px, Anchor, App, AppContext as _, ClipboardItem, Context, Entity, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
};

/// One color control and the theme-file keys it writes.
struct Token {
    id: &'static str,
    label: &'static str,
    keys: &'static [&'static str],
}

const fn token(id: &'static str, label: &'static str, keys: &'static [&'static str]) -> Token {
    Token { id, label, keys }
}

const GROUPS: &[(&str, &[Token])] = &[
    (
        "Surfaces",
        &[
            token(
                "bg",
                "Background",
                &["background", "table.background", "tab.active.background"],
            ),
            token(
                "panel",
                "Panels",
                &[
                    "sidebar.background",
                    "list.background",
                    "list.even.background",
                    "list.head.background",
                    "tab_bar.background",
                    "tab.background",
                    "popover.background",
                ],
            ),
            token(
                "bars",
                "Title & status bar",
                &["title_bar.background", "status_bar.background"],
            ),
            token(
                "muted",
                "Muted",
                &[
                    "muted.background",
                    "secondary.background",
                    "accent.background",
                    "tab_bar.segmented.background",
                ],
            ),
        ],
    ),
    (
        "Text",
        &[
            token(
                "text",
                "Text",
                &[
                    "foreground",
                    "sidebar.foreground",
                    "popover.foreground",
                    "accent.foreground",
                    "secondary.foreground",
                    "tab.active.foreground",
                ],
            ),
            token(
                "text_muted",
                "Muted text",
                &[
                    "muted.foreground",
                    "tab.foreground",
                    "table.head.foreground",
                ],
            ),
        ],
    ),
    (
        "Accent",
        &[
            token(
                "accent",
                "Accent",
                &[
                    "primary.background",
                    "ring",
                    "caret",
                    "link.foreground",
                    "progress_bar.background",
                    "drag_border",
                    "sidebar.primary.background",
                ],
            ),
            token(
                "on_accent",
                "Text on accent",
                &["primary.foreground", "sidebar.primary.foreground"],
            ),
        ],
    ),
    (
        "Lists",
        &[
            token(
                "hover",
                "Hover",
                &["list.hover.background", "secondary.hover.background"],
            ),
            token(
                "selected",
                "Selected",
                &[
                    "list.active.background",
                    "sidebar.accent.background",
                    "secondary.active.background",
                ],
            ),
        ],
    ),
    (
        "Lines",
        &[
            token(
                "border",
                "Borders",
                &["border", "sidebar.border", "table.row.border"],
            ),
            token(
                "border_strong",
                "Strong borders",
                &[
                    "window.border",
                    "input.border",
                    "title_bar.border",
                    "status_bar.border",
                ],
            ),
        ],
    ),
    (
        "Status — diffs and changed files",
        &[
            token("red", "Removed", &["base.red", "danger.background"]),
            token("green", "Added", &["base.green", "success.background"]),
            token("yellow", "Modified", &["base.yellow", "warning.background"]),
            token("blue", "Renamed", &["base.blue", "info.background"]),
        ],
    ),
];

/// A preset: values for token ids. Spor's own palette is "no overrides".
struct Preset {
    name: &'static str,
    dark: bool,
    colors: &'static [(&'static str, &'static str)],
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "Nord",
        dark: true,
        colors: &[
            ("bg", "#2e3440"),
            ("panel", "#3b4252"),
            ("bars", "#434c5e"),
            ("muted", "#3b4252"),
            ("text", "#eceff4"),
            ("text_muted", "#9aa3b5"),
            ("accent", "#88c0d0"),
            ("on_accent", "#2e3440"),
            ("hover", "#434c5e"),
            ("selected", "#4c566a"),
            ("border", "#3b4252"),
            ("border_strong", "#4c566a"),
            ("red", "#bf616a"),
            ("green", "#a3be8c"),
            ("yellow", "#ebcb8b"),
            ("blue", "#81a1c1"),
        ],
    },
    Preset {
        name: "Dracula",
        dark: true,
        colors: &[
            ("bg", "#282a36"),
            ("panel", "#21222c"),
            ("bars", "#191a21"),
            ("muted", "#343746"),
            ("text", "#f8f8f2"),
            ("text_muted", "#9ea3c4"),
            ("accent", "#bd93f9"),
            ("on_accent", "#282a36"),
            ("hover", "#343746"),
            ("selected", "#44475a"),
            ("border", "#343746"),
            ("border_strong", "#44475a"),
            ("red", "#ff5555"),
            ("green", "#50fa7b"),
            ("yellow", "#f1fa8c"),
            ("blue", "#8be9fd"),
        ],
    },
    Preset {
        name: "Midnight",
        dark: true,
        colors: &[
            ("bg", "#0d1117"),
            ("panel", "#161b22"),
            ("bars", "#010409"),
            ("muted", "#161b22"),
            ("text", "#e6edf3"),
            ("text_muted", "#8d96a0"),
            ("accent", "#2f81f7"),
            ("on_accent", "#ffffff"),
            ("hover", "#1f242c"),
            ("selected", "#262c36"),
            ("border", "#21262d"),
            ("border_strong", "#30363d"),
            ("red", "#f85149"),
            ("green", "#3fb950"),
            ("yellow", "#d29922"),
            ("blue", "#58a6ff"),
        ],
    },
    Preset {
        name: "Solarized",
        dark: false,
        colors: &[
            ("bg", "#fdf6e3"),
            ("panel", "#eee8d5"),
            ("bars", "#e4ddc8"),
            ("muted", "#eee8d5"),
            ("text", "#44555d"),
            ("text_muted", "#839496"),
            ("accent", "#268bd2"),
            ("on_accent", "#fdf6e3"),
            ("hover", "#e8e1cc"),
            ("selected", "#ddd6c1"),
            ("border", "#e4ddc8"),
            ("border_strong", "#d3cbb4"),
            ("red", "#dc322f"),
            ("green", "#859900"),
            ("yellow", "#b58900"),
            ("blue", "#268bd2"),
        ],
    },
    Preset {
        name: "Paper",
        dark: false,
        colors: &[
            ("bg", "#ffffff"),
            ("panel", "#f6f8fa"),
            ("bars", "#f0f2f4"),
            ("muted", "#f3f4f6"),
            ("text", "#1f2328"),
            ("text_muted", "#656d76"),
            ("accent", "#0969da"),
            ("on_accent", "#ffffff"),
            ("hover", "#eef1f4"),
            ("selected", "#dde4ec"),
            ("border", "#e5e7eb"),
            ("border_strong", "#d0d7de"),
            ("red", "#cf222e"),
            ("green", "#1a7f37"),
            ("yellow", "#9a6700"),
            ("blue", "#0969da"),
        ],
    },
];

fn tokens() -> impl Iterator<Item = &'static Token> {
    GROUPS.iter().flat_map(|(_, tokens)| tokens.iter())
}

pub struct ThemeEditor {
    /// Which variant is being edited: Spor Dark or Spor Light.
    dark: bool,
    overrides: Overrides,
    /// One per token, in `tokens()` order.
    pickers: Vec<Entity<ColorPickerState>>,
    _subscriptions: Vec<Subscription>,
}

impl ThemeEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut pickers = Vec::new();
        let mut subscriptions = Vec::new();
        for (ix, _) in tokens().enumerate() {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx));
            subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this, _, event: &ColorPickerEvent, _, cx| {
                    if let ColorPickerEvent::Change(Some(color)) = event {
                        this.set_token(ix, *color, cx);
                    }
                },
            ));
            pickers.push(picker);
        }
        let mut this = Self {
            dark: cx.theme().is_dark(),
            overrides: Overrides::saved(),
            pickers,
            _subscriptions: subscriptions,
        };
        this.sync_pickers(window, cx);
        this
    }

    /// The value a key has now: the user's override, else the shipped one.
    fn current(&self, key: &str, cx: &App) -> Option<String> {
        self.overrides
            .mode(self.dark)
            .get(key)
            .cloned()
            .or_else(|| theme::base_value(self.dark, key, cx))
    }

    fn current_color(&self, token: &Token, cx: &App) -> Option<Hsla> {
        Hsla::parse_hex(&self.current(token.keys[0], cx)?).ok()
    }

    fn sync_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (token, picker) in tokens().zip(self.pickers.clone()) {
            if let Some(color) = self.current_color(token, cx) {
                picker.update(cx, |state, cx| state.set_value(color, window, cx));
            }
        }
        cx.notify();
    }

    /// Write a token's keys. The accent also sets the shades derived from it.
    fn write_token(&mut self, token: &Token, color: Hsla) {
        let dark = self.dark;
        let map = self.overrides.mode_mut(dark);
        let hex = color.to_hex();
        for key in token.keys {
            map.insert((*key).to_string(), hex.clone());
        }
        if token.id == "accent" {
            let (hover, active) = if dark {
                (color.lighten(0.1), color.darken(0.1))
            } else {
                (color.darken(0.1), color.darken(0.2))
            };
            for (key, c) in [
                ("primary.hover.background", hover),
                ("primary.active.background", active),
                ("link.hover.foreground", hover),
                ("link.active.foreground", active),
                ("selection.background", color.opacity(0.24)),
                ("drop_target.background", color.opacity(0.1)),
            ] {
                map.insert(key.to_string(), c.to_hex());
            }
        }
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        self.overrides.save();
        theme::install(&self.overrides, cx);
        cx.notify();
    }

    fn set_token(&mut self, ix: usize, color: Hsla, cx: &mut Context<Self>) {
        if let Some(token) = tokens().nth(ix) {
            self.write_token(token, color);
            self.commit(cx);
        }
    }

    /// Edit the other variant, and show it so the window previews it.
    fn set_mode(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.dark = dark;
        let appearance = if dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        theme::apply(appearance, window, cx);
        self.sync_pickers(window, cx);
    }

    fn apply_preset(&mut self, preset: &Preset, window: &mut Window, cx: &mut Context<Self>) {
        let dark = self.dark;
        self.overrides.mode_mut(dark).clear();
        for (id, hex) in preset.colors {
            let Some(token) = tokens().find(|t| t.id == *id) else {
                continue;
            };
            if let Ok(color) = Hsla::parse_hex(hex) {
                self.write_token(token, color);
            }
        }
        self.commit(cx);
        self.sync_pickers(window, cx);
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = self.dark;
        self.overrides.mode_mut(dark).clear();
        self.commit(cx);
        self.sync_pickers(window, cx);
    }

    fn number(&self, key: &str, cx: &App) -> f32 {
        self.current(key, cx)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.)
    }

    fn step(&mut self, key: &'static str, delta: f32, range: (f32, f32), cx: &mut Context<Self>) {
        let value = (self.number(key, cx) + delta).clamp(range.0, range.1);
        let dark = self.dark;
        let map = self.overrides.mode_mut(dark);
        map.insert(key.to_string(), value.to_string());
        if key == "radius" {
            // Dialogs and notifications keep their larger rounding.
            map.insert("radius.lg".to_string(), (value * 2.).max(4.).to_string());
        }
        self.commit(cx);
    }

    /// The edited variant as a theme file, to share or keep.
    fn copy_json(&self, cx: &mut Context<Self>) {
        let Some(base) = theme::base_config(self.dark, cx) else {
            return;
        };
        let mut config = theme::patched(&base, self.overrides.mode(self.dark));
        config.name = format!("{} (edited)", base.name).into();
        let file = serde_json::json!({ "name": config.name.to_string(), "themes": [config] });
        if let Ok(text) = serde_json::to_string_pretty(&file) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
}

fn section(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_medium()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

impl Render for ThemeEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let dark = self.dark;
        let edited = !self.overrides.mode(dark).is_empty();

        let mode_switch = h_flex()
            .gap_1()
            .p_0p5()
            .rounded(theme.radius)
            .bg(theme.muted)
            .child(
                Button::new("edit-dark")
                    .ghost()
                    .small()
                    .flex_1()
                    .label("Dark")
                    .selected(dark)
                    .on_click(cx.listener(|this, _, window, cx| this.set_mode(true, window, cx))),
            )
            .child(
                Button::new("edit-light")
                    .ghost()
                    .small()
                    .flex_1()
                    .label("Light")
                    .selected(!dark)
                    .on_click(cx.listener(|this, _, window, cx| this.set_mode(false, window, cx))),
            );

        let swatch = |color: &str| {
            div()
                .size(px(12.))
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .bg(Hsla::parse_hex(color).unwrap_or_default())
        };
        let spor_bg = theme::base_value(dark, "background", cx).unwrap_or_default();
        let spor_accent = theme::base_value(dark, "primary.background", cx).unwrap_or_default();
        let preset_button = |id: SharedString, name: &'static str, bg: String, accent: String| {
            h_flex()
                .id(id)
                .gap_1p5()
                .px_2()
                .py_1()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|this| this.bg(theme.list_hover))
                .text_xs()
                .child(
                    h_flex()
                        .child(swatch(&bg))
                        .child(div().ml(px(-4.)).child(swatch(&accent))),
                )
                .child(name)
        };
        let presets = h_flex()
            .flex_wrap()
            .gap_1p5()
            .child(
                preset_button("preset-spor".into(), "Spor", spor_bg, spor_accent)
                    .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
            )
            .children(PRESETS.iter().filter(|p| p.dark == dark).map(|preset| {
                let get = |id: &str| {
                    preset
                        .colors
                        .iter()
                        .find(|(k, _)| *k == id)
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_default()
                };
                preset_button(
                    SharedString::from(format!("preset-{}", preset.name)),
                    preset.name,
                    get("bg"),
                    get("accent"),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.apply_preset(preset, window, cx)),
                )
            }));

        let mut body = v_flex()
            .gap_0p5()
            .child(mode_switch)
            .child(section("Presets", cx))
            .child(presets);
        let mut ix = 0;
        for (title, group) in GROUPS {
            body = body.child(section(title, cx));
            for token in group.iter() {
                let picker = &self.pickers[ix];
                let hex = self
                    .current_color(token, cx)
                    .map(|c| c.to_hex())
                    .unwrap_or_default();
                body = body.child(
                    h_flex()
                        .h(px(30.))
                        .gap_2()
                        .text_sm()
                        .child(div().flex_1().child(token.label))
                        .child(
                            div()
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .child(hex),
                        )
                        .child(ColorPicker::new(picker).small().anchor(Anchor::TopRight)),
                );
                ix += 1;
            }
        }

        let stepper = |id: &'static str,
                       label: &'static str,
                       key: &'static str,
                       step: f32,
                       range: (f32, f32),
                       value: f32| {
            h_flex()
                .h(px(30.))
                .gap_2()
                .text_sm()
                .child(div().flex_1().child(label))
                .child(
                    Button::new(SharedString::from(format!("{id}-less")))
                        .ghost()
                        .xsmall()
                        .label("−")
                        .disabled(value <= range.0)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.step(key, -step, range, cx)),
                        ),
                )
                .child(
                    div()
                        .w(px(28.))
                        .text_center()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .child(format!("{value}")),
                )
                .child(
                    Button::new(SharedString::from(format!("{id}-more")))
                        .ghost()
                        .xsmall()
                        .label("+")
                        .disabled(value >= range.1)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.step(key, step, range, cx)),
                        ),
                )
        };
        body = body
            .child(section("Shape & type", cx))
            .child(stepper(
                "radius",
                "Corner radius",
                "radius",
                1.,
                (0., 12.),
                self.number("radius", cx),
            ))
            .child(stepper(
                "font-size",
                "Font size",
                "font.size",
                1.,
                (12., 18.),
                self.number("font.size", cx),
            ));

        let footer = h_flex()
            .flex_none()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Button::new("theme-reset")
                    .outline()
                    .small()
                    .label(if dark {
                        "Reset Dark Theme"
                    } else {
                        "Reset Light Theme"
                    })
                    .disabled(!edited)
                    .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("theme-copy")
                    .ghost()
                    .small()
                    .label("Copy as JSON")
                    .tooltip("Copy this theme as a theme file")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_json(cx))),
            );

        v_flex()
            .gap_2()
            // The sheet scrolls; the footer ends the list.
            .child(body)
            .child(footer)
    }
}
