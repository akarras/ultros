use leptos::*;
use xiv_gen::Item;

use super::item_icon::*;
use leptos_router::*;

#[component]
fn ItemDetails(item: &'static Item) -> impl IntoView {
    view! {<ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small/>
    <span style="width: 300px;">{&item.name}</span>
    <span style="color: #abc; width: 50px;">{item.level_item.0}</span>}
}

#[component]
pub fn SmallItemDisplay(item: &'static Item) -> impl IntoView {
    view! {
        <div class="flex-row">
        // If the item isn't marketable then do not display a market link
        {if item.item_search_category.0 == 0 {
            view!{
                <ItemDetails item />
            }.into_view()
        } else {
            view!{
            <A class="flex-row" href=format!("/item/North-America/{}", item.key_id.0)>
                <ItemDetails item />
            </A>}.into_view()
        }}
        </div>
    }
}
