use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;

use crate::api::get_cheapest_listings;
use crate::api::get_recent_sales_for_world;
use crate::components::ad::Ad;
use crate::components::add_to_list::AddToList;
use crate::components::clipboard::Clipboard;
use crate::components::control_bar::{ColumnOption, ControlBar, FilterOption};
use crate::components::filter_chip::FilterChip;
use crate::components::icon::Icon;
use crate::components::item_icon::ItemIcon;
use crate::components::meta::MetaDescription;
use crate::components::meta::MetaTitle;
use crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton};
use crate::components::sort_header::{SortColumn, SortDir, SortHeader};
use crate::components::tool_help::ToolHeader;
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
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::components::A;
use leptos_router::components::Outlet;
use leptos_router::hooks::*;

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

/// Stable URL IDs for optional columns, in picker + `?cols=` order.
/// Required columns (item, qty received, profit) are not listed — they
/// always render, and lead the table so a phone's visible slice is the
/// answer, not the trivia.
const COL_PRICE_PER_ITEM: &str = "price_per_item";
const COL_SHOPS: &str = "shops";
const COL_COST: &str = "cost";
const COL_HOURS: &str = "hours_between_sales";

const ALL_OPTIONAL_COLS: &[&str] = &[COL_PRICE_PER_ITEM, COL_SHOPS, COL_COST, COL_HOURS];

/// All four default on; `?cols=` absent = this set, explicitly set (even
/// to "") = respected exactly — same contract as the flip finder.
const DEFAULT_VISIBLE_COLS: &[&str] = ALL_OPTIONAL_COLS;

fn parse_visible_cols(raw: Option<&str>) -> std::collections::HashSet<&'static str> {
    match raw {
        None => DEFAULT_VISIBLE_COLS.iter().copied().collect(),
        Some(s) => s
            .split(',')
            .filter_map(|tok| ALL_OPTIONAL_COLS.iter().find(|c| **c == tok).copied())
            .collect(),
    }
}

fn serialize_visible_cols(visible: &std::collections::HashSet<&'static str>) -> String {
    ALL_OPTIONAL_COLS
        .iter()
        .filter(|c| visible.contains(*c))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
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

/// Skeleton columns matching the visible column set, so the placeholder
/// table has the same rhythm as the one that loads in. Order mirrors the
/// real DOM order: item, qty, profit, then whatever `?cols=` has on.
fn skeleton_columns(visible: &std::collections::HashSet<&'static str>) -> Vec<SkeletonColumn> {
    let mut cols = vec![
        SkeletonColumn::new("flex-1 min-w-40", SkeletonCell::IconText),
        SkeletonColumn::new("w-20", SkeletonCell::Number),
        SkeletonColumn::new("w-24", SkeletonCell::Number),
    ];
    if visible.contains(COL_PRICE_PER_ITEM) {
        cols.push(SkeletonColumn::new("w-24", SkeletonCell::Number));
    }
    if visible.contains(COL_SHOPS) {
        cols.push(SkeletonColumn::new("w-40", SkeletonCell::Text));
    }
    if visible.contains(COL_COST) {
        cols.push(SkeletonColumn::new("w-40", SkeletonCell::IconText));
    }
    if visible.contains(COL_HOURS) {
        cols.push(SkeletonColumn::new("w-20", SkeletonCell::Number));
    }
    cols
}

/// One min/max half of a numeric column filter: everything the chip, the
/// `+ Filter` menu, and Clear-all need to agree on.
struct RangeFilter {
    /// Query key, kept verbatim from the old UI so deep links survive.
    key: &'static str,
    /// Spinner floor for the chip's inline input. `None` for profit —
    /// a negative profit floor is a legitimate filter.
    min: Option<&'static str>,
}

const RANGE_FILTERS: &[RangeFilter] = &[
    RangeFilter {
        key: "price_per_item_min",
        min: Some("0"),
    },
    RangeFilter {
        key: "price_per_item_max",
        min: Some("0"),
    },
    RangeFilter {
        key: "number_received_min",
        min: Some("0"),
    },
    RangeFilter {
        key: "number_received_max",
        min: Some("0"),
    },
    RangeFilter {
        key: "total_profit_min",
        min: None,
    },
    RangeFilter {
        key: "total_profit_max",
        min: None,
    },
    RangeFilter {
        key: "hours_between_sales_min",
        min: Some("0"),
    },
    RangeFilter {
        key: "hours_between_sales_max",
        min: Some("0"),
    },
];

/// Set the `.hscroll-fade` mask variables from the scrollport's geometry: a
/// 24px fade on any side that still has content past the fold, 0 otherwise.
/// The 1px deadbands absorb the browser's rounding of `scrollWidth`.
#[cfg(feature = "hydrate")]
fn apply_table_fades(el: &web_sys::HtmlDivElement) {
    const FADE_PX: f64 = 24.0;
    let left = el.scroll_left();
    let right = (el.scroll_width() as f64 - el.client_width() as f64 - left).max(0.0);
    let px = |on: bool| {
        if on {
            format!("{FADE_PX}px")
        } else {
            "0px".to_string()
        }
    };
    // Fully qualified for the same reason as the analyzer's chip fades:
    // tachys' `ElementExt::style` wins method resolution over the inherent
    // `HtmlElement::style` on a bare `el.style()` call.
    let style = web_sys::HtmlElement::style(el);
    let _ = style.set_property("--hfade-start", &px(left > 1.0));
    let _ = style.set_property("--hfade-end", &px(right > 1.0));
}

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
    let query = use_query_map();
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
    let (cols_param, set_cols_param) = query_signal::<String>("cols");
    let visible_cols = Memo::new(move |_| parse_visible_cols(cols_param().as_deref()));
    let list_scroll = NodeRef::<leptos::html::Div>::new();
    let item_name = move || item().map(|i| i.name.as_str()).unwrap_or_default();

    // One (getter, setter) per range filter, in RANGE_FILTERS order. The
    // filter *logic* keeps reading the raw query map through `is_in_range`;
    // these signals exist for the chips, bound with `filter_query_signal`
    // (replace: true, scroll: false) so editing a filter neither pushes a
    // history entry per keystroke nor yanks the window back to the top.
    type RangeFilterSignal = (Memo<Option<i32>>, SignalSetter<Option<i32>>);
    let filter_signals: Vec<RangeFilterSignal> = RANGE_FILTERS
        .iter()
        .map(|f| filter_query_signal::<i32>(f.key))
        .collect();
    let filter_signals = StoredValue::new(filter_signals);

    // A filter the user just added from the `+ Filter` menu but hasn't
    // committed yet — its chip mounts in edit state with an empty input.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

    // Filters currently drawn as a chip. Drives the "no active filters"
    // hint and keeps `+ Filter` from offering a second copy of something
    // the user can already see.
    let active_filters = Memo::new(move |_| {
        filter_signals.with_value(|sigs| {
            RANGE_FILTERS
                .iter()
                .zip(sigs)
                .filter(|(f, (get, _))| get.get().is_some() || pending_filter.get() == Some(f.key))
                .map(|(f, _)| f.key)
                .collect::<Vec<_>>()
        })
    });

    // Menu label for a filter: the long, explanatory one — the menu is where
    // a filter has to be recognized, not just recalled. The chip reuses the
    // terser comparison-shaped label.
    let menu_label = move |key: &str| -> String {
        match key {
            "price_per_item_min" => {
                t_string!(i18n, currency_exchange_filter_price_min_label).to_string()
            }
            "price_per_item_max" => {
                t_string!(i18n, currency_exchange_filter_price_max_label).to_string()
            }
            "number_received_min" => {
                t_string!(i18n, currency_exchange_filter_qty_min_label).to_string()
            }
            "number_received_max" => {
                t_string!(i18n, currency_exchange_filter_qty_max_label).to_string()
            }
            "total_profit_min" => {
                t_string!(i18n, currency_exchange_filter_profit_min_label).to_string()
            }
            "total_profit_max" => {
                t_string!(i18n, currency_exchange_filter_profit_max_label).to_string()
            }
            "hours_between_sales_min" => {
                t_string!(i18n, currency_exchange_filter_hours_min_label).to_string()
            }
            "hours_between_sales_max" => {
                t_string!(i18n, currency_exchange_filter_hours_max_label).to_string()
            }
            _ => String::new(),
        }
    };
    let chip_label = move |key: &str| -> String {
        match key {
            "price_per_item_min" => t_string!(i18n, currency_exchange_chip_price_min).to_string(),
            "price_per_item_max" => t_string!(i18n, currency_exchange_chip_price_max).to_string(),
            "number_received_min" => t_string!(i18n, currency_exchange_chip_qty_min).to_string(),
            "number_received_max" => t_string!(i18n, currency_exchange_chip_qty_max).to_string(),
            "total_profit_min" => t_string!(i18n, currency_exchange_chip_profit_min).to_string(),
            "total_profit_max" => t_string!(i18n, currency_exchange_chip_profit_max).to_string(),
            "hours_between_sales_min" => {
                t_string!(i18n, currency_exchange_chip_hours_min).to_string()
            }
            "hours_between_sales_max" => {
                t_string!(i18n, currency_exchange_chip_hours_max).to_string()
            }
            _ => String::new(),
        }
    };

    // What the `+ Filter` menu offers: everything not already on screen.
    let filter_options = Memo::new(move |_| {
        let active = active_filters();
        RANGE_FILTERS
            .iter()
            .filter(|f| !active.contains(&f.key))
            .map(|f| FilterOption {
                id: f.key,
                label: menu_label(f.key),
            })
            .collect::<Vec<_>>()
    });
    let column_options = Memo::new(move |_| {
        vec![
            ColumnOption {
                id: COL_PRICE_PER_ITEM,
                label: t_string!(i18n, currency_exchange_table_price_per_item).to_string(),
            },
            ColumnOption {
                id: COL_SHOPS,
                label: t_string!(i18n, currency_exchange_table_shops).to_string(),
            },
            ColumnOption {
                id: COL_COST,
                label: t_string!(i18n, currency_exchange_table_cost).to_string(),
            },
            ColumnOption {
                id: COL_HOURS,
                label: t_string!(i18n, currency_exchange_table_hours_per_sale).to_string(),
            },
        ]
    });
    let toggle_column = Callback::new(move |col: &'static str| {
        let mut set = visible_cols.get_untracked();
        if set.contains(col) {
            set.remove(col);
        } else {
            set.insert(col);
        }
        set_cols_param.set(Some(serialize_visible_cols(&set)));
    });
    let reset_columns = Callback::new(move |_| set_cols_param.set(None));
    let add_filter = Callback::new(move |key: &'static str| pending_filter.set(Some(key)));
    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        filter_signals.with_value(|sigs| {
            for (_, set) in sigs.iter() {
                set.set(None);
            }
        });
    });

    // Filtered row total, written from inside the Suspense closure where the
    // rows are computed and read by the control bar's summary. Guarded so a
    // re-render with an unchanged count doesn't re-notify the bar.
    let trade_count = RwSignal::new(0usize);

    // --- Table scrollport: edge fades --------------------------------------
    // The table is wider than a phone viewport and scrolls horizontally with
    // no scrollbar to say so; `--hfade-start`/`--hfade-end` drive the
    // `.hscroll-fade` mask so a fade appears on whichever side has more
    // columns. Client-only, same listener-parking shape as the analyzer's
    // chip fades: a forgotten listener keeps firing after disposal, and a
    // `new_local` StoredValue must never exist in an SSR-compiled path.
    #[cfg(feature = "hydrate")]
    {
        use web_sys::wasm_bindgen::JsCast;
        use web_sys::wasm_bindgen::closure::Closure;
        let fade_listeners = StoredValue::new_local(
            None::<(
                web_sys::HtmlDivElement,
                Closure<dyn FnMut()>,
                Closure<dyn FnMut()>,
            )>,
        );
        on_cleanup(move || {
            fade_listeners.update_value(|slot| {
                if let Some((el, scroll_cb, resize_cb)) = slot.take() {
                    let _ = el.remove_event_listener_with_callback(
                        "scroll",
                        scroll_cb.as_ref().unchecked_ref(),
                    );
                    if let Some(win) = web_sys::window() {
                        let _ = win.remove_event_listener_with_callback(
                            "resize",
                            resize_cb.as_ref().unchecked_ref(),
                        );
                    }
                }
            });
        });
        Effect::new(move |_| {
            // Tracked: toggling a column changes scrollWidth without a
            // scroll or resize event firing.
            let _ = visible_cols.get();
            let Some(el) = list_scroll.get() else {
                return;
            };
            apply_table_fades(&el);
            if fade_listeners.with_value(|slot| slot.is_some()) {
                return;
            }
            let on_scroll = {
                let el = el.clone();
                Closure::wrap(Box::new(move || apply_table_fades(&el)) as Box<dyn FnMut()>)
            };
            let on_resize = {
                let el = el.clone();
                Closure::wrap(Box::new(move || apply_table_fades(&el)) as Box<dyn FnMut()>)
            };
            let _ =
                el.add_event_listener_with_callback("scroll", on_scroll.as_ref().unchecked_ref());
            if let Some(win) = web_sys::window() {
                let _ = win
                    .add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
            }
            fade_listeners.set_value(Some((el, on_scroll, on_resize)));
        });
    }

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
            <ToolHeader
                title=format!("{} — {}", item_name(), t_string!(i18n, currency_exchange_title))
                summary=t_string!(i18n, currency_exchange_tool_summary).to_string()
                context=t_string!(i18n, currency_exchange_tool_context).to_string()
                help_href="/help"
                help_body=t_string!(i18n, currency_exchange_tool_help).to_string()
            />
            <div class="flex flex-row justify-end items-center gap-3 my-3">
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
            </div>
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, currency_exchange_trade_count, n = move || trade_count.get())}
                        </span>
                    }
                    .into_any()
                }
                columns=Signal::derive(column_options)
                visible_columns=Signal::derive(move || visible_cols.get())
                on_toggle_column=toggle_column
                on_reset_columns=reset_columns
                available_filters=Signal::derive(filter_options)
                on_add_filter=add_filter
                on_clear_all=clear_all
                empty_label=Signal::derive(move || {
                    t_string!(i18n, currency_exchange_no_filters_hint).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    filter_signals
                        .with_value(|sigs| {
                            RANGE_FILTERS
                                .iter()
                                .zip(sigs.iter().copied())
                                .filter(|(f, (get, _))| {
                                    get.get().is_some() || pending_filter.get() == Some(f.key)
                                })
                                .map(|(f, (get, set))| {
                                    let key = f.key;
                                    let value = Signal::derive(move || {
                                        get.get().map(|v| v.to_string())
                                    });
                                    let start_editing =
                                        pending_filter.get_untracked() == Some(key);
                                    let on_commit = Callback::new(move |v: Option<String>| {
                                        set.set(v.and_then(|v| v.parse::<i32>().ok()));
                                        if pending_filter.get_untracked() == Some(key) {
                                            pending_filter.set(None);
                                        }
                                    });
                                    // `min` is an `into`-String prop, so "no floor"
                                    // has to omit the prop rather than pass None.
                                    match f.min {
                                        Some(m) => Either::Left(view! {
                                            <FilterChip
                                                label=chip_label(key)
                                                value=value
                                                numeric=true
                                                min=m
                                                start_editing=start_editing
                                                on_commit=on_commit
                                            />
                                        }),
                                        None => Either::Right(view! {
                                            <FilterChip
                                                label=chip_label(key)
                                                value=value
                                                numeric=true
                                                start_editing=start_editing
                                                on_commit=on_commit
                                            />
                                        }),
                                    }
                                })
                                .collect_view()
                        })
                }}
            </ControlBar>
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
                                <Suspense fallback=move || {
                                    view! {
                                        <TableSkeleton
                                            columns=skeleton_columns(&visible_cols.get())
                                            rows=10
                                        />
                                    }
                                }>
                                    {move || {
                                    let s_res = s_getter_2.get();
                                    let l_res = l_getter_2.get();
                                    let s = s_res.as_ref().and_then(|r| r.as_ref().ok());
                                    let l = l_res.as_ref().and_then(|r| r.as_ref().ok());
                                    let q = currency_quantity.get();
                                    compute_prices(s, l, q)
                                        .map(|p: Vec<CurrencyTrade>| {
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
                                                let mode = sort_mode.get().unwrap_or_else(SortMode::fallback);
                                                let dir = sort_dir.get().unwrap_or_else(|| mode.default_dir());
                                                sort_trades(&mut p, mode, dir);
                                                // Feed the control bar's "N trades" summary. Guarded
                                                // so a re-render with an unchanged count doesn't
                                                // re-notify the bar.
                                                if trade_count.get_untracked() != p.len() {
                                                    trade_count.set(p.len());
                                                }
                                                // Filters that matched nothing get a message and a
                                                // way out, not a silently empty body.
                                                if p.is_empty() && !active_filters.get().is_empty() {
                                                    return view! {
                                                        <tr>
                                                            <td colspan="7" class="px-3 py-8 text-center text-[color:var(--color-text-muted)]">
                                                                <div class="flex flex-col items-center gap-2">
                                                                    {t!(i18n, currency_exchange_no_matches)}
                                                                    <button class="btn-secondary" on:click=move |_| clear_all.run(())>
                                                                        {t!(i18n, currency_exchange_clear_all)}
                                                                    </button>
                                                                </div>
                                                            </td>
                                                        </tr>
                                                    }
                                                    .into_any();
                                                }
                                                let visible = visible_cols.get();
                                                p.into_iter()
                                                    .map(|p| {
                                                        view! {
                                                            <tr class="hover:bg-white/5 transition-colors">
                                                                <td class="px-3 py-2">
                                                                    <ItemAmount item_amount=p.receive_item />
                                                                </td>
                                                                <td class="px-3 py-2 text-right tabular-nums">{p.number_received}</td>
                                                                <td class="px-3 py-2 text-right tabular-nums font-medium text-[color:var(--color-text)]">
                                                                    {p.total_profit}
                                                                </td>
                                                                {visible.contains(COL_PRICE_PER_ITEM).then(|| view! {
                                                                    <td class="px-3 py-2 text-right tabular-nums">{p.price_per_item}</td>
                                                                })}
                                                                {visible.contains(COL_SHOPS).then(|| view! {
                                                                    <td class="px-3 py-2 text-[color:var(--color-text-muted)]">
                                                                        <ShopNames shop_names=p.shop_names.clone() />
                                                                    </td>
                                                                })}
                                                                {visible.contains(COL_COST).then(|| view! {
                                                                    <td class="px-3 py-2">
                                                                        <ItemAmount item_amount=p.cost_item />
                                                                    </td>
                                                                })}
                                                                {visible.contains(COL_HOURS).then(|| view! {
                                                                    <td class="px-3 py-2 text-right tabular-nums text-[color:var(--color-text-muted)]">
                                                                        {p.hours_between_sales}
                                                                    </td>
                                                                })}
                                                            </tr>
                                                        }
                                                    })
                                                    .collect_view()
                                                    .into_any()
                                            };
                                            view! {
                                                // Only the table scrolls sideways on narrow
                                                // viewports; the surrounding panel must not, or
                                                // `overflow-x` would force `overflow-y: auto` and
                                                // trap anything absolutely positioned inside it.
                                                <div class="overflow-x-auto hscroll-fade" node_ref=list_scroll>
                                                <table class="w-full text-sm text-left">
                                                    <thead class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                                        <tr class="border-b border-white/5">
                                                            <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                {t!(i18n, currency_exchange_table_item)}
                                                            </th>
                                                            <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                <div class="flex justify-end">
                                                                    <SortHeader
                                                                        mode=SortMode::QtyReceived
                                                                        label=t_string!(i18n, currency_exchange_table_qty_recv).to_string()
                                                                        sort_mode=sort_mode
                                                                        sort_dir=sort_dir
                                                                    />
                                                                </div>
                                                            </th>
                                                            <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                <div class="flex justify-end">
                                                                    <SortHeader
                                                                        mode=SortMode::Profit
                                                                        label=t_string!(i18n, currency_exchange_table_profit).to_string()
                                                                        sort_mode=sort_mode
                                                                        sort_dir=sort_dir
                                                                    />
                                                                </div>
                                                            </th>
                                                            {move || visible_cols.get().contains(COL_PRICE_PER_ITEM).then(|| view! {
                                                                <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                    <div class="flex justify-end">
                                                                        <SortHeader
                                                                            mode=SortMode::PricePerItem
                                                                            label=t_string!(i18n, currency_exchange_table_price_per_item).to_string()
                                                                            sort_mode=sort_mode
                                                                            sort_dir=sort_dir
                                                                        />
                                                                    </div>
                                                                </th>
                                                            })}
                                                            {move || visible_cols.get().contains(COL_SHOPS).then(|| view! {
                                                                <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                    {t!(i18n, currency_exchange_table_shops)}
                                                                </th>
                                                            })}
                                                            {move || visible_cols.get().contains(COL_COST).then(|| view! {
                                                                <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                    {t!(i18n, currency_exchange_table_cost)}
                                                                </th>
                                                            })}
                                                            {move || visible_cols.get().contains(COL_HOURS).then(|| view! {
                                                                <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                                                                    <div class="flex justify-end">
                                                                        <SortHeader
                                                                            mode=SortMode::HoursBetweenSales
                                                                            label=t_string!(i18n, currency_exchange_table_hours_per_sale).to_string()
                                                                            sort_mode=sort_mode
                                                                            sort_dir=sort_dir
                                                                        />
                                                                    </div>
                                                                </th>
                                                            })}
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

#[derive(Clone)]
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
    view! {
        <div class="app-inline-ad">
            <Ad class="w-full h-[100px]" />
        </div>
        <div class="main-content">
            <Outlet />
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `?cols=` is a URL contract: absent means the default set, an explicit
    /// value (even empty) is honored exactly, and unknown tokens are dropped
    /// rather than erroring — same semantics as the flip finder's.
    #[test]
    fn cols_param_round_trips() {
        let all: std::collections::HashSet<_> = ALL_OPTIONAL_COLS.iter().copied().collect();
        assert_eq!(
            parse_visible_cols(None),
            all,
            "absent ?cols= means defaults, and all four default on"
        );
        assert_eq!(
            parse_visible_cols(Some("")),
            std::collections::HashSet::new(),
            "explicit empty set is respected"
        );
        let mut some = std::collections::HashSet::new();
        some.insert(COL_SHOPS);
        some.insert(COL_HOURS);
        assert_eq!(
            parse_visible_cols(Some(&serialize_visible_cols(&some))),
            some
        );
        assert_eq!(
            parse_visible_cols(Some("shops,bogus,hours_between_sales")),
            some,
            "unknown tokens are dropped"
        );
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

    /// `RANGE_FILTERS` drives the chips, the `+ Filter` menu, and Clear-all,
    /// and its keys are a URL contract: they must stay exactly the names the
    /// pre-kit page wrote, or every bookmarked filter deep link silently
    /// stops filtering. `is_in_range` reads these same `{key}` names off the
    /// raw query map, so a drifted key would also detach a chip from the
    /// filtering it claims to do.
    #[test]
    fn range_filter_keys_are_a_stable_url_contract() {
        let keys: Vec<&str> = RANGE_FILTERS.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            [
                "price_per_item_min",
                "price_per_item_max",
                "number_received_min",
                "number_received_max",
                "total_profit_min",
                "total_profit_max",
                "hours_between_sales_min",
                "hours_between_sales_max",
            ]
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
