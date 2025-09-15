use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;

use crate::api::get_cheapest_listings;
use crate::api::get_recent_sales_for_world;
use crate::components::add_to_list::AddToList;
use crate::components::clipboard::Clipboard;
use crate::components::item_icon::ItemIcon;
use crate::components::loading::Loading;
use crate::components::meta::MetaDescription;
use crate::components::meta::MetaTitle;
use crate::components::modal::Modal;
use crate::components::number_input::ParseableInputBox;
use crate::components::query_button::QueryButton;
use crate::error::AppError;
use crate::global_state::home_world::use_home_world;
use crate::Ad;
use crate::Tooltip;
use chrono::TimeDelta;
use chrono::Utc;
use field_iterator::field_iter;
use field_iterator::FieldLabels;
use field_iterator::SortableVec;
use itertools::Itertools;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_icons::Icon;
use leptos_router::components::Outlet;
use leptos_router::components::A;
use leptos_router::hooks::*;

use leptos_router::params::ParamsMap;
use log::info;
use ultros_api_types::cheapest_listings::CheapestListingItem;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::recent_sales::SaleData;
use xiv_gen::Item;
use xiv_gen::{ItemId, SpecialShop};

#[derive(Copy, Clone, PartialEq, Debug)]
struct ItemAmount {
    item: &'static Item,
    amount: u32,
}

impl Hash for ItemAmount {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.item.key_id.hash(state);
        self.amount.hash(state);
    }
}

impl PartialOrd for ItemAmount {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.item.name.partial_cmp(&other.item.name) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self.amount.partial_cmp(&other.amount)
    }
}

impl Eq for ItemAmount {}

impl Ord for ItemAmount {
    fn cmp(&self, other: &Self) -> Ordering {
        self.item
            .name
            .cmp(&other.item.name)
            .then_with(|| self.amount.cmp(&other.amount))
    }
}

#[component]
fn ItemAmount(item_amount: Option<ItemAmount>) -> impl IntoView {
    item_amount
        .map(|item_amount| {
            view! {
                <div class="flex flex-row gap-1">
                    <A
                        attr:class="flex flex-row gap-1"
                        href=format!("/item/{}", item_amount.item.key_id.0)
                    >
                        <ItemIcon item_id=item_amount.item.key_id.0 icon_size=IconSize::Small />
                        <span>{item_amount.item.name.as_str()}</span>
                    </A>
                    <div>"x" {item_amount.amount}</div>
                    <AddToList item_id=item_amount.item.key_id.0 />
                    <Clipboard clipboard_text=item_amount.item.name.as_str() />
                </div>
            }
        })
        .into_any()
}

struct ShopItems {
    recv: Vec<ItemAmount>,
    cost: Vec<ItemAmount>,
}

fn from_lists(
    item: impl Iterator<Item = ItemId>,
    amount: impl Iterator<Item = u32>,
) -> impl Iterator<Item = Option<ItemAmount>> {
    let items = &xiv_gen_db::data().items;
    item.zip(amount).map(|(item_id, amount)| {
        let item = items.get(&item_id)?;
        Some(ItemAmount { item, amount })
    })
}

fn shop_items(special_shop: &SpecialShop) -> impl Iterator<Item = ShopItems> + '_ {
    let SpecialShop {
        item_receive_0,
        count_receive_0,
        item_receive_1,
        count_receive_1,
        item_cost_0,
        count_cost_0,
        item_cost_1,
        count_cost_1,
        item_cost_2,
        count_cost_2,
        ..
    } = special_shop;

    let recv_0 = from_lists(
        item_receive_0.iter().copied(),
        count_receive_0.iter().copied(),
    );
    let recv_1 = from_lists(
        item_receive_1.iter().copied(),
        count_receive_1.iter().copied(),
    );
    let cost_0 = from_lists(item_cost_0.iter().copied(), count_cost_0.iter().copied());
    let cost_1 = from_lists(item_cost_1.iter().copied(), count_cost_1.iter().copied());
    let cost_2 = from_lists(item_cost_2.iter().copied(), count_cost_2.iter().copied());
    recv_0
        .zip(recv_1)
        .zip(
            cost_0
                .zip(cost_1.zip(cost_2))
                .map(|(cost_0, (cost_1, cost_2))| (cost_0, cost_1, cost_2)),
        )
        .map(|((recv_0, recv_1), (cost_0, cost_1, cost_2))| ShopItems {
            recv: [recv_0, recv_1].into_iter().flatten().collect(),
            cost: [cost_0, cost_1, cost_2].into_iter().flatten().collect(),
        })
}

#[component]
fn FilterModal(filter_name: &'static str) -> impl IntoView {
    let (is_open, set_open) = signal(false);

    // highlight the filter icon when an active min/max is set for this column
    let query = use_query_map();
    let is_active = Signal::derive(move || {
        let q = query.get();
        let has_min = q
            .get(&format!("{filter_name}_min"))
            .and_then(|p| p.parse::<i32>().ok())
            .is_some();
        let has_max = q
            .get(&format!("{filter_name}_max"))
            .and_then(|p| p.parse::<i32>().ok())
            .is_some();
        has_min || has_max
    });

    view! {
        <div on:click=move |_| set_open(true)>
            <div class=move || {
                if is_active() {
                    "cursor-pointer inline-flex items-center justify-center w-8 h-8 rounded-md border border-[color:var(--brand-fg)] text-[color:var(--brand-fg)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]".to_string()
                } else {
                    "cursor-pointer inline-flex items-center justify-center w-8 h-8 rounded-md border border-[color:var(--color-outline)] text-[color:var(--color-text)] hover:text-[color:var(--brand-fg)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]".to_string()
                }
            }>
                <Icon icon=icondata::AiFilterFilled />
            </div>
            {move || {
                is_open()
                    .then(|| {
                        let (min, set_min) = query_signal::<i32>(format!("{filter_name}_min"));
                        let (max, set_max) = query_signal::<i32>(format!("{filter_name}_max"));
                        view! {
                            <Modal set_visible=set_open>
                                <h3 class="text-2xl font-bold text-[color:var(--brand-fg)]">"Edit filter"</h3>
                                <div class="text-sm text-[color:var(--color-text-muted)] mb-2">
                                    {filter_name.replace("_", " ")}
                                </div>
                                <div class="flex flex-col gap-3">
                                    <div class="flex items-center justify-between">
                                        <span class="text-[color:var(--color-text)]">"Max"</span>
                                        <ParseableInputBox
                                            input=Signal::derive(move || { max() })
                                            set_value=SignalSetter::map(move |value| set_max(value))
                                        />
                                    </div>
                                    <div class="flex items-center justify-between">
                                        <span class="text-[color:var(--color-text)]">"Min"</span>
                                        <ParseableInputBox
                                            input=Signal::derive(move || { min() })
                                            set_value=SignalSetter::map(move |value| set_min(value))
                                        />
                                    </div>
                                </div>
                            </Modal>
                        }
                    })
            }}

        </div>
    }
    .into_any()
}

fn is_in_range(value: i32, field_label: &str, query_map: &ParamsMap) -> bool {
    let max = query_map
        .get(&format!("{field_label}_max"))
        .and_then(|p| p.parse::<i32>().ok());
    let min = query_map
        .get(&format!("{field_label}_min"))
        .and_then(|p| p.parse::<i32>().ok());

    match (min, max) {
        (None, None) => true,
        (None, Some(max)) => value < max,
        (Some(min), None) => value > min,
        (Some(min), Some(max)) => (min..max).contains(&value),
    }
}

#[component]
pub fn ExchangeItem() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let (currency_quantity, set_currency_quantity) = query_signal::<i32>("currency_amount");
    let sales = Resource::new(home_world, move |world| async move {
        let world = world.ok_or(AppError::NoHomeWorld)?;
        get_recent_sales_for_world(&world.name).await
    });

    let world_cheapest_listings = Resource::new(home_world, move |world| async move {
        let world = world.ok_or(AppError::NoHomeWorld)?;
        get_cheapest_listings(&world.name).await
    });
    let data = xiv_gen_db::data();
    let item_id = move || {
        ItemId(
            params
                .get()
                .get("id")
                .and_then(|p| p.parse::<i32>().ok())
                .unwrap_or_default(),
        )
    };
    let item = move || data.items.get(&item_id());
    let currency_quantity = Memo::new(move |_| {
        if let Some(quantity) = currency_quantity() {
            return quantity;
        }
        let Some(item) = item() else {
            return 0;
        };
        item.stack_size as i32
    });
    let shop_data = move || {
        let item = item_id();
        data.special_shops
            .values()
            .flat_map(move |shop| {
                shop_items(shop)
                    .filter(move |items| {
                        // make sure the item is valid on the marketboard before we lookup prices for it
                        items.cost.iter().any(|i| i.item.key_id.0 == item.0)
                            && items
                                .recv
                                .iter()
                                .any(|i| i.item.item_search_category.0 != 0)
                    })
                    .map(move |items| (items, shop))
            })
            .collect::<Vec<_>>()
    };
    let with_prices = move || {
        let current_quantity = currency_quantity.get();
        let sales: HashMap<(bool, i32), SaleData> = sales
            .get()?
            .ok()?
            .sales
            .into_iter()
            .map(|sale| ((sale.hq, sale.item_id), sale))
            .collect();
        let world_listings: HashMap<(bool, i32), CheapestListingItem> = world_cheapest_listings
            .get()?
            .ok()?
            .cheapest_listings
            .into_iter()
            .map(|cheapest| ((cheapest.hq, cheapest.item_id), cheapest))
            .collect();
        let shops_with_item = shop_data();
        let now = Utc::now().naive_utc();
        let rows = shops_with_item
            .iter()
            .filter_map(|(item, shop)| {
                // going to just assume first item matters?
                let cost = item.cost[0];
                let recv = item
                    .recv
                    .iter()
                    .find(|i| i.item.item_search_category.0 >= 0)?;
                let item_key = (false, recv.item.key_id.0);
                let sales = &sales.get(&item_key)?.sales;
                let sale = sales.first()?.price_per_unit;
                let current_listing_price = world_listings
                    .get(&item_key)
                    .map(|listing| listing.cheapest_price - 1);
                let guessed_price_per_item = current_listing_price.unwrap_or(sale).min(sale);
                let input_amount = current_quantity;
                let number_received = recv.amount as i32 * (input_amount / cost.amount as i32);
                let sales_len = sales.len();
                let hours_between_sales = sales
                    .last()
                    .map(|last| {
                        let time_between: TimeDelta = (now - last.sale_date) / sales_len as i32;
                        time_between.num_hours() as i16
                    })
                    .unwrap_or(i16::MAX);
                Some((
                    (
                        cost,
                        *recv,
                        guessed_price_per_item,
                        number_received,
                        guessed_price_per_item as i64 * number_received as i64,
                        hours_between_sales,
                    ),
                    shop.name.to_string(),
                ))
            })
            .into_group_map()
            .into_iter()
            .map(
                |(
                    (
                        cost,
                        recv,
                        guessed_price_per_item,
                        number_received,
                        total_profit,
                        hours_between_sales,
                    ),
                    shop_names,
                )| {
                    CurrencyTrade {
                        shop_names: ShopNames {
                            shops: shop_names.into_iter().unique().collect(),
                        },
                        cost_item: Some(cost),
                        receive_item: Some(recv),
                        price_per_item: guessed_price_per_item,
                        number_received,
                        total_profit,
                        hours_between_sales,
                    }
                },
            )
            .collect::<Vec<_>>();
        Some(rows)
    };

    let (sorted_by, _set_sorted_by) = query_signal::<String>("sorted-by");
    let item_name = move || item().map(|i| i.name.as_str()).unwrap_or_default();
    view! {
        <div class="container mx-auto p-4">
            <MetaTitle title=move || format!("Currency Exchange - {}", item_name()) />
            <MetaDescription text=move || {
                format!(
                    "All items that can be exchanged for {} with how much you stand to earn",
                    item_name(),
                )
            } />
            <div class="panel p-6 rounded-xl mb-6">
                <h2 class="text-2xl font-bold mb-4 text-[color:var(--brand-fg)]">
                    {move || item().map(|i| i.name.as_str())} " - Currency Exchange"
                </h2>
                <div class="flex items-center gap-4 mb-4">
                    <label class="text-[color:var(--color-text-muted)]">How many of this currency do you have?</label>
                    <input
                        class="input w-24"
                        prop:value=currency_quantity
                        on:input=move |e| {
                            let event = event_target_value(&e);
                            if let Ok(p) = event.parse() {
                                set_currency_quantity.set(Some(p));
                            }
                        }
                    />
                </div>
                <div class="flex items-center flex-wrap gap-3 text-sm">
                    <span class="text-[color:var(--color-text-muted)] mr-1">"Quick filters:"</span>

                    <div class="inline-flex items-center gap-2">
                        <span class="text-[color:var(--color-text)]">"Price/item"</span>
                        {move || {
                            let (min, set_min) = query_signal::<i32>("price_per_item_min");
                            let (max, set_max) = query_signal::<i32>("price_per_item_max");
                            view! {
                                <ParseableInputBox
                                    input=Signal::derive(move || min())
                                    set_value=SignalSetter::map(move |v| set_min(v))
                                />
                                <span class="text-[color:var(--color-text-muted)]">"–"</span>
                                <ParseableInputBox
                                    input=Signal::derive(move || max())
                                    set_value=SignalSetter::map(move |v| set_max(v))
                                />
                                <FilterModal filter_name="price_per_item" />
                            }
                        }}
                    </div>

                    <div class="inline-flex items-center gap-2">
                        <span class="text-[color:var(--color-text)]">"Qty recv"</span>
                        {move || {
                            let (min, set_min) = query_signal::<i32>("number_received_min");
                            let (max, set_max) = query_signal::<i32>("number_received_max");
                            view! {
                                <ParseableInputBox
                                    input=Signal::derive(move || min())
                                    set_value=SignalSetter::map(move |v| set_min(v))
                                />
                                <span class="text-[color:var(--color-text-muted)]">"–"</span>
                                <ParseableInputBox
                                    input=Signal::derive(move || max())
                                    set_value=SignalSetter::map(move |v| set_max(v))
                                />
                                <FilterModal filter_name="number_received" />
                            }
                        }}
                    </div>

                    <div class="inline-flex items-center gap-2">
                        <span class="text-[color:var(--color-text)]">"Profit"</span>
                        {move || {
                            let (min, set_min) = query_signal::<i32>("total_profit_min");
                            let (max, set_max) = query_signal::<i32>("total_profit_max");
                            view! {
                                <ParseableInputBox
                                    input=Signal::derive(move || min())
                                    set_value=SignalSetter::map(move |v| set_min(v))
                                />
                                <span class="text-[color:var(--color-text-muted)]">"–"</span>
                                <ParseableInputBox
                                    input=Signal::derive(move || max())
                                    set_value=SignalSetter::map(move |v| set_max(v))
                                />
                                <FilterModal filter_name="total_profit" />
                            }
                        }}
                    </div>

                    <div class="inline-flex items-center gap-2">
                        <span class="text-[color:var(--color-text)]">"Hours/sale"</span>
                        {move || {
                            let (min, set_min) = query_signal::<i32>("hours_between_sales_min");
                            let (max, set_max) = query_signal::<i32>("hours_between_sales_max");
                            view! {
                                <ParseableInputBox
                                    input=Signal::derive(move || min())
                                    set_value=SignalSetter::map(move |v| set_min(v))
                                />
                                <span class="text-[color:var(--color-text-muted)]">"–"</span>
                                <ParseableInputBox
                                    input=Signal::derive(move || max())
                                    set_value=SignalSetter::map(move |v| set_max(v))
                                />
                                <FilterModal filter_name="hours_between_sales" />
                            }
                        }}
                    </div>
                </div>

                <div class="flex flex-wrap gap-2 mt-2">
                    {move || {
                        let q = query();
                        let mut chips: Vec<AnyView> = Vec::new();

                        let get_i = |k: &str| q.get(k).and_then(|v| v.parse::<i32>().ok());

                        let mut push_chip = |label: &str, key: &'static str, val: Option<i32>| {
                            if let Some(v) = val {
                                let key_owned = key.to_string();
                                chips.push(view! {
                                    <span class="inline-flex items-center gap-2 rounded-full border px-2 py-0.5 text-xs
                                                  text-[color:var(--color-text)]
                                                  bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]
                                                  border-[color:var(--color-outline)]">
                                        {format!("{label}: {v}")}
                                        <QueryButton
                                            key=key_owned.clone()
                                            value=""
                                            class="text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                            active_classes=""
                                        >
                                            <Icon icon=icondata::MdiClose />
                                        </QueryButton>
                                    </span>
                                }.into_any());
                            }
                        };

                        push_chip("Price min", "price_per_item_min", get_i("price_per_item_min"));
                        push_chip("Price max", "price_per_item_max", get_i("price_per_item_max"));
                        push_chip("Qty min", "number_received_min", get_i("number_received_min"));
                        push_chip("Qty max", "number_received_max", get_i("number_received_max"));
                        push_chip("Profit min", "total_profit_min", get_i("total_profit_min"));
                        push_chip("Profit max", "total_profit_max", get_i("total_profit_max"));
                        push_chip("Hours min", "hours_between_sales_min", get_i("hours_between_sales_min"));
                        push_chip("Hours max", "hours_between_sales_max", get_i("hours_between_sales_max"));

                        if !chips.is_empty() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-2 py-0.5 text-xs
                                              text-[color:var(--color-text)]
                                              bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]
                                              border-[color:var(--color-outline)]">
                                    <QueryButton
                                        key="sorted-by"
                                        value=Signal::derive(move || sorted_by().unwrap_or_else(|| "total_profit".into()))
                                        class="inline-flex items-center gap-1 text-[color:var(--color-text)] hover:text-[color:var(--brand-fg)]"
                                        active_classes=""
                                        remove_queries=&[
                                            "price_per_item_min",
                                            "price_per_item_max",
                                            "number_received_min",
                                            "number_received_max",
                                            "total_profit_min",
                                            "total_profit_max",
                                            "hours_between_sales_min",
                                            "hours_between_sales_max",
                                        ]
                                    >
                                        <span class="inline-flex items-center gap-1">
                                            <Icon icon=icondata::MdiClose />
                                            "Clear all"
                                        </span>
                                    </QueryButton>
                                </span>
                            }.into_any());
                        }
                        view! { <>{chips}</> }
                    }}
                </div>
            </div>
            <div class="overflow-x-auto">
                {move || {
                    if home_world().is_none() {
                        let left = view! {
                            <div class="bg-red-900/50 p-4 rounded-lg text-white">
                                "Home world is not set, go to the "
                                <A
                                    href="/settings"
                                    attr:class="underline"
                                >
                                    "settings"
                                </A> " page and set your home world to see prices on this page"
                            </div>
                        };
                        Either::Left(left)
                    } else {
                        let right = view! {
                            <Suspense fallback=Loading>
                                {move || {
                                    let sort_label = sorted_by();
                                    with_prices()
                                        .map(|p: Vec<CurrencyTrade>| {
                                            let trades = p.len();
                                            let sorted_and_filtered_rows = move || {
                                                let query = query();
                                                let mut p = p
                                                    .clone()
                                                    .into_iter()
                                                    .filter(|currency| {
                                                        let query = &query;
                                                        is_in_range(
                                                            currency.price_per_item as i32,
                                                            "price_per_item",
                                                            query,
                                                        )
                                                            && is_in_range(
                                                                currency.number_received as i32,
                                                                "number_received",
                                                                query,
                                                            )
                                                            && is_in_range(
                                                                currency.total_profit as i32,
                                                                "total_profit",
                                                                query,
                                                            )
                                                            && is_in_range(
                                                                currency.hours_between_sales as i32,
                                                                "hours_between_sales",
                                                                query,
                                                            )
                                                    })
                                                    .collect::<Vec<_>>();
                                                CurrencyTrade::sort_vec_by_label(
                                                    &mut p,
                                                    sort_label.as_deref().unwrap_or("total_profit"),
                                                    None,
                                                );
                                                p.into_iter()
                                                    .map(|p| {
                                                        view! {
                                                            <tr class="transition-colors">
                                                                <td class="px-6 py-4">
                                                                    <ShopNames shop_names=p.shop_names />
                                                                </td>
                                                                <td class="px-6 py-4">
                                                                    <ItemAmount item_amount=p.cost_item />
                                                                </td>
                                                                <td class="px-6 py-4">
                                                                    <ItemAmount item_amount=p.receive_item />
                                                                </td>
                                                                <td class="px-6 py-4">{p.price_per_item}</td>
                                                                <td class="px-6 py-4">{p.number_received}</td>
                                                                <td class="px-6 py-4">{p.total_profit}</td>
                                                                <td class="px-6 py-4">{p.hours_between_sales}</td>
                                                            </tr>
                                                        }
                                                    })
                                                    .collect_view()
                                            };
                                            let count = sorted_and_filtered_rows().len();
                                            let s = sales.get();
                                            let sales = s
                                                .as_ref()
                                                .map(|sales| sales.as_ref().map(|sales| sales.sales.len()));
                                            info!("{sales:?} items: {count} p: {trades}");
                                            let labels = CurrencyTrade::field_labels();
                                            view! {
                                                <table class="w-full text-sm text-left">
                                                    <thead class="text-xs uppercase">
                                                        <tr>
                                                            {labels
                                                                .into_iter()
                                                                .enumerate()
                                                                .map(|(i, l)| {
                                                                    view! {
                                                                        <th class="px-6 py-3">
                                                                            <div class="flex flex-row items-center gap-2">
                                                                                <QueryButton
                                                                                    key="sorted-by"
                                                                                    value=*l
                                                                                    class="underline decoration-transparent hover:text-[color:var(--brand-fg)] transition-colors"
                                                                                    active_classes="text-[color:var(--brand-fg)] underline underline-offset-4 decoration-2"
                                                                                    default="total_profit" == *l
                                                                                >
                                                                                    {l.replace("_", " ")}
                                                                                </QueryButton>
                                                                                {(i > 2)
                                                                                    .then(|| {
                                                                                        view! {
                                                                                            <Tooltip tooltip_text=format!(
                                                                                                "Filter {}",
                                                                                                l.replace("_", " "),
                                                                                            )>
                                                                                                <FilterModal filter_name=l />
                                                                                            </Tooltip>
                                                                                        }
                                                                                    })}
                                                                            </div>
                                                                        </th>
                                                                    }
                                                                })
                                                                .collect_view()}
                                                        </tr>
                                                    </thead>
                                                    <tbody class="divide-y divide-[color:var(--color-outline)]">
                                                        {sorted_and_filtered_rows}
                                                    </tbody>
                                                </table>
                                            }
                                        })
                                }}
                                {move || {
                                    sales
                                        .with(|sales| {
                                            if let Some(Err(e)) = sales {
                                                Either::Left(
                                                    view! {
                                                        <div class="bg-red-900/50 p-4 rounded-lg text-white mt-4">
                                                            "Error loading, try again in 30 seconds!"<br />
                                                            {e.to_string()}
                                                        </div>
                                                    },
                                                )
                                            } else {
                                                Either::Right(())
                                            }
                                        })
                                }}
                            </Suspense>
                        };
                        Either::Right(right)
                    }
                }}
            </div>
        </div>
    }.into_any()
}

#[field_iter(field_prefix = "item_cost_", count = 3)]
fn item_cost_iter(shop: &SpecialShop) -> impl Iterator<Item = ItemId> + '_ {}

// #[derive(TableRow, Clone, Default, Debug)]
// #[table(
//     impl_vec_data_provider,
//     sortable,
//     classes_provider = "TailwindClassesPreset"
// )]
#[derive(SortableVec, FieldLabels, Clone)]
pub struct CurrencyTrade {
    shop_names: ShopNames,
    cost_item: Option<ItemAmount>,
    receive_item: Option<ItemAmount>,
    price_per_item: i32,
    number_received: i32,
    total_profit: i64,
    hours_between_sales: i16,
}

#[derive(PartialEq, Eq, Clone, PartialOrd, Ord)]
struct ShopNames {
    shops: Vec<String>,
}

#[component]
fn ShopNames(shop_names: ShopNames) -> impl IntoView {
    view! {
        <div class="flex flex-col">
            {shop_names
                .shops
                .into_iter()
                .map(|shop| view! { <div>{shop}</div> })
                .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn CurrencySelection() -> impl IntoView {
    let data = xiv_gen_db::data();
    let ui_categories = &data.item_ui_categorys;
    let disallowed_items = &["Gil", "MGP"];
    let allowed_item_ui_categories = ["Currency", "Miscellany", "Other"]
        .into_iter()
        .map(|category| {
            ui_categories
                .iter()
                .find(|f| f.1.name == category)
                .map(|(id, _)| *id)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let currencies = data
        .special_shops
        .iter()
        .flat_map(|(_shops, special_shop)| {
            shop_items(special_shop)
                .filter(|items| {
                    items
                        .recv
                        .iter()
                        .any(|i| i.item.item_search_category.0 != 0)
                })
                .flat_map(|f| f.cost.into_iter().map(|i| i.item.key_id))
        })
        .filter(|f| {
            let Some(item) = data.items.get(f) else {
                return false;
            };
            allowed_item_ui_categories.contains(&item.item_ui_category)
        })
        .unique_by(|i| i.0)
        .collect::<Vec<_>>();
    let items = &data.items;
    let currencies = currencies
        .into_iter()
        .sorted_by_key(|item| item.0)
        .filter_map(|c| {
            let item = items.get(&c)?;
            if disallowed_items.contains(&item.name.as_str()) {
                return None;
            }
            let ui_category = item.item_ui_category;
            let category = ui_categories.get(&ui_category)?;
            Some((item.key_id.0, item.name.as_str(), category.name.as_str()))
        })
        .collect::<Vec<_>>();

    let body_currencies = currencies.clone();
    let (search_text, set_search_text) = signal(String::new());
    let filtered_currencies = Memo::new(move |_| {
        let search = search_text().to_lowercase();
        body_currencies
            .iter()
            .filter(|(_, name, category)| {
                name.to_lowercase().contains(&search) || category.to_lowercase().contains(&search)
            })
            .cloned()
            .collect::<Vec<_>>()
    });

    view! {
        <div class="container mx-auto space-y-6">
            // Description Card
            <div class="panel p-6 rounded-xl">
                <p class="text-[color:var(--color-text-muted)] leading-relaxed">
                    "Discover lucrative opportunities in Final Fantasy 14 with our Currency Exchange tool.
                        Easily locate items purchasable with in-game currencies, such as Allied Seals or Wolf Marks, that can be resold for significant profits on the marketboard.
                        Whether you're a seasoned trader or just starting out, maximize your earnings by identifying high-value items and optimizing your currency investments."
                </p>
            </div>

            <MetaTitle title="Currency Exchange - Ultros" />
            <MetaDescription text="Find valuable items bought with in-game currency, sell for gil. Maximize earnings effortlessly. " />

            // Search Section
            <div class="panel p-6 rounded-xl">
                <div class="flex items-center gap-4">
                    <div class="relative flex-1 max-w-xl">
                        <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                            <Icon
                                icon=icondata::BiSearchAlt2Regular
                                attr:class="w-5 h-5 text-[color:var(--color-text-muted)]"
                            />
                        </div>
                        <input
                            type="text"
                            placeholder="Search currencies..."
                            class="input w-full pl-10"
                            on:input=move |ev| set_search_text(event_target_value(&ev))
                        />
                    </div>
                </div>
            </div>

            // Currency List
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                {move || {
                    filtered_currencies()
                        .into_iter()
                        .map(|(item_id, item_name, category_name)| {
                            view! {
                                <A
                                    href=item_id.to_string()
                                    attr:class="card p-4 rounded-lg transition-colors duration-200 group"
                                >
                                    <div class="flex flex-col gap-2">
                                        <span class="text-lg font-medium text-[color:var(--color-text)]
                                        group-hover:text-[color:var(--brand-fg)] transition-colors">
                                            {item_name}
                                        </span>
                                        <span class="text-sm text-[color:var(--color-text-muted)] italic
                                        group-hover:text-[color:var(--brand-fg)] transition-colors">
                                            {category_name}
                                        </span>
                                    </div>
                                </A>
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            </div>

            // Empty State
            {move || {
                if filtered_currencies().is_empty() {
                    Either::Left(
                        view! {
                            <div class="text-center p-8 text-[color:var(--color-text-muted)]">
                                "No currencies found matching your search."
                            </div>
                        },
                    )
                } else {
                    Either::Right(view! { <div></div> })
                }
            }}
        </div>
    }.into_any()
}

#[component]
pub fn CurrencyExchange() -> impl IntoView {
    view! {
        <Ad class="w-full h-[100px]" />
        <div class="main-content">
            <A href="/currency-exchange">
                <h3 class="text-2xl font-bold text-[color:var(--brand-fg)] hover:opacity-90 transition-all ease-in-out duration-500">
                    "Currency Exchange"
                </h3>
            </A>
            <Outlet />
        </div>
    }.into_any()
}
