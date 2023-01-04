use leptos::*;

#[component]
pub fn Gil(cx: Scope, amount: i32) -> impl IntoView {
    view! {
        cx,
        <span class="gil"><img src="/static/images/gil.webp"/></span>{amount}
    }
}
