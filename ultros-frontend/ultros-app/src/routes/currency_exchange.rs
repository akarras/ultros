use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;

use crate::Tooltip;
use crate::api::get_cheapest_listings;
use crate::api::get_recent_sales_for_world;
use crate::components::ad::Ad;
use crate::components::add_to_list::AddToList;
use crate::components::clipboard::Clipboard;
use crate::components::icon::Icon;
use crate::components::item_icon::ItemIcon;
use crate::components::loading::Loading;
use crate::components::meta::MetaDescription;
use crate::components::meta::MetaTitle;
use crate::components::modal::Modal;
use crate::components::number_input::ParseableInputBox;
use crate::components::query_button::QueryButton;
use crate::error::AppError;
use crate::global_state::home_world::use_home_world;
use crate::global_state::xiv_data::{resolve_item_id, tracked_data};
use crate::i18n::*;
use crate::routes::not_found::NotFound;
use chrono::TimeDelta;
use chrono::Utc;
use field_iterator::FieldLabels;
use field_iterator::SortableVec;
use itertools::Itertools;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::components::A;
use leptos_router::components::Outlet;
use leptos_router::hooks::*;

use leptos_router::params::ParamsMap;
use log::info;
use ultros_api_types::cheapest_listings::CheapestListingItem;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::recent_sales::SaleData;
use xiv_gen::Item;
use xiv_gen::{ItemId, ItemUiCategoryId, SpecialShop};

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
        Some(self.cmp(other))
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
fn ItemAmount(#[prop(into)] item_amount: Option<ItemAmount>) -> impl IntoView {
    let i18n = use_i18n();
    item_amount
        .map(|item_amount| {
            view! {
                <div class="flex flex-row gap-1">
                    <A
                        attr:class="flex flex-row gap-1 min-w-0"
                        href=format!("/item/{}", item_amount.item.key_id.0)
                    >
                        <ItemIcon item_id=item_amount.item.key_id.0 icon_size=IconSize::Small />
                        <span class="truncate" title=item_amount.item.name.as_str()>{item_amount.item.name.as_str()}</span>
                    </A>
                    <div>{t!(i18n, currency_exchange_quantity_x)} {item_amount.amount}</div>
                    <span on:click=move |ev| { ev.stop_propagation(); ev.prevent_default(); }>
                        <AddToList item_id=item_amount.item.key_id.0 />
                    </span>
                    <span on:click=move |ev| { ev.stop_propagation(); ev.prevent_default(); }>
                        <Clipboard clipboard_text=item_amount.item.name.as_str() />
                    </span>
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
    item: impl Iterator<Item = u16>,
    amount: impl Iterator<Item = u32>,
) -> impl Iterator<Item = Option<ItemAmount>> {
    let items = &tracked_data().items;
    item.zip(amount).map(|(item_id, amount)| {
        if item_id == 0 || amount == 0 {
            return None;
        }

        let item_id = ItemId(item_id as i32);
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
    let i18n = use_i18n();
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
                                <h3 class="text-2xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, currency_exchange_edit_filter)}</h3>
                                <div class="text-sm text-[color:var(--color-text-muted)] mb-2">
                                    {filter_name.replace("_", " ")}
                                </div>
                                <div class="flex flex-col gap-3">
                                    <div class="flex items-center justify-between">
                                        <span class="text-[color:var(--color-text)]">{t!(i18n, currency_exchange_max)}</span>
                                        <ParseableInputBox
                                            input=Signal::derive(max)
                                            set_value=SignalSetter::map(set_max)
                                            aria_label=t_string!(i18n, currency_exchange_max_field_aria, name = filter_name.replace("_", " ")).to_string()
                                            placeholder=t_string!(i18n, currency_exchange_max).to_string()
                                        />
                                    </div>
                                    <div class="flex items-center justify-between">
                                        <span class="text-[color:var(--color-text)]">{t!(i18n, currency_exchange_min)}</span>
                                        <ParseableInputBox
                                            input=Signal::derive(min)
                                            set_value=SignalSetter::map(set_min)
                                            aria_label=t_string!(i18n, currency_exchange_min_field_aria, name = filter_name.replace("_", " ")).to_string()
                                            placeholder=t_string!(i18n, currency_exchange_min).to_string()
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

/// Every min/max query key the filter bar can set; used both to detect
/// "any filter active" and to clear them all at once.
const FILTER_QUERY_KEYS: &[&str] = &[
    "price_per_item_min",
    "price_per_item_max",
    "number_received_min",
    "number_received_max",
    "total_profit_min",
    "total_profit_max",
    "hours_between_sales_min",
    "hours_between_sales_max",
];

/// Per-column responsive visibility for the results table, keyed by the
/// column's index in `CurrencyTrade::field_labels()`.
///
/// All seven columns at once are ~1200px wide, so on a phone the table became
/// a horizontal scroll strip where the profit — the number the page exists to
/// show — sat off-screen. Drop to item/qty/profit on the smallest screens and
/// add the rest back as width allows, the same tiering the item explorer's
/// result rows use. Header and body cells must be given the same class or the
/// columns shear apart.
fn column_visibility(index: usize) -> &'static str {
    match index {
        // Shops and Cost: the least useful pair on a phone. Cost in particular
        // barely varies — every row on this page is priced in the same currency.
        0 | 1 => "hidden lg:table-cell",
        // Price per item and hours between sales: useful, not essential.
        3 | 6 => "hidden sm:table-cell",
        // Item, quantity received, profit.
        _ => "",
    }
}

/// Compact min/max input pair for one numeric column, bound to the
/// `{filter_name}_min` / `{filter_name}_max` query params.
#[component]
fn FilterRange(#[prop(into)] label: String, filter_name: &'static str) -> impl IntoView {
    let i18n = use_i18n();
    let (min, set_min) = query_signal::<i32>(format!("{filter_name}_min"));
    let (max, set_max) = query_signal::<i32>(format!("{filter_name}_max"));
    let aria_name = filter_name.replace("_", " ");
    view! {
        <div class="flex flex-col gap-1">
            <span class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                {label}
            </span>
            <div class="flex flex-row gap-2">
                <ParseableInputBox
                    input=Signal::derive(min)
                    set_value=SignalSetter::map(set_min)
                    aria_label=t_string!(i18n, currency_exchange_min_field_aria, name = aria_name.clone())
                        .to_string()
                    placeholder=t_string!(i18n, currency_exchange_min).to_string()
                />
                <ParseableInputBox
                    input=Signal::derive(max)
                    set_value=SignalSetter::map(set_max)
                    aria_label=t_string!(i18n, currency_exchange_max_field_aria, name = aria_name)
                        .to_string()
                    placeholder=t_string!(i18n, currency_exchange_max).to_string()
                />
            </div>
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

/// Gates the exchange-item page on the `:id` route param naming a real item —
/// same "fake item 0 page at 200" bug as `ItemView`, see
/// `crate::routes::item_view::ItemView`.
#[component]
pub fn ExchangeItem() -> impl IntoView {
    let params = use_params_map();
    let item_id_valid =
        Memo::new(move |_| params.with(|p| resolve_item_id(p.get_str("id"))).is_some());

    view! {
        <Show when=move || item_id_valid.get() fallback=|| view! { <NotFound /> }.into_any()>
            <ExchangeItemContent />
        </Show>
    }
}

#[component]
fn ExchangeItemContent() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let (currency_quantity, set_currency_quantity) = query_signal::<i32>("currency_amount");
    let sales = ArcResource::new(home_world, move |world| async move {
        let world = world.ok_or(AppError::NoHomeWorld)?;
        get_recent_sales_for_world(&world.name).await
    });

    let world_cheapest_listings = ArcResource::new(home_world, move |world| async move {
        let world = world.ok_or(AppError::NoHomeWorld)?;
        get_cheapest_listings(&world.name).await
    });
    let data = tracked_data();
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
                    .filter_map(move |mut items| {
                        // make sure the item is valid on the marketboard before we lookup prices for it
                        let has_marketable_item =
                            items.recv.iter().any(|i| i.item.item_search_category != 0);
                        items.cost.retain(|i| i.item.key_id.0 == item.0);

                        (!items.cost.is_empty() && has_marketable_item).then_some(items)
                    })
                    .map(move |items| (items, shop))
            })
            .collect::<Vec<_>>()
    };

    let (sorted_by, _set_sorted_by) = query_signal::<String>("sorted-by");
    let item_name = move || item().map(|i| i.name.as_str()).unwrap_or_default();

    // The filter inputs live behind a toggle: the chip row below already says
    // which filters are set, so the expanded controls were pure duplication
    // occupying the whole fold. Start expanded only when a deep link arrives
    // with filters already applied, so those inputs are editable without a
    // click.
    let active_filter_count = Signal::derive(move || {
        let q = query();
        FILTER_QUERY_KEYS
            .iter()
            .filter(|key| q.get(key).and_then(|v| v.parse::<i32>().ok()).is_some())
            .count()
    });
    let (filters_open, set_filters_open) = signal(active_filter_count.get_untracked() > 0);

    // Define the computation logic as a separate closure that takes data as arguments.
    // This avoids capturing the ArcResources directly, preventing move/FnOnce issues.
    let compute_prices =
        move |sales: Option<&ultros_api_types::recent_sales::RecentSales>,
              listings: Option<&ultros_api_types::cheapest_listings::CheapestListings>,
              quantity: i32| {
            let sales: HashMap<(bool, i32), SaleData> = sales?
                .sales
                .iter()
                .map(|sale| ((sale.hq, sale.item_id), sale.clone()))
                .collect();
            let world_listings: HashMap<(bool, i32), CheapestListingItem> = listings?
                .cheapest_listings
                .iter()
                .map(|cheapest| ((cheapest.hq, cheapest.item_id), cheapest.clone()))
                .collect();
            let shops_with_item = shop_data();
            let now = Utc::now().naive_utc();
            let rows = shops_with_item
                .iter()
                .filter_map(|(item, shop)| {
                    let cost = item.cost[0];
                    let recv = item.recv.iter().find(|i| i.item.item_search_category > 0)?;
                    let item_key = (false, recv.item.key_id.0);
                    let sales = &sales.get(&item_key)?.sales;
                    let recent = sales.first()?;
                    let most_recent = recent.sale_date;
                    let stale_threshold = now - TimeDelta::days(60);
                    if most_recent < stale_threshold {
                        return None;
                    }
                    let sale = recent.price_per_unit;
                    let current_listing_price = world_listings
                        .get(&item_key)
                        .map(|listing| listing.cheapest_price - 1);
                    let guessed_price_per_item = current_listing_price.unwrap_or(sale).min(sale);
                    let input_amount = quantity;
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

    // Create derived signals to access resources, avoiding ownership issues in view closures.
    let sales_2 = sales.clone();
    let s_getter_2 = Signal::derive(move || sales_2.get());

    let listings_2 = world_cheapest_listings.clone();
    let l_getter_2 = Signal::derive(move || listings_2.get());

    view! {
        <div class="container mx-auto p-4">
            <MetaTitle title=move || t_string!(i18n, currency_exchange_meta_title).replace("%item%", item_name()) />
            <MetaDescription text=move || {
                t_string!(i18n, currency_exchange_meta_desc).replace("%item%", item_name())
            } />
            <div class="panel p-4 rounded-xl mb-4">
                <div class="flex flex-wrap items-center justify-between gap-3">
                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                        {move || item().map(|i| i.name.as_str())} " - " {t!(i18n, currency_exchange_title)}
                    </h2>
                    <div class="flex flex-wrap items-center gap-3">
                        <label
                            for="currency-quantity"
                            class="text-sm text-[color:var(--color-text-muted)]"
                        >
                            {t!(i18n, currency_exchange_how_many)}
                        </label>
                        <input
                            id="currency-quantity"
                            class="input w-24"
                            prop:value=currency_quantity
                            on:input=move |e| {
                                let event = event_target_value(&e);
                                if let Ok(p) = event.parse() {
                                    set_currency_quantity.set(Some(p));
                                }
                            }
                        />
                        <button
                            type="button"
                            class="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium
                            border border-[color:var(--color-outline)] text-[color:var(--color-text-muted)]
                            hover:text-[color:var(--brand-fg)] hover:bg-white/5 transition-colors"
                            aria-expanded=move || filters_open().to_string()
                            aria-controls="currency-filter-panel"
                            on:click=move |_| set_filters_open.update(|open| *open = !*open)
                        >
                            <Icon icon=icondata::AiFilterFilled />
                            {t!(i18n, currency_exchange_filters)}
                            {move || {
                                let count = active_filter_count();
                                (count > 0)
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center justify-center min-w-5 h-5 px-1 rounded-full text-xs
                                            bg-[color:color-mix(in_srgb,var(--brand-ring)_25%,transparent)]
                                            text-[color:var(--brand-fg)]">
                                                {count}
                                            </span>
                                        }
                                    })
                            }}
                        </button>
                    </div>
                </div>

                <Show when=move || filters_open() fallback=|| ()>
                    <div
                        id="currency-filter-panel"
                        class="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-3 mt-3 pt-3
                        border-t border-[color:var(--color-outline)]"
                    >
                        <FilterRange
                            label=t_string!(i18n, currency_exchange_price_per_item_title).to_string()
                            filter_name="price_per_item"
                        />
                        <FilterRange
                            label=t_string!(i18n, currency_exchange_qty_received_title).to_string()
                            filter_name="number_received"
                        />
                        <FilterRange
                            label=t_string!(i18n, currency_exchange_profit_title).to_string()
                            filter_name="total_profit"
                        />
                        <FilterRange
                            label=t_string!(i18n, currency_exchange_sales_velocity_title).to_string()
                            filter_name="hours_between_sales"
                        />
                    </div>
                </Show>

                <div class="flex flex-wrap gap-2 mt-3">
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
                                                  bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]
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

                        push_chip(t_string!(i18n, currency_exchange_chip_price_min), "price_per_item_min", get_i("price_per_item_min"));
                        push_chip(t_string!(i18n, currency_exchange_chip_price_max), "price_per_item_max", get_i("price_per_item_max"));
                        push_chip(t_string!(i18n, currency_exchange_chip_qty_min), "number_received_min", get_i("number_received_min"));
                        push_chip(t_string!(i18n, currency_exchange_chip_qty_max), "number_received_max", get_i("number_received_max"));
                        push_chip(t_string!(i18n, currency_exchange_chip_profit_min), "total_profit_min", get_i("total_profit_min"));
                        push_chip(t_string!(i18n, currency_exchange_chip_profit_max), "total_profit_max", get_i("total_profit_max"));
                        push_chip(t_string!(i18n, currency_exchange_chip_hours_min), "hours_between_sales_min", get_i("hours_between_sales_min"));
                        push_chip(t_string!(i18n, currency_exchange_chip_hours_max), "hours_between_sales_max", get_i("hours_between_sales_max"));

                        if !chips.is_empty() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-2 py-0.5 text-xs
                                              text-[color:var(--color-text)]
                                              bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]
                                              border-[color:var(--color-outline)]">
                                    <QueryButton
                                        key="sorted-by"
                                        value=Signal::derive(move || sorted_by().unwrap_or_else(|| "total_profit".into()))
                                        class="inline-flex items-center gap-1 text-[color:var(--color-text)] hover:text-[color:var(--brand-fg)]"
                                        active_classes=""
                                        remove_queries=FILTER_QUERY_KEYS
                                    >
                                        <span class="inline-flex items-center gap-1">
                                            <Icon icon=icondata::MdiClose />
                                            {t!(i18n, currency_exchange_clear_all)}
                                        </span>
                                    </QueryButton>
                                </span>
                            }.into_any());
                        }
                        view! { <>{chips}</> }
                    }}
                </div>
            </div>
            <div>
                {move || {
                    if home_world().is_none() {
                        let left = view! {
                            <div class="bg-red-900/50 p-4 rounded-lg text-white">
                                {t!(i18n, currency_exchange_home_world_not_set_prefix)}
                                <A
                                    href="/settings"
                                    attr:class="underline"
                                >
                                    {t!(i18n, currency_exchange_settings)}
                                </A> {t!(i18n, currency_exchange_home_world_not_set_suffix)}
                            </div>
                        };
                        Either::Left(left)
                    } else {
                        let right = view! {
                            <div class="text-xs text-[color:var(--color-text-muted)] mb-2">
                                {move || home_world().map(|w| t!(i18n, currency_exchange_assuming_sales_on, world = w.name))}
                            </div>
                            <div class="panel rounded-xl border border-white/5 overflow-hidden mb-4">
                                <h3 class="px-3 py-2 border-b border-white/5 text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                    {t!(i18n, currency_exchange_full_results)}
                                </h3>
                                <Suspense fallback=Loading>
                                    {move || {
                                        let sort_label = sorted_by();
                                    let s_res = s_getter_2.get();
                                    let l_res = l_getter_2.get();
                                    let s = s_res.as_ref().and_then(|r| r.as_ref().ok());
                                    let l = l_res.as_ref().and_then(|r| r.as_ref().ok());
                                    let q = currency_quantity.get();
                                    compute_prices(s, l, q)
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
                                                            currency.price_per_item,
                                                            "price_per_item",
                                                            query,
                                                        )
                                                            && is_in_range(
                                                                currency.number_received,
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
                                                // surface best option at top by default (total_profit desc)
                                                match sort_label.as_deref() {
                                                    None => {
                                                        p.sort_by(|a, b| b.total_profit.cmp(&a.total_profit));
                                                    }
                                                    Some("total_profit") => {
                                                        p.sort_by(|a, b| b.total_profit.cmp(&a.total_profit));
                                                    }
                                                    Some(label) => {
                                                        CurrencyTrade::sort_vec_by_label(&mut p, label, None);
                                                    }
                                                }
                                                p.into_iter()
                                                    .map(|p| {
                                                        view! {
                                                            <tr class="hover:bg-white/5 transition-colors">
                                                                <td class=format!("px-3 py-2 text-[color:var(--color-text-muted)] {}", column_visibility(0))>
                                                                    <ShopNames shop_names=p.shop_names />
                                                                </td>
                                                                <td class=format!("px-3 py-2 {}", column_visibility(1))>
                                                                    <ItemAmount item_amount=p.cost_item />
                                                                </td>
                                                                <td class="px-3 py-2">
                                                                    <ItemAmount item_amount=p.receive_item />
                                                                </td>
                                                                <td class=format!("px-3 py-2 text-right tabular-nums {}", column_visibility(3))>
                                                                    {p.price_per_item}
                                                                </td>
                                                                <td class="px-3 py-2 text-right tabular-nums">{p.number_received}</td>
                                                                <td class="px-3 py-2 text-right tabular-nums font-medium text-[color:var(--color-text)]">
                                                                    {p.total_profit}
                                                                </td>
                                                                <td class=format!("px-3 py-2 text-right tabular-nums text-[color:var(--color-text-muted)] {}", column_visibility(6))>
                                                                    {p.hours_between_sales}
                                                                </td>
                                                            </tr>
                                                        }
                                                    })
                                                    .collect_view()
                                            };
                                            let count = sorted_and_filtered_rows().len();
                                            let s = s_getter_2.get();
                                            let sales = s
                                                .as_ref()
                                                .map(|sales| sales.as_ref().map(|sales| sales.sales.len()));
                                            info!("{sales:?} items: {count} p: {trades}");
                                            let labels = CurrencyTrade::field_labels();
                                            view! {
                                                // Only the table scrolls sideways on narrow
                                                // viewports; the surrounding panel must not, or
                                                // `overflow-x` would force `overflow-y: auto` and
                                                // trap anything absolutely positioned inside it.
                                                <div class="overflow-x-auto">
                                                <table class="w-full text-sm text-left">
                                                    <thead class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                                        <tr class="border-b border-white/5">
                                                            {labels
                                                                .iter()
                                                                .enumerate()
                                                                .filter(|(i, _)| *i <= 6)
                                                                .map(|(i, l)| {
                                                                    // Columns 3+ are numeric and right-aligned in the
                                                                    // body, so their headers follow.
                                                                    let align = if i >= 3 { "justify-end" } else { "" };
                                                                    view! {
                                                                        <th
                                                                            scope="col"
                                                                            class=format!("px-3 py-2 font-bold whitespace-nowrap {}", column_visibility(i))
                                                                        >
                                                                            <div class=format!("flex flex-row items-center gap-2 {align}")>
                                                                                <QueryButton
                                                                                    key="sorted-by"
                                                                                    value=*l
                                                                                    class="underline decoration-transparent hover:text-[color:var(--brand-fg)] transition-colors"
                                                                                    active_classes="text-[color:var(--brand-fg)] underline underline-offset-4 decoration-2"
                                                                                    default="total_profit" == *l
                                                                                >
                                                                                    {match *l {
                                                                                        "shop_names" => t_string!(i18n, currency_exchange_table_shops).to_string(),
                                                                                        "cost_item" => t_string!(i18n, currency_exchange_table_cost).to_string(),
                                                                                        "receive_item" => t_string!(i18n, currency_exchange_table_item).to_string(),
                                                                                        "price_per_item" => t_string!(i18n, currency_exchange_table_price_per_item).to_string(),
                                                                                        "number_received" => t_string!(i18n, currency_exchange_table_qty_recv).to_string(),
                                                                                        "total_profit" => t_string!(i18n, currency_exchange_table_profit).to_string(),
                                                                                        "hours_between_sales" => t_string!(i18n, currency_exchange_table_hours_per_sale).to_string(),
                                                                                        _ => l.replace("_", " "),
                                                                                    }}
                                                                                </QueryButton>
                                                                                {(i > 2)
                                                                                    .then(|| {
                                                                                        view! {
                                                                                            <Tooltip tooltip_text=t_string!(i18n, currency_exchange_filter_tooltip).to_string().replace("%column%", &l.replace("_", " "))>
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
                                                    <tbody class="divide-y divide-white/5">
                                                        {sorted_and_filtered_rows}
                                                    </tbody>
                                                </table>
                                                </div>
                                            }
                                        })
                                }}
                                {move || {
                                    s_getter_2
                                        .with(|sales| {
                                            if let Some(Err(e)) = sales {
                                                Either::Left(
                                                    view! {
                                                        <div class="bg-red-900/50 p-4 rounded-lg text-white mt-4">
                                                            {t!(i18n, currency_exchange_error_loading)}<br />
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
                            </div>
                        };
                        Either::Right(right)
                    }
                }}
            </div>
        </div>
    }.into_any()
}

#[allow(dead_code)]
fn item_cost_iter(shop: &SpecialShop) -> impl Iterator<Item = ItemId> + '_ {
    shop.item_cost_0
        .iter()
        .chain(shop.item_cost_1.iter())
        .chain(shop.item_cost_2.iter())
        .copied()
        .filter(|item_id| *item_id != 0)
        .map(|item_id| ItemId(item_id as i32))
}

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

#[derive(PartialEq, Eq, Clone, PartialOrd, Ord, Debug)]
pub struct ShopNames {
    shops: Vec<String>,
}

#[component]
fn ShopNames(#[prop(into)] shop_names: ShopNames) -> impl IntoView {
    view! {
        <div class="flex flex-col">
            {shop_names
                .shops
                .into_iter()
                .map(|shop| {
                    let title = shop.clone();
                    view! { <div class="truncate" title=title>{shop}</div> }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn CurrencySelection() -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let ui_categories = &data.item_ui_categorys;
    let disallowed_items = &["Gil", "MGP"];
    // `ItemUICategory` row IDs are stable across game locales; only the `name`
    // column is translated. Matching by the English name panicked on every
    // non-English dataset (e.g. `cn` names these "货币"/"杂货"/"其他"), which
    // crashed `/currency-exchange` for all localized users — GlitchTip #6849.
    // Match by the stable IDs instead: Currency = 100, Miscellany = 61, Other = 63.
    let allowed_item_ui_categories = [
        ItemUiCategoryId(100),
        ItemUiCategoryId(61),
        ItemUiCategoryId(63),
    ];
    let currencies = data
        .special_shops
        .iter()
        .flat_map(|(_shops, special_shop)| {
            shop_items(special_shop)
                .filter(|items| items.recv.iter().any(|i| i.item.item_search_category != 0))
                .flat_map(|f| f.cost.into_iter().map(|i| i.item.key_id))
        })
        .filter(|f| {
            let Some(item) = data.items.get(f) else {
                return false;
            };
            allowed_item_ui_categories.contains(&ItemUiCategoryId(item.item_ui_category))
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
            let ui_category = ItemUiCategoryId(item.item_ui_category as i32);
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
        <div class="container mx-auto space-y-4">
            <MetaTitle title=t_string!(i18n, currency_exchange_meta_title_ultros) />
            <MetaDescription text=t_string!(i18n, currency_exchange_meta_desc_default) />

            // One panel for the blurb and the search box. The blurb is a single
            // sentence — the long marketing paragraph pushed the currency grid
            // below the fold on every viewport.
            <div class="panel p-4 rounded-xl flex flex-col sm:flex-row sm:items-center gap-3">
                <p class="flex-1 text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, currency_exchange_hero_desc)}
                </p>
                <div class="relative w-full sm:w-72">
                    <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                        <Icon
                            icon=icondata::BiSearchAlt2Regular
                            attr:class="w-4 h-4 text-[color:var(--color-text-muted)]"
                        />
                    </div>
                    <input
                        type="text"
                        placeholder=t_string!(i18n, currency_exchange_search_placeholder)
                        aria-label=t_string!(i18n, currency_exchange_search_placeholder)
                        class="input w-full pl-9"
                        on:input=move |ev| set_search_text(event_target_value(&ev))
                    />
                </div>
            </div>

            // Currency grid: icon + name + category on one dense row per
            // currency, the same shape the item explorer's result rows use.
            // `card-link` opts the anchor out of the global `a:not(...)` rule
            // in tailwind.css, which otherwise forces a transparent background
            // and underlines every text node inside the tile on hover.
            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-2">
                <For
                    each=filtered_currencies
                    key=|(item_id, _, _)| *item_id
                    children=|(item_id, item_name, category_name)| {
                        view! {
                            <A
                                href=item_id.to_string()
                                attr:class="card-link group flex items-center gap-2 px-3 py-2 rounded-lg border \
                                           border-white/5 bg-[color:var(--color-background-elevated)] \
                                           hover:bg-white/5 hover:border-brand-500/40 transition-colors"
                            >
                                <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                <div class="flex flex-col min-w-0">
                                    <span
                                        class="truncate text-sm font-medium text-[color:var(--color-text)]
                                        group-hover:text-brand-300 transition-colors"
                                        title=item_name
                                    >
                                        {item_name}
                                    </span>
                                    <span class="truncate text-xs text-[color:var(--color-text-muted)]">
                                        {category_name}
                                    </span>
                                </div>
                            </A>
                        }
                    }
                />
            </div>

            // Empty State
            {move || {
                if filtered_currencies().is_empty() {
                    Either::Left(
                        view! {
                            <div class="text-center p-8 text-[color:var(--color-text-muted)]">
                                {t!(i18n, currency_exchange_no_currencies_found)}
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
    let i18n = use_i18n();
    view! {
        <div class="app-inline-ad">
            <Ad class="w-full h-[100px]" />
        </div>
        <div class="main-content">
            <A href="/currency-exchange">
                <h3 class="text-2xl font-bold text-[color:var(--brand-fg)] hover:opacity-90 transition-all ease-in-out duration-500">
                    {t!(i18n, currency_exchange_title)}
                </h3>
            </A>
            <Outlet />
        </div>
    }.into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phone layout drops four of the seven columns. Whatever else moves,
    /// the three that answer "which trade should I make" have to survive, so
    /// name them explicitly rather than trusting the index arithmetic in
    /// `column_visibility`.
    #[test]
    fn the_smallest_layout_keeps_the_columns_that_carry_the_answer() {
        let labels = CurrencyTrade::field_labels();
        let always_visible: Vec<&str> = (0..=6)
            .filter(|i| column_visibility(*i).is_empty())
            .map(|i| labels[i])
            .collect();

        assert_eq!(
            always_visible,
            ["receive_item", "number_received", "total_profit"],
            "the phone layout must always show the item, how many you get, \
             and the profit",
        );
    }

    /// The results table hangs a `FilterModal` off every column past index 2,
    /// and each of those writes `{field_label}_min` / `{field_label}_max` to
    /// the query string. `FILTER_QUERY_KEYS` is the other half of that
    /// contract: it drives the "N active" badge, whether the filter panel
    /// opens on a deep link, and which keys "Clear all" removes. Adding a
    /// numeric column to `CurrencyTrade` without extending the constant would
    /// silently give that column a filter no button can clear, so pin the two
    /// lists together.
    #[test]
    fn filter_query_keys_cover_every_filterable_column() {
        let expected: Vec<String> = CurrencyTrade::field_labels()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > 2 && *i <= 6)
            .flat_map(|(_, label)| [format!("{label}_min"), format!("{label}_max")])
            .collect();

        assert_eq!(
            FILTER_QUERY_KEYS.to_vec(),
            expected,
            "FILTER_QUERY_KEYS is out of sync with the filterable columns of \
             CurrencyTrade; the active-filter badge and 'Clear all' would miss \
             the difference",
        );
    }

    /// `CurrencySelection` builds its category whitelist from the stable
    /// `ItemUICategory` row IDs (Currency = 100, Miscellany = 61, Other = 63)
    /// rather than the localized `name`, because matching the English name
    /// panicked on non-English datasets (GlitchTip #6849 → cascade #6850). This
    /// pins the ID→name mapping in the embedded (English) dataset so a future
    /// game-data bump that renumbers these categories fails loudly here instead
    /// of silently emptying the currency list.
    #[test]
    fn allowed_currency_category_ids_match_expected_names() {
        let data = xiv_gen_db::data();
        let cats = &data.item_ui_categorys;
        for (id, expected) in [(100, "Currency"), (61, "Miscellany"), (63, "Other")] {
            let cat = cats
                .get(&ItemUiCategoryId(id))
                .unwrap_or_else(|| panic!("ItemUICategory {id} missing from embedded data"));
            assert_eq!(
                cat.name, expected,
                "ItemUICategory {id} should be '{expected}' but was '{}'; \
                 update allowed_item_ui_categories in CurrencySelection",
                cat.name
            );
        }
    }

    /// Regression for GlitchTip #6849: on a non-English locale the category
    /// `name`s are translated, so the old
    /// `find(|c| c.name == "Currency").unwrap()` hit `None` and panicked,
    /// crashing `/currency-exchange`. Confirm both the crash precondition (the
    /// English names are absent from the `cn` dataset) and that the stable IDs
    /// the fix uses still resolve there.
    #[test]
    fn currency_categories_resolve_by_id_on_localized_data() {
        let cn = xiv_gen_db::data_for(xiv_gen::Language::Cn);
        let cats = &cn.item_ui_categorys;

        // Precondition that made the old name-based lookup unwrap `None`.
        for english in ["Currency", "Miscellany", "Other"] {
            assert!(
                !cats.values().any(|c| c.name == english),
                "the `cn` dataset should have no category literally named \
                 '{english}' (names are translated); the old name-based lookup \
                 unwrapped None here and panicked",
            );
        }

        // The IDs the fix uses must still exist (and be named) on `cn`.
        for id in [100, 61, 63] {
            let cat = cats
                .get(&ItemUiCategoryId(id))
                .unwrap_or_else(|| panic!("ItemUICategory {id} missing from cn dataset"));
            assert!(
                !cat.name.is_empty(),
                "ItemUICategory {id} should have a localized name on cn",
            );
        }
    }
}
