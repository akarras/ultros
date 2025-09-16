use leptos::prelude::*;
use log::{debug, warn};
use std::str::FromStr;

use crate::global_state::cookies::Cookies;

/// The visual theme mode of the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

impl Default for ThemeMode {
    fn default() -> Self {
        ThemeMode::Dark
    }
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
            "dark" | _ => ThemeMode::Dark,
        })
    }
}

/// The brand color palette used throughout the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePalette {
    Violet,
    Teal,
    Emerald,
    Amber,
    Rose,
    Sky,
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

impl Default for ThemePalette {
    fn default() -> Self {
        ThemePalette::Violet
    }
}

impl ThemePalette {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemePalette::Violet => "violet",
            ThemePalette::Teal => "teal",
            ThemePalette::Emerald => "emerald",
            ThemePalette::Amber => "amber",
            ThemePalette::Rose => "rose",
            ThemePalette::Sky => "sky",
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
            "teal" => ThemePalette::Teal,
            "emerald" => ThemePalette::Emerald,
            "amber" => ThemePalette::Amber,
            "rose" => ThemePalette::Rose,
            "sky" => ThemePalette::Sky,
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
            "violet" | _ => ThemePalette::Violet,
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
            let settings = settings.clone();
            move |_| {
                let m = settings.mode.get();
                let p = settings.palette.get();
                apply_to_dom(m, p);
                persist_all(settings);
            }
        });

        settings
    }

    pub fn set_mode(&self, mode: ThemeMode) {
        self.mode.set(mode);
    }

    pub fn set_palette(&self, palette: ThemePalette) {
        self.palette.set(palette);
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

/// Retrieve ThemeSettings from context. Panics if not provided.
pub fn use_theme_settings() -> ThemeSettings {
    use_context::<ThemeSettings>().expect("ThemeSettings not provided")
}

fn persist_all(settings: ThemeSettings) {
    let mode_str = settings.mode.get().as_str().to_string();
    let palette_str = settings.palette.get().as_str().to_string();

    // localStorage
    #[cfg(feature = "hydrate")]
    {
        if let Some(win) = web_sys::window() {
            if let Ok(Some(storage)) = win.local_storage() {
                let _ = storage.set_item("theme.mode", &mode_str);
                let _ = storage.set_item("theme.palette", &palette_str);
            }
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
        if let Some(win) = web_sys::window() {
            if let Ok(Some(storage)) = win.local_storage() {
                if let Ok(Some(value)) = storage.get_item("theme.mode") {
                    if let Ok(mode) = ThemeMode::from_str(&value) {
                        return Some(mode);
                    }
                }
            }
        }
    }

    if let Some(cookies) = use_context::<Cookies>() {
        let (sig, _setter) = cookies.use_cookie_typed::<_, String>("theme_mode");
        if let Some(val) = sig.get_untracked() {
            if let Ok(mode) = ThemeMode::from_str(&val) {
                return Some(mode);
            }
        }
    }

    None
}

fn load_palette_from_storage() -> Option<ThemePalette> {
    // Priority: localStorage -> cookie -> None
    #[cfg(feature = "hydrate")]
    {
        if let Some(win) = web_sys::window() {
            if let Ok(Some(storage)) = win.local_storage() {
                if let Ok(Some(value)) = storage.get_item("theme.palette") {
                    if let Ok(palette) = ThemePalette::from_str(&value) {
                        return Some(palette);
                    }
                }
            }
        }
    }

    if let Some(cookies) = use_context::<Cookies>() {
        let (sig, _setter) = cookies.use_cookie_typed::<_, String>("theme_palette");
        if let Some(val) = sig.get_untracked() {
            if let Ok(palette) = ThemePalette::from_str(&val) {
                return Some(palette);
            }
        }
    }

    None
}

fn apply_to_dom(mode: ThemeMode, palette: ThemePalette) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Some(el) = doc.document_element() {
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
                {
                    if meta_list.length() > 0 {
                        if let Some(node) = meta_list.item(0) {
                            if let Some(meta) = node.dyn_ref::<web_sys::HtmlMetaElement>() {
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
            }
        }
    }
}
