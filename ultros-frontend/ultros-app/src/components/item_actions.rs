use crate::components::{add_to_list::AddToList, clipboard::Clipboard};
use crate::global_state::xiv_data::tracked_data;
use leptos::prelude::*;
use xiv_gen::ItemId;

/// Keep the two everyday item actions in the same order on every analyzer.
/// Copy always uses the plain name, even when the row represents HQ or a stack.
#[component]
pub fn ItemActions(
    item_id: i32,
    #[prop(into)] item_name: String,
    #[prop(optional)] hq: bool,
    #[prop(default = 1)] quantity: i32,
    #[prop(default = true)] allow_list: bool,
) -> impl IntoView {
    let marketable = tracked_data()
        .items
        .get(&ItemId(item_id))
        .is_some_and(|item| item.item_search_category != 0);
    view! {
        <span class="inline-flex shrink-0 items-center gap-1" data-item-actions=item_id>
            <Clipboard clipboard_text=item_name />
            {(allow_list && marketable).then(|| view! {
                <AddToList item_id initial_hq=hq initial_quantity=quantity
                    class="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded hover:bg-[color:var(--color-panel)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)]" />
            })}
        </span>
    }
}
