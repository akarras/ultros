use leptos::prelude::*;

#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div class="lds-ellipsis">
            <div></div>
            <div></div>
            <div></div>
            <div></div>
        </div>
    }
    .into_any()
}

#[component]
pub fn LargeLoading(#[prop(into)] pending: Signal<bool>) -> impl IntoView {
    view! {
        <div
            class:opacity-50=pending
            class:opacity-0=move || !pending()
            class="bg-brand-950 absolute left-0 right-0 z-40 transition ease-in-out delay-250"
        >
            <div class="ml-[50%]">
                <Loading />
            </div>
        </div>
    }
}
