use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use log::{debug, warn};
use std::str::FromStr;

use crate::global_state::cookies::Cookies;

/// The visual theme mode of the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    System,
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        }
    }
}
impl FromStr for ThemeMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "system" => ThemeMode::System,
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::Dark,
        })
    }
}

/// The brand color palette used throughout the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemePalette {
    #[default]
    Ultros,
    Maelstrom,
    TwinAdder,
    Ascian,
    Ishgard,
    Crystarium,
    Sharlayan,
    Tuliyollal,
    ImmortalFlames,
    Uldah,
    Limsa,
    Garlemald,
}

impl ThemePalette {
    /// The complete picker catalog; legacy names are accepted only on input.
    pub const ALL: [Self; 12] = [
        Self::Ultros,
        Self::Maelstrom,
        Self::TwinAdder,
        Self::Ascian,
        Self::Ishgard,
        Self::Crystarium,
        Self::Sharlayan,
        Self::Tuliyollal,
        Self::ImmortalFlames,
        Self::Uldah,
        Self::Limsa,
        Self::Garlemald,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ultros => "Ultros",
            Self::Maelstrom => "Maelstrom",
            Self::TwinAdder => "Twin Adder",
            Self::Ascian => "Ascian",
            Self::Ishgard => "Ishgard",
            Self::Crystarium => "Crystarium",
            Self::Sharlayan => "Sharlayan",
            Self::Tuliyollal => "Tuliyollal",
            Self::ImmortalFlames => "Immortal Flames",
            Self::Uldah => "Ul’dah",
            Self::Limsa => "Limsa",
            Self::Garlemald => "Garlemald",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ThemePalette::Ultros => "ultros",
            ThemePalette::Maelstrom => "maelstrom",
            ThemePalette::TwinAdder => "twin-adder",
            ThemePalette::Ascian => "ascian",
            ThemePalette::Ishgard => "ishgard",
            ThemePalette::Crystarium => "crystarium",
            ThemePalette::Sharlayan => "sharlayan",
            ThemePalette::Tuliyollal => "tuliyollal",
            ThemePalette::ImmortalFlames => "immortal-flames",
            ThemePalette::Uldah => "uldah",
            ThemePalette::Limsa => "limsa",
            ThemePalette::Garlemald => "garlemald",
        }
    }
}
impl FromStr for ThemePalette {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "teal" => ThemePalette::Limsa,
            "emerald" => ThemePalette::TwinAdder,
            "amber" => ThemePalette::Uldah,
            "rose" => ThemePalette::Ascian,
            "sky" => ThemePalette::Ishgard,
            "ultros" => ThemePalette::Ultros,
            "maelstrom" => ThemePalette::Maelstrom,
            "twin-adder" => ThemePalette::TwinAdder,
            "ascian" => ThemePalette::Ascian,
            "ishgard" => ThemePalette::Ishgard,
            "crystarium" => ThemePalette::Crystarium,
            "sharlayan" => ThemePalette::Sharlayan,
            "tuliyollal" => ThemePalette::Tuliyollal,
            "immortal-flames" => ThemePalette::ImmortalFlames,
            "uldah" => ThemePalette::Uldah,
            "limsa" => ThemePalette::Limsa,
            "garlemald" => ThemePalette::Garlemald,
            "violet" => ThemePalette::Ultros,
            _ => ThemePalette::Ultros,
        })
    }
}

/// Global theme settings state.
/// - Persisted to localStorage and a cookie
/// - Applies `data-theme` and `data-palette` to <html>
#[derive(Clone, Copy)]
pub struct ThemeSettings {
    pub mode: RwSignal<ThemeMode>,
    pub palette: RwSignal<ThemePalette>,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeSettings {
    pub fn new() -> Self {
        // Load initial values from storage if available
        let initial_mode = load_mode_from_storage().unwrap_or_default();
        let initial_palette = load_palette_from_storage().unwrap_or_default();

        let mode = RwSignal::new(initial_mode);
        let palette = RwSignal::new(initial_palette);

        let settings = ThemeSettings { mode, palette };
        apply_to_dom(
            settings.mode.get_untracked(),
            settings.palette.get_untracked(),
        );
        persist_all(settings);

        // React to changes: apply to DOM and persist
        Effect::new({
            move |_| {
                let m = settings.mode.get();
                let p = settings.palette.get();
                apply_to_dom(m, p);
                persist_all(settings);
            }
        });

        settings
    }
}

/// Provide ThemeSettings into context if not already present and return it.
pub fn provide_theme_settings() -> ThemeSettings {
    if let Some(existing) = use_context::<ThemeSettings>() {
        return existing;
    }
    let settings = ThemeSettings::new();
    provide_context(settings);
    settings
}

fn persist_all(settings: ThemeSettings) {
    let mode_str = settings.mode.get_untracked().as_str().to_string();
    let palette_str = settings.palette.get_untracked().as_str().to_string();

    // localStorage
    #[cfg(feature = "hydrate")]
    {
        if let Some(win) = web_sys::window()
            && let Ok(Some(storage)) = win.local_storage()
        {
            let _ = storage.set_item("theme.mode", &mode_str);
            let _ = storage.set_item("theme.palette", &palette_str);
        }
    }

    // Cookie (if Cookies context is available)
    if let Some(cookies) = use_context::<Cookies>() {
        let (_m_sig, set_mode_cookie) = cookies.use_cookie_typed::<_, String>("theme_mode");
        set_mode_cookie(Some(mode_str));
        let (_p_sig, set_palette_cookie) = cookies.use_cookie_typed::<_, String>("theme_palette");
        set_palette_cookie(Some(palette_str));
    }
}

fn load_mode_from_storage() -> Option<ThemeMode> {
    // Priority: localStorage -> cookie -> None
    #[cfg(feature = "hydrate")]
    {
        if let Some(win) = web_sys::window()
            && let Ok(Some(storage)) = win.local_storage()
            && let Ok(Some(value)) = storage.get_item("theme.mode")
            && let Ok(mode) = ThemeMode::from_str(&value)
        {
            return Some(mode);
        }
    }

    if let Some(cookies) = use_context::<Cookies>() {
        let (sig, _setter) = cookies.use_cookie_typed::<_, String>("theme_mode");
        if let Some(val) = sig.get_untracked()
            && let Ok(mode) = ThemeMode::from_str(&val)
        {
            return Some(mode);
        }
    }

    None
}

fn load_palette_from_storage() -> Option<ThemePalette> {
    // Priority: localStorage -> cookie -> None
    #[cfg(feature = "hydrate")]
    {
        if let Some(win) = web_sys::window()
            && let Ok(Some(storage)) = win.local_storage()
            && let Ok(Some(value)) = storage.get_item("theme.palette")
            && let Ok(palette) = ThemePalette::from_str(&value)
        {
            return Some(palette);
        }
    }

    if let Some(cookies) = use_context::<Cookies>() {
        let (sig, _setter) = cookies.use_cookie_typed::<_, String>("theme_palette");
        if let Some(val) = sig.get_untracked()
            && let Ok(palette) = ThemePalette::from_str(&val)
        {
            return Some(palette);
        }
    }

    None
}

fn apply_to_dom(
    #[allow(unused_variables)] mode: ThemeMode,
    #[allow(unused_variables)] palette: ThemePalette,
) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(el) = doc.document_element()
        {
            // Resolve system to light/dark
            let resolved = match mode {
                ThemeMode::Light => "light",
                ThemeMode::Dark => "dark",
                ThemeMode::System => {
                    match web_sys::window()
                        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok())
                        .flatten()
                        .and_then(|mq| {
                            js_sys::Reflect::get(
                                mq.as_ref(),
                                &wasm_bindgen::JsValue::from_str("matches"),
                            )
                            .ok()
                            .and_then(|v| v.as_bool())
                        }) {
                        Some(true) => "dark",
                        _ => "light",
                    }
                }
            };
            if let Err(e) = el.set_attribute("data-theme", resolved) {
                warn!("failed to set data-theme: {:?}", e.as_string());
            }
            if let Err(e) = el.set_attribute("data-palette", palette.as_str()) {
                warn!("failed to set data-palette: {:?}", e.as_string());
            }

            // For debugging in dev
            debug!(
                "applied theme to DOM => mode: {:?} (resolved: {}), palette: {}",
                mode,
                resolved,
                palette.as_str()
            );

            // Also update a <meta name="theme-color"> if present for PWA feel
            if let Some(meta_list) = doc
                .get_elements_by_name("theme-color")
                .dyn_into::<web_sys::NodeList>()
                .ok()
                && meta_list.length() > 0
                && let Some(node) = meta_list.item(0)
                && let Some(meta) = node.dyn_ref::<web_sys::HtmlMetaElement>()
            {
                // heuristic background based on mode
                let color = if resolved == "light" {
                    "#f8fafc"
                } else {
                    "#0f0710"
                };
                meta.set_content(color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_theme_mode_from_str() {
        assert_eq!(ThemeMode::from_str("system").unwrap(), ThemeMode::System);
        assert_eq!(ThemeMode::from_str("SYSTEM").unwrap(), ThemeMode::System); // case-insensitive
        assert_eq!(ThemeMode::from_str("dark").unwrap(), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_str("light").unwrap(), ThemeMode::Light);
        assert_eq!(ThemeMode::from_str("unknown").unwrap(), ThemeMode::Dark); // fallback
        assert_eq!(ThemeMode::from_str("").unwrap(), ThemeMode::Dark); // empty fallback
    }

    #[test]
    fn palette_catalog_round_trips_and_has_a_visible_default() {
        assert!(ThemePalette::ALL.contains(&ThemePalette::default()));
        for palette in ThemePalette::ALL {
            assert_eq!(palette.as_str().parse(), Ok(palette));
            assert_eq!(palette.as_str().to_uppercase().parse(), Ok(palette));
        }
    }

    #[test]
    fn saved_generic_names_migrate_to_named_themes() {
        for (saved, expected) in [
            ("violet", ThemePalette::Ultros),
            ("TeAl", ThemePalette::Limsa),
            ("emerald", ThemePalette::TwinAdder),
            ("amber", ThemePalette::Uldah),
            ("rose", ThemePalette::Ascian),
            ("sky", ThemePalette::Ishgard),
            ("unknown", ThemePalette::Ultros),
            ("", ThemePalette::Ultros),
        ] {
            assert_eq!(saved.parse(), Ok(expected));
        }
    }
}
