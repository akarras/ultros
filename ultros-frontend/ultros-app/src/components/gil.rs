use leptos::*;
use thousands::Separable;

#[component]
pub fn Gil(amount: i32) -> impl IntoView {
    view! {
        
        <span class="gil"><img style="height: 1em" src="/static/images/gil.webp"/></span><span>{amount.separate_with_commas()}</span>
    }
}
