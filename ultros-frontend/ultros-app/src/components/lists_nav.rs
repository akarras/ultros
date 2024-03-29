use leptos::*;
use leptos_router::*;
use leptos_icons::*;

#[component]
pub fn ListsNav() -> impl IntoView {
    view! {<div class="content-nav">
        <A class="btn-secondary" href="/list/edit">
            <Icon icon=AiEditFilled
            "Edit"
        </A>
        <A class="btn-secondary" href="/list">
            <Icon icon=AiOrderedListOutlined />
            "Lists"
        </A>
    </div>}
}
