use leptos::*;
use thousands::Separable;

#[component]
pub fn Gil(cx: Scope, amount: i32) -> impl IntoView {
    view! {
        cx,
        <span class="gil"><img style="height: 1em" src="/static/images/gil.webp"/></span><span>{amount.separate_with_commas()}</span>
    }
}
