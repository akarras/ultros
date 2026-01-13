use leptos::prelude::*;
pub use ultros_api_types::icon_size::IconSize;
use xiv_gen::ItemId;

#[component]
pub fn ItemIcon(
    #[prop(into)] item_id: Signal<i32>,
    icon_size: IconSize,
    #[prop(optional)] loading: &'static str,
) -> impl IntoView {
    // Memoize the item lookup to prevent repeated HashMap lookups on every render/effect
    let item = Memo::new(move |_| xiv_gen_db::data().items.get(&ItemId(item_id())));

    let valid_search_category = Memo::new(move |_| {
        item()
            .map(|item| item.item_search_category.0 > 0)
            .unwrap_or_default()
    });

    let (failed, set_failed) = signal(0);
    // Determine if the current item_id has failed loading
    let failed_item = move || failed() == item_id();

    // Memoize the alt text string construction
    let item_name = Memo::new(move |_| {
        format!(
            "Image for item {}",
            item().as_ref().map(|i| i.name.as_str()).unwrap_or_default()
        )
    });

    // Memoize the src string to avoid unnecessary format! calls
    let src = Memo::new(move |_| {
        if !failed_item() && valid_search_category() {
            format!("/static/itemicon/{}?size={}", item_id(), icon_size)
        } else {
            "/static/itemicon/fallback".to_string()
        }
    });

    view! {
        <div
            class="overflow-hidden"
            style:width=icon_size.get_size_px()
            style:height=icon_size.get_size_px()
        >
            <img
                prop:alt=item_name
                class=format!("{} max-w-full max-h-full object-contain", icon_size.get_class())
                src=src
                loading=move || { if loading.is_empty() { "lazy" } else { loading } }
                on:error=move |_| {
                    set_failed(item_id.get_untracked());
                }
            />
        </div>
    }
    .into_any()
}
