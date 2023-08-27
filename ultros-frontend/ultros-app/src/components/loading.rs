use leptos::*;

#[component]
pub fn Loading() -> impl IntoView {
    view! {<div class="lds-ellipsis"><div></div><div></div><div></div><div></div></div>}
}
