use leptos::*;
use leptos_router::*;

#[component]
pub fn ListsNav(cx: Scope) -> impl IntoView {
    view! {cx, <div class="content-nav">
        <A class="btn-secondary" href="/list/edit">
            <span class="fa fa-pen-to-square"></span>
            "Edit"
        </A>
        <A class="btn-secondary" href="/list">
            <i class="fa-solid fa-list"></i>
            "Lists"
        </A>
    </div>}
}
