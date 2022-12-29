use leptos::*;

#[component]
pub fn Item(cx: Scope, item_id: i32, item_name: String) -> impl IntoView {
    view! {
        cx,
        <div>
            <img class="iconsmall" src=format!("/static/itemicon/{item_id}?size=40") />
            {item_name}
        </div>
    }
}
