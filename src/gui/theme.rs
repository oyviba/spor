//! Fonts and colors: Inter and JetBrains Mono are bundled so the app looks
//! the same everywhere, and the Spor themes (Zed's One Dark / One Light
//! palettes) replace the kit's defaults. Appearance follows the system unless
//! the user picks Light or Dark.

use super::support::{read_setting, write_setting};
use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Window};
use std::borrow::Cow;

const THEMES: &str = include_str!("../../assets/themes/spor.json");
const DARK: &str = "Spor Dark";
const LIGHT: &str = "Spor Light";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn saved() -> Self {
        match read_setting("appearance").as_deref() {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    fn save(self) {
        let name = match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        };
        write_setting("appearance", name);
    }
}

pub fn init(cx: &mut App) {
    let fonts = [
        include_bytes!("../../assets/fonts/Inter-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf").as_slice(),
        include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
    ];
    if let Err(e) = cx
        .text_system()
        .add_fonts(fonts.iter().map(|f| Cow::Borrowed(*f)).collect())
    {
        eprintln!("spor: couldn't load bundled fonts: {e}");
    }

    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(THEMES) {
        eprintln!("spor: couldn't load themes: {e}");
        return;
    }
    let themes = ThemeRegistry::global(cx).themes();
    let (light, dark) = (themes.get(LIGHT).cloned(), themes.get(DARK).cloned());
    if let (Some(light), Some(dark)) = (light, dark) {
        let theme = Theme::global_mut(cx);
        theme.light_theme = light;
        theme.dark_theme = dark;
    }
}

/// Apply `appearance` now. `System` reads the window's appearance.
pub fn apply(appearance: Appearance, window: &mut Window, cx: &mut App) {
    match appearance {
        Appearance::System => Theme::sync_system_appearance(Some(window), cx),
        Appearance::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
    }
}

/// Apply and remember the user's choice.
pub fn set(appearance: Appearance, window: &mut Window, cx: &mut App) {
    appearance.save();
    apply(appearance, window, cx);
}
