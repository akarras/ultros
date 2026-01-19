use leptos::prelude::*;
pub use ultros_api_types::icon_size::IconSize;
use xiv_gen::{Item, ItemId};

#[component]
pub fn ItemIcon(
    #[prop(into)] item_id: Signal<i32>,
    icon_size: IconSize,
    #[prop(optional)] loading: &'static str,
    #[prop(optional)] item: Option<&'static Item>,
) -> impl IntoView {
    let valid_search_category = move || {
        if let Some(item) = item {
            item.item_search_category.0 > 0
        } else {
            xiv_gen_db::data()
                .items
                .get(&ItemId(item_id()))
                .map(|item| item.item_search_category.0 > 0)
                .unwrap_or_default()
        }
    };
    let (failed, set_failed) = signal(0);
    let failed_item = move || failed() == item_id();
    let data = xiv_gen_db::data();
    let item_name = move || {
        if let Some(item) = item {
            item.name.as_str().to_string()
        } else {
            let item = data.items.get(&ItemId(item_id()));
            item.as_ref()
                .map(|i| i.name.as_str().to_string())
                .unwrap_or_default()
        }
    };
    view! {
        <div
            class="overflow-hidden"
            style:width=icon_size.get_size_px()
            style:height=icon_size.get_size_px()
        >
            <img
                prop:alt=item_name
                class=format!("{} max-w-full max-h-full object-contain", icon_size.get_class())
                src=move || {
                    if !failed_item() && valid_search_category() {
                        format!("/static/itemicon/{}?size={}", item_id(), icon_size)
                    } else {
                        "/static/itemicon/fallback".to_string()
                    }
                }
                loading=move || { if loading.is_empty() { "lazy" } else { loading } }
                on:error=move |_| {
                    set_failed(item_id.get_untracked());
                }
            />
        </div>
    }
    .into_any()
}
