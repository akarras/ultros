//! Slim application topbar: hamburger (mobile), search box, and a
//! right-aligned cluster of global controls (language picker, theme
//! toggle, user menu).

use crate::components::apps_menu::UserMenu;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguageNavMenu;
use crate::components::search_box::SearchBox;
use crate::components::theme_picker::QuickThemeToggle;
use crate::global_state::side_nav::use_side_nav_settings;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;

/// Slim topbar: hamburger (mobile), search, then global controls
/// (language, theme, user). 56px tall.
#[component]
pub fn TopBar() -> impl IntoView {
    let i18n = use_i18n();
    let nav = use_side_nav_settings();
    let user = Resource::new(
        move || {},
        move |_| async move { crate::api::get_login().await.ok() },
    );

    view! {
        <header class="top-bar" role="banner">
            <button
                class="top-bar-hamburger"
                aria-label=t_string!(i18n, side_nav_toggle_navigation).to_string()
                aria-expanded=move || if nav.drawer_open.get() { "true" } else { "false" }
                on:click=move |_| nav.drawer_open.update(|v| *v = !*v)
            >
                <Icon icon=i::AiMenuOutlined width="1.4em" height="1.4em" />
            </button>

            <div class="top-bar-search">
                <SearchBox />
            </div>

            <div class="top-bar-actions">
                <Suspense fallback=move || view! { <div class="w-24 h-8 bg-zinc-800/50 animate-pulse rounded-lg"></div> }>
                    {move || {
                        let u = user.get().flatten();
                        u.is_none().then(|| view! {
                            <a
                                rel="external"
                                href="/login"
                                class="btn-primary py-1.5 px-3 text-sm flex items-center gap-1.5 whitespace-nowrap !rounded-lg"
                            >
                                <Icon icon=i::BsDiscord width="1.15em" height="1.15em" />
                                <span>{t!(i18n, login_with_discord)}</span>
                            </a>
                        })
                    }}
                </Suspense>
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
