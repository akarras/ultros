use crate::components::ad::DesktopAdRail;
use crate::components::app_link::provide_router_available;
use crate::components::mobile_bar::MobileBar;
use crate::components::search_overlay::SearchOverlay;
use crate::components::side_nav::SideNav;
use crate::global_state::search_overlay::provide_search_overlay_state;
use crate::global_state::side_nav::provide_side_nav_settings;
use leptos::prelude::*;
use leptos_router::hooks::use_location;
use ultros_ui::components::dialog::DialogSurface;

/// Application shell: persistent sidebar + fluid content + optional ad
/// rail. Mobile collapses the sidebar into a hamburger-toggled overlay
/// drawer and adds a fixed bottom bar (Menu, Search, Items).
#[component]
pub fn AppShell(children: Children) -> impl IntoView {
    // Everything below here renders inside `<Router>`, so this is the one
    // place the router's `Location` can be read without risking the `expect`
    // in `use_location()`. Publishing it here is what lets
    // `use_location_or_default` degrade instead of panicking under a disposed
    // owner. See `components::app_link`.
    provide_router_available();
    crate::last_view::track_last_view();
    let i18n = crate::i18n::use_i18n();
    let main = NodeRef::<leptos::html::Main>::new();
    let nav = provide_side_nav_settings();
    provide_search_overlay_state();
    let location = use_location();

    // A new page gets a focus destination after dialogs have closed. Query-only
    // filter changes keep the user's current control focused.
    Effect::new(move |previous: Option<String>| {
        let path = location.pathname.get();
        nav.drawer_open.set(false);
        #[cfg(feature = "hydrate")]
        if previous.as_ref().is_some_and(|previous| previous != &path) {
            request_animation_frame(move || {
                use wasm_bindgen::JsCast;
                if let Some(main) = document()
                    .get_element_by_id("main-content")
                    .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    let _ = main.focus();
                }
            });
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = previous;
        path
    });
    #[cfg(feature = "hydrate")]
    {
        let desktop = leptos_use::use_media_query("(min-width: 1024px)");
        Effect::new(move |_| {
            if desktop.get() {
                nav.drawer_open.set(false);
            }
        });
    }

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
        <div class=shell_classes>
            <a class="skip-content" href="#main-content" on:click=move |event: leptos::ev::MouseEvent| { event.prevent_default(); if let Some(main) = main.get_untracked() { let _ = main.focus(); } }>{crate::i18n::t!(i18n, a11y_skip_content)}</a>
            <SideNav />

            <Show when=move || drawer_open.get()>
                <DialogSurface class="nav-dialog" set_visible=drawer_open.write_only()
                    aria_label=Some(Signal::derive(move || crate::i18n::t_string!(i18n, side_nav_aria_primary).to_string()))>
                    <button type="button" class="btn-secondary m-2" on:click=move |_| drawer_open.set(false)>{crate::i18n::t!(i18n, close)}</button>
                    <SideNav />
                </DialogSurface>
            </Show>

            <main node_ref=main id="main-content" tabindex="-1" class="app-shell-content" role="main">
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
