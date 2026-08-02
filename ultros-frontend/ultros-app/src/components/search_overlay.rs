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

use crate::components::icon::Icon;
use crate::components::search_box::SearchBox;
use crate::global_state::search_overlay::use_search_overlay_state;
use crate::i18n::{t_string, use_i18n};
use icondata as i;
use leptos::html::Div;
use leptos::prelude::*;
use leptos_hotkeys::use_hotkeys;
use leptos_router::hooks::use_location;

#[component]
pub fn SearchOverlay() -> impl IntoView {
    let i18n = use_i18n();
    let state = use_search_overlay_state();
    let open = state.open;
    let panel = NodeRef::<Div>::new();

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
                // On mobile the panel is an opaque full-viewport sheet, so
                // the backdrop underneath can never be tapped — instead a tap
                // on the sheet's own empty space dismisses it (#1067).
                // Matching on the event target rather than stopping
                // propagation in the header keeps this independent of how
                // Leptos's delegated click handling bubbles: only a hit on
                // the panel's own box counts, so the input, its clear button
                // and the absolutely-positioned result/hint dropdowns are all
                // untouched.
                <div
                    node_ref=panel
                    class="search-overlay-panel"
                    on:click=move |ev: leptos::ev::MouseEvent| {
                        let hit_panel = ev
                            .target()
                            .zip(panel.get_untracked())
                            .is_some_and(|(target, panel)| {
                                let panel: &web_sys::EventTarget = &panel;
                                &target == panel
                            });
                        if hit_panel {
                            state.close();
                        }
                    }
                >
                    <div class="search-overlay-header">
                        <div class="search-overlay-searchbox">
                            <SearchBox autofocus=true />
                        </div>
                        // Explicit close affordance. Visible on desktop only:
                        // at 375px it sat 20px from the input's own clear-X —
                        // two adjacent X's meaning different things (#1067) —
                        // and cost 52px of a 351px row. Below 1024px the CSS
                        // collapses it to a visually-hidden control (it comes
                        // back at full size on :focus-visible) and tap-off
                        // takes over as the pointer path. It stays in the DOM
                        // at every width so keyboard and assistive-tech users
                        // always have a reachable, labelled exit — tap-off is
                        // a plain div and Escape needs a hardware keyboard.
                        <button
                            type="button"
                            class="search-overlay-close"
                            aria-label=t_string!(i18n, close).to_string()
                            on:click=move |_| state.close()
                        >
                            <Icon icon=i::BsX width="1.5em" height="1.5em" aria_hidden=true />
                        </button>
                    </div>
                </div>
            </div>
        </Show>
    }
    .into_any()
}
