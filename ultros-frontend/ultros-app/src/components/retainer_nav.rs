use leptos::*;

#[component]
pub fn RetainerNav(cx: Scope) -> impl IntoView {
    view! {cx, <div class="content-nav">
        <a class="btn-secondary" href="/retainers/edit">
            <span class="fa fa-pen-to-square"></span>
            "Edit"
        </a>
        <a class="btn-secondary" href="/retainers">
            <span class="fa fa-pencil"></span>
            "Listings"
        </a>
        <a class="btn-secondary" href="/retainers/undercuts">
            <span class="fa fa-exclamation"></span>
            "Undercuts"
        </a>
    </div>}
}
