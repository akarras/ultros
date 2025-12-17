use std::collections::HashMap;

use crate::{
    error::AppError,
    global_state::{home_world::use_home_world, LocalWorldData},
};
use leptos::prelude::*;
use ultros_api_types::{list::ListItem, listings::ActiveListing, world_helper::AnySelector};
use xiv_gen::ItemId;

use crate::components::{item_icon::*, world_name::*};

mod distance_logic;

#[component]
pub(crate) fn PurchasingView(
    items: StoredValue<Vec<(ListItem, Vec<ActiveListing>)>>,
    edit_item: Action<ListItem, Result<(), AppError>>,
) -> impl IntoView {
    let listings_by_world = Memo::new(move |_| {
        let mut map: HashMap<i32, Vec<(ListItem, ActiveListing)>> = HashMap::new();
        items.with_value(|items| {
            for (list_item, listings) in items {
                for listing in listings {
                    let listings = map.entry(listing.world_id).or_insert(vec![]);
                    listings.push((list_item.clone(), listing.clone()));
                }
            }
        });
        let (home_world, _) = use_home_world();
        let world_helper = use_context::<LocalWorldData>().unwrap().0.unwrap();
        let mut sorted: Vec<_> = map.into_iter().collect();
        sorted.sort_by_key(|(world_id, _)| {
            home_world
                .get()
                .and_then(|w| Some(w.id))
                .zip(Some(world_helper.clone()))
                .map(|(home_world, world_helper)| {
                    distance_logic::get_teleport_cost(&world_helper, home_world, *world_id)
                })
                .unwrap_or(i32::MAX)
        });
        sorted
    });

    view! {
        <For
            each=move || listings_by_world()
            key=|(world, _)| *world
            children=move |(world, listings): (i32, Vec<(ListItem, ActiveListing)>)| {
                view! {
                    <div>
                        <h2>
                            <WorldName id=AnySelector::World(world) />
                        </h2>
                        <For
                            each=move || listings.clone()
                            key=|(_, listing)| listing.id.clone()
                            children=move |(list_item, listing): (ListItem, ActiveListing)| {
                                let (item, _) =
                                    signal(xiv_gen_db::data().items.get(&ItemId(list_item.item_id)));
                                view! {
                                    <div>
                                        <ItemIcon item_id=item().unwrap().key_id.0 icon_size=IconSize::Small />
                                        <span>
                                            {format!(
                                                "{} {} @ {}g",
                                                list_item.quantity.unwrap_or_default(),
                                                item()
                                                    .map(|i| i.name.to_string())
                                                    .unwrap_or_else(|| "unknown".to_string()),
                                                listing.price_per_unit
                                            )}
                                        </span>
                                        <div
                                            class="btn"
                                            on:click=move |_| {
                                                let mut item = list_item.clone();
                                                let quantity =
                                                    item.quantity.unwrap_or_default() - listing.quantity;
                                                item.acquired = Some(listing.quantity);
                                                edit_item.dispatch(item);
                                            }
                                        >
                                            "Purchased"
                                        </div>
                                    </div>
                                }
                                .into_any()
                            }
                        />
                    </div>
                }
                .into_any()
            }
        />
    }
}
