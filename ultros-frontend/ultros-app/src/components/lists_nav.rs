use leptos::*;

#[component]
pub fn ListsNav(cx: Scope) -> impl IntoView {
    view! {cx, <div class="content-nav">
        <a class="btn-secondary" href="/list/edit">
            <span class="fa fa-pen-to-square"></span>
            "Edit"
        </a>
        <a class="btn-secondary" href="/list">
            <i class="fa-solid fa-list"></i>
            "Lists"
        </a>
    </div>}
}
