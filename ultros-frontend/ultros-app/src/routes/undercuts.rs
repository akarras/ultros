use leptos::*;
use crate::components::retainer_nav::*;

#[component]
pub fn RetainerUndercuts(cx: Scope) -> impl IntoView {
    view!{cx, <div class="container">
        <RetainerNav />
        <div class="main-content">
            <span class="content-title">"Undercuts"</span>
        </div>
    </div>}
}
