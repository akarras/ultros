use leptos::prelude::*;
use thousands::Separable;

use crate::i18n::*;

#[cfg(feature = "hydrate")]
fn spawn_gil_party(mut x: f64, mut y: f64) {
    let document = document();
    let body = document.body().expect("body");

    #[allow(clippy::collapsible_if)]
    if x == 0.0 && y == 0.0 {
        if let Some(window) = web_sys::window() {
            x = window
                .inner_width()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                / 2.0;
            y = window
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                / 2.0;
        }
    }

    for _ in 0..20 {
        let el = document.create_element("img").expect("create element");
        let _ = el.set_attribute("src", "/static/images/gil.webp");
        let _ = el.set_attribute("alt", "gil");

        // Initial style
        let size = 15 + (js_sys::Math::random() * 20.0) as i32;
        let initial_style = format!(
            "position: fixed; left: {}px; top: {}px; width: {}px; height: {}px; pointer-events: none; z-index: 9999; transition: transform 1s cubic-bezier(0.25, 1, 0.5, 1), opacity 1s ease-in;",
            x - (size as f64 / 2.0),
            y - (size as f64 / 2.0),
            size,
            size
        );
        let _ = el.set_attribute("style", &initial_style);

        body.append_child(&el).expect("append");

        let el_clone = el.clone();

        // Destination calculation
        let angle = js_sys::Math::random() * std::f64::consts::TAU;
        // Explode outwards
        let dist = 60.0 + js_sys::Math::random() * 100.0;
        let dx = angle.cos() * dist;
        let dy = angle.sin() * dist + 150.0; // Add gravity component
        let rot = js_sys::Math::random() * 720.0 - 360.0;

        let final_style = format!(
            "{}; transform: translate({}px, {}px) rotate({}deg); opacity: 0;",
            initial_style, dx, dy, rot
        );

        set_timeout(
            move || {
                let _ = el_clone.set_attribute("style", &final_style);
            },
            std::time::Duration::from_millis(10),
        );

        let el_remove = el.clone();
        set_timeout(
            move || {
                el_remove.remove();
            },
            std::time::Duration::from_millis(1000),
        );
    }
}

#[component]
fn GilIcon() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <button
            type="button"
            class="h-7 w-7 -m-1 aspect-square p-1 cursor-pointer hover:scale-110 transition-transform active:scale-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-1 focus-visible:ring-[var(--brand-ring)] rounded-full bg-transparent border-none appearance-none"
            aria-label=move || t_string!(i18n, gil_spawn_party_aria).to_string()
            on:click=move |ev| {
                #[cfg(feature = "hydrate")]
                #[allow(clippy::unnecessary_cast)] // client_x() is i32 in WASM, f64 in SSR
                spawn_gil_party(ev.client_x() as f64, ev.client_y() as f64);
                #[cfg(not(feature = "hydrate"))]
                {
                    let _ = ev;
                }
            }
        >
            <img
                alt=""
                src="/static/images/gil.webp"
                aria-hidden="true"
                class="w-full h-full object-contain pointer-events-none"
            />
        </button>
    }
}

#[component]
pub fn Gil(#[prop(into)] amount: Signal<i32>) -> impl IntoView {
    view! {
        <div class="flex flex-row items-center">
            <GilIcon />
            <div>{move || amount().separate_with_commas()}</div>
        </div>
    }
}

/// Render a gil amount when present, falling back to an em-dash placeholder
/// when `amount` is `None` — without changing the element shape.
///
/// Switching between `<Gil>` (`<div><button/><div/></div>`) and a bare
/// `<span>"—"</span>` via `into_any()` triggered tachys hydration mismatches
/// at `hydration.rs:163` (`failed_to_cast_element`) on `/items/jobset/<JOB>`:
/// if the server resolved the cheapest-listings resource and rendered a
/// `<Gil>` but the client briefly evaluated the `None` arm (or vice versa),
/// the dynamic-block child had a different root tag than the SSR DOM and the
/// hydration walker panicked. Always emitting the same `<div>` + icon + value
/// shape removes that class of mismatch entirely — the icon is just hidden
/// via CSS when the value is unknown, so the SSR and CSR view trees agree on
/// element types/positions regardless of resource state.
#[component]
pub fn GilOrDash(#[prop(into)] amount: Signal<Option<i32>>) -> impl IntoView {
    let icon_class = move || {
        if amount().is_some() {
            "inline-flex"
        } else {
            "hidden"
        }
    };
    let value_class = move || {
        if amount().is_some() {
            ""
        } else {
            "text-[color:var(--color-text-muted)]"
        }
    };
    view! {
        <div class="flex flex-row items-center">
            <span class=icon_class>
                <GilIcon />
            </span>
            <div class=value_class>
                {move || amount()
                    .map(|t| t.separate_with_commas())
                    .unwrap_or_else(|| "—".to_string())
                }
            </div>
        </div>
    }
}

#[component]
pub fn GenericGil<T>(#[prop(into)] amount: Signal<T>) -> impl IntoView
where
    T: Separable + 'static + Copy + Send + Sync,
{
    view! {
        <div class="flex flex-row items-center">
            <GilIcon />
            <div>{move || amount().separate_with_commas()}</div>
        </div>
    }
    .into_any()
}
