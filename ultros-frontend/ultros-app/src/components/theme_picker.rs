use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;

use crate::global_state::theme::{ThemeMode, ThemePalette, provide_theme_settings};

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
        <div class="panel p-6 rounded-xl space-y-6">
            <div class="space-y-2">
                <h3 class="text-2xl font-bold text-[color:var(--brand-fg)]">"Theme"</h3>
                <p class="text-sm text-[color:var(--color-text-muted)]">"choose your vibe and brand color palette"</p>
            </div>

            <div class="space-y-4">
                <div class="space-y-2">
                    <div class="text-[color:var(--brand-fg)] font-semibold">"Mode"</div>
                    <div class="flex flex-wrap gap-2">
                        {mode_button("Dark", ThemeMode::Dark)}
                        {mode_button("Light", ThemeMode::Light)}
                        {mode_button("System", ThemeMode::System)}
                    </div>
                </div>

                <div class="space-y-2">
                    <div class="text-[color:var(--brand-fg)] font-semibold">"Palette"</div>
                    <div class="flex flex-wrap gap-2">
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
