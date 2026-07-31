//! Fixed bottom bar, below 1024px only.
//!
//! Three slots — Menu, Search, Items. Buttons only, never a text input:
//! focusing an input inside a `position: fixed; bottom: 0` element leaves
//! it behind the iOS virtual keyboard, because Safari shrinks the visual
//! viewport without shrinking the layout viewport. Search therefore opens
//! the overlay, which anchors its input to the top of the sheet.
//!
//! Account is deliberately absent — it lives in the sidebar footer
//! drop-up, reached through the Menu slot. This is an accepted trade: the
//! old top bar showed a persistent sign-in button on phones.

use crate::components::icon::Icon;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::global_state::side_nav::use_side_nav_settings;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use leptos_router::components::A;

#[component]
pub fn MobileBar() -> impl IntoView {
    let i18n = use_i18n();
    let nav = use_side_nav_settings();
    let search_overlay = use_search_overlay_state();

    view! {
        <nav class="mobile-bar" aria-label=t_string!(i18n, side_nav_aria_primary)>
            <button
                class="mobile-bar-slot"
                // WCAG 2.5.3 Label in Name: the visible label below is
                // "Menu" (the `menu` key), so the accessible name has to
                // start with/equal that text rather than the more
                // descriptive but mismatched "Toggle navigation".
                aria-label=t_string!(i18n, menu).to_string()
                aria-expanded=move || if nav.drawer_open.get() { "true" } else { "false" }
                on:click=move |_| nav.drawer_open.update(|v| *v = !*v)
            >
                <Icon icon=i::AiMenuOutlined width="1.4em" height="1.4em" />
                <span class="mobile-bar-label">{t!(i18n, menu)}</span>
            </button>

            <button
                class="mobile-bar-slot"
                aria-label=t_string!(i18n, search).to_string()
                on:click=move |_| search_overlay.toggle()
            >
                <Icon icon=i::AiSearchOutlined width="1.4em" height="1.4em" />
                <span class="mobile-bar-label">{t!(i18n, search)}</span>
            </button>

            <A href="/items" attr:class="mobile-bar-slot">
                <Icon icon=i::MdiJellyfish width="1.4em" height="1.4em" />
                <span class="mobile-bar-label">{t!(i18n, items)}</span>
            </A>
        </nav>
    }
    .into_any()
}
