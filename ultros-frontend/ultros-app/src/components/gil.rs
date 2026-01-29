use leptos::prelude::*;
use thousands::Separable;
use wasm_bindgen::JsCast;

#[cfg(feature = "hydrate")]
fn spawn_gil_party(x: f64, y: f64) {
    let document = document();
    let body = document.body().expect("body");

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
pub fn Gil(#[prop(into)] amount: Signal<i32>) -> impl IntoView {
    view! {
        <div class="flex flex-row items-center">
            <button
                type="button"
                aria-label="Make it rain"
                class="h-7 w-7 -m-1 aspect-square p-1 cursor-pointer hover:scale-110 transition-transform active:scale-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)] rounded-full"
                on:click=move |ev| {
                    #[cfg(feature = "hydrate")]
                    {
                        let x = ev.client_x() as f64;
                        let y = ev.client_y() as f64;
                        if x == 0.0 && y == 0.0 {
                            if let Some(target) = ev.current_target() {
                                if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                                    let rect = el.get_bounding_client_rect();
                                    let center_x = rect.left() + rect.width() / 2.0;
                                    let center_y = rect.top() + rect.height() / 2.0;
                                    spawn_gil_party(center_x, center_y);
                                }
                            }
                        } else {
                            spawn_gil_party(x, y);
                        }
                    }
                    #[cfg(not(feature = "hydrate"))]
                    {
                        let _ = ev;
                    }
                }
            >
                <img alt="" aria-hidden="true" src="/static/images/gil.webp" />
            </button>
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
            <button
                type="button"
                aria-label="Make it rain"
                class="h-7 w-7 -m-1 aspect-square p-1 cursor-pointer hover:scale-110 transition-transform active:scale-90 focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)] rounded-full"
                on:click=move |ev| {
                    #[cfg(feature = "hydrate")]
                    {
                        let x = ev.client_x() as f64;
                        let y = ev.client_y() as f64;
                        if x == 0.0 && y == 0.0 {
                            if let Some(target) = ev.current_target() {
                                if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                                    let rect = el.get_bounding_client_rect();
                                    let center_x = rect.left() + rect.width() / 2.0;
                                    let center_y = rect.top() + rect.height() / 2.0;
                                    spawn_gil_party(center_x, center_y);
                                }
                            }
                        } else {
                            spawn_gil_party(x, y);
                        }
                    }
                    #[cfg(not(feature = "hydrate"))]
                    {
                        let _ = ev;
                    }
                }
            >
                <img alt="" aria-hidden="true" src="/static/images/gil.webp" />
            </button>
            <div>{move || amount().separate_with_commas()}</div>
        </div>
    }
    .into_any()
}
