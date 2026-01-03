use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_to_list::AddToList, clipboard::*, filter_card::*, gil::*, icon::Icon, item_icon::*,
        meta::*, query_button::QueryButton, skeleton::BoxSkeleton, toggle::Toggle,
        virtual_scroller::*, world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
};
use chrono::{Duration, Utc};
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_meta::Title;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_params_map, use_query_map},
};
use std::{cmp::Reverse, collections::HashMap, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    world_helper::WorldHelper,
};
use xiv_gen::ItemId;

/// Computed sale stats
#[derive(Hash, Clone, Debug, PartialEq)]
struct SaleSummary {
    item_id: i32,
    hq: bool,
    /// this value is limited by the summary returned by the API
    num_sold: usize,
    /// Represents the average time between sales within the `num_sold`
    avg_sale_duration: Option<Duration>,
    max_price: i32,
    avg_price: i32,
    min_price: i32,
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
struct VendorProfitKey {
    item_id: i32,
    hq: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct VendorProfitData {
    item_id: i32,
    vendor_price: i32,
    market_price: i32,
    sale_summary: Option<SaleSummary>,
}

#[derive(Clone, Debug, PartialEq)]
struct CalculatedVendorProfitData {
    inner: Arc<VendorProfitData>,
    profit: i32,
    return_on_investment: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
}

#[derive(Clone, Debug)]
struct VendorProfitTable(Vec<Arc<VendorProfitData>>);

fn compute_summary(sale: SaleData) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;
    let min_price = sales
        .iter()
        .map(|price| price.price_per_unit)
        .min()
        .unwrap_or_default();
    let max_price = sales
        .iter()
        .map(|price| price.price_per_unit)
        .max()
        .unwrap_or_default();
    let avg_price = (sales
        .iter()
        .map(|price| price.price_per_unit as i64)
        .sum::<i64>()
        / sales.len() as i64) as i32;
    let t = sales
        .last()
        .map(|last| (last.sale_date - now).num_milliseconds().abs() / sales.len() as i64);
    let avg_sale_duration = t.map(Duration::milliseconds);
    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        max_price,
        avg_price,
        min_price,
    }
}

// Add FromStr and ToString implementations for SortMode
impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
        };
        f.write_str(val)
    }
}

impl VendorProfitTable {
    fn new(sales: RecentSales, world_cheapest_listings: CheapestListings) -> Self {
        let data = xiv_gen_db::data();

        // Build map of vendor items: ItemId -> VendorPrice
        // We only care about base items, HQ doesn't exist for vendors usually (or is same price)
        let mut vendor_prices = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if let Some(item_def) = data.items.get(&shop_item.item) {
                    vendor_prices.insert(shop_item.item.0, item_def.price_mid as i32);
                }
            }
        }

        let mut sales_map: HashMap<VendorProfitKey, SaleData> = HashMap::new();
        for sale in sales.sales {
            sales_map.insert(
                VendorProfitKey {
                    item_id: sale.item_id,
                    hq: sale.hq,
                },
                sale,
            );
        }

        let mut table = Vec::new();

        for listing in world_cheapest_listings.cheapest_listings {
            if let Some(&vendor_price) = vendor_prices.get(&listing.item_id) {
                // If the item is sold by a vendor
                // Note: Vendor items are always NQ when bought, but can be sold as NQ.
                // If listing is HQ, we can compare, but usually vendor resale is NQ -> NQ.
                // However, sometimes people buy NQ from vendor and sell as HQ? No, that's crafting.
                // We strictly look for Vendor -> Market.
                // If the market listing is HQ, we shouldn't compare directly unless we want to compete with HQ?
                // Usually vendor resale competes with NQ.
                // Let's filter to only NQ listings for simplicity and correctness,
                // OR we can include HQ listings if the user wants to see if they can undercut HQ with NQ (unlikely to work well).
                // "Flip Finder" logic usually matches HQ to HQ.
                // Vendor items are NQ. So we should compare with NQ market prices.

                if listing.hq {
                    continue;
                }

                let sale_summary = sales_map
                    .remove(&VendorProfitKey {
                        item_id: listing.item_id,
                        hq: false,
                    })
                    .map(compute_summary);

                table.push(Arc::new(VendorProfitData {
                    item_id: listing.item_id,
                    vendor_price,
                    market_price: listing.cheapest_price,
                    sale_summary,
                }));
            }
        }

        VendorProfitTable(table)
    }
}

#[component]
fn VendorResaleTable(
    sales: RecentSales,
    world_cheapest_listings: CheapestListings,
    _worlds: Arc<WorldHelper>,
    world: Signal<String>,
) -> impl IntoView {
    let profits = VendorProfitTable::new(sales, world_cheapest_listings);

    let items = &xiv_gen_db::data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (max_predicted_time, set_max_predicted_time) = query_signal::<String>("next-sale");
    let (tax_enabled, set_tax_enabled) = query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = query_signal::<usize>("sales");
    let (category_filter, set_category_filter) = query_signal::<i32>("category");

    let predicted_time =
        Memo::new(move |_| max_predicted_time().and_then(|d| parse_duration(d.as_str()).ok()));
    let predicted_time_string = Memo::new(move |_| {
        predicted_time()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or("---".to_string())
    });

    let sorted_data = Memo::new(move |_| {
        let include_tax = tax_enabled().unwrap_or(true);
        let mut sorted_data = profits
            .0
            .iter()
            .map(|data| {
                let estimated_revenue = if include_tax {
                    (data.market_price as f32 * 0.95) as i32
                } else {
                    data.market_price
                };
                let profit = estimated_revenue - data.vendor_price;
                let return_on_investment = if data.vendor_price > 0 {
                    ((profit as f32 / data.vendor_price as f32) * 100.0) as i32
                } else {
                    0
                };
                CalculatedVendorProfitData {
                    inner: data.clone(),
                    profit,
                    return_on_investment,
                }
            })
            .filter(move |data| {
                minimum_profit()
                    .map(|min| data.profit > min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_roi()
                    .map(|roi| data.return_on_investment > roi)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_sales()
                    .map(|sales| {
                        data.inner
                            .sale_summary
                            .as_ref()
                            .map(|s| s.num_sold >= sales)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                category_filter()
                    .map(|cat_id| {
                        items
                            .get(&ItemId(data.inner.item_id))
                            .map(|item| item.item_search_category.0 == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                predicted_time()
                    .map(|time| {
                        data.inner
                            .sale_summary
                            .as_ref()
                            .and_then(|s| s.avg_sale_duration)
                            .map(|dur| dur.to_std().ok().map(|dur| dur < time).unwrap_or(false))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        match sort_mode().unwrap_or(SortMode::Roi) {
            SortMode::Roi => sorted_data.sort_by_key(|data| Reverse(data.return_on_investment)),
            SortMode::Profit => sorted_data.sort_by_key(|data| Reverse(data.profit)),
        }
        sorted_data
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, CalculatedVendorProfitData)>>()
    });

    view! {
        <div class="flex flex-col gap-6">
            <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
                <FilterCard
                    title="Minimum Profit"
                    description="Set the minimum profit margin you want to see"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_profit()
                                    .map(|profit| Either::Left(view! { <Gil amount=profit /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=100000
                            step=1000
                            placeholder="e.g. 10000"
                            type="number"
                            prop:value=minimum_profit
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(profit) = value.parse::<i32>() {
                                    set_minimum_profit(Some(profit))
                                } else if value.is_empty() {
                                    set_minimum_profit(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Item Category"
                    description="Filter by item category"
                >
                    <div class="flex flex-col gap-2">
                         <select
                            class="input"
                            on:change=move |ev| {
                                let val = event_target_value(&ev);
                                if let Ok(id) = val.parse::<i32>() {
                                    set_category_filter(Some(id));
                                } else {
                                    set_category_filter(None);
                                }
                            }
                            prop:value=move || category_filter().map(|c| c.to_string()).unwrap_or_default()
                        >
                            <option value="">"All Categories"</option>
                            {
                                let mut categories = xiv_gen_db::data().item_search_categorys
                                    .iter()
                                    .map(|(id, cat)| (id.0, cat.name.clone()))
                                    .collect::<Vec<_>>();
                                categories.sort_by(|a, b| a.1.cmp(&b.1));
                                categories.into_iter().map(|(id, name)| {
                                    view! { <option value=id.to_string() selected=move || category_filter() == Some(id)>{name}</option> }
                                }).collect_view()
                            }
                        </select>
                    </div>
                </FilterCard>

                <FilterCard
                    title="Minimum Sales"
                    description="Filter by minimum number of recent sales"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_sales()
                                    .map(|sales| format!("{} sales", sales))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=1000
                            step=1
                            placeholder="e.g. 5"
                            type="number"
                            prop:value=minimum_sales
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(sales) = value.parse::<usize>() {
                                    set_minimum_sales(Some(sales));
                                } else if value.is_empty() {
                                    set_minimum_sales(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Minimum ROI"
                    description="Set the minimum return on investment percentage"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_roi()
                                    .map(|roi| format!("{roi}%"))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=100000
                            step=10
                            placeholder="e.g. 50"
                            type="number"
                            prop:value=minimum_roi
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(roi) = value.parse::<i32>() {
                                    set_minimum_roi(Some(roi));
                                } else if value.is_empty() {
                                    set_minimum_roi(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Sale Time Prediction"
                    description="Filter by predicted time to next sale (e.g., 1w 30m)"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">{predicted_time_string}</div>
                        <input
                            class="input"
                            placeholder="e.g. 7d 12h"
                            title="Accepts formats like 1h 30m, 7d, 1M (month), etc."
                            prop:value=move || max_predicted_time().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_max_predicted_time(Some(value))
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Tax Calculation"
                    description="Include 5% market tax in profit calculations"
                >
                    <div class="flex items-center">
                        <Toggle
                            checked=Signal::derive(move || tax_enabled().unwrap_or(true))
                            set_checked=SignalSetter::map(move |val: bool| set_tax_enabled(Some(val)))
                            checked_label=Oco::Borrowed("Tax enabled (5%)")
                            unchecked_label=Oco::Borrowed("Tax disabled")
                        />
                    </div>
                </FilterCard>
            </div>

            // Results summary
            <div class="panel px-4 py-3 flex flex-col md:flex-row md:items-center gap-3 md:gap-0 md:justify-between">
                <div class="text-sm text-[color:var(--color-text)]">
                    <span class="text-brand-300 font-semibold">{move || sorted_data().len()}</span> " results"
                </div>
                <div class="flex flex-wrap gap-2">
                    {move || {
                        let mut chips: Vec<_> = Vec::new();
                        if let Some(p) = minimum_profit() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Profit ≥ " <Gil amount=p />
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(cat_id) = category_filter() {
                             let cat_name = xiv_gen_db::data()
                                .item_search_categorys
                                .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| format!("Category {}", cat_id));
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Category: " {cat_name}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_category_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(sales) = minimum_sales() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Sales ≥ " {sales}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_sales(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(roi) = minimum_roi() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "ROI ≥ " {format!("{roi}%")}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_roi(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(_ns) = max_predicted_time() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Next Sale ≤ " {predicted_time_string()}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_predicted_time(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if chips.is_empty() {
                            Either::Left(view! { <span class="text-sm text-[color:var(--color-text-muted)]">"no active filters"</span> })
                        } else {
                            Either::Right(view! { <>{chips}</> })
                        }
                    }}
                </div>
                <button class="text-sm text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] self-start md:self-auto" on:click=move |_| {
                    set_minimum_profit(None);
                    set_minimum_roi(None);
                    set_max_predicted_time(None);
                    set_minimum_sales(None);
                    set_category_filter(None);
                }>
                    "Clear all"
                </button>
            </div>

            // Results table
            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                        viewport_height=720.0
                        row_height=40.0
                        overscan=8
                        header_height=64.0
                        variable_height=false
                        header=view! {
                            <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                                <div role="columnheader" class="w-[40px] p-4 text-center">
                                    "HQ"
                                </div>
                                <div role="columnheader" class="w-84 p-4">
                                    "Item"
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="profit"
                                    >
                                        <div class="flex items-center gap-2">
                                            "Profit"
                                            {move || {
                                                (sort_mode() == Some(SortMode::Profit))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="roi"
                                        default=true
                                    >
                                        <div class="flex items-center gap-2">
                                            "ROI"
                                            {move || {
                                                (sort_mode() == Some(SortMode::Roi))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    "Vendor Price"
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    "Market Price"
                                </div>
                                <div role="columnheader" class="w-30 p-4 hidden md:block">
                                    "Avg Sale Time"
                                </div>
                            </div>
                        }.into_any()
                        each=sorted_data.into()
                        key=move |(index, data): &(usize, CalculatedVendorProfitData)| (
                            *index,
                            data.inner.item_id,
                            data.profit,
                        )
                        view=move |(index, data): (usize, CalculatedVendorProfitData)| {
                            let world = Signal::derive(move || world().to_string());
                            let item_id = data.inner.item_id;
                            let item = items
                                .get(&ItemId(item_id))
                                .map(|item| item.name.as_str())
                                .unwrap_or_default();
                            let icon_loading = if index < 20 { "eager" } else { "" };
                            let classes = if (index % 2) == 0 {
                                "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                            } else {
                                "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                            };
                            let data_clone = data.clone();
                            view! {
                                <div class=classes role="row-group">
                                    <div role="cell" class="px-2 py-2 w-[40px] flex items-center justify-center">
                                        // Vendor items are always NQ effectively
                                    </div>
                                    <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
                                        <a
                                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                            href=format!("/item/{}/{item_id}", world())
                                        >
                                            <div class="shrink-0">
                                                <ItemIcon item_id icon_size=IconSize::Small loading=icon_loading />
                                            </div>
                                            {item}
                                        </a>
                                        <AddToList item_id />
                                        <Clipboard clipboard_text=item.to_string() />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.profit />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <span class={
                                            move || {
                                                let roi = data_clone.return_on_investment;
                                                let tint = if roi >= 500 {
                                                    "24%"
                                                } else if roi >= 200 {
                                                    "20%"
                                                } else if roi >= 100 {
                                                    "16%"
                                                } else if roi >= 50 {
                                                    "12%"
                                                } else {
                                                    "10%"
                                                };
                                                format!(
                                                    "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_{tint},transparent)]"
                                                )
                                            }
                                        }>
                                            {format!("{}%", data.return_on_investment)}
                                        </span>
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.vendor_price />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.market_price />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 truncate hidden md:block flex items-center">
                                        {data.inner
                                            .sale_summary
                                            .as_ref()
                                            .and_then(|s| s.avg_sale_duration)
                                            .and_then(|duration| duration.to_std().ok())
                                            .map(|duration| {
                                                let secs = duration.as_secs();
                                                let days = secs / 86_400;
                                                let hours = (secs % 86_400) / 3_600;
                                                let minutes = (secs % 3_600) / 60;
                                                let seconds = secs % 60;
                                                let mut parts = Vec::new();
                                                if days > 0 { parts.push(format!("{}d", days)); }
                                                if hours > 0 { parts.push(format!("{}h", hours)); }
                                                if minutes > 0 && parts.len() < 2 { parts.push(format!("{}m", minutes)); }
                                                if seconds > 0 && parts.len() < 2 { parts.push(format!("{}s", seconds)); }
                                                if parts.is_empty() { "0s".to_string() } else { parts[..parts.len().min(2)].join(" ") }
                                            })
                                            .unwrap_or_else(|| "---".to_string())}
                                    </div>
                                </div>
                            }
                                .into_any()
                        }
                    />
            </div>
        </div>
    }.into_any()
}

#[component]
pub fn VendorWorldView() -> impl IntoView {
    let params = use_params_map();
    let world = Memo::new(move |_| params.with(|p| p.get("world").clone()).unwrap_or_default());

    // We fetch sales for better estimation, even though we are comparing to vendor prices
    let sales = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(&world).await
        },
    );
    let worlds = use_context::<LocalWorldData>()
        .expect("Worlds should always be populated here")
        .0
        .unwrap();

    view! {
        <div class="main-content p-2 sm:p-6">
            <Title text=move || format!("Vendor Resale - {}", world()) />
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Header Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            "Vendor Resale for " {world}
                        </h1>
                        <div class="flex flex-col gap-4">
                            <MetaTitle title=move || format!("Vendor Resale - {}", world()) />
                            <MetaDescription text=move || {
                                format!(
                                    "Find items sold by vendors that can be resold on the {} marketboard for a profit.",
                                    world(),
                                )
                            } />

                            // World Navigator
                            <div class="flex flex-col md:flex-row gap-4 items-center">
                                <VendorWorldNavigator />
                            </div>

                            // Preset Filters
                            <div class="flex flex-wrap gap-4">
                                <PresetFilterButton
                                    href="?next-sale=7d&roi=100&profit=1000&sort=profit&"
                                    label="100% ROI - 1000 gil profit"
                                />
                                <PresetFilterButton
                                    href="?next-sale=1M&roi=500&profit=5000&"
                                    label="500% ROI - 5000 gil profit"
                                />
                                <PresetFilterButton href="?profit=50000" label="50K+ profit" />
                            </div>
                        </div>
                    </div>

                    // Main Content
                    <div class="min-h-screen">
                        <Suspense fallback=BoxSkeleton>
                            {move || {
                                let world_cheapest = world_cheapest_listings.get();
                                let sales = sales.get();
                                let worlds = worlds.clone();

                                match (world_cheapest, sales) {
                                    (Some(Ok(w)), Some(Ok(s))) => {
                                        Either::Left(
                                            view! {
                                                <VendorResaleTable
                                                    sales=s
                                                    world_cheapest_listings=w
                                                    _worlds=worlds
                                                    world=world.into()
                                                />
                                            },
                                        )
                                    }
                                    _ => {
                                        Either::Right(
                                            view! {
                                                <div class="text-xl text-[color:var(--color-text)] text-center p-8
                                                bg-brand-900/20 rounded-2xl border border-white/10">
                                                    "Loading data..."
                                                </div>
                                            },
                                        )
                                    }
                                }
                            }}
                        </Suspense>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn PresetFilterButton(href: &'static str, label: &'static str) -> impl IntoView {
    view! {
        <a
            href=href
            class="btn-secondary"
        >
            {label}
        </a>
    }
}

#[component]
fn VendorWorldNavigator() -> impl IntoView {
    let nav = use_navigate();
    let params = use_params_map();
    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();

    let initial_world = params.with_untracked(|p| {
        let world = p.get_str("world").unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });

    let (current_world, set_current_world) = signal(initial_world);
    let query = use_query_map();

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            let query_map = query.get_untracked();
            let query = query_map.to_query_string();
            nav(
                &format!("/vendor-resale/{world}?{query}"),
                NavigateOptions::default(),
            );
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">"Select World:"</label>
            <div class="w-full md:w-auto">
                <WorldOnlyPicker
                    current_world=current_world.into()
                    set_current_world=set_current_world.into()
                />
            </div>
        </div>
    }
}

#[component]
pub fn VendorResale() -> impl IntoView {
    view! {
        <MetaTitle title="Vendor Resale - Ultros" />
        <MetaDescription text="Find items sold by vendors that can be resold on the marketboard for a profit." />

        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Hero Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            "Vendor Resale Tool"
                        </h1>
                        <p class="text-xl text-[color:var(--color-text)] leading-relaxed mb-6">
                            "This tool helps you find items that are sold by NPCs (vendors) for less than they are currently selling for on the market board.
                             Buy low from vendors, sell high to players!"
                        </p>
                        <p class="text-lg text-[color:var(--color-text)]/90 mb-8">
                            "Select your world below to see profitable vendor resale opportunities."
                        </p>

                        // World Selection
                        <div class="panel p-6 rounded-xl">
                            <h2 class="text-xl font-semibold text-[color:var(--brand-fg)] mb-4">
                                "Choose a world to get started:"
                            </h2>
                            <VendorWorldNavigator />
                        </div>
                    </div>

                    // Features Grid
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">"Arbitrage"</h3>
                            <p class="text-gray-300">
                                "Profit from price differences between NPC vendors and the market board."
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaShopSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">"Vendor Data"</h3>
                            <p class="text-gray-300">
                                "Automatically identifies items sold by vendors."
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaFilterSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">"Filters"</h3>
                            <p class="text-gray-300">
                                "Filter by profit, ROI, and sales velocity to find safe bets."
                            </p>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
