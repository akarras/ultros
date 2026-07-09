use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;

use crate::global_state::theme::{ThemeMode, ThemePalette, provide_theme_settings};

pub fn theme_mode_icon(mode: ThemeMode) -> i::Icon {
    match mode {
        ThemeMode::Dark => i::BiMoonRegular,
        ThemeMode::Light => i::BiSunRegular,
        ThemeMode::System => i::BiLaptopRegular,
    }
}

pub fn next_theme_mode(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::Dark => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::System,
        ThemeMode::System => ThemeMode::Dark,
    }
}

#[component]
pub fn ThemePicker() -> impl IntoView {
    let i18n = use_i18n();
    // ensure settings exist in context
    let settings = provide_theme_settings();

    let mode = settings.mode;
    let palette = settings.palette;

    let set_mode = move |m: ThemeMode| mode.set(m);
    let set_palette = move |p: ThemePalette| palette.set(p);

    let mode_button = move |label: Signal<String>, val: ThemeMode| {
        let is_active = Signal::derive(move || mode.get() == val);
        view! {
            <button
                role="radio"
                class=move || {
                    if is_active() {
                        "btn-primary"
                    } else {
                        "btn-secondary"
                    }
                }
                aria-checked=move || is_active().to_string()
                on:click=move |_| set_mode(val)
            >
                {move || label.get()}
            </button>
        }
    };

    let palette_button = move |label: &'static str, val: ThemePalette| {
        let is_active = Signal::derive(move || palette.get() == val);
        view! {
            <button
                role="radio"
                class=move || {
                    if is_active() {
                        "btn-primary"
                    } else {
                        "btn-secondary"
                    }
                }
                aria-checked=move || is_active().to_string()
                on:click=move |_| set_palette(val)
            >
                {label}
            </button>
        }
    };

    view! {
        <div class="panel p-6 rounded-xl space-y-6">
            <div class="space-y-2">
                <h3 class="text-2xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, theme_title)}</h3>
                <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, theme_subtitle)}</p>
            </div>

            <div class="space-y-4">
                <div class="space-y-2">
                    <div class="text-[color:var(--brand-fg)] font-semibold" id="theme-mode-label">{t!(i18n, theme_mode_label)}</div>
                    <div class="flex flex-wrap gap-2" role="radiogroup" aria-labelledby="theme-mode-label">
                        {mode_button(Signal::derive(move || t_string!(i18n, theme_mode_dark).to_string()), ThemeMode::Dark)}
                        {mode_button(Signal::derive(move || t_string!(i18n, theme_mode_light).to_string()), ThemeMode::Light)}
                        {mode_button(Signal::derive(move || t_string!(i18n, theme_mode_system).to_string()), ThemeMode::System)}
                    </div>
                </div>

                <div class="space-y-2">
                    <div class="text-[color:var(--brand-fg)] font-semibold" id="theme-palette-label">{t!(i18n, theme_palette_label)}</div>
                    <div class="flex flex-wrap gap-2" role="radiogroup" aria-labelledby="theme-palette-label">
                        {palette_button("Ultros", ThemePalette::Ultros)}
                        {palette_button("Maelstrom", ThemePalette::Maelstrom)}
                        {palette_button("Twin Adder", ThemePalette::TwinAdder)}
                        {palette_button("Ascian", ThemePalette::Ascian)}
                        {palette_button("Ishgard", ThemePalette::Ishgard)}
                        {palette_button("Crystarium", ThemePalette::Crystarium)}
                        {palette_button("Sharlayan", ThemePalette::Sharlayan)}
                        {palette_button("Tuliyollal", ThemePalette::Tuliyollal)}
                        {palette_button("Immortal Flames", ThemePalette::ImmortalFlames)}
                        {palette_button("Ul'dah", ThemePalette::Uldah)}
                        {palette_button("Limsa", ThemePalette::Limsa)}
                        {palette_button("Garlemald", ThemePalette::Garlemald)}
                    </div>
                </div>
            </div>

            <div class="divider"></div>

            <div class="flex items-center gap-2 text-sm text-gray-400">
                <Icon icon=i::BiInfoCircleRegular />
                <span>
                    {t!(i18n, theme_persistence_note)}
                </span>
            </div>
        </div>
    }
    .into_any()
}

/// A compact button for the navbar that cycles theme mode Dark -> Light -> System
#[component]
pub fn QuickThemeToggle() -> impl IntoView {
    let i18n = use_i18n();
    let settings = provide_theme_settings();
    let mode = settings.mode;

    let icon = Signal::derive(move || theme_mode_icon(mode.get()));

    let label = Signal::derive(move || match mode.get() {
        ThemeMode::Dark => t_string!(i18n, theme_mode_dark).to_string(),
        ThemeMode::Light => t_string!(i18n, theme_mode_light).to_string(),
        ThemeMode::System => t_string!(i18n, theme_mode_system).to_string(),
    });

    let cycle = move || {
        mode.set(next_theme_mode(mode.get_untracked()));
    };

    view! {
        <button
            class="nav-link"
            title=move || t_string!(i18n, theme_toggle_title).to_string()
            aria-label=move || t_string!(i18n, theme_toggle_aria, theme = label.get()).to_string()
            on:click=move |_| cycle()
        >
            <Icon icon=icon />
            <span class="hidden lg:inline">{label}</span>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_mode_icon_mapping() {
        // We ensure that each theme mode maps to the correct visual representation
        assert_eq!(theme_mode_icon(ThemeMode::Dark), i::BiMoonRegular);
        assert_eq!(theme_mode_icon(ThemeMode::Light), i::BiSunRegular);
        assert_eq!(theme_mode_icon(ThemeMode::System), i::BiLaptopRegular);
    }

    #[test]
    fn test_next_theme_mode_cycle() {
        // We ensure the toggle cycles correctly: Dark -> Light -> System -> Dark
        assert_eq!(next_theme_mode(ThemeMode::Dark), ThemeMode::Light);
        assert_eq!(next_theme_mode(ThemeMode::Light), ThemeMode::System);
        assert_eq!(next_theme_mode(ThemeMode::System), ThemeMode::Dark);
    }
}
