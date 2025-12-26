use crate::api::UndercutData;
use crate::components::clipboard::Clipboard;
use crate::components::gil::*;
use crate::components::item_icon::*;
use crate::components::world_name::*;
use crate::global_state::LocalWorldData;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;
use ultros_api_types::{ActiveListing, FfxivCharacter, Retainer, world_helper::AnySelector};
use xiv_gen::ItemId;

#[derive(PartialOrd, Ord, Eq, PartialEq, Debug)]
pub(crate) struct ItemSortKey(u8, i32, bool);

impl From<(ItemId, bool)> for ItemSortKey {
    fn from((item_id, hq): (ItemId, bool)) -> Self {
        let inner = move || {
            let data = xiv_gen_db::data();
            let items = &data.items;
            let sort_category = &data.item_sort_categorys;
            let item = items.get(&item_id)?;
            let sort_weight = sort_category
                .get(&item.item_sort_category)
                .map(|category| category.param)?;
            Some(Self(sort_weight, item.key_id.0, hq))
        };
        inner().unwrap_or(Self(u8::MAX, i32::MAX, hq))
    }
}

impl From<&ActiveListing> for ItemSortKey {
    fn from(listing: &ActiveListing) -> Self {
        ItemSortKey::from((ItemId(listing.item_id), listing.hq))
    }
}

#[component]
pub fn RetainerUndercutTable(retainer: Retainer, listings: Vec<UndercutData>) -> impl IntoView {
    let mut listings = listings;
    let data = xiv_gen_db::data();
    let items = &data.items;
    listings.sort_by_key(|u| ItemSortKey::from(&u.current));
    let worlds = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let world = worlds.lookup_selector(AnySelector::World(retainer.world_id));
    let world_name = world.as_ref().map(|w| w.get_name()).unwrap_or_default();
    let listings: Vec<_> = listings
        .into_iter()
        .map(|undercut_data| {
            let listing = undercut_data.current;
            let item = items.get(&ItemId(listing.item_id));
            let total = listing.quantity * listing.price_per_unit;
            let new_best_price = undercut_data.cheapest - 1;
            view! {
                <tr>
                    <td>{listing.hq.then_some("HQ")}</td>
                    <td class="flex flex-row">
                        {if let Some(item) = item {
                            Either::Left(
                                view! {
                                    <A
                                        attr:class="flex flex-row"
                                        href=format!("/item/{world_name}/{}", listing.item_id)
                                    >
                                        <ItemIcon
                                            icon_size=IconSize::Small
                                            item_id=listing.item_id
                                        />
                                        {item.name.as_str()}
                                    </A>
                                    <Clipboard clipboard_text=item.name.as_str() />
                                },
                            )
                        } else {
                            Either::Right(view! { "Item not found" })
                        }}

                    </td>
                    <td>
                        <Gil amount=listing.price_per_unit />
                    </td>
                    <td>{listing.quantity}</td>
                    <td>
                        <Gil amount=total />
                    </td>
                    <td>
                        <div class="flex flex-row">
                            <Gil amount=new_best_price />
                            <Clipboard clipboard_text=new_best_price.to_string() />
                        </div>
                    </td>
                </tr>
            }
            .into_any()
        })
        .collect();
    view! {
        <div class="panel p-4 rounded-xl">
            <span class="content-title">
                {retainer.name} " - " <WorldName id=AnySelector::World(retainer.world_id) />
            </span>
            <table class="w-full">
                <thead>
                    <tr>
                        <th>"HQ"</th>
                        <th>"Item"</th>
                        <th>"Price Per Unit"</th>
                        <th>"Quantity"</th>
                        <th>"Total"</th>
                        <th>"Undercut by one"</th>
                    </tr>
                </thead>
                <tbody>{listings}</tbody>
            </table>
        </div>
    }
    .into_any()
}

#[component]
pub fn RetainerTable(retainer: Retainer, listings: Vec<ActiveListing>) -> impl IntoView {
    let data = xiv_gen_db::data();
    let items = &data.items;
    let mut listings = listings;
    listings.sort_by_key(|u| ItemSortKey::from(u));
    let world_data = use_context::<LocalWorldData>().unwrap();
    let worlds = world_data.0.unwrap();
    let world = worlds.lookup_selector(AnySelector::World(retainer.world_id));
    let world_name = world.as_ref().map(|w| w.get_name()).unwrap_or_default();
    let listings: Vec<_> = listings
        .into_iter()
        .map(|listing| {
            let item = items.get(&ItemId(listing.item_id));
            let total = listing.quantity * listing.price_per_unit;
            view! {
                <tr>
                    <td>{listing.hq.then_some("HQ")}</td>
                    <td class="flex flex-row">
                        {if let Some(item) = item {
                            Either::Left(
                                view! {
                                    <A
                                        attr:class="flex flex-row"
                                        href=format!("/item/{}/{}", world_name, listing.item_id)
                                    >
                                        <ItemIcon
                                            icon_size=IconSize::Small
                                            item_id=listing.item_id
                                        />
                                        {item.name.as_str()}
                                    </A>
                                    <Clipboard clipboard_text=item.name.as_str() />
                                },
                            )
                        } else {
                            Either::Right(view! { "Item not found" })
                        }}

                    </td>
                    <td>
                        <Gil amount=listing.price_per_unit />
                    </td>
                    <td>{listing.quantity}</td>
                    <td>
                        <Gil amount=total />
                    </td>
                </tr>
            }
            .into_any()
        })
        .collect();
    view! {
        <div class="panel p-4 rounded-xl">
            <span class="content-title">
                {retainer.name} " - " <WorldName id=AnySelector::World(retainer.world_id) />
            </span>
            <table>
                <thead>
                    <tr>
                        <th>"HQ"</th>
                        <th>"Item"</th>
                        <th>"Price Per Unit"</th>
                        <th>"Quantity"</th>
                        <th>"Total"</th>
                    </tr>
                </thead>
                <tbody>{listings}</tbody>
            </table>
        </div>
    }
    .into_any()
}

#[component]
pub fn CharacterRetainerList(
    character: Option<FfxivCharacter>,
    retainers: Vec<(Retainer, Vec<ActiveListing>)>,
) -> impl IntoView {
    let listings: Vec<_> = retainers
        .into_iter()
        .map(|(retainer, listings)| view! { <RetainerTable retainer listings /> })
        .collect();
    view! {
        <div>
            {if let Some(character) = character {
                Either::Left(view! { <span>{character.first_name} {character.last_name}</span> })
            } else {
                Either::Right(listings)
            }}

        </div>
    }
    .into_any()
}

#[component]
pub fn CharacterRetainerUndercutList(
    character: Option<FfxivCharacter>,
    retainers: Vec<(Retainer, Vec<UndercutData>)>,
) -> impl IntoView {
    let listings: Vec<_> = retainers
        .into_iter()
        .map(|(retainer, listings)| view! { <RetainerUndercutTable retainer listings /> })
        .collect();
    view! {
        <div>
            {if let Some(character) = character {
                Either::Left(
                    view! { <span>{character.first_name} {character.last_name}</span> }.into_view(),
                )
            } else {
                Either::Right(listings)
            }}

        </div>
    }
    .into_any()
}

#[cfg(test)]
mod test {

    use super::ItemSortKey;

    #[cfg(feature = "ssr")]
    #[test]
    fn test_sort_order() {
        // these item ids are in the correct order- so if we run it through our sort, it should still match up
        use chrono::NaiveDateTime;
        use ultros_api_types::ActiveListing;
        let item_ids = vec![
            29417, 30842, 36837, 31840, 17325, 9050, 15532, 4737, 19853, 24250,
        ];
        let mut item_vec: Vec<_> = item_ids
            .into_iter()
            .map(|item| ActiveListing {
                id: 0,
                world_id: 0,
                item_id: item,
                retainer_id: 0,
                price_per_unit: 1000,
                quantity: 1,
                hq: true,
                timestamp: NaiveDateTime::MIN,
            })
            .collect();
        let original = item_vec.clone();
        item_vec.sort_by_key(|i| ItemSortKey::from(i));
        assert_eq!(original, item_vec);
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn same_sort_category() {
        use xiv_gen::ItemId;

        let expected_order = vec![
            41509, // red corsage
            41516, // black corsage
            41517,
        ]; // rainbow corsage
        let mut rearranged = vec![41516, 41517, 41509];
        rearranged.sort_by_key(|id| ItemSortKey::from((ItemId(*id), true)));
        assert_eq!(expected_order, rearranged);
    }
}
