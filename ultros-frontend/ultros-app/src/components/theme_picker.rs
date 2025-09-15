use icondata as i;
use leptos::prelude::*;
use leptos_icons::Icon;

use crate::global_state::theme::{
    provide_theme_settings, use_theme_settings, ThemeMode, ThemePalette,
};

#[component]
pub fn ThemePicker() -> impl IntoView {
    // ensure settings exist in context
    let settings = provide_theme_settings();

    let mode = settings.mode;
    let palette = settings.palette;

    let set_mode = move |m: ThemeMode| mode.set(m);
    let set_palette = move |p: ThemePalette| palette.set(p);

    let mode_button = move |label: &'static str, val: ThemeMode| {
        let is_active = Signal::derive(move || mode.get() == val);
        view! {
            <button
                class=move || {
                    if is_active() {
                        "btn-primary"
                    } else {
                        "btn-secondary"
                    }
                }
                on:click=move |_| set_mode(val)
            >
                {label}
            </button>
        }
    };

    let palette_button = move |label: &'static str, val: ThemePalette| {
        let is_active = Signal::derive(move || palette.get() == val);
        view! {
            <button
                class=move || {
                    if is_active() {
                        "btn-primary"
                    } else {
                        "btn-secondary"
                    }
                }
                on:click=move |_| set_palette(val)
            >
                {label}
            </button>
        }
    };

    view! {
        <div class="p-6 rounded-xl bg-gradient-to-br from-brand-950/10 to-black/20 border border-white/10 space-y-6">
            <div class="space-y-2">
                <h3 class="text-2xl font-bold text-brand-300">"Theme"</h3>
                <p class="text-sm text-gray-400">"choose your vibe and brand color palette"</p>
            </div>

            <div class="space-y-4">
                <div class="space-y-2">
                    <div class="text-brand-200 font-semibold">"Mode"</div>
                    <div class="flex flex-wrap gap-2">
                        {mode_button("Dark", ThemeMode::Dark)}
                        {mode_button("Light", ThemeMode::Light)}
                        {mode_button("System", ThemeMode::System)}
                    </div>
                </div>

                <div class="space-y-2">
                    <div class="text-brand-200 font-semibold">"Palette"</div>
                    <div class="flex flex-wrap gap-2">
                        {palette_button("Violet", ThemePalette::Violet)}
                        {palette_button("Teal", ThemePalette::Teal)}
                        {palette_button("Emerald", ThemePalette::Emerald)}
                        {palette_button("Amber", ThemePalette::Amber)}
                        {palette_button("Rose", ThemePalette::Rose)}
                        {palette_button("Sky", ThemePalette::Sky)}
                    </div>
                </div>
            </div>

            <div class="divider"></div>

            <div class="flex items-center gap-2 text-sm text-gray-400">
                <Icon icon=i::BiInfoCircleRegular />
                <span>
                    "theme is saved to your browser and applied instantly. no reloads, no drama."
                </span>
            </div>
        </div>
    }
    .into_any()
}

/// A compact button for the navbar that cycles theme mode Dark -> Light -> System
#[component]
pub fn QuickThemeToggle() -> impl IntoView {
    let settings = provide_theme_settings();
    let mode = settings.mode;

    let icon = Signal::derive(move || match mode.get() {
        ThemeMode::Dark => i::BiMoonRegular,
        ThemeMode::Light => i::BiSunRegular,
        ThemeMode::System => i::BiLaptopRegular,
    });

    let label = Signal::derive(move || match mode.get() {
        ThemeMode::Dark => "Dark",
        ThemeMode::Light => "Light",
        ThemeMode::System => "System",
    });

    let cycle = move || {
        let next = match mode.get_untracked() {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::System,
            ThemeMode::System => ThemeMode::Dark,
        };
        mode.set(next);
    };

    view! {
        <button
            class="nav-link"
            title="Toggle theme"
            on:click=move |_| cycle()
        >
            <Icon icon=icon />
            <span class="hidden lg:inline">{label}</span>
        </button>
    }
}
