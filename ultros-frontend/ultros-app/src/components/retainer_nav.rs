use leptos::*;
use leptos_router::*;

#[component]
pub fn RetainerNav(cx: Scope) -> impl IntoView {
    view! {cx, <div class="content-nav">
        <A class="btn-secondary" href="/retainers/edit">
            <span class="fa fa-pen-to-square"></span>
            "Edit"
        </A>
        <A class="btn-secondary" href="/retainers/listings">
            <span class="fa fa-pencil"></span>
            "All Listings"
        </A>
        <A class="btn-secondary" href="/retainers/undercuts">
            <span class="fa fa-exclamation"></span>
            "Undercuts"
        </A>
    </div>}
}
