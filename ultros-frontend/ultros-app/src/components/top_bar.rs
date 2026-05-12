//! Slim application topbar: hamburger (mobile), search box, and a
//! right-aligned cluster of global controls (language picker, theme
//! toggle, user menu).

use crate::components::apps_menu::UserMenu;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguageNavMenu;
use crate::components::search_box::SearchBox;
use crate::components::theme_picker::QuickThemeToggle;
use crate::global_state::side_nav::use_side_nav_settings;
use icondata as i;
use leptos::prelude::*;

/// Slim topbar: hamburger (mobile), search, then global controls
/// (language, theme, user). 56px tall.
#[component]
pub fn TopBar() -> impl IntoView {
    let nav = use_side_nav_settings();

    view! {
        <header class="top-bar" role="banner">
            <button
                class="top-bar-hamburger"
                aria-label="Toggle navigation"
                aria-expanded=move || if nav.drawer_open.get() { "true" } else { "false" }
                on:click=move |_| nav.drawer_open.update(|v| *v = !*v)
            >
                <Icon icon=i::AiMenuOutlined width="1.4em" height="1.4em" />
            </button>

            <div class="top-bar-search">
                <SearchBox />
            </div>

            <div class="top-bar-actions">
                <div class="hidden md:block">
                    <LanguageNavMenu />
                </div>
                <div class="hidden md:block">
                    <QuickThemeToggle />
                </div>
                <UserMenu />
            </div>
        </header>
    }
    .into_any()
}
