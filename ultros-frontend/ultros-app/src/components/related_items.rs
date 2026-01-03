use itertools::Itertools;
/// Related items links items that are related to the current set
use leptos::prelude::*;
use leptos_router::components::A;
use std::collections::HashSet;
use std::sync::LazyLock;
use ultros_api_types::{
    cheapest_listings::{CheapestListingMapKey, CheapestListingsMap},
    icon_size::IconSize,
};
use xiv_gen::{
    ENpcBase, ENpcResidentId, GilShopId, Item, ItemId, Leve, LeveRewardItem, LeveRewardItemGroup,
    Recipe, SpecialShop,
};

use crate::{
    components::{
        add_recipe_to_list::AddRecipeToList, icon::Icon, item_icon::ItemIcon,
        skeleton::SingleLineSkeleton,
    },
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
pub(crate) struct IngredientsIter<'a>(&'a Recipe, i32);
impl<'a> IngredientsIter<'a> {
    pub(crate) fn new(recipe: &'a Recipe) -> Self {
        Self(recipe, 0)
    }
}
impl<'a> Iterator for IngredientsIter<'a> {
    type Item = (ItemId, i32);

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
            if id.0.0 != 0 {
                let id = (id.0, id.1 as i32);
                return Some(id);
            }
        }
    }
}

/// This iterator will traverse the recipe tree for items that are related to using this item for crafting
pub(crate) fn recipe_tree_iter(item_id: ItemId) -> impl Iterator<Item = &'static Recipe> {
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

pub(crate) fn calculate_crafting_cost(recipe: &Recipe, prices: &CheapestListingsMap) -> (i32, i32) {
    let items = &xiv_gen_db::data().items;
    let sum_for = |prefer_hq: bool| -> i32 {
        IngredientsIter::new(recipe)
            .flat_map(|(ingredient, amount)| items.get(&ingredient).map(|item| (item, amount)))
            .flat_map(|(item, quantity)| {
                let pref_key = CheapestListingMapKey {
                    item_id: item.key_id.0,
                    hq: prefer_hq && item.can_be_hq,
                };
                let fallback_key = CheapestListingMapKey {
                    item_id: item.key_id.0,
                    hq: false,
                };
                prices
                    .map
                    .get(&pref_key)
                    .or_else(|| prices.map.get(&fallback_key))
                    .map(|d| d.price * quantity)
            })
            .sum()
    };
    (sum_for(true), sum_for(false))
}

#[component]
fn RecipePriceEstimate(recipe: &'static Recipe) -> impl IntoView {
    let cheapest_prices = use_context::<CheapestPrices>().unwrap();

    view! {
        <Suspense fallback=move || {
            view! { <SingleLineSkeleton /> }
        }>
            {move || {
                cheapest_prices
                    .read_listings
                    .with(|prices| {
                        let prices = prices.as_ref()?;
                        let prices = prices.as_ref().ok()?;
                        let (hq_amount, lq_amount) = calculate_crafting_cost(recipe, prices);
                        let result_view = view! {
                            <span class="flex flex-row gap-2 items-center">
                                <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_16%,transparent)] text-xs">"HQ:"</span>
                                <Gil amount=hq_amount />
                                <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] text-xs">"LQ:"</span>
                                <Gil amount=lq_amount />
                            </span>
                        };
                        Some(result_view)
                    })
            }}
        </Suspense>
    }
}

#[component]
fn Recipe(recipe: &'static Recipe, item_id: ItemId) -> impl IntoView {
    let items = &xiv_gen_db::data().items;
    let ingredients = IngredientsIter::new(recipe)
        .flat_map(|(ingredient, amount)| items.get(&ingredient).map(|item| (item, amount)))
        .map(|(ingredient, amount)| {
            view! {
                <div class="flex items-center justify-between gap-2 py-0.5">
                    <div class="flex items-center gap-2">
                        <span class="px-1.5 py-0.5 rounded-md bg-[color:color-mix(in_srgb,_var(--brand_ring)_14%,_transparent)] text-[color:var(--color-text)] text-xs">{amount.to_string()}</span>
                        <SmallItemDisplay item=ingredient />
                    </div>
                    <div class="text-xs"><CheapestPrice item_id=ingredient.key_id /></div>
                </div>
            }
        })
        .collect::<Vec<_>>();
    let target_item = items.get(&recipe.item_result)?;
    // role chips
    let is_target = recipe.item_result == item_id;
    let is_ingredient = IngredientsIter::new(recipe).any(|(i, _)| i == item_id);

    Some(view! {
        <div class="card p-3 space-y-2 rounded-lg">
            "Crafting Recipe:"
            <div class="flex items-center justify-between gap-2">
                <div class="flex items-center gap-2">
                    <SmallItemDisplay item=target_item />
                    <CheapestPrice item_id=target_item.key_id />
                </div>
                <div class="flex items-center gap-1">
                    {is_target.then(|| view! {
                        <span class="px-2 py-0.5 rounded-full text-xs font-medium
                                     bg-[color:color-mix(in_srgb,var(--brand-ring)_22%,transparent)]
                                     text-[color:var(--brand-fg)]">
                            "target"
                        </span>
                    })}
                    {is_ingredient.then(|| view! {
                        <span class="px-2 py-0.5 rounded-full text-xs font-medium
                                     bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]
                                     text-[color:var(--color-text)]">
                            "ingredient"
                        </span>
                    })}
                    <AddRecipeToList recipe />
                </div>
            </div>

            "Ingredients:"
            {ingredients}

            <div class="flex items-center gap-2 text-sm pt-1">
                <span class="underline">"Total craft cost:"</span>
                " "
                <RecipePriceEstimate recipe />
            </div>

            // Profitability at a glance
            <Suspense fallback=move || {
                view! { <SingleLineSkeleton /> }
            }>
                {move || {
                    use_context::<CheapestPrices>()
                        .unwrap()
                        .read_listings
                        .with(|data| {
                            let data = data.as_ref()?.as_ref().ok()?;
                            // compute costs again to compare directly
                            let sum_for = |prefer_hq: bool| -> i32 {
                                IngredientsIter::new(recipe)
                                    .flat_map(|(ingredient, amount)| {
                                        items.get(&ingredient).map(|item| (item, amount))
                                    })
                                    .flat_map(|(item, quantity)| {
                                        let pref_key = CheapestListingMapKey {
                                            item_id: item.key_id.0,
                                            hq: prefer_hq && item.can_be_hq,
                                        };
                                        let fallback_key = CheapestListingMapKey {
                                            item_id: item.key_id.0,
                                            hq: false,
                                        };
                                        data.map
                                                    .get(&pref_key)
                                                    .or_else(|| data.map.get(&fallback_key))
                                                    .map(|d| d.price * quantity)
                                            })
                                            .sum()
                                    };
                                    let hq_cost = sum_for(true);
                                    let lq_cost = sum_for(false);

                            let lq_sell = data
                                .map
                                .get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: false })
                                .map(|d| d.price);
                            let hq_sell = if target_item.can_be_hq {
                                data.map
                                    .get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: true })
                                    .or_else(|| data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: false }))
                                    .map(|d| d.price)
                            } else {
                                None
                            };

                            let profit_chip = |label: &str, profit_opt: Option<i32>| {
                                profit_opt.map(|profit| {
                                    let cls = if profit >= 0 {
                                        "px-2 py-0.5 rounded-full text-xs font-medium bg-[color:color-mix(in_srgb,#16a34a_22%,transparent)] text-[color:#bbf7d0]"
                                    } else {
                                        "px-2 py-0.5 rounded-full text-xs font-medium bg-[color:color-mix(in_srgb,#dc2626_18%,transparent)] text-[color:#fecaca]"
                                    };
                                    view! {
                                        <span class=cls>
                                            {label} ": " <Gil amount=profit />
                                        </span>
                                    }.into_any()
                                })
                            };

                            Some(view! {
                                <div class="flex flex-wrap items-center gap-2 text-sm">
                                    <span class="text-[color:var(--color-text-muted)] mr-1">"Profit:"</span>
                                    {profit_chip("HQ", hq_sell.map(|p| p - hq_cost))}
                                    {profit_chip("LQ", lq_sell.map(|p| p - lq_cost))}
                                </div>
                            })
                        })
                }}
            </Suspense>
        </div>
    }.into_any())
}

fn npc_rows(npc: &ENpcBase) -> impl Iterator<Item = u32> + '_ {
    // TODO- can I just parse the csv into a vec?
    #[allow(clippy::useless_conversion)]
    npc.e_npc_data.iter().map(|i| u32::from(i.0))
}

fn gil_shop_to_npc(gil_shops: &[GilShopId]) -> Vec<(GilShopId, &'static ENpcBase)> {
    let data = xiv_gen_db::data();

    data.e_npc_bases
        .values()
        .flat_map(|npc: &'static ENpcBase| {
            npc_rows(npc).flat_map(move |row| {
                let mut shops = Vec::new();
                let row_as_i32 = row as i32;
                if gil_shops.contains(&GilShopId(row_as_i32)) {
                    shops.push(GilShopId(row_as_i32));
                }

                if let Some(ts) = data.topic_selects.get(&xiv_gen::TopicSelectId(row_as_i32)) {
                    let ts_shops = [
                        &ts.shop_0, &ts.shop_1, &ts.shop_2, &ts.shop_3, &ts.shop_4, &ts.shop_5,
                        &ts.shop_6, &ts.shop_7, &ts.shop_8, &ts.shop_9,
                    ];
                    for shop in ts_shops {
                        let shop_id = GilShopId(shop.0 as i32);
                        if gil_shops.contains(&shop_id) {
                            shops.push(shop_id);
                        }
                    }
                }

                #[allow(clippy::collapsible_if)]
                if let Some(ph) = data.pre_handlers.get(&xiv_gen::PreHandlerId(row_as_i32)) {
                    if let Some(ts) = data
                        .topic_selects
                        .get(&xiv_gen::TopicSelectId(ph.target.0 as i32))
                    {
                        let ts_shops = [
                            &ts.shop_0, &ts.shop_1, &ts.shop_2, &ts.shop_3, &ts.shop_4, &ts.shop_5,
                            &ts.shop_6, &ts.shop_7, &ts.shop_8, &ts.shop_9,
                        ];
                        for shop in ts_shops {
                            let shop_id = GilShopId(shop.0 as i32);
                            if gil_shops.contains(&shop_id) {
                                shops.push(shop_id);
                            }
                        }
                    }
                }

                shops.into_iter().map(move |gil_shop| (gil_shop, npc))
            })
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
    let npcs = Memo::new(move |_| {
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
                        class="group flex flex-col gap-2 rounded-lg card p-3 transition-colors h-full hover:bg-[color:var(--color-base)]/50"
                    >
                        <div class="flex items-center justify-between gap-2 border-b border-[color:var(--color-outline)] pb-2">
                            <div class="font-medium text-[color:var(--color-text)]">{resident.singular.as_str()}</div>
                            <Gil amount=price />
                        </div>
                        <div class="text-sm text-[color:var(--color-text-muted)] flex items-center gap-1">
                            <Icon icon=icondata::FaStoreSolid attr:class="text-xs opacity-70" />
                            <span class="truncate">{shop.name.as_str()}</span>
                        </div>
                    </a>
                })
            }).collect_view())
    };
    let empty = move || npcs.with(|n| n.is_empty());
    view! {
        <div id="vendor-sources" class:collapse=empty class="space-y-1.5 p-1 max-h-80 overflow-y-auto w-full sm:w-96 xl:w-[600px]">
            <span class="text-sm font-semibold text-[color:var(--brand-fg)]">"Vendor sources"</span>
            <div class="grid grid-cols-1 gap-1.5">{data}</div>
        </div>
    }
    .into_any()
}

static VENDOR_ITEM_IDS: LazyLock<HashSet<i32>> = LazyLock::new(|| {
    let data = xiv_gen_db::data();
    let mut set = HashSet::new();
    for items in data.gil_shop_items.values() {
        for shop_item in items {
            set.insert(shop_item.item.0);
        }
    }
    set
});

pub(crate) fn is_vendor_item(item_id: i32) -> bool {
    VENDOR_ITEM_IDS.contains(&item_id)
}

pub(crate) fn get_vendor_price(item_id: i32) -> Option<u32> {
    if is_vendor_item(item_id) {
        let data = xiv_gen_db::data();
        if let Some(item) = data.items.get(&ItemId(item_id)) {
            let price = if item.price_mid > 0 {
                item.price_mid
            } else {
                item.price_low
            };
            return Some(price);
        }
    }
    None
}

pub(crate) fn special_shop_has_item(shop: &SpecialShop, item_id: i32) -> bool {
    // Check first slot (vector)
    if shop.item_receive_0.iter().any(|i| i.0 == item_id) {
        return true;
    }
    // Check second slot (vector)
    if shop.item_receive_1.iter().any(|i| i.0 == item_id) {
        return true;
    }
    false
}

type Cost = (ItemId, u32);
type TradeCosts = Vec<Cost>;

fn get_trade_costs(shop: &SpecialShop, item_id: i32) -> Vec<TradeCosts> {
    shop.item_receive_0
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let matches_0 = item.0 == item_id;
            // Check receive_1 if it exists at this index
            let matches_1 = shop
                .item_receive_1
                .get(i)
                .map(|x| x.0 == item_id)
                .unwrap_or(false);

            if matches_0 || matches_1 {
                Some(i)
            } else {
                None
            }
        })
        .map(|i| {
            let costs_0 = (shop.item_cost_0.get(i), shop.count_cost_0.get(i));
            let costs_1 = (shop.item_cost_1.get(i), shop.count_cost_1.get(i));
            let costs_2 = (shop.item_cost_2.get(i), shop.count_cost_2.get(i));
            [costs_0, costs_1, costs_2]
                .into_iter()
                .filter_map(|(item, count)| {
                    #[allow(clippy::collapsible_if)]
                    if let (Some(item), Some(count)) = (item, count) {
                        if item.0 != 0 && *count > 0 {
                            return Some((*item, *count));
                        }
                    }
                    None
                })
                .collect()
        })
        .collect()
}

#[component]
fn ExchangeSources(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let data = xiv_gen_db::data();
    let exchanges = Memo::new(move |_| {
        let item_id = item_id();
        data.special_shops
            .values()
            .filter(move |shop| special_shop_has_item(shop, item_id))
            .collect::<Vec<_>>()
    });

    let view = move || {
        exchanges
            .with(|exchanges| {
                exchanges
                    .iter()
                   .flat_map(|shop| {
                        let trades = get_trade_costs(shop, item_id());
                        trades.into_iter().map(move |costs| {
                            view! {
                                <div class="group flex items-center justify-between gap-2 rounded-lg card p-1.5 transition-colors">
                                    <div class="flex items-center gap-2 flex-wrap">
                                        <span class="text-sm font-medium">{shop.name.as_str()}</span>
                                        <div class="flex items-center gap-1.5 text-xs text-[color:var(--color-text-muted)]">
                                            "Costs:"
                                            {
                                                costs.into_iter().map(|(item_id, count)| {
                                                    if let Some(item) = data.items.get(&item_id) {
                                                        view! {
                                                            <div class="flex items-center gap-1 bg-[color:var(--color-base)]/50 px-1.5 py-0.5 rounded border border-[color:var(--color-outline)]">
                                                                <span>{count} "x"</span>
                                                                <SmallItemDisplay item />
                                                            </div>
                                                        }.into_any()
                                                    } else {
                                                        ().into_any()
                                                    }
                                                }).collect_view()
                                            }
                                        </div>
                                    </div>
                                </div>
                            }
                        })
                    })
                    .collect_view()
            })
    };

    let empty = move || exchanges.with(|e| e.is_empty());

    view! {
        <div id="exchange-sources" class:collapse=empty class="space-y-1.5 p-1 max-h-80 overflow-y-auto w-full sm:w-96 xl:w-[600px]">
            <span class="text-sm font-semibold text-[color:var(--brand-fg)]">"Exchange sources"</span>
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
                {view}
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiv_gen::{ItemId, SpecialShop};

    #[test]
    fn test_get_trade_costs() {
        let shop = SpecialShop {
            key_id: xiv_gen::SpecialShopId(1),
            name: "Test Shop".to_string(),
            complete_text: xiv_gen::DefaultTalkId(0),
            not_complete_text: xiv_gen::DefaultTalkId(0),
            item_receive_0: vec![ItemId(100), ItemId(200), ItemId(100)],
            count_receive_0: vec![1, 1, 1],
            item_receive_1: vec![ItemId(0), ItemId(0), ItemId(0)],
            count_receive_1: vec![0, 0, 0],
            item_cost_0: vec![ItemId(10), ItemId(20), ItemId(30)],
            count_cost_0: vec![5, 10, 15],
            item_cost_1: vec![ItemId(0), ItemId(0), ItemId(0)],
            count_cost_1: vec![0, 0, 0],
            item_cost_2: vec![ItemId(0), ItemId(0), ItemId(0)],
            count_cost_2: vec![0, 0, 0],
            hq_receive_0: vec![false, false, false],
            hq_receive_1: vec![false, false, false],
            hq_cost_0: vec![0, 0, 0],
            hq_cost_1: vec![0, 0, 0],
            hq_cost_2: vec![0, 0, 0],
            achievement_unlock: vec![
                xiv_gen::AchievementId(0),
                xiv_gen::AchievementId(0),
                xiv_gen::AchievementId(0),
            ],
            use_currency_type: 0,
            // Filling missing fields manually as Default is not implemented
            special_shop_item_category_0: vec![xiv_gen::SpecialShopItemCategoryId(0); 3],
            special_shop_item_category_1: vec![xiv_gen::SpecialShopItemCategoryId(0); 3],
            collectability_rating_cost_0: vec![0; 3],
            collectability_rating_cost_1: vec![0; 3],
            collectability_rating_cost_2: vec![0; 3],
            quest_item: vec![xiv_gen::QuestId(0); 3],
            patch_number: vec![0; 3],
            quest_unlock: xiv_gen::QuestId(0),
        };

        // Case 1: Searching for item 100. Should appear at indices 0 and 2.
        // Index 0 cost: Item 10, count 5
        // Index 2 cost: Item 30, count 15
        let costs_100 = get_trade_costs(&shop, 100);
        assert_eq!(costs_100.len(), 2);
        assert_eq!(costs_100[0], vec![(ItemId(10), 5)]);
        assert_eq!(costs_100[1], vec![(ItemId(30), 15)]);

        // Case 2: Searching for item 200. Should appear at index 1.
        // Index 1 cost: Item 20, count 10
        let costs_200 = get_trade_costs(&shop, 200);
        assert_eq!(costs_200.len(), 1);
        assert_eq!(costs_200[0], vec![(ItemId(20), 10)]);

        // Case 3: Searching for item 300. Not present.
        let costs_300 = get_trade_costs(&shop, 300);
        assert!(costs_300.is_empty());
    }
}

pub fn leve_rewards_item(
    leve: &Leve,
    item_id: i32,
    reward_items: &std::collections::HashMap<xiv_gen::LeveRewardItemId, LeveRewardItem>,
    groups: &std::collections::HashMap<xiv_gen::LeveRewardItemGroupId, LeveRewardItemGroup>,
) -> bool {
    if let Some(reward) = reward_items.get(&leve.leve_reward_item) {
        // Check all 8 groups
        let group_ids = [
            reward.leve_reward_item_group_0,
            reward.leve_reward_item_group_1,
            reward.leve_reward_item_group_2,
            reward.leve_reward_item_group_3,
            reward.leve_reward_item_group_4,
            reward.leve_reward_item_group_5,
            reward.leve_reward_item_group_6,
            reward.leve_reward_item_group_7,
        ];

        for group_id in group_ids {
            if let Some(group) = groups.get(&group_id) {
                // Check all items in group (0-8)
                let items = [
                    group.item_0,
                    group.item_1,
                    group.item_2,
                    group.item_3,
                    group.item_4,
                    group.item_5,
                    group.item_6,
                    group.item_7,
                    group.item_8,
                ];
                if items.iter().any(|i| i.0 == item_id) {
                    return true;
                }
            }
        }
    }
    false
}

#[component]
fn LeveSources(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let data = xiv_gen_db::data();
    let leves = Memo::new(move |_| {
        let item_id = item_id();
        data.leves
            .values()
            .filter(|leve| {
                leve_rewards_item(
                    leve,
                    item_id,
                    &data.leve_reward_items,
                    &data.leve_reward_item_groups,
                )
            })
            .collect::<Vec<_>>()
    });

    let view = move || {
        leves.with(|leves| {
            leves
                .iter()
                .map(|leve| {
                    let job_name = data.class_job_categorys.get(&leve.class_job_category).map(|c| c.name.as_str()).unwrap_or("Unknown");
                    view! {
                        <div class="group flex flex-col gap-2 rounded-lg card p-3 transition-colors h-full">
                             <div class="text-sm font-medium border-b border-[color:var(--color-outline)] pb-2 text-[color:var(--color-text)]">{leve.name.as_str()}</div>
                             <div class="flex items-center gap-2">
                                <span class="px-2 py-1 rounded bg-[color:var(--brand-900)]/30 border border-[color:var(--brand-700)]/30 text-xs text-[color:var(--brand-200)] font-medium">
                                    "Lvl " {leve.class_job_level}
                                </span>
                                <span class="text-xs text-[color:var(--color-text-muted)] truncate flex items-center gap-1">
                                    <Icon icon=icondata::FaHammerSolid attr:class="text-[10px] opacity-70" />
                                    {job_name}
                                </span>
                             </div>
                        </div>
                    }
                })
                .collect_view()
        })
    };

    let empty = move || leves.with(|l| l.is_empty());

    view! {
        <div id="leve-sources" class:collapse=empty class="space-y-1.5 p-1 max-h-80 overflow-y-auto w-full sm:w-96 xl:w-[600px]">
            <span class="text-sm font-semibold text-[color:var(--brand-fg)]">"Levequest rewards"</span>
            <div class="grid grid-cols-1 gap-1.5">{view}</div>
        </div>
    }
    .into_any()
}

#[component]
pub fn RelatedItems(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let db = xiv_gen_db::data();
    let item = Memo::new(move |_| db.items.get(&ItemId(item_id())));
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
                                attr:class="group flex flex-col gap-1 rounded-lg card p-1.5 transition-colors shadow-sm"
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

                                <div class="flex items-center gap-2 text-sm">
                                    <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium />
                                    <span class="flex-1 truncate">{item.name.as_str()}</span>
                                    <span class="text-xs text-[color:var(--color-text-muted)]">{item.level_item.0}</span>
                                </div>
                                <div class="text-xs text-[color:var(--brand-fg)]">
                                    <CheapestPrice item_id=item.key_id />
                                </div>
                            </A>
                        }
                    })
                    .take(12)
                    .collect_view()
            })
            .unwrap_or_default()
    };
    let recipes = Signal::derive(move || {
        recipe_tree_iter(ItemId(item_id.get()))
            .take(30)
            .collect::<Vec<_>>()
    });
    let (show_more, set_show_more) = signal(false);
    let has_more = move || {
        item()
            .map(|item| {
                item_set_iter(item)
                    .chain(prefix_item_iterator(item))
                    .chain(suffix_item_iterator(item))
                    .unique_by(|i| i.key_id)
                    .filter(|i| i.item_search_category.0 > 0)
                    .filter(|i| i.key_id.0 != item.key_id.0)
                    .count()
                    > 12
            })
            .unwrap_or(false)
    };

    view! {
        <div class="flex-col flex-auto flex-wrap p-1 w-full" class:hidden=move || item_set().is_empty()>
            <span class="content-title">"related items"</span>
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-2">
                {item_set}
                {move || {
                    show_more().then(|| {
                        item()
                            .map(|item| {
                                item_set_iter(item)
                                    .chain(prefix_item_iterator(item))
                                    .chain(suffix_item_iterator(item))
                                    .sorted_by_key(|i| i.key_id.0)
                                    .unique_by(|i| i.key_id)
                                    .filter(|i| i.item_search_category.0 > 0)
                                    .filter(|i| i.key_id.0 != item.key_id.0)
                                    .skip(12)
                                    .map(|item| {
                                        view! {
                                            <A
                                                attr:class="group flex flex-col gap-1 rounded-lg card p-1.5 transition-colors shadow-sm"
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

                                                <div class="flex items-center gap-2 text-sm">
                                                    <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium />
                                                    <span class="flex-1 truncate">{item.name.as_str()}</span>
                                                    <span class="text-xs text-[color:var(--color-text-muted)]">{item.level_item.0}</span>
                                                </div>
                                                <div class="text-xs text-[color:var(--brand-fg)]">
                                                    <CheapestPrice item_id=item.key_id />
                                                </div>
                                            </A>
                                        }
                                    })
                                    .collect_view()
                            })
                            .unwrap_or_default()
                    })
                }}
            </div>
            <div class="mt-2 flex justify-center" class:hidden=move || !has_more()>
                <button class="btn-secondary" on:click=move |_| set_show_more(!show_more())>
                    {move || if show_more() { "Show less" } else { "Show more" }}
                </button>
            </div>
        </div>
        <VendorItems item_id />
        <ExchangeSources item_id />
        <LeveSources item_id />
        <div
            id="crafting-recipes"
            class="content-well flex-col p-1"
            class:hidden=move || recipes.with(|recipes| recipes.is_empty())
        >
            <span class="content-title">"crafting recipes"</span>
            <div class="flex-wrap">
                <For
                    each=Signal::derive(move || recipes().into_iter().take(5).collect::<Vec<_>>())
                    key=|recipe| recipe.key_id
                    children=move |recipe: &'static Recipe| {
                        view! { <Recipe recipe item_id=ItemId(item_id()) /> }
                    }
                />
            </div>
        </div>
    }
    .into_any()
}
