use itertools::Itertools;
/// Related items links items that are related to the current set
use leptos::*;
use leptos_router::A;
use ultros_api_types::{cheapest_listings::CheapestListingMapKey, icon_size::IconSize};
use xiv_gen::{ENpcBase, ENpcResidentId, GilShopId, Item, ItemId, Recipe};

use crate::{
    components::{item_icon::ItemIcon, skeleton::SingleLineSkeleton},
    global_state::{cheapest_prices::CheapestPrices, home_world::get_price_zone},
};

use super::{cheapest_price::*, gil::*, small_item_display::*};

/// Matches against items that start with the same prefix
/// "Diadochos" -> "Diadochos Helmet" etc
fn prefix_item_iterator(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &xiv_gen_db::data().items;
    let prefix = item.name.split_once(' ').map(|(prefix, _)| prefix);
    items.values().filter(move |f| {
        if let Some(prefix) = prefix {
            f.name.starts_with(prefix)
                && f.item_search_category.0 != 0
                && f.level_item.0 == item.level_item.0
        } else {
            false
        }
    })
}

fn suffix_item_iterator(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &xiv_gen_db::data().items;
    let suffix = item.name.rsplit_once(' ').map(|(_, suffix)| suffix);
    items.values().filter(move |f| {
        if let Some(suffix) = suffix {
            f.name.ends_with(suffix)
                && f.item_search_category.0 != 0
                && f.level_item.0 == item.level_item.0
        } else {
            false
        }
    })
}

/// This iterator will attempt to find related items using the classjobcategory && ilvl
fn item_set_iter(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &xiv_gen_db::data().items;
    items.values().filter(|i| {
        item.class_job_category.0 != 0
            && item.class_job_category.0 == i.class_job_category.0
            && item.level_item.0 == i.level_item.0
            && i.key_id != item.key_id
            && item.item_search_category.0 > 0
    })
}

/// Creates an iterator over the ingredients in a recipe
#[derive(Copy, Clone, Debug)]
pub(crate) struct IngredientsIter<'a>(&'a Recipe, u8);
impl<'a> IngredientsIter<'a> {
    pub(crate) fn new(recipe: &'a Recipe) -> Self {
        Self(recipe, 0)
    }
}
impl<'a> Iterator for IngredientsIter<'a> {
    type Item = (ItemId, u8);

    fn next(&mut self) -> Option<Self::Item> {
        // I don't remember entirely if the ingredients all are in order.
        loop {
            let counter = self.1;
            let id = match counter {
                0 => (self.0.item_ingredient_0, self.0.amount_ingredient_0),
                1 => (self.0.item_ingredient_1, self.0.amount_ingredient_1),
                2 => (self.0.item_ingredient_2, self.0.amount_ingredient_2),
                3 => (self.0.item_ingredient_3, self.0.amount_ingredient_3),
                4 => (self.0.item_ingredient_4, self.0.amount_ingredient_4),
                5 => (self.0.item_ingredient_5, self.0.amount_ingredient_5),
                6 => (self.0.item_ingredient_6, self.0.amount_ingredient_6),
                7 => (self.0.item_ingredient_7, self.0.amount_ingredient_7),
                // 8 => (self.0.item_ingredient_8, self.0.amount_ingredient_8),
                // 9 => (self.0.item_ingredient_9, self.0.amount_ingredient_9),
                _ => return None,
            };
            self.1 += 1;
            // check if this is a valid id
            if id.0 .0 != 0 {
                return Some(id);
            }
        }
    }
}

/// This iterator will traverse the recipe tree for items that are related to using this item for crafting
fn recipe_tree_iter(item_id: ItemId) -> impl Iterator<Item = &'static Recipe> {
    let recipes = &xiv_gen_db::data().recipes;
    // our item id could be in item_result, or item_ingredient
    recipes
        .values()
        .filter(move |filter| {
            filter.item_result == item_id
                || IngredientsIter::new(filter).any(|(i, _amount)| i.0 == item_id.0)
        })
        .sorted_by_key(|r| r.key_id.0)
}

#[component]
fn RecipePriceEstimate(recipe: &'static Recipe) -> impl IntoView {
    let items = &xiv_gen_db::data().items;
    let cheapest_prices = use_context::<CheapestPrices>().unwrap();

    view! {
        <Suspense fallback=move || {
            view! { <SingleLineSkeleton/> }
        }>
            {move || {
                cheapest_prices
                    .read_listings
                    .with(|prices| {
                        prices
                            .as_ref()
                            .and_then(|prices| prices.as_ref().ok())
                            .map(|prices| {
                                let hq_amount: i32 = IngredientsIter::new(recipe)
                                    .flat_map(|(ingredient, amount)| {
                                        items.get(&ingredient).map(|item| (item, amount))
                                    })
                                    .flat_map(|(item, quantity)| {
                                        prices
                                            .1
                                            .map
                                            .get(
                                                &CheapestListingMapKey {
                                                    item_id: item.key_id.0,
                                                    hq: item.can_be_hq,
                                                },
                                            )
                                            .map(|data| data.price * quantity as i32)
                                    })
                                    .sum();
                                let amount: i32 = IngredientsIter::new(recipe)
                                    .flat_map(|(ingredient, amount)| {
                                        items.get(&ingredient).map(|item| (item, amount))
                                    })
                                    .flat_map(|(item, quantity)| {
                                        prices
                                            .1
                                            .map
                                            .get(
                                                &CheapestListingMapKey {
                                                    item_id: item.key_id.0,
                                                    hq: item.can_be_hq,
                                                },
                                            )
                                            .map(|data| data.price * quantity as i32)
                                    })
                                    .sum();
                                view! {
                                    <span class="flex flex-row gap-1">
                                        "HQ: " <Gil amount=hq_amount/> " LQ:" <Gil amount/>
                                    </span>
                                }
                            })
                    })
            }}

        </Suspense>
    }
}

#[component]
fn Recipe(recipe: &'static Recipe) -> impl IntoView {
    let items = &xiv_gen_db::data().items;
    let ingredients = IngredientsIter::new(recipe)
        .flat_map(|(ingredient, amount)| items.get(&ingredient).map(|item| (item, amount)))
        .map(|(ingredient, amount)| view! {
            <div class="flex md:flex-row flex-col">
                <div class="flex flex-row">
                    <span style="color:#dab;">{amount.to_string()}</span>
                    "x"
                    <SmallItemDisplay item=ingredient/>
                </div>
                <CheapestPrice item_id=ingredient.key_id/>
            </div>
        })
        .collect::<Vec<_>>();
    let target_item = items.get(&recipe.item_result)?;
    Some(view! {
        <div class="content-well">
            "Crafting Recipe:" <div class="flex md:flex-row flex-col">
                <SmallItemDisplay item=target_item/>
                <CheapestPrice item_id=target_item.key_id/>
            </div> "Ingredients:" {ingredients} <div class="flex md:flex-row flex-col p-1 gap-1">
                <span class="underline">"Total craft cost:"</span>
                " "
                <RecipePriceEstimate recipe/>
            </div>
        </div>
    })
}

fn npc_rows(npc: &ENpcBase) -> impl Iterator<Item = u32> + '_ {
    // TODO- can I just parse the csv into a vec?
    npc.e_npc_data.iter().map(|row| row.0)
}

fn gil_shop_to_npc(gil_shops: &Vec<GilShopId>) -> Vec<(GilShopId, &'static ENpcBase)> {
    let data = xiv_gen_db::data();

    data.e_npc_bases
        .values()
        .flat_map(|npc: &'static ENpcBase| {
            npc_rows(npc)
                .filter(move |row| gil_shops.contains(&GilShopId(*row as i32)))
                .map(move |gil_shop| (GilShopId(gil_shop as i32), npc))
        })
        .collect()
}

#[component]
fn VendorItems(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let data = xiv_gen_db::data();
    // lookup items
    // from miu on xiv api discord:
    // GilShop => ENpcResident,
    // GilShop => TopicSelect => ENpcResident,
    // GilShop => PreHandler => TopicSelect => ENpcResident (or the other way around I don't remember),
    // GilShop => PreHandler => ENpcResident and last but not least, lua scripts (mostly for fate shops)
    // https://github.com/ffxiv-teamcraft/ffxiv-teamcraft/blob/staging/apps/data-extraction/src/extractors/shops.extractor.ts
    let npcs = create_memo(move |_| {
        let item_id = item_id();
        let gil_shops = data
            .gil_shop_items
            .iter()
            .filter(|(_shop_id, items)| items.iter().any(|shop_item| shop_item.item.0 == item_id))
            .flat_map(|(shop_id, _)| data.gil_shops.get(shop_id))
            .collect::<Vec<_>>();
        let shop_ids = gil_shops.iter().map(|shop| shop.key_id).collect::<Vec<_>>();
        gil_shop_to_npc(&shop_ids)
    });
    let data = move || {
        let items = npcs().into_iter().filter_map(|(shop_id, npc)| {
            data.e_npc_residents
                .get(&ENpcResidentId(npc.key_id.0))
                .map(|resident| (shop_id, resident))
        });
        let item = data.items.get(&ItemId(item_id()))?;
        Some(
            items.into_iter()
            .filter_map(|(shop, resident)| {
                let shop = data.gil_shops.get(&shop)?;
                let price = item.price_mid as i32;
                Some(view! {
                    <a
                        href=format!("https://garlandtools.org/db/#npc/{}", resident.key_id.0)
                        class="flex flex-col p-1 bg-gradient- border border-solid border-violet-950
                        transition-all duration-500 bg-gradient-to-tl to-fuchsia-950 via-black from-violet-950 bg-size-200 bg-pos-0 hover:bg-pos-100"
                    >
                        <div class="flex flex-row">
                            <div class="text-md">{&resident.singular}</div>
                            <Gil amount=price/>
                        </div>
                        <div class="text-sm italic">"(" {&shop.name} ")"</div>
                    </a>
                })
            }).collect::<Vec<_>>())
    };
    let empty = move || npcs.with(|n| n.is_empty());
    view! {
        <div class:collapse=empty class="flex-col p-2 max-h-96 overflow-y-auto w-96 xl-w-[600px]">
            <span class="text-2xl">"Vendor sources"</span>
            {data}
        </div>
    }
}

#[component]
pub fn RelatedItems(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let db = xiv_gen_db::data();
    let item = create_memo(move |_| db.items.get(&ItemId(item_id())));
    let (price_zone, _) = get_price_zone();
    let item_set = move || {
        item()
            .map(|item| {
                item_set_iter(item)
                    .chain(prefix_item_iterator(item))
                    .chain(suffix_item_iterator(item))
                    .sorted_by_key(|i| i.key_id.0)
                    .unique_by(|i| i.key_id)
                    .filter(|i| i.item_search_category.0 > 0)
                    .filter(|i| i.key_id.0 != item.key_id.0)
                    .map(|item| {
                        view! {
                            <A
                                class="flex flex-col gap-1 rounded border border-violet-950
                                transition-all duration-500 bg-gradient-to-br to-fuchsia-950 via-black from-violet-950 bg-size-200 bg-pos-0
                                hover:bg-pos-100 p-2"
                                exact=true
                                href=format!(
                                    "/item/{}/{}",
                                    price_zone()
                                        .as_ref()
                                        .map(|z| z.get_name())
                                        .unwrap_or("North-America"),
                                    item.key_id.0,
                                )
                            >

                                <div class="flex flex-row">
                                    <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium/>
                                    <span style="width: 300px;">{&item.name}</span>
                                    <span style="color: #abc; width: 50px;">
                                        {item.level_item.0}
                                    </span>
                                </div>
                                <div class="min-w-60 h-5">
                                    <CheapestPrice item_id=item.key_id/>
                                </div>
                            </A>
                        }
                    })
                    .take(15)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let recipes = create_memo(move |_| {
        recipe_tree_iter(ItemId(item_id()))
            .map(|recipe| view! { <Recipe recipe/> })
            .take(30)
            .collect::<Vec<_>>()
    });

    view! {
        <div class="flex-col flex-auto flex-wrap p-2" class:hidden=move || item_set().is_empty()>
            <span class="content-title">"related items"</span>
            <div class="flex-row flex-wrap gap-3">{item_set}</div>
        </div>
        <VendorItems item_id/>
        <div class="content-well flex-col p-2" class:hidden=move || recipes().is_empty()>
            <span class="content-title">"crafting recipes"</span>
            <div class="flex-wrap">{recipes}</div>
        </div>
    }
}
