use leptos::*;

use crate::api::get_retainers;

#[component]
fn RetainerView(cx: Scope) -> impl IntoView {
    view! {cx, <div></div>};
}

#[component]
pub fn Retainers(cx: Scope) -> impl IntoView {
    let retainers = create_resource(cx, || {}, move |()| get_retainers(cx));
    view! {
        cx,
        <div class="container">
            <div class="main-content">
                <span class="content-title">"Retainers"</span>
                <Suspense fallback=move || view!{cx, <span>"Loading..."</span>}>
                {move || {
                    match retainers() {
                        Some(Some(retainers)) => {
                            view!{cx, <div>
                                    "Retainer data loaded"
                                </div>}
                        },
                        _ => view!{cx, <div></div>}
                    }
                }}
                </Suspense>
            </div>
        </div>
    }
}
