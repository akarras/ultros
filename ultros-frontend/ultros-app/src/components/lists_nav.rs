use leptos::prelude::*;
use leptos_icons::*;
use leptos_router::*;

#[component]
pub fn ListsNav() -> impl IntoView {
    view! {<div class="content-nav">
        <A class="btn-secondary flex items-center gap-2" href="/list/edit">
            <Icon icon=AiEditFilled />
            "Edit"
        </A>
        <A class="btn-secondary flex items-center gap-2" href="/list">
            <Icon icon=AiOrderedListOutlined />
            "Lists"
        </A>
    </div>}
    .into_any()
}
