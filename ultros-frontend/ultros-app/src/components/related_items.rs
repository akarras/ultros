/// Related items links items that are related to the current set
use leptos::*;
use xiv_gen::ItemId;

/// This iterator will attempt to find related items based on the set
fn item_set_iter(item_id: ItemId) -> impl Iterator<Item = ItemId> {
    unimplemented!("Item set iterator is unimplemented");
    vec![].into_iter()
}

/// This iterator will traverse the recipe tree for items that are related to using this item for crafting
fn recipe_tree_iter(item_id: ItemId) -> impl Iterator<Item = ItemId> {
    unimplemented!("blah");
    vec![].into_iter()
}

#[component]
pub fn RelatedItems(cx: Scope, item_id: ItemId) -> impl IntoView {
    let db = xiv_gen_db::decompress_data();
    view! {cx,
    <div>

    </div>}
}
