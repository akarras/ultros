use crate::global_state::xiv_data::tracked_data;
use leptos::prelude::*;
pub use ultros_api_types::icon_size::IconSize;
use xiv_gen::ItemId;

#[component]
pub fn ItemIcon(
    #[prop(into)] item_id: Signal<i32>,
    icon_size: IconSize,
    #[prop(optional)] loading: &'static str,
) -> impl IntoView {
    // ⚡ Bolt Optimization: Replace Memo::new with Signal::derive
    // `valid_search_category` and `item_name` are simple map lookups.
    // Creating reactive `Memo` nodes for these O(1) derivations carries overhead
    // that exceeds the cost of recomputing them.
    let valid_search_category = Signal::derive(move || {
        tracked_data()
            .items
            .get(&ItemId(item_id()))
            .map(|item| item.item_search_category > 0)
            .unwrap_or_default()
    });

    let item_name = Signal::derive(move || {
        tracked_data()
            .items
            .get(&ItemId(item_id()))
            .map(|i| i.name.as_str().to_string())
            .unwrap_or_default()
    });

    let (failed, set_failed) = signal(0);
    let failed_item = move || failed() == item_id();
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
                    if !failed_item() && valid_search_category.get() {
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
