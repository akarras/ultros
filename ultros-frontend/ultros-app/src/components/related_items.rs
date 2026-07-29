use itertools::Itertools;
/// Related items links items that are related to the current set
use leptos::prelude::*;
use leptos_router::components::A;
use std::collections::HashSet;
use std::sync::LazyLock;
use ultros_api_types::{cheapest_listings::CheapestListingMapKey, icon_size::IconSize};
use xiv_gen::{
    ENpcBase, ENpcResidentId, GilShopId, Item, ItemId, Leve, LeveRewardItem, LeveRewardItemGroup,
    Recipe, SpecialShop,
};

use crate::{
    components::{
        add_recipe_to_list::AddRecipeToList,
        crafting_cost::{
            CraftingCostOptions, EmptyOnHand, IngredientsIter, ShardsMode, compute_cost,
        },
        icon::Icon,
        item_icon::ItemIcon,
        on_hand_input::{ActiveListBanner, LocalOnHand, OnHandMap},
        skeleton::SingleLineSkeleton,
    },
    global_state::{
        cheapest_prices::CheapestPrices, home_world::get_price_zone, xiv_data::tracked_data,
    },
    i18n::*,
};

use super::{cheapest_price::*, gil::*, small_item_display::*};

fn job_code_from_craft_type(craft_type: i32) -> &'static str {
    match craft_type {
        0 => "CRP",
        1 => "BSM",
        2 => "ARM",
        3 => "GSM",
        4 => "LTW",
        5 => "WVR",
        6 => "ALC",
        7 => "CUL",
        _ => "",
    }
}

pub(crate) fn is_shard_item(item_id: ItemId) -> bool {
    tracked_data()
        .items
        .get(&item_id)
        .map(|i| i.item_search_category == 59)
        .unwrap_or(false)
}

/// Matches against items that start with the same prefix
/// "Diadochos" -> "Diadochos Helmet" etc
fn prefix_item_iterator(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &tracked_data().items;
    let prefix = item.name.split_once(' ').map(|(prefix, _)| prefix);
    items.values().filter(move |f| {
        if let Some(prefix) = prefix {
            f.name.starts_with(prefix)
                && f.item_search_category != 0
                && f.level_item == item.level_item
        } else {
            false
        }
    })
}

fn suffix_item_iterator(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &tracked_data().items;
    let suffix = item.name.rsplit_once(' ').map(|(_, suffix)| suffix);
    items.values().filter(move |f| {
        if let Some(suffix) = suffix {
            f.name.ends_with(suffix)
                && f.item_search_category != 0
                && f.level_item == item.level_item
        } else {
            false
        }
    })
}

/// This iterator will attempt to find related items using the classjobcategory && ilvl
fn item_set_iter(item: &'static Item) -> impl Iterator<Item = &'static Item> {
    let items = &tracked_data().items;
    items.values().filter(|i| {
        item.class_job_category != 0
            && item.class_job_category == i.class_job_category
            && item.level_item == i.level_item
            && i.key_id != item.key_id
            && item.item_search_category > 0
    })
}

/// This iterator will traverse the recipe tree for items that are related to using this item for crafting
pub(crate) fn recipe_tree_iter(item_id: ItemId) -> impl Iterator<Item = &'static Recipe> {
    let recipes = &tracked_data().recipes;
    // our item id could be in item_result, or item_ingredient
    recipes
        .values()
        .filter(move |filter| {
            ItemId(filter.item_result) == item_id
                || IngredientsIter::new(filter).any(|(i, _amount)| i.0 == item_id.0)
        })
        .sorted_by_key(|r| r.key_id.0)
}

#[component]
fn RecipePriceEstimate(recipe: &'static Recipe) -> impl IntoView {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::{self, CraftOptions};

    let i18n = use_i18n();
    let cheapest_prices = use_context::<CheapestPrices>().unwrap();
    let cookies = use_context::<Cookies>().unwrap();
    let (opts_cookie, _) = cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);
    let on_hand_map = use_context::<OnHandMap>();

    // Defer the resource read until after the first client render so SSR and
    // the initial CSR hydration both render the skeleton (the same shape as
    // the Suspense fallback). Same idiom as #740 (cheapest-price), #732
    // (source-callout), #730 (relative-time), #725 (chart cutoff), #719
    // (item-explorer): `Resource::with()` does not subscribe-and-suspend the
    // wrapping `<Suspense>`, so SSR sees the resource as pending and the body
    // returns `None` while CSR receives the serialised resource and would
    // immediately emit the populated `<span>`. The structural mismatch trips
    // tachys' walker at `tachys-0.2.15/src/hydration.rs:227` and cascades
    // into the `RefCell already borrowed` panic from the
    // wasm-bindgen-futures executor.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });

    view! {
        <Suspense fallback=move || view! { <SingleLineSkeleton /> }>
            {move || {
                if !hydrated.get() {
                    return view! { <SingleLineSkeleton /> }.into_any();
                }
                cheapest_prices.read_listings.with(|prices| {
                    let prices = prices.as_ref()?.as_ref().ok()?;
                    let opts_value = opts_cookie.get().unwrap_or_default();
                    let shards = if opts_value.exclude_shards {
                        ShardsMode::ExcludeShards
                    } else {
                        ShardsMode::IncludeMarket
                    };

                    // Snapshot the LocalStorage on-hand if available.
                    let local = on_hand_map
                        .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                        .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
                    let empty = EmptyOnHand;
                    let active_on_hand: &dyn crate::components::crafting_cost::OnHand =
                        if opts_value.use_on_hand { &local } else { &empty };

                    let recipes_by_output = std::collections::HashMap::new();

                    let lq_opts = CraftingCostOptions {
                        require_hq: false,
                        max_subcraft_depth: 0,
                        shards,
                        on_hand: active_on_hand,
                    };
                    let lq = compute_cost(recipe, prices, &recipes_by_output, &lq_opts, &is_shard_item);

                    // Re-snapshot on-hand for the HQ pass (the LQ pass consumed it).
                    let local_hq = on_hand_map
                        .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                        .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
                    let active_on_hand_hq: &dyn crate::components::crafting_cost::OnHand =
                        if opts_value.use_on_hand { &local_hq } else { &empty };
                    let hq_opts = CraftingCostOptions {
                        require_hq: true,
                        max_subcraft_depth: 0,
                        shards,
                        on_hand: active_on_hand_hq,
                    };
                    let hq = compute_cost(recipe, prices, &recipes_by_output, &hq_opts, &is_shard_item);

                    Some(view! {
                        <span class="flex flex-row gap-2 items-center flex-wrap">
                            <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_16%,transparent)] text-xs">{t!(i18n, related_recipe_hq_label)}</span>
                            <Gil amount=hq.cost />
                            <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] text-xs">{t!(i18n, related_recipe_lq_label)}</span>
                            <Gil amount=lq.cost />
                            {(lq.shard_cost > 0 && opts_value.exclude_shards).then(|| view! {
                                <span class="px-1.5 py-0.5 rounded bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] text-[10px] text-[color:var(--color-text-muted)]">
                                    "shards excl. " <Gil amount=lq.shard_cost />
                                </span>
                            })}
                            {(lq.on_hand_savings > 0).then(|| view! {
                                <span class="px-1.5 py-0.5 rounded bg-emerald-900/30 text-emerald-300 text-[10px]">
                                    "saved " <Gil amount=lq.on_hand_savings />
                                </span>
                            })}
                        </span>
                    }.into_any())
                }).unwrap_or_else(|| ().into_any())
            }}
        </Suspense>
    }
}

#[component]
fn CraftOptionsToggleRow() -> impl IntoView {
    use crate::global_state::cookies::Cookies;
    use crate::global_state::craft_options::{self, CraftOptions};
    let cookies = use_context::<Cookies>().unwrap();
    let (opts_signal, set_opts) =
        cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);

    let opts = move || opts_signal.get().unwrap_or_default();
    let toggle = move |mutator: Box<dyn Fn(&mut CraftOptions)>| {
        let mut current = opts();
        mutator(&mut current);
        set_opts(Some(current));
    };

    // The item-page recipe panel always shows both HQ and LQ chips, so a
    // "Require HQ" toggle would be a no-op here. The analyzers (Tasks 8-9)
    // get their own filter cards that read/write the same cookie field.
    view! {
        <div class="flex flex-row items-center gap-3 text-xs flex-wrap">
            <label class="flex flex-row items-center gap-1">
                <input
                    type="checkbox"
                    class="checkbox checkbox-xs"
                    prop:checked=move || opts().exclude_shards
                    on:change=move |_| toggle(Box::new(|o| o.exclude_shards = !o.exclude_shards))
                />
                "Exclude shards"
            </label>
            <label class="flex flex-row items-center gap-1">
                <input
                    type="checkbox"
                    class="checkbox checkbox-xs"
                    prop:checked=move || opts().use_on_hand
                    on:change=move |_| toggle(Box::new(|o| o.use_on_hand = !o.use_on_hand))
                />
                "Use on-hand"
            </label>
        </div>
    }
}

#[component]
fn Recipe(recipe: &'static Recipe, item_id: ItemId) -> impl IntoView {
    let i18n = use_i18n();
    let job = job_code_from_craft_type(recipe.craft_type);
    let analyzer_href = move || {
        use crate::global_state::cookies::Cookies;
        use crate::global_state::craft_options::{self, CraftOptions};
        let cookies = use_context::<Cookies>().unwrap();
        let (opts, _) = cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);
        let o = opts.get().unwrap_or_default();
        format!(
            "/recipe-analyzer?job={job}&require-hq={hq}&subcrafts={sub}&shards-exclude={shards}&on-hand={oh}",
            job = job,
            hq = o.require_hq,
            sub = o.include_subcrafts,
            shards = o.exclude_shards,
            oh = o.use_on_hand,
        )
    };
    let items = &tracked_data().items;
    let ingredients = IngredientsIter::new(recipe)
        .flat_map(|(ingredient, amount)| items.get(&ingredient).map(|item| (item, amount)))
        .map(|(ingredient, amount)| {
            view! {
                <div class="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 py-1">
                    <span class="px-1.5 py-0.5 rounded-md bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)] text-[color:var(--color-text)] text-xs tabular-nums text-center min-w-7">{amount.to_string()}</span>
                    <div class="min-w-0">
                        <SmallItemDisplay item=ingredient />
                    </div>
                    <div class="text-xs justify-self-end whitespace-nowrap"><CheapestPrice item_id=ingredient.key_id /></div>
                </div>
            }
        })
        .collect::<Vec<_>>();
    let target_item = items.get(&ItemId(recipe.item_result))?;
    // role chips
    let is_target = ItemId(recipe.item_result) == item_id;
    let is_ingredient = IngredientsIter::new(recipe).any(|(i, _)| i == item_id);

    // Defer the inner profit-chip Suspense's resource read until after the
    // first client render — same idiom as #740 and `RecipePriceEstimate`
    // above. `Resource::with()` does not subscribe-and-suspend, so SSR
    // returns `None` (skeleton fallback) while CSR hydration sees the
    // serialised resource and would otherwise emit the populated profit
    // `<div>` immediately, tripping tachys' walker at
    // `tachys-0.2.15/src/hydration.rs:227`.
    let profit_hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        profit_hydrated.set(true);
    });

    Some(view! {
        <div class="card p-4 sm:p-5 space-y-4 rounded-lg border border-brand-700/30 hover:shadow-lg hover:border-brand-500/50 transition-all min-w-0">
            <div class="flex flex-col gap-3 border-b border-brand-700/30 pb-3 lg:flex-row lg:items-center lg:justify-between">
                <div class="flex min-w-0 flex-wrap items-center gap-3">
                    <SmallItemDisplay item=target_item />
                    <CheapestPrice item_id=target_item.key_id />
                </div>
                <div class="flex shrink-0 flex-wrap items-center gap-1.5">
                    {is_target.then(|| view! {
                        <span class="px-2 py-0.5 rounded-full text-xs font-bold
                                     border border-emerald-400/40
                                     text-emerald-200">
                            {t!(i18n, related_recipe_target_chip)}
                        </span>
                    })}
                    {is_ingredient.then(|| view! {
                        <span class="px-2 py-0.5 rounded-full text-xs font-bold
                                     border border-blue-400/40
                                     text-blue-200">
                            {t!(i18n, related_recipe_ingredient_chip)}
                        </span>
                    })}
                    <AddRecipeToList recipe />
                    <a
                        class="btn-secondary text-xs px-2 py-1 flex flex-row items-center gap-1"
                        href=analyzer_href
                        aria-label=t_string!(i18n, related_items_aria_open_recipe)
                    >
                        <Icon icon=icondata::AiBarChartOutlined />
                        "Analyzer"
                    </a>
                </div>
            </div>

            <div class="space-y-2">
                <div class="text-xs font-semibold text-brand-300 uppercase tracking-wide">{t!(i18n, related_recipe_ingredients_heading)}</div>
                <div class="rounded-md border border-brand-700/25 bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] px-3 py-2">
                    {ingredients}
                </div>
            </div>

            <div class="grid gap-3 pt-3 border-t border-brand-700/30 sm:grid-cols-2">
                <div class="flex flex-wrap items-center justify-between gap-2 text-sm">
                    <span class="text-brand-300">{t!(i18n, related_recipe_est_cost)}</span>
                    <RecipePriceEstimate recipe />
                </div>

                // Profitability at a glance
                <Suspense fallback=move || view! { <SingleLineSkeleton /> }>
                    {move || {
                        if !profit_hydrated.get() {
                            return view! { <SingleLineSkeleton /> }.into_any();
                        }
                        use crate::global_state::cookies::Cookies;
                        use crate::global_state::craft_options::{self, CraftOptions};
                        let cookies = use_context::<Cookies>().unwrap();
                        let (opts_cookie, _) = cookies.use_cookie_typed::<_, CraftOptions>(craft_options::COOKIE_NAME);
                        let on_hand_map = use_context::<OnHandMap>();

                        use_context::<CheapestPrices>().unwrap().read_listings.with(|data| {
                            let data = data.as_ref()?.as_ref().ok()?;
                            let opts_value = opts_cookie.get().unwrap_or_default();
                            let shards = if opts_value.exclude_shards {
                                ShardsMode::ExcludeShards
                            } else { ShardsMode::IncludeMarket };

                            let local = on_hand_map
                                .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                                .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
                            let empty = EmptyOnHand;
                            let active: &dyn crate::components::crafting_cost::OnHand =
                                if opts_value.use_on_hand { &local } else { &empty };
                            let recipes_by_output = std::collections::HashMap::new();

                            let lq_opts = CraftingCostOptions {
                                require_hq: false, max_subcraft_depth: 0, shards, on_hand: active,
                            };
                            let lq = compute_cost(recipe, data, &recipes_by_output, &lq_opts, &is_shard_item);

                            let local_hq = on_hand_map
                                .map(|m| LocalOnHand::from_map(m.0.get_untracked()))
                                .unwrap_or_else(|| LocalOnHand::from_map(Default::default()));
                            let active_hq: &dyn crate::components::crafting_cost::OnHand =
                                if opts_value.use_on_hand { &local_hq } else { &empty };
                            let hq_opts = CraftingCostOptions {
                                require_hq: true, max_subcraft_depth: 0, shards, on_hand: active_hq,
                            };
                            let hq = compute_cost(recipe, data, &recipes_by_output, &hq_opts, &is_shard_item);

                            let lq_sell = data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: false }).map(|d| d.price);
                            let hq_sell = if target_item.can_be_hq {
                                data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: true })
                                    .or_else(|| data.map.get(&CheapestListingMapKey { item_id: target_item.key_id.0, hq: false }))
                                    .map(|d| d.price)
                            } else { None };

                            let profit_chip = |label: String, profit_opt: Option<i32>| {
                                profit_opt.map(|profit| {
                                    let cls = if profit >= 0 {
                                        "px-2 py-0.5 rounded-full text-xs font-bold text-emerald-300 border border-emerald-400/40 flex items-center gap-1"
                                    } else {
                                        "px-2 py-0.5 rounded-full text-xs font-bold text-red-300 border border-red-400/40 flex items-center gap-1"
                                    };
                                    view! { <span class=cls><span>{label}</span><Gil amount=profit /></span> }.into_any()
                                })
                            };

                            Some(view! {
                                <div class="flex flex-wrap items-center justify-between gap-2 text-sm mt-2">
                                    <span class="text-brand-300">{t!(i18n, related_recipe_est_profit)}</span>
                                    <div class="flex flex-wrap justify-end gap-2">
                                        {profit_chip(t_string!(i18n, hq).to_string(), hq_sell.map(|p| p - hq.cost))}
                                        {profit_chip(t_string!(i18n, lq).to_string(), lq_sell.map(|p| p - lq.cost))}
                                    </div>
                                </div>
                            }.into_any())
                        }).unwrap_or_else(|| ().into_any())
                    }}
                </Suspense>
            </div>
        </div>
    }.into_any())
}

fn npc_rows(npc: &ENpcBase) -> impl Iterator<Item = u32> + '_ {
    npc.e_npc_data.iter().copied()
}

fn gil_shop_to_npc(gil_shops: &[GilShopId]) -> Vec<(GilShopId, &'static ENpcBase)> {
    let data = tracked_data();

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
                    for shop in ts.shop {
                        let shop_id = GilShopId(shop);
                        if gil_shops.contains(&shop_id) {
                            shops.push(shop_id);
                        }
                    }
                }

                #[allow(clippy::collapsible_if)]
                if let Some(ph) = data.pre_handlers.get(&xiv_gen::PreHandlerId(row_as_i32)) {
                    if let Some(ts) = data.topic_selects.get(&xiv_gen::TopicSelectId(ph.target)) {
                        for shop in ts.shop {
                            let shop_id = GilShopId(shop);
                            if gil_shops.contains(&shop_id) {
                                shops.push(shop_id);
                            }
                        }
                    }
                }

                shops.into_iter().map(move |gil_shop| (gil_shop, npc))
            })
        })
        // `e_npc_bases` is a std HashMap whose iteration order is randomized
        // per process (RandomState). Without a stable sort the SSR server and
        // the hydrating wasm client emit the vendor rows in different orders,
        // desyncing the DOM and tripping tachys' hydration walker (#6831).
        // Sort by stable ids so both sides render the same sequence.
        .sorted_by_key(|(gil_shop, npc)| (npc.key_id.0, gil_shop.0))
        .collect()
}

#[component]
fn VendorItems(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let data = tracked_data();
    // lookup items
    let npcs = Memo::new(move |_| {
        let item_id = item_id();
        let gil_shops = data
            .gil_shop_items
            .iter()
            .filter(|(_shop_id, items)| items.iter().any(|shop_item| shop_item.item == item_id))
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
                        class="group flex flex-col gap-2 rounded-lg card p-3 transition-all h-full hover:bg-[color:var(--color-base)]/50 hover:shadow-md border border-brand-700/30"
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
        <div id="vendor-sources" class:hidden=empty class="panel p-4 sm:p-6 flex flex-col gap-4 max-h-[500px] overflow-y-auto">
            <h3 class="text-lg font-bold text-brand-200 flex items-center gap-2">
                <Icon icon=icondata::FaShopSolid attr:class="text-brand-300" />
                "Vendor Sources"
            </h3>
            <div class="grid grid-cols-1 gap-3">{data}</div>
        </div>
    }
    .into_any()
}

static VENDOR_ITEM_IDS: LazyLock<HashSet<i32>> = LazyLock::new(|| {
    let data = tracked_data();
    let mut set = HashSet::new();
    for items in data.gil_shop_items.values() {
        for shop_item in items {
            set.insert(shop_item.item);
        }
    }
    set
});

pub(crate) fn is_vendor_item(item_id: i32) -> bool {
    VENDOR_ITEM_IDS.contains(&item_id)
}

pub(crate) fn get_vendor_price(item_id: i32) -> Option<u32> {
    if is_vendor_item(item_id) {
        let data = tracked_data();
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
    // Check both possible receive slots for each of the 60 entries
    shop.item_receive_0.iter().any(|&i| i as i32 == item_id)
        || shop.item_receive_1.iter().any(|&i| i as i32 == item_id)
}

type Cost = (ItemId, u32);
type TradeCosts = Vec<Cost>;

fn get_trade_costs(shop: &SpecialShop, item_id: i32) -> Vec<TradeCosts> {
    let mut results = Vec::new();
    // SpecialShop has 60 entries, each with up to 2 receive items and 3 cost items
    for i in 0..60 {
        let is_receive_0 = shop
            .item_receive_0
            .get(i)
            .map(|&id| id as i32 == item_id)
            .unwrap_or(false);
        let is_receive_1 = shop
            .item_receive_1
            .get(i)
            .map(|&id| id as i32 == item_id)
            .unwrap_or(false);

        if is_receive_0 || is_receive_1 {
            let mut costs = Vec::new();
            // Check all three possible cost slots
            if let Some(&cost_item) = shop.item_cost_0.get(i)
                && cost_item > 0
            {
                let count = shop.count_cost_0.get(i).cloned().unwrap_or(0);
                costs.push((ItemId(cost_item as i32), count));
            }
            if let Some(&cost_item) = shop.item_cost_1.get(i)
                && cost_item > 0
            {
                let count = shop.count_cost_1.get(i).cloned().unwrap_or(0);
                costs.push((ItemId(cost_item as i32), count));
            }
            if let Some(&cost_item) = shop.item_cost_2.get(i)
                && cost_item > 0
            {
                let count = shop.count_cost_2.get(i).cloned().unwrap_or(0);
                costs.push((ItemId(cost_item as i32), count));
            }

            if !costs.is_empty() {
                results.push(costs);
            }
        }
    }
    results
}

/// Collect the special shops that trade for `item_id`, in a stable,
/// `key_id`-ascending order.
///
/// `special_shops` is a `std::collections::HashMap` whose iteration order is
/// randomized per process (`RandomState`). The SSR server process and the
/// client wasm instance each build their own copy of the game data, so an
/// unsorted `.values()` yields the shops in a *different* order on each side.
/// That makes the server-rendered DOM and the hydrating DOM disagree, tripping
/// tachys' hydration walker (`failed_to_cast_element`) — the #6831 crash.
/// Sorting by the stable `key_id` makes both sides render the same sequence.
fn exchange_shops_for_item(
    special_shops: &std::collections::HashMap<xiv_gen::SpecialShopId, SpecialShop>,
    item_id: i32,
) -> Vec<&SpecialShop> {
    special_shops
        .values()
        .filter(|shop| special_shop_has_item(shop, item_id))
        .sorted_by_key(|shop| shop.key_id.0)
        .collect()
}

#[component]
fn ExchangeSources(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let exchanges = Memo::new(move |_| {
        let item_id = item_id();
        exchange_shops_for_item(&data.special_shops, item_id)
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
                                <div class="group flex flex-col gap-2 rounded-lg card p-3 transition-all hover:shadow-md border border-brand-700/30">
                                    <span class="text-sm font-medium border-b border-[color:var(--color-outline)] pb-2 text-brand-100">{shop.name.as_str()}</span>
                                    <div class="flex items-center gap-2 flex-wrap text-xs text-[color:var(--color-text-muted)] mt-1">
                                        <span class="font-semibold text-brand-300">{t!(i18n, related_items_costs_label)}</span>
                                        {
                                            costs.into_iter().map(|(item_id, count)| {
                                                if let Some(item) = data.items.get(&item_id) {
                                                    view! {
                                                        <div class="flex items-center gap-1 px-2 py-1 rounded border border-[color:var(--color-outline)] hover:border-brand-300/60 transition-colors">
                                                            <span class="font-bold text-brand-200">{count} "x"</span>
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
                            }
                        })
                    })
                    .collect_view()
            })
    };

    let empty = move || exchanges.with(|e| e.is_empty());

    view! {
        <div id="exchange-sources" class:hidden=empty class="panel p-4 sm:p-6 flex flex-col gap-4 max-h-[500px] overflow-y-auto">
            <h3 class="text-lg font-bold text-brand-200 flex items-center gap-2">
                <Icon icon=icondata::BsArrowLeftRight attr:class="text-brand-300" />
                "Exchange Sources"
            </h3>
            <div class="grid grid-cols-1 gap-3">
                {view}
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiv_gen::SpecialShop;

    #[test]
    fn test_special_shop_has_item() {
        let shop = SpecialShop {
            key_id: xiv_gen::SpecialShopId(1),
            name: "Test Shop".to_string(),
            item: vec![],
            item_receive_0: vec![123, 456],
            count_receive_0: vec![1, 1],
            item_receive_1: vec![0, 789],
            count_receive_1: vec![0, 1],
            item_cost_0: vec![10, 20],
            count_cost_0: vec![100, 200],
            item_cost_1: vec![0, 0],
            count_cost_1: vec![0, 0],
            item_cost_2: vec![0, 0],
            count_cost_2: vec![0, 0],
        };
        assert!(special_shop_has_item(&shop, 123));
        assert!(special_shop_has_item(&shop, 789));
        assert!(!special_shop_has_item(&shop, 999));
    }

    #[test]
    fn test_get_trade_costs() {
        let shop = SpecialShop {
            key_id: xiv_gen::SpecialShopId(1),
            name: "Test Shop".to_string(),
            item: vec![],
            item_receive_0: vec![123, 456],
            count_receive_0: vec![1, 1],
            item_receive_1: vec![0, 789],
            count_receive_1: vec![0, 1],
            item_cost_0: vec![10, 20],
            count_cost_0: vec![100, 200],
            item_cost_1: vec![11, 0],
            count_cost_1: vec![50, 0],
            item_cost_2: vec![0, 0],
            count_cost_2: vec![0, 0],
        };

        let costs_123 = get_trade_costs(&shop, 123);
        assert_eq!(costs_123.len(), 1);
        assert_eq!(costs_123[0].len(), 2);
        assert_eq!(costs_123[0][0], (ItemId(10), 100));
        assert_eq!(costs_123[0][1], (ItemId(11), 50));

        let costs_789 = get_trade_costs(&shop, 789);
        assert_eq!(costs_789.len(), 1);
        assert_eq!(costs_789[0].len(), 1);
        assert_eq!(costs_789[0][0], (ItemId(20), 200));
    }

    /// Regression guard for #6831: the exchange-source shops must come out in a
    /// stable, `key_id`-ascending order no matter what order they sit in the
    /// (randomly seeded) `special_shops` HashMap. If this order ever depends on
    /// HashMap iteration order again, SSR and CSR render different DOM and the
    /// item page crashes on hydration.
    #[test]
    fn exchange_shops_for_item_is_deterministically_sorted() {
        use std::collections::HashMap;

        // A shop whose first receive slot trades for `received`.
        fn mk_shop(key_id: i32, received: u16) -> SpecialShop {
            SpecialShop {
                key_id: xiv_gen::SpecialShopId(key_id),
                name: format!("Shop {key_id}"),
                item: vec![],
                item_receive_0: vec![received],
                count_receive_0: vec![1],
                item_receive_1: vec![0],
                count_receive_1: vec![0],
                item_cost_0: vec![10],
                count_cost_0: vec![1],
                item_cost_1: vec![0],
                count_cost_1: vec![0],
                item_cost_2: vec![0],
                count_cost_2: vec![0],
            }
        }

        let mut shops = HashMap::new();
        // Insert in a deliberately non-ascending key order.
        for key_id in [7, 2, 9, 4, 1] {
            shops.insert(xiv_gen::SpecialShopId(key_id), mk_shop(key_id, 123));
        }
        // Two shops that do NOT trade for item 123 must be filtered out.
        shops.insert(xiv_gen::SpecialShopId(50), mk_shop(50, 999));
        shops.insert(xiv_gen::SpecialShopId(51), mk_shop(51, 888));

        let ids: Vec<i32> = exchange_shops_for_item(&shops, 123)
            .iter()
            .map(|shop| shop.key_id.0)
            .collect();

        // Ascending key_id order regardless of HashMap seed — the deterministic
        // sequence both SSR and CSR must produce.
        assert_eq!(ids, vec![1, 2, 4, 7, 9]);
    }
}

pub fn leve_rewards_item(
    leve: &Leve,
    item_id: i32,
    reward_items: &std::collections::HashMap<xiv_gen::LeveRewardItemId, LeveRewardItem>,
    groups: &std::collections::HashMap<xiv_gen::LeveRewardItemGroupId, LeveRewardItemGroup>,
) -> bool {
    if let Some(reward) = reward_items.get(&xiv_gen::LeveRewardItemId(leve.leve_reward_item)) {
        // Check all 8 groups
        let group_ids: [u16; 8] = [
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
            if let Some(group) = groups.get(&xiv_gen::LeveRewardItemGroupId(group_id as i32)) {
                // Check all items in group (0-8)
                let items: [u16; 9] = [
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
                if items.iter().any(|&i| i as i32 == item_id) {
                    return true;
                }
            }
        }
    }
    false
}

#[component]
fn LeveSources(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let data = tracked_data();
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
            // `leves` is a randomized std HashMap; sort by stable id so SSR and
            // CSR render the levequest rows in the same order (#6831 hydration).
            .sorted_by_key(|leve| leve.key_id.0)
            .collect::<Vec<_>>()
    });

    let view = move || {
        leves.with(|leves| {
            leves
                .iter()
                .map(|leve| {
                    let job_name = data.class_job_categorys.get(&xiv_gen::ClassJobCategoryId(leve.class_job_category)).map(|c| c.name.as_str()).unwrap_or("Unknown");
                    view! {
                        <div class="group flex flex-col gap-2 rounded-lg card p-3 transition-all h-full hover:shadow-md border border-[color:var(--color-outline)] hover:border-brand-300/60">
                             <div class="text-sm font-medium border-b border-[color:var(--color-outline)] pb-2 text-brand-100">{leve.name.as_str()}</div>
                             <div class="flex items-center gap-2 mt-1">
                                <span class="px-2 py-1 rounded border border-brand-400/40 text-xs text-brand-200 font-bold">
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
        <div id="leve-sources" class:hidden=empty class="panel p-4 sm:p-6 flex flex-col gap-4 max-h-[500px] overflow-y-auto">
            <h3 class="text-lg font-bold text-brand-200 flex items-center gap-2">
                <Icon icon=icondata::FaScrollSolid attr:class="text-brand-300" />
                "Levequest Rewards"
            </h3>
            <div class="grid grid-cols-1 gap-3">{view}</div>
        </div>
    }
    .into_any()
}

#[component]
pub fn RelatedItems(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let i18n = use_i18n();
    let db = tracked_data();
    // ⚡ Bolt Optimization: Replace Memo::new with Signal::derive for O(1) ops
    let item = Signal::derive(move || db.items.get(&ItemId(item_id())));
    let (price_zone, _) = get_price_zone();
    let related_items_data = Memo::new(move |_| {
        item()
            .map(|item| {
                item_set_iter(item)
                    .chain(prefix_item_iterator(item))
                    .chain(suffix_item_iterator(item))
                    .sorted_by_key(|i| i.key_id.0)
                    .unique_by(|i| i.key_id)
                    .filter(|i| i.item_search_category > 0)
                    .filter(|i| i.key_id.0 != item.key_id.0)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });

    let item_set = move || {
        related_items_data.with(|items| {
            items.iter().take(12).map(|&item| {
                view! {
                    <A
                        attr:class="group flex flex-col gap-2 rounded-lg p-3 transition-all hover:scale-[1.02] hover:shadow-lg border border-[color:var(--color-outline)] hover:border-brand-300/60"
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
                            <span class="flex-1 truncate font-medium text-brand-100">{item.name.as_str()}</span>
                            <span class="text-xs text-[color:var(--color-text-muted)] px-1.5 py-0.5 rounded border border-[color:var(--color-outline)]">"iLvl " {item.level_item}</span>
                        </div>
                        <div class="text-sm font-bold text-[color:var(--brand-fg)] mt-1 ml-1">
                            <CheapestPrice item_id=item.key_id />
                        </div>
                    </A>
                }
            }).collect_view()
        })
    };

    let recipes = Memo::new(move |_| {
        recipe_tree_iter(ItemId(item_id.get()))
            .take(30)
            .collect::<Vec<_>>()
    });

    let (show_more, set_show_more) = signal(false);
    let has_more = move || related_items_data.with(|items| items.len() > 12);

    view! {
        <div class="flex flex-col gap-6">
            <div class="panel p-4 sm:p-6" class:hidden=move || related_items_data.with(|i| i.is_empty())>
                <h2 class="text-xl font-bold text-brand-200 mb-4 px-1">{t!(i18n, related_items_heading)}</h2>
                <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
                    {item_set}
                    {move || {
                        show_more().then(|| {
                            related_items_data.with(|items| {
                                items.iter().skip(12).map(|&item| {
                                    view! {
                                        <A
                                            attr:class="group flex flex-col gap-2 rounded-lg p-3 transition-all hover:scale-[1.02] hover:shadow-lg border border-[color:var(--color-outline)] hover:border-brand-300/60"
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
                                                <span class="flex-1 truncate font-medium text-brand-100">{item.name.as_str()}</span>
                                                <span class="text-xs text-[color:var(--color-text-muted)] px-1.5 py-0.5 rounded border border-[color:var(--color-outline)]">"iLvl " {item.level_item}</span>
                                            </div>
                                            <div class="text-sm font-bold text-[color:var(--brand-fg)] mt-1 ml-1">
                                                <CheapestPrice item_id=item.key_id />
                                            </div>
                                        </A>
                                    }
                                }).collect_view()
                            })
                        })
                    }}
                </div>
                <div class="mt-4 flex justify-center" class:hidden=move || !has_more()>
                    <button class="btn-secondary" on:click=move |_| set_show_more(!show_more())>
                        {move || if show_more() { "Show less" } else { "Show more" }}
                    </button>
                </div>
            </div>

            <div class="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6 empty:hidden">
                <VendorItems item_id />
                <ExchangeSources item_id />
                <LeveSources item_id />
            </div>

            <div
                id="crafting-recipes"
                class="panel p-4 sm:p-6"
                class:hidden=move || recipes.with(|recipes| recipes.is_empty())
            >
                <div class="flex flex-row items-center justify-between mb-3 flex-wrap gap-2">
                    <h2 class="text-xl font-bold text-brand-200 px-1">{t!(i18n, related_items_crafting_recipes_heading)}</h2>
                    <CraftOptionsToggleRow />
                </div>
                <ActiveListBanner />
                <div class="grid grid-cols-1 2xl:grid-cols-2 gap-4 max-w-6xl">
                    <For
                        each=Signal::derive(move || recipes().into_iter().take(5).collect::<Vec<_>>())
                        key=|recipe| recipe.key_id
                        children=move |recipe: &'static Recipe| {
                            view! { <Recipe recipe item_id=ItemId(item_id()) /> }
                        }
                    />
                </div>
            </div>
        </div>
    }
    .into_any()
}
