use leptos::{either::Either, prelude::*};
use xiv_gen::Item;

use crate::global_state::home_world::get_price_zone;

use super::item_icon::*;
use leptos_router::components::A;

#[component]
fn ItemDetails(item: &'static Item) -> impl IntoView {
    view! {
        <div class="flex flex-row items-center gap-2 min-w-0">
            <div class="shrink-0">
                <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small />
            </div>
            <span class="flex-1 min-w-0 truncate">{item.name.as_str()}</span>
            <span class="w-10 text-gray-500 text-right shrink-0">{item.level_item.0}</span>
        </div>
    }
    .into_any()
}

#[component]
pub fn SmallItemDisplay(item: &'static Item) -> impl IntoView {
    let (price_zone, _) = get_price_zone();
    view! {
        <div class="flex flex-row items-center gap-2 min-w-0">
            // If the item isn't marketable then do not display a market link
            {if item.item_search_category.0 == 0 {
                Either::Left(view! { <ItemDetails item /> })
            } else {
                Either::Right(
                    view! {
                        <A
                            attr:class="flex flex-row items-center gap-2 min-w-0"
                            exact=true
                            href=move || {
                                format!(
                                    "/item/{}/{}",
                                    price_zone()
                                        .as_ref()
                                        .map(|z| z.get_name())
                                        .unwrap_or("North-America"),
                                    item.key_id.0,
                                )
                            }
                        >
                            <ItemDetails item />
                        </A>
                    },
                )
            }}

        </div>
    }
    .into_any()
}

