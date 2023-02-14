use leptos::*;
pub use ultros_api_types::icon_size::IconSize;

#[component]
pub fn ItemIcon(cx: Scope, item_id: i32, icon_size: IconSize) -> impl IntoView {
    view! {
        cx,
        <img class=icon_size.get_class() src=format!("/static/itemicon/{item_id}?size={}", icon_size) loading="lazy" />
    }
}
