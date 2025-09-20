use leptos::prelude::*;
use leptos_icons::*;
use leptos_router::*;

#[component]
pub fn ListsNav() -> impl IntoView {
    view! {<div class="content-nav">

        <A exact=true attr:class="nav-link" href="/list">
            <Icon height="1.25em" width="1.25em" icon=AiOrderedListOutlined />
            <span>"Lists"</span>
        </A>
    </div>}
    .into_any()
}
