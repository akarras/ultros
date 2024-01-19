use leptos::*;
use thousands::Separable;

#[component]
pub fn Gil(amount: i32) -> impl IntoView {
    view! {
        <div class="flex flex-row"><div class="h-7 w-7 -m-1 aspect-square p-1"><img alt="gil" src="/static/images/gil.webp"/></div><div>{amount.separate_with_commas()}</div></div>
    }
}
