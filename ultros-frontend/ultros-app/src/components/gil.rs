use leptos::prelude::*;
use thousands::Separable;

#[component]
pub fn Gil(#[prop(into)] amount: Signal<i32>) -> impl IntoView {
    view! {
        <div class="flex flex-row">
            <div class="h-7 w-7 -m-1 aspect-square p-1">
                <img alt="gil" src="/static/images/gil.webp" />
            </div>
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
        <div class="flex flex-row">
            <div class="h-7 w-7 -m-1 aspect-square p-1">
                <img alt="gil" src="/static/images/gil.webp" />
            </div>
            <div>{move || amount().separate_with_commas()}</div>
        </div>
    }
    .into_any()
}

