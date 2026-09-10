use crate::components::app_link::AppLink;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use crate::analyzer_kit::filters::register_filters;
use crate::analyzer_kit::market::{MarketGrid, MarketSubject, use_market_data};
use crate::analyzer_kit::stat_columns::{market_picker_options, shared_cols_in, toggle_shared_col};
use crate::analyzer_kit::window::MarketWindowControl;
use crate::api::get_cheapest_listings;
use crate::api::get_recent_sales_for_world;
use crate::components::ad::Ad;
use crate::components::add_to_list::AddToList;
use crate::components::clipboard::Clipboard;
use crate::components::control_bar::{ColumnOption, ControlBar};
use crate::components::icon::Icon;
use crate::components::item_icon::ItemIcon;
use crate::components::meta::MetaDescription;
use crate::components::meta::MetaTitle;
use crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton};
use crate::components::sort_header::{SortColumn, SortDir, SortHeader};
use crate::components::tool_help::ToolHeader;
use crate::components::virtual_grid::GridColumn;
use crate::components::virtual_grid::metrics::{FilterOp, GridMetric, GridValue};
use crate::components::virtual_grid::registry::FilterAlias;
use crate::components::virtual_grid::saved_views::provide_grid_saved_views;
use crate::error::AppError;
use crate::global_state::home_world::use_home_world;
use crate::global_state::xiv_data::{resolve_item_id, tracked_data};
use crate::i18n::*;
use crate::query_defaults::filter_query_signal;
use crate::routes::not_found::NotFound;
use chrono::TimeDelta;
use chrono::Utc;
use itertools::Itertools;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::Outlet;
use leptos_router::hooks::*;

use crate::query_defaults::query_signal;
#[cfg(test)]
use leptos_router::params::ParamsMap;
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
                    <AppLink
                        attr:class="flex flex-row gap-1 min-w-0"
                        href=format!("/item/{}", item_amount.item.key_id.0)
                    >
                        <ItemIcon item_id=item_amount.item.key_id.0 icon_size=IconSize::Small />
                        <span class="truncate" title=item_amount.item.name.as_str()>{item_amount.item.name.as_str()}</span>
                    </AppLink>
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

/// Keep native exchange revenue and recent-sale cadence independent of the
/// selectable market-history window.
fn compute_prices(
    shops_with_item: &[(ShopItems, &SpecialShop)],
    sales: Option<&ultros_api_types::recent_sales::RecentSales>,
    listings: Option<&ultros_api_types::cheapest_listings::CheapestListings>,
    quantity: i32,
    now: chrono::NaiveDateTime,
) -> Option<Vec<CurrencyTrade>> {
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
                    listing_price: world_listings
                        .get(&(false, recv.item.key_id.0))
                        .map(|l| l.cheapest_price),
                    price_per_item: guessed_price_per_item,
                    number_received,
                    total_profit,
                    hours_between_sales,
                }
            },
        )
        .collect::<Vec<_>>();
    Some(rows)
}

/// Existing optional-column IDs are part of saved exchange links.
const COL_PRICE_PER_ITEM: &str = "price_per_item";
const COL_SHOPS: &str = "shops";
const COL_COST: &str = "cost";
const COL_HOURS: &str = "hours_between_sales";

/// Native optional columns, in picker order. All four are on by default.
const ALL_OPTIONAL_COLS: &[&str] = &[COL_PRICE_PER_ITEM, COL_SHOPS, COL_COST, COL_HOURS];

/// The `?cols=` value an absent param stands for: every native optional
/// column and no shared sale-history column — the grid's own defaults.
fn default_cols_query() -> String {
    ALL_OPTIONAL_COLS.join(",")
}

/// Picker checkboxes for a `?cols=` value. The grid owns the param (absent =
/// defaults, explicit — even empty — = exact), so the toolbar reads the same
/// token list the grid does rather than keeping a second visible set.
fn picker_visible_cols(raw: Option<&str>) -> std::collections::HashSet<&'static str> {
    let Some(raw) = raw else {
        return ALL_OPTIONAL_COLS.iter().copied().collect();
    };
    let mut visible: std::collections::HashSet<&'static str> = ALL_OPTIONAL_COLS
        .iter()
        .copied()
        .filter(|col| raw.split(',').any(|tok| tok == *col))
        .collect();
    visible.extend(shared_cols_in(Some(raw)));
    visible
}

fn exchange_filter_aliases() -> Vec<FilterAlias> {
    [
        ("price_per_item_min", COL_PRICE_PER_ITEM, FilterOp::Gte),
        ("price_per_item_max", COL_PRICE_PER_ITEM, FilterOp::Lte),
        ("number_received_min", "number_received", FilterOp::Gte),
        ("number_received_max", "number_received", FilterOp::Lte),
        ("total_profit_min", "total_profit", FilterOp::Gte),
        ("total_profit_max", "total_profit", FilterOp::Lte),
        ("hours_between_sales_min", COL_HOURS, FilterOp::Gte),
        ("hours_between_sales_max", COL_HOURS, FilterOp::Lte),
    ]
    .into_iter()
    .map(|(key, column, op)| FilterAlias::integer(key, column, op))
    .collect()
}

fn exchange_metrics() -> Vec<GridMetric<CurrencyTrade>> {
    vec![
        GridMetric::number(COL_PRICE_PER_ITEM, |t: &CurrencyTrade| {
            GridValue::Number(t.price_per_item as f64)
        }),
        GridMetric::number("number_received", |t: &CurrencyTrade| {
            GridValue::Number(t.number_received as f64)
        }),
        GridMetric::number("total_profit", |t: &CurrencyTrade| {
            GridValue::Number(t.total_profit as f64)
        }),
        GridMetric::number(COL_HOURS, |t: &CurrencyTrade| {
            GridValue::Number(t.hours_between_sales as f64)
        }),
    ]
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    PricePerItem,
    QtyReceived,
    HoursBetweenSales,
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortMode::Profit => "profit",
            SortMode::PricePerItem => "price",
            SortMode::QtyReceived => "qty",
            SortMode::HoursBetweenSales => "hours",
        })
    }
}

impl std::str::FromStr for SortMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "price" => Ok(SortMode::PricePerItem),
            "qty" => Ok(SortMode::QtyReceived),
            "hours" => Ok(SortMode::HoursBetweenSales),
            _ => Err(()),
        }
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }
    /// Hours-between-sales reads best-first ascending — descending would
    /// put the slowest sellers on top. Everything else is best-first
    /// descending, the kit default.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::HoursBetweenSales => SortDir::Asc,
            _ => SortDir::Desc,
        }
    }
}

fn sort_trades(rows: &mut [CurrencyTrade], mode: SortMode, dir: SortDir) {
    let key = |t: &CurrencyTrade| -> i64 {
        match mode {
            SortMode::Profit => t.total_profit,
            SortMode::PricePerItem => t.price_per_item as i64,
            SortMode::QtyReceived => t.number_received as i64,
            SortMode::HoursBetweenSales => t.hours_between_sales as i64,
        }
    };
    match dir {
        SortDir::Desc => rows.sort_by_key(|t| std::cmp::Reverse(key(t))),
        SortDir::Asc => rows.sort_by_key(key),
    }
}

#[cfg(test)]
fn is_in_range(value: i32, field_label: &str, query_map: &ParamsMap) -> bool {
    let max = query_map
        .get(&format!("{field_label}_max"))
        .and_then(|p| p.parse::<i32>().ok());
    let min = query_map
        .get(&format!("{field_label}_min"))
        .and_then(|p| p.parse::<i32>().ok());

    // Inclusive on both ends: the chips read "Profit ≥ 5000" / "Profit ≤ 5000",
    // and every other tool in the app (analyzer, recipe/fc analyzers) filters
    // with `>= min` / `<= max`. The pre-kit exclusive bounds silently dropped
    // the row sitting exactly on the number the user typed.
    match (min, max) {
        (None, None) => true,
        (None, Some(max)) => value <= max,
        (Some(min), None) => value >= min,
        (Some(min), Some(max)) => (min..=max).contains(&value),
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
    let (home_world, _) = use_home_world();
    // `filter_query_signal`, not a plain `query_signal`: this box is typed into
    // a digit at a time, and the router default (replace: false, scroll: true)
    // would push a history entry and yank the window to the top per keystroke —
    // the same bug this rebuild fixed for the filter chips.
    let (currency_quantity, set_currency_quantity) = filter_query_signal::<i32>("currency_amount");
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

    let (sort_param, _) = query_signal::<String>("sort");
    let (dir_param, _) = query_signal::<String>("dir");
    let sort_mode = Memo::new(move |_| sort_param().and_then(|s| s.parse::<SortMode>().ok()));
    let sort_dir = Memo::new(move |_| dir_param().and_then(|s| s.parse::<SortDir>().ok()));
    let market = use_market_data(Signal::derive(move || {
        home_world.get().map(|w| w.name).unwrap_or_default()
    }));
    let filters = register_filters(exchange_filter_aliases(), Signal::derive(Vec::new));
    provide_grid_saved_views("currency-exchange-grid");
    let item_name = move || item().map(|i| i.name.as_str()).unwrap_or_default();
    let column_label = move |id| -> String {
        match id {
            "item" => t_string!(i18n, currency_exchange_table_item).to_string(),
            "number_received" => t_string!(i18n, currency_exchange_table_qty_recv).to_string(),
            "total_profit" => t_string!(i18n, currency_exchange_table_profit).to_string(),
            COL_PRICE_PER_ITEM => {
                t_string!(i18n, currency_exchange_table_price_per_item).to_string()
            }
            COL_SHOPS => t_string!(i18n, currency_exchange_table_shops).to_string(),
            COL_COST => t_string!(i18n, currency_exchange_table_cost).to_string(),
            COL_HOURS => t_string!(i18n, currency_exchange_table_hours_per_sale).to_string(),
            _ => String::new(),
        }
    };
    // Toolbar column picker: the four native optional columns, then every
    // shared sale-history column grouped by window — the same picker the
    // flip finder and recipe analyzer offer. Checked state and toggles go
    // through `?cols=`, which the grid already reads for all optional
    // columns, so saved links keep their exact meaning.
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    let picker_visible = Memo::new(move |_| picker_visible_cols(cols_param.get().as_deref()));
    let column_options = Memo::new(move |_| {
        ALL_OPTIONAL_COLS
            .iter()
            .copied()
            .map(|col| ColumnOption::new(col, column_label(col)))
            .chain(market_picker_options(market.window.selected.get()))
            .collect::<Vec<_>>()
    });
    let toggle_column = Callback::new(move |col: &'static str| {
        let previous = cols_param.get_untracked();
        set_cols_param.set(Some(toggle_shared_col(
            previous.as_deref(),
            &default_cols_query(),
            col,
        )));
    });
    let reset_columns = Callback::new(move |_| set_cols_param.set(None));
    let column_sort = |id| match id {
        "number_received" => Some(SortMode::QtyReceived),
        "total_profit" => Some(SortMode::Profit),
        COL_PRICE_PER_ITEM => Some(SortMode::PricePerItem),
        COL_HOURS => Some(SortMode::HoursBetweenSales),
        _ => None,
    };
    let columns = Signal::derive(move || {
        [
            ("item", 320.0, false),
            ("number_received", 130.0, false),
            ("total_profit", 130.0, false),
            (COL_PRICE_PER_ITEM, 150.0, true),
            (COL_SHOPS, 240.0, true),
            (COL_COST, 320.0, true),
            (COL_HOURS, 180.0, true),
        ]
        .into_iter()
        .map(|(id, width, optional)| {
            let column = GridColumn::new(id, column_label(id), width, optional, true);
            if let Some(mode) = column_sort(id) {
                column.sorted(
                    sort_mode.get().unwrap_or_else(SortMode::fallback) == mode,
                    sort_dir.get().unwrap_or_else(|| mode.default_dir()) == SortDir::Asc,
                )
            } else {
                column
            }
        })
        .collect::<Vec<_>>()
    });
    // Create derived signals to access resources, avoiding ownership issues in view closures.
    let sales_2 = sales.clone();
    let s_getter_2 = Signal::derive(move || sales_2.get());

    let listings_2 = world_cheapest_listings.clone();
    let l_getter_2 = Signal::derive(move || listings_2.get());

    // Keep the grid mounted when quantity or market resources change so the
    // shared filter registry never points at a disposed grid owner.
    let rows = Memo::new(move |_| {
        let sales = s_getter_2.get();
        let listings = l_getter_2.get();
        let mut rows = compute_prices(
            &shop_data(),
            sales.as_ref().and_then(|r| r.as_ref().ok()),
            listings.as_ref().and_then(|r| r.as_ref().ok()),
            currency_quantity.get(),
            Utc::now().naive_utc(),
        )
        .unwrap_or_default();
        let mode = sort_mode.get().unwrap_or_else(SortMode::fallback);
        sort_trades(
            &mut rows,
            mode,
            sort_dir.get().unwrap_or_else(|| mode.default_dir()),
        );
        rows
    });
    view! {
        <div>
            <MetaTitle title=move || t_string!(i18n, currency_exchange_meta_title).replace("%item%", item_name()) />
            <MetaDescription text=move || {
                t_string!(i18n, currency_exchange_meta_desc).replace("%item%", item_name())
            } />
            <ToolHeader
                title=format!("{} — {}", item_name(), t_string!(i18n, currency_exchange_title))
                summary=t_string!(i18n, currency_exchange_tool_summary).to_string()
                context=t_string!(i18n, currency_exchange_tool_context).to_string()
                help_href="/help"
                help_body=t_string!(i18n, currency_exchange_tool_help).to_string()
            >
                <label for="currency-quantity" class="text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, currency_exchange_how_many)}
                </label>
                <input
                    id="currency-quantity"
                    class="input w-24"
                    inputmode="numeric"
                    prop:value=currency_quantity
                    on:input=move |e| {
                        if let Ok(p) = event_target_value(&e).parse() {
                            set_currency_quantity.set(Some(p));
                        }
                    }
                />
            </ToolHeader>
            <ControlBar
                sticky=false
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, currency_exchange_trade_count, n = move || filters.row_count())}
                        </span>
                    }
                    .into_any()
                }
                columns=column_options
                visible_columns=picker_visible
                on_toggle_column=toggle_column
                on_reset_columns=reset_columns
                empty_label=Signal::derive(move || {
                    t_string!(i18n, currency_exchange_no_filters_hint).to_string()
                })
            />
            <MarketWindowControl window=market.window />
            <div>
                <Show when=move || home_world().is_none()>
                            <div class="bg-red-900/50 p-4 rounded-lg text-white">
                                {t!(i18n, currency_exchange_home_world_not_set_prefix)}
                                <AppLink
                                    href="/settings"
                                    attr:class="underline"
                                >
                                    {t!(i18n, currency_exchange_settings)}
                                </AppLink> {t!(i18n, currency_exchange_home_world_not_set_suffix)}
                            </div>
                </Show>
                // The toolbar registry keeps signals owned by this grid. Keep
                // the grid mounted as the home world changes; disposing it
                // would leave the toolbar reading a previous owner's signals.
                <div class:hidden=move || home_world().is_none()>
                            <div class="text-xs text-[color:var(--color-text-muted)] mb-2">
                                {move || home_world().map(|w| t!(i18n, currency_exchange_assuming_sales_on, world = w.name))}
                            </div>
                            <div class="panel rounded-xl border border-white/5 overflow-hidden mb-4">
                                <h3 class="px-3 py-2 border-b border-white/5 text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                    {t!(i18n, currency_exchange_full_results)}
                                </h3>
                                <Suspense fallback=move || {
                                    view! {
                                        <TableSkeleton
                                            columns=vec![SkeletonColumn::new("flex-1", SkeletonCell::IconText), SkeletonColumn::new("w-24", SkeletonCell::Number)]
                                            rows=10
                                        />
                                    }
                                }>
                                    <MarketGrid
                                        id="currency-exchange-grid"
                                        label=t_string!(i18n, currency_exchange_full_results).to_string()
                                        each=rows columns market
                                        row_height=72.0
                                        key=|t: &CurrencyTrade| (t.cost_item, t.receive_item)
                                        metrics=exchange_metrics()
                                        subject=Arc::new(move |t: &CurrencyTrade| t.market_subject(home_world.get().map(|w| w.id).unwrap_or_default()))
                                        header=move |id| {
                                            let label = column_label(id);
                                            if let Some(mode) = column_sort(id) {
                                                view! { <SortHeader mode label sort_mode sort_dir /> }.into_any()
                                            } else { label.into_any() }
                                        }
                                        view=move |trade: CurrencyTrade, id| {
                                            let content = match id {
                                            "item" => view! { <ItemAmount item_amount=trade.receive_item /> }.into_any(),
                                            COL_COST => view! { <ItemAmount item_amount=trade.cost_item /> }.into_any(),
                                            COL_SHOPS => view! { <div class="max-h-full overflow-y-auto"><ShopNames shop_names=trade.shop_names /></div> }.into_any(),
                                            _ => view! { <div class="text-right tabular-nums w-full">{trade.cell_text(id)}</div> }.into_any(),
                                        };
                                        view! { <div class="px-3 flex h-full items-center min-w-0">{content}</div> }.into_any()
                                        }
                                        measure=|trade: &CurrencyTrade, id| (trade.cell_text(id), if matches!(id, "item" | COL_COST) { 120.0 } else { 24.0 })
                                    />
                                    <Show when=move || filters.row_count() == 0>
                                        <p role="status" class="px-3 py-8 text-center text-[color:var(--color-text-muted)]">
                                            {t!(i18n, currency_exchange_no_matches)}
                                        </p>
                                    </Show>
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
                </div>
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

#[derive(Clone, PartialEq)]
pub struct CurrencyTrade {
    shop_names: ShopNames,
    cost_item: Option<ItemAmount>,
    receive_item: Option<ItemAmount>,
    /// Actual NQ listing; the native revenue estimate may be lower.
    listing_price: Option<i32>,
    price_per_item: i32,
    number_received: i32,
    total_profit: i64,
    hours_between_sales: i16,
}

impl CurrencyTrade {
    fn market_subject(&self, world_id: i32) -> MarketSubject {
        MarketSubject {
            item_id: self
                .receive_item
                .map(|i| i.item.key_id.0)
                .unwrap_or_default(),
            hq: false,
            world_id,
            label: self
                .receive_item
                .map(|i| i.item.name.clone())
                .unwrap_or_default(),
            listing_price: self.listing_price,
        }
    }

    fn cell_text(&self, id: &str) -> String {
        match id {
            "item" | COL_COST => {
                let item = if id == "item" {
                    self.receive_item
                } else {
                    self.cost_item
                };
                item.map(|i| format!("{} ×{}", i.item.name, i.amount))
                    .unwrap_or_default()
            }
            COL_SHOPS => self
                .shop_names
                .shops
                .iter()
                .max_by_key(|s| s.len())
                .cloned()
                .unwrap_or_default(),
            COL_PRICE_PER_ITEM => self.price_per_item.to_string(),
            "number_received" => self.number_received.to_string(),
            "total_profit" => self.total_profit.to_string(),
            COL_HOURS => self.hours_between_sales.to_string(),
            _ => String::new(),
        }
    }
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
        <div class="space-y-4">
            <MetaTitle title=t_string!(i18n, currency_exchange_meta_title_ultros) />
            <MetaDescription text=t_string!(i18n, currency_exchange_meta_desc_default) />

            // The route wrapper no longer renders a shared heading (the
            // exchange-item page brings its own ToolHeader), so the landing
            // page names itself.
            <h1 class="text-2xl font-bold text-[color:var(--brand-fg)]">
                {t!(i18n, currency_exchange_title)}
            </h1>

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
                            <AppLink
                                // Absolute: `AppLink` is a plain anchor and
                                // does not resolve hrefs against the matched
                                // route. See `components::app_link`.
                                href=format!("/currency-exchange/{item_id}")
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
                            </AppLink>
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
    view! {
        <div class="app-inline-ad">
            <Ad class="w-full h-[100px]" />
        </div>
        <div class="main-content p-2 sm:p-6">
            <Outlet />
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(value: i32) -> CurrencyTrade {
        CurrencyTrade {
            shop_names: ShopNames { shops: vec![] },
            cost_item: None,
            receive_item: None,
            listing_price: Some(200),
            price_per_item: value,
            number_received: value,
            total_profit: value as i64,
            hours_between_sales: value as i16,
        }
    }

    #[test]
    fn pricing_quantity_grouping_and_recent_cadence_stay_independent_of_market_stats() {
        use ultros_api_types::cheapest_listings::CheapestListings;
        use ultros_api_types::recent_sales::{RecentSales, Sales};
        let data = xiv_gen_db::data();
        let shop = data.special_shops.values().next().unwrap();
        let received = data
            .items
            .values()
            .find(|i| i.item_search_category > 0)
            .unwrap();
        let shop_items = || ShopItems {
            recv: vec![ItemAmount {
                item: received,
                amount: 3,
            }],
            cost: vec![ItemAmount {
                item: &data.items[&ItemId(1)],
                amount: 5,
            }],
        };
        let shops = vec![(shop_items(), shop), (shop_items(), shop)];
        let now = chrono::DateTime::from_timestamp(1_800_000_000, 0)
            .unwrap()
            .naive_utc();
        let mut sales = RecentSales {
            sales: vec![SaleData {
                item_id: received.key_id.0,
                hq: false,
                sales: vec![
                    Sales {
                        price_per_unit: 100,
                        sale_date: now,
                    },
                    Sales {
                        price_per_unit: 90,
                        sale_date: now - TimeDelta::hours(12),
                    },
                ],
            }],
        };
        let mut listings = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: received.key_id.0,
                hq: false,
                cheapest_price: 80,
                world_id: 21,
            }],
        };
        for (listing, expected) in [(80, 79), (200, 100)] {
            listings.cheapest_listings[0].cheapest_price = listing;
            let rows = compute_prices(&shops, Some(&sales), Some(&listings), 12, now).unwrap();
            assert_eq!(rows.len(), 1, "identical shop offers stay grouped");
            assert_eq!(rows[0].shop_names.shops.len(), 1);
            assert_eq!(rows[0].price_per_item, expected);
            assert_eq!(rows[0].listing_price, Some(listing));
            assert_eq!(rows[0].number_received, 6, "only complete trades count");
            assert_eq!(rows[0].total_profit, i64::from(expected) * 6);
            assert_eq!(rows[0].hours_between_sales, 6);
        }
        listings.cheapest_listings.clear();
        let rows = compute_prices(&shops, Some(&sales), Some(&listings), 12, now).unwrap();
        assert_eq!(
            rows[0].price_per_item, 100,
            "missing listings fall back to latest sale"
        );
        assert_eq!(rows[0].listing_price, None);
        sales.sales[0].sales[0].sale_date = now - TimeDelta::days(60);
        assert_eq!(
            compute_prices(&shops, Some(&sales), Some(&listings), 12, now)
                .unwrap()
                .len(),
            1
        );
        sales.sales[0].sales[0].sale_date -= TimeDelta::seconds(1);
        assert!(
            compute_prices(&shops, Some(&sales), Some(&listings), 12, now)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn legacy_ranges_match_shared_filters_and_survive_canonical_links() {
        use crate::components::virtual_grid::metrics::query_rows;
        use crate::components::virtual_grid::registry::{canonical_query, resolve_filters};
        let aliases = exchange_filter_aliases();
        let rows = vec![trade(-1), trade(0), trade(9), trade(10), trade(11)];
        for field in [
            COL_PRICE_PER_ITEM,
            "number_received",
            "total_profit",
            COL_HOURS,
        ] {
            for (low, high) in [
                (None, None),
                (Some("10"), None),
                (None, Some("10")),
                (Some("10"), Some("10")),
                (Some("11"), Some("9")),
                (Some("invalid"), Some("10")),
            ] {
                let mut query = ParamsMap::new();
                if let Some(low) = low {
                    query.insert(format!("{field}_min"), low.to_string());
                }
                if let Some(high) = high {
                    query.insert(format!("{field}_max"), high.to_string());
                }
                let filters = resolve_filters(&query, &aliases);
                let expected: Vec<_> = rows
                    .iter()
                    .filter(|r| is_in_range(r.price_per_item, field, &query))
                    .map(|r| r.price_per_item)
                    .collect();
                let result = query_rows(&rows, &exchange_metrics(), &filters, None, false);
                assert_eq!(
                    result
                        .rows
                        .as_ref()
                        .unwrap_or(&rows)
                        .iter()
                        .map(|r| r.price_per_item)
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(
                    resolve_filters(&canonical_query(&query, &aliases), &aliases),
                    filters
                );
            }
        }
    }

    /// The toolbar picker and the grid read the same `?cols=`: absent means
    /// the four native columns, an explicit value (even empty) is exact,
    /// unknown tokens are dropped, and toggling flips one token while
    /// leaving every other native or shared column in place.
    #[test]
    fn toolbar_picker_mirrors_the_grid_cols_contract() {
        type Cols = std::collections::HashSet<&'static str>;
        let all: Cols = ALL_OPTIONAL_COLS.iter().copied().collect();
        assert_eq!(picker_visible_cols(None), all);
        assert_eq!(picker_visible_cols(Some("")), Cols::new());
        let mixed = picker_visible_cols(Some("shops,bogus,market-sale-median"));
        assert_eq!(
            mixed,
            [COL_SHOPS, "market-sale-median"]
                .into_iter()
                .collect::<Cols>()
        );

        let defaults = default_cols_query();
        assert_eq!(defaults, "price_per_item,shops,cost,hours_between_sales");
        let with_shared = toggle_shared_col(None, &defaults, "market-sale-median");
        assert_eq!(
            picker_visible_cols(Some(&with_shared)),
            all.iter()
                .copied()
                .chain(["market-sale-median"])
                .collect::<Cols>()
        );
        let without_shops = toggle_shared_col(Some(&with_shared), &defaults, COL_SHOPS);
        assert_eq!(
            picker_visible_cols(Some(&without_shops)),
            [
                COL_PRICE_PER_ITEM,
                COL_COST,
                COL_HOURS,
                "market-sale-median"
            ]
            .into_iter()
            .collect::<Cols>()
        );
        assert_eq!(
            toggle_shared_col(Some(""), &defaults, COL_SHOPS),
            COL_SHOPS,
            "an explicit empty set starts from nothing, not the defaults"
        );
    }

    #[test]
    fn market_subject_uses_received_nq_item_and_raw_listing_on_home_world() {
        let data = xiv_gen_db::data();
        let mut row = trade(100);
        row.cost_item = Some(ItemAmount {
            item: &data.items[&ItemId(1)],
            amount: 5,
        });
        row.receive_item = Some(ItemAmount {
            item: &data.items[&ItemId(2)],
            amount: 1,
        });
        for world_id in [21, 74] {
            let subject = row.market_subject(world_id);
            assert_eq!(subject.item_id, 2);
            assert!(!subject.hq);
            assert_eq!(subject.world_id, world_id);
            assert_eq!(subject.listing_price, Some(200));
            assert_ne!(subject.listing_price, Some(row.price_per_item));
        }
        row.listing_price = None;
        assert_eq!(row.market_subject(21).listing_price, None);
    }

    /// `Display` must produce exactly the token `FromStr` parses back out of
    /// `?sort=` — that round trip is `SortHeader`'s whole mechanism. And
    /// hours-between-sales must default ascending: descending hours puts the
    /// slowest sellers on top, which is never why the column was clicked.
    #[test]
    fn sort_tokens_round_trip_and_hours_defaults_ascending() {
        for mode in [
            SortMode::Profit,
            SortMode::PricePerItem,
            SortMode::QtyReceived,
            SortMode::HoursBetweenSales,
        ] {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert_eq!(SortMode::fallback(), SortMode::Profit);
        assert_eq!(SortMode::HoursBetweenSales.default_dir(), SortDir::Asc);
        assert_eq!(SortMode::Profit.default_dir(), SortDir::Desc);
    }

    /// The chips read "Profit ≥ 5000" / "Profit ≤ 5000", so the row sitting
    /// exactly on the typed number has to survive the filter. The pre-kit
    /// bounds were exclusive on both ends, which quietly dropped it and
    /// disagreed with every other tool in the app (`analyzer.rs`,
    /// `recipe_analyzer.rs`, `fc_crafting_analyzer.rs` all use `>=`/`<=`).
    #[test]
    fn range_filter_bounds_are_inclusive() {
        let params = |pairs: &[(&str, &str)]| {
            let mut q = ParamsMap::new();
            for (k, v) in pairs {
                q.insert(k.to_string(), v.to_string());
            }
            q
        };

        let min_only = params(&[("total_profit_min", "5000")]);
        assert!(
            is_in_range(5000, "total_profit", &min_only),
            "≥ includes 5000"
        );
        assert!(is_in_range(5001, "total_profit", &min_only));
        assert!(!is_in_range(4999, "total_profit", &min_only));

        let max_only = params(&[("total_profit_max", "5000")]);
        assert!(
            is_in_range(5000, "total_profit", &max_only),
            "≤ includes 5000"
        );
        assert!(is_in_range(4999, "total_profit", &max_only));
        assert!(!is_in_range(5001, "total_profit", &max_only));

        // A both-ends range on a single value must keep that value.
        let exact = params(&[("total_profit_min", "5000"), ("total_profit_max", "5000")]);
        assert!(is_in_range(5000, "total_profit", &exact));
        assert!(!is_in_range(4999, "total_profit", &exact));
        assert!(!is_in_range(5001, "total_profit", &exact));

        // No bounds at all still means "everything".
        assert!(is_in_range(0, "total_profit", &params(&[])));
    }

    #[test]
    fn sort_trades_orders_by_the_requested_column() {
        let trade = |profit: i64, hours: i16| CurrencyTrade {
            shop_names: ShopNames { shops: vec![] },
            cost_item: None,
            receive_item: None,
            listing_price: None,
            price_per_item: 0,
            number_received: 0,
            total_profit: profit,
            hours_between_sales: hours,
        };
        let mut rows = vec![trade(10, 5), trade(30, 1), trade(20, 9)];
        sort_trades(&mut rows, SortMode::Profit, SortDir::Desc);
        assert_eq!(
            rows.iter().map(|t| t.total_profit).collect::<Vec<_>>(),
            [30, 20, 10]
        );
        sort_trades(&mut rows, SortMode::HoursBetweenSales, SortDir::Asc);
        assert_eq!(
            rows.iter()
                .map(|t| t.hours_between_sales)
                .collect::<Vec<_>>(),
            [1, 5, 9]
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
