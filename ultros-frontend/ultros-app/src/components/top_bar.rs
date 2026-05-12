//! Slim application topbar: hamburger (mobile), breadcrumb (desktop),
//! search box, and a right-aligned cluster of global controls
//! (language picker, theme toggle, user menu). The breadcrumb is
//! derived from the current route via `breadcrumb_for_path`.

use crate::components::apps_menu::UserMenu;
use crate::components::icon::Icon;
use crate::components::language_picker::LanguagePicker;
use crate::components::search_box::SearchBox;
use crate::components::theme_picker::QuickThemeToggle;
use crate::global_state::side_nav::use_side_nav_settings;
use icondata as i;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Breadcrumb {
    pub(crate) section: &'static str,
    pub(crate) trailing: Option<String>,
}

pub(crate) fn breadcrumb_for_path(path: &str) -> Breadcrumb {
    let mut parts = path.trim_start_matches('/').split('/');
    let first = parts.next().unwrap_or("");
    let rest: Vec<_> = parts.filter(|s| !s.is_empty()).collect();
    let trailing = if rest.is_empty() {
        None
    } else {
        Some(rest.join(" / "))
    };
    let section = match first {
        "" => "Home",
        "flip-finder" | "analyzer" => "Flip Finder",
        "vendor-resale" => "Vendor Resale",
        "recipe-analyzer" => "Recipe Analyzer",
        "fc-crafting-analyzer" => "FC Crafting",
        "leve-analyzer" => "Leve Analyzer",
        "trends" => "Market Trends",
        "scrip-sources" => "Scrip Sources",
        "venture-analyzer" => "Venture Analyzer",
        "items" => "Item Explorer",
        "item" => "Item",
        "currency-exchange" => "Currency Exchange",
        "list" => "Lists",
        "retainers" => "Retainers",
        "alerts" => "Alerts",
        "settings" => "Settings",
        "profile" => "Profile",
        "history" => "History",
        "welcome" => "Welcome",
        "about" => "About",
        "privacy" | "cookie-policy" => "Legal",
        _ => "Ultros",
    };
    Breadcrumb { section, trailing }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root() {
        let b = breadcrumb_for_path("/");
        assert_eq!(
            b,
            Breadcrumb {
                section: "Home",
                trailing: None
            }
        );
    }

    #[test]
    fn flip_finder_with_world() {
        let b = breadcrumb_for_path("/flip-finder/Gilgamesh");
        assert_eq!(b.section, "Flip Finder");
        assert_eq!(b.trailing.as_deref(), Some("Gilgamesh"));
    }

    #[test]
    fn legacy_analyzer_alias_maps_to_flip_finder() {
        let b = breadcrumb_for_path("/analyzer/Goblin");
        assert_eq!(b.section, "Flip Finder");
    }

    #[test]
    fn item_with_world_and_id() {
        let b = breadcrumb_for_path("/item/Gilgamesh/12345");
        assert_eq!(b.section, "Item");
        assert_eq!(b.trailing.as_deref(), Some("Gilgamesh / 12345"));
    }

    #[test]
    fn unknown_route_falls_back() {
        let b = breadcrumb_for_path("/blarghl");
        assert_eq!(b.section, "Ultros");
        assert_eq!(b.trailing, None);
    }

    #[test]
    fn no_leading_slash() {
        let b = breadcrumb_for_path("settings");
        assert_eq!(b.section, "Settings");
    }

    #[test]
    fn trailing_slash_produces_no_trailing() {
        let b = breadcrumb_for_path("/flip-finder/");
        assert_eq!(b.section, "Flip Finder");
        assert_eq!(b.trailing, None);
    }

    #[test]
    fn double_slash_filters_empty_segments() {
        let b = breadcrumb_for_path("/item//12345");
        assert_eq!(b.section, "Item");
        assert_eq!(b.trailing.as_deref(), Some("12345"));
    }
}

/// Slim topbar: hamburger (mobile), breadcrumb (desktop), search, then
/// global controls (language, theme, user). 56px tall.
#[component]
pub fn TopBar() -> impl IntoView {
    let location = use_location();
    let breadcrumb = Signal::derive(move || location.pathname.with(|p| breadcrumb_for_path(p)));
    let nav = use_side_nav_settings();

    view! {
        <header class="top-bar" role="banner">
            <button
                class="top-bar-hamburger lg:hidden"
                aria-label="Toggle navigation"
                aria-expanded=move || if nav.drawer_open.get() { "true" } else { "false" }
                on:click=move |_| nav.drawer_open.update(|v| *v = !*v)
            >
                <Icon icon=i::AiMenuOutlined width="1.4em" height="1.4em" />
            </button>

            <div class="top-bar-breadcrumb hidden lg:flex">
                {move || {
                    let b = breadcrumb.get();
                    view! {
                        <span class="top-bar-section">{b.section}</span>
                        {b.trailing.map(|t| view! {
                            <span class="top-bar-divider">"/"</span>
                            <span class="top-bar-trailing">{t}</span>
                        })}
                    }
                }}
            </div>

            <div class="top-bar-search">
                <SearchBox />
            </div>

            <div class="top-bar-actions">
                <div class="hidden md:block">
                    <LanguagePicker />
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
