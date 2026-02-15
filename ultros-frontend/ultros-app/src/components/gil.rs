use leptos::prelude::*;
use thousands::Separable;

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
    view! {
        <button
            type="button"
            class="h-7 w-7 -m-1 aspect-square p-1 cursor-pointer hover:scale-110 transition-transform active:scale-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-1 focus-visible:ring-[var(--brand-ring)] rounded-full bg-transparent border-none appearance-none"
            aria-label="Spawn gil party"
            on:click=move |ev| {
                #[cfg(feature = "hydrate")]
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
