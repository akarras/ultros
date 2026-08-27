use crate::components::ad::DesktopAdRail;
use crate::components::app_link::provide_router_available;
use crate::components::mobile_bar::MobileBar;
use crate::components::search_overlay::SearchOverlay;
use crate::components::side_nav::SideNav;
use crate::global_state::search_overlay::provide_search_overlay_state;
use crate::global_state::side_nav::provide_side_nav_settings;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

/// Application shell: persistent sidebar + fluid content + optional ad
/// rail. Mobile collapses the sidebar into a hamburger-toggled overlay
/// drawer and adds a fixed bottom bar (Menu, Search, Items).
#[component]
pub fn AppShell(children: Children) -> impl IntoView {
    // Everything below here renders inside `<Router>`; `AppLink` reads this
    // marker to tell a live render from one whose owner has been disposed.
    // See `components::app_link`.
    provide_router_available();
    let nav = provide_side_nav_settings();
    provide_search_overlay_state();
    let location = use_location();

    // Dismiss the mobile drawer on any navigation.
    Effect::new(move |_| {
        let _ = location.pathname.get();
        nav.drawer_open.set(false);
    });

    // Escape closes the drawer.
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" && nav.drawer_open.get_untracked() {
            nav.drawer_open.set(false);
        }
    };

    let drawer_open = nav.drawer_open;
    let collapsed = nav.collapsed;

    let shell_classes = move || {
        let mut classes = String::from("app-shell");
        if collapsed.get() {
            classes.push_str(" app-shell-collapsed");
        }
        if drawer_open.get() {
            classes.push_str(" app-shell-drawer-open");
        }
        classes
    };

    view! {
        <div class=shell_classes on:keydown=on_keydown>
            <SideNav />

            <div
                class="app-shell-backdrop"
                aria-hidden="true"
                on:click=move |_| drawer_open.set(false)
            />

            <main class="app-shell-content" role="main">
                {children()}
            </main>

            <MobileBar />

            <div class="app-shell-ad-rail">
                <DesktopAdRail />
            </div>

            <SearchOverlay />
        </div>
    }
    .into_any()
}
