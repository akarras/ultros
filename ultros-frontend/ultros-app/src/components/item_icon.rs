use leptos::*;
pub use ultros_api_types::icon_size::IconSize;

#[component]
pub fn ItemIcon(cx: Scope, item_id: i32, icon_size: IconSize) -> impl IntoView {
    let (failed, set_failed) = create_signal(cx, false);
    view! {
        cx,
        <img class=icon_size.get_class()
            src=move || { if failed() {
                format!("/static/itemicon/{item_id}?size={}", icon_size)
            } else {
                "/static/itemicon/fallback".to_string()
            } } loading="lazy" on:error=move |_| {
            set_failed(true);
        } />
    }
}
