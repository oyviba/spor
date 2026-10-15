//! Fonts and colors: Inter and JetBrains Mono are bundled so the app looks
//! the same everywhere, and the Spor themes (Zed's One Dark / One Light
//! palettes) replace the kit's defaults. Appearance follows the system unless
//! the user picks Light or Dark. Edits made in the theme editor are stored
//! as overrides and patched over the two Spor themes at startup.

use super::support::{read_setting, write_setting};
use gpui_kit::component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Window};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::rc::Rc;

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
    install(&Overrides::saved(), cx);
}

/// The user's edits on top of Spor Light and Spor Dark: theme-file keys
/// (`background`, `list.hover.background`, `radius`, `font.size`) mapped to
/// values. Stored one per line as `dark<TAB>key<TAB>value`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
}

impl Overrides {
    const SETTING: &'static str = "theme";

    pub fn saved() -> Self {
        Self::parse(&read_setting(Self::SETTING).unwrap_or_default())
    }

    pub fn save(&self) {
        write_setting(Self::SETTING, &self.serialize());
    }

    pub fn mode(&self, dark: bool) -> &BTreeMap<String, String> {
        if dark {
            &self.dark
        } else {
            &self.light
        }
    }

    pub fn mode_mut(&mut self, dark: bool) -> &mut BTreeMap<String, String> {
        if dark {
            &mut self.dark
        } else {
            &mut self.light
        }
    }

    fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let mut fields = line.splitn(3, '\t');
            if let (Some(mode), Some(key), Some(value)) =
                (fields.next(), fields.next(), fields.next())
            {
                match mode {
                    "dark" => o.dark.insert(key.into(), value.into()),
                    "light" => o.light.insert(key.into(), value.into()),
                    _ => None,
                };
            }
        }
        o
    }

    fn serialize(&self) -> String {
        let mut out = String::new();
        for (mode, map) in [("light", &self.light), ("dark", &self.dark)] {
            for (key, value) in map {
                out += &format!("{mode}\t{key}\t{value}\n");
            }
        }
        out
    }
}

/// Spor Light or Spor Dark as shipped, before any overrides.
pub fn base_config(dark: bool, cx: &App) -> Option<Rc<ThemeConfig>> {
    ThemeRegistry::global(cx)
        .themes()
        .get(if dark { DARK } else { LIGHT })
        .cloned()
}

/// The shipped value of a theme-file key, as the editor shows it.
pub fn base_value(dark: bool, key: &str, cx: &App) -> Option<String> {
    let config = serde_json::to_value(&*base_config(dark, cx)?).ok()?;
    let value = match key {
        "radius" | "radius.lg" | "font.size" => config.get(key)?,
        _ => config.get("colors")?.get(key)?,
    };
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `base` with `overrides` written over it. Unknown keys or bad values
/// leave the theme as it was.
pub fn patched(base: &ThemeConfig, overrides: &BTreeMap<String, String>) -> ThemeConfig {
    let Ok(mut value) = serde_json::to_value(base) else {
        return base.clone();
    };
    for (key, v) in overrides {
        match key.as_str() {
            "radius" | "radius.lg" => {
                if let Ok(n) = v.parse::<u64>() {
                    value[key.as_str()] = n.into();
                }
            }
            "font.size" => {
                if let Ok(n) = v.parse::<f64>() {
                    value[key.as_str()] = n.into();
                }
            }
            _ => value["colors"][key.as_str()] = v.clone().into(),
        }
    }
    serde_json::from_value(value).unwrap_or_else(|_| base.clone())
}

/// Use the Spor themes with `overrides` applied, and repaint in the
/// current mode.
pub fn install(overrides: &Overrides, cx: &mut App) {
    let (Some(light), Some(dark)) = (base_config(false, cx), base_config(true, cx)) else {
        return;
    };
    let light = Rc::new(patched(&light, &overrides.light));
    let dark = Rc::new(patched(&dark, &overrides.dark));
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    let mode = theme.mode;
    Theme::change(mode, None, cx);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_round_trip() {
        let mut o = Overrides::default();
        o.dark.insert("background".into(), "#101010".into());
        o.light.insert("radius".into(), "6".into());
        assert_eq!(Overrides::parse(&o.serialize()), o);
    }

    #[test]
    fn patched_writes_colors_and_sizes() {
        let base: ThemeConfig = serde_json::from_value(serde_json::json!({
            "name": "T", "mode": "dark", "radius": 4,
            "colors": { "background": "#000000", "foreground": "#ffffff" }
        }))
        .unwrap();
        let mut o = BTreeMap::new();
        o.insert("background".to_string(), "#123456".to_string());
        o.insert("radius".to_string(), "9".to_string());
        o.insert("font.size".to_string(), "15".to_string());
        let value = serde_json::to_value(patched(&base, &o)).unwrap();
        assert_eq!(value["colors"]["background"], "#123456");
        assert_eq!(value["colors"]["foreground"], "#ffffff");
        assert_eq!(value["radius"], 9);
        assert_eq!(value["font.size"], 15.0);
    }
}
