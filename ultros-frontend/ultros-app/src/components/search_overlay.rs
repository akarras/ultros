//! Global search overlay.
//!
//! Owns the `Cmd`/`Ctrl`+K hotkey (moved here out of [`SearchBox`], which
//! could only handle it while a single instance was permanently mounted in
//! the old top bar).
//!
//! On mobile the input is anchored to the **top** of the sheet. This is
//! load-bearing, not cosmetic: iOS Safari shrinks the visual viewport but
//! not the layout viewport when the keyboard opens, so a bottom-anchored
//! input ends up behind the keyboard with no pure-CSS remedy.

use crate::components::search_box::SearchBox;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::i18n::{t_string, use_i18n};
use leptos::prelude::*;
use leptos_hotkeys::use_hotkeys;
use leptos_router::hooks::use_location;

#[component]
pub fn SearchOverlay() -> impl IntoView {
    let i18n = use_i18n();
    let state = use_search_overlay_state();
    let open = state.open;

    use_hotkeys!(("MetaLeft+KeyK,ControlLeft+KeyK", "*") => move |_| {
        state.toggle();
    });

    // Any navigation dismisses the overlay — selecting a result routes, and
    // leaving the sheet up over the destination would be a trap.
    let location = use_location();
    Effect::new(move |_| {
        let _ = location.pathname.get();
        state.close();
    });

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Escape" {
            state.close();
        }
    };

    view! {
        <Show when=move || open.get()>
            // No `aria-modal="true"`: focus is not trapped and background
            // content is not inert, so claiming it would tell assistive tech
            // to ignore a page it can still tab into. Add the attribute when
            // a real focus trap lands — the existing `Modal` component has
            // the same gap and should get one at the same time.
            <div
                class="search-overlay"
                role="dialog"
                aria-label=t_string!(i18n, search).to_string()
                on:keydown=on_keydown
            >
                <div
                    class="search-overlay-backdrop"
                    on:click=move |_| state.close()
                />
                <div class="search-overlay-panel">
                    <SearchBox autofocus=true />
                </div>
            </div>
        </Show>
    }
    .into_any()
}
