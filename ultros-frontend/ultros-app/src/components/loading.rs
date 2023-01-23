use leptos::*;

#[component]
pub fn Loading(cx: Scope) -> impl IntoView {
    view! {cx, <div class="lds-ellipsis"><div></div><div></div><div></div><div></div></div>}
}
