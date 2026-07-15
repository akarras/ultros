use crate::analysis::{SaleSummary, format_duration_short, roi_badge_class};
use crate::global_state::xiv_data::tracked_data;
use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_to_list::AddToList,
        clipboard::*,
        gil::*,
        icon::Icon,
        item_icon::*,
        meta::*,
        query_button::QueryButton,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills, ToolbarSpacer},
        virtual_scroller::*,
        world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
    i18n::*,
    ws::realtime::use_realtime,
};
use chrono::{Duration, Utc};
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{either::Either, prelude::*};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_params_map, use_query_map},
};
use std::{cmp::Reverse, collections::HashMap, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
};
use xiv_gen::ItemId;

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
    let days_since_last_sale = sales
        .first()
        .map(|first| Duration::milliseconds((now - first.sale_date).num_milliseconds().max(0)));
    let mut prices = sales
        .iter()
        .map(|price| price.price_per_unit)
        .collect::<Vec<_>>();
    // ⚡ Bolt: Optimization: Use select_nth_unstable instead of sort_unstable for median calculation.
    let median_price = match prices.as_mut_slice() {
        [] => 0,
        values if values.len() % 2 == 1 => {
            let len = values.len();
            let (_, &mut median, _) = values.select_nth_unstable(len / 2);
            median
        }
        values => {
            let mid = values.len() / 2;
            let (left, &mut mid_val, _) = values.select_nth_unstable(mid);
            let mid_left_val = *left.iter().max().unwrap();
            ((mid_val as i64 + mid_left_val as i64) / 2) as i32
        }
    };
    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        days_since_last_sale,
        max_price,
        avg_price,
        median_price,
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
        let data = tracked_data();

        // Build map of vendor items: ItemId -> VendorPrice
        // We only care about base items, HQ doesn't exist for vendors usually (or is same price)
        let mut vendor_prices = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if let Some(item_def) = data.items.get(&ItemId(shop_item.item)) {
                    vendor_prices.insert(shop_item.item, item_def.price_mid as i32);
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
    world: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let realtime = use_realtime();
    let rt_status = realtime.clone();
    let realtime_status = Signal::derive(move || {
        rt_status
            .as_ref()
            .map(|r| r.status.get())
            .unwrap_or_else(|| "offline".to_string())
    });
    let rt_update = realtime;
    let last_update = Signal::derive(move || rt_update.as_ref().and_then(|r| r.last_update.get()));
    let profits = VendorProfitTable::new(sales, world_cheapest_listings);

    let items = &tracked_data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (max_predicted_time, set_max_predicted_time) = query_signal::<String>("next-sale");
    let (tax_enabled, set_tax_enabled) = query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = query_signal::<usize>("sales");
    let (category_filter, set_category_filter) = query_signal::<i32>("category");
    let show_more = RwSignal::new(false);

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
                            .map(|item| item.item_search_category == cat_id)
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
            // Primary filter toolbar
            <Toolbar>
                <ToolbarField label=t_string!(i18n, vendor_resale_filter_profit_min_label).to_string()>
                    <input
                        class="input input-sm w-32"
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
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, vendor_resale_filter_roi_min_label).to_string()>
                    <input
                        class="input input-sm w-28"
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
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, vendor_resale_filter_sales_min_label).to_string()>
                    <input
                        class="input input-sm w-24"
                        min=0
                        max=6
                        step=1
                        placeholder="0–6"
                        title=t_string!(i18n, analyzer_tooltip_sales_min)
                        type="number"
                        prop:value=minimum_sales
                        on:input=move |input| {
                            let value = event_target_value(&input);
                            if let Ok(sales) = value.parse::<usize>() {
                                set_minimum_sales(Some(sales.min(6)));
                            } else if value.is_empty() {
                                set_minimum_sales(None);
                            }
                        }
                    />
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, vendor_resale_filter_category_label).to_string()>
                    <select
                        class="input input-sm w-48"
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
                        <option value="">{t!(i18n, vendor_resale_all_categories)}</option>
                        {
                            let mut categories = tracked_data().item_search_categorys
                                .iter()
                                .filter(|(_, cat)| !cat.name.is_empty())
                                .map(|(id, cat)| (id.0, cat.name.clone()))
                                .collect::<Vec<_>>();
                            categories.sort_by(|a, b| a.1.cmp(&b.1));
                            categories.into_iter().map(|(id, name)| {
                                view! { <option value=id.to_string() selected=move || category_filter() == Some(id)>{name}</option> }
                            }).collect_view()
                        }
                    </select>
                </ToolbarField>
                <ToolbarField label=t_string!(i18n, vendor_resale_filter_prices_label).to_string()>
                    <ToolbarPills>
                        <button
                            aria-pressed=move || if tax_enabled().unwrap_or(true) { "false" } else { "true" }
                            on:click=move |_| set_tax_enabled(Some(false))
                        >
                            "Pre-tax"
                        </button>
                        <button
                            aria-pressed=move || if tax_enabled().unwrap_or(true) { "true" } else { "false" }
                            on:click=move |_| set_tax_enabled(Some(true))
                        >
                            "Post-tax"
                        </button>
                    </ToolbarPills>
                </ToolbarField>
                <ToolbarSpacer />
                <button
                    class="btn-secondary flex items-center gap-2"
                    on:click=move |_| show_more.update(|v| *v = !*v)
                    aria-expanded=move || show_more.get().to_string()
                >
                    <Icon icon=i::FaFilterSolid />
                    {move || if show_more.get() { "Fewer Filters" } else { "More Filters" }}
                </button>
            </Toolbar>

            // Secondary filter toolbar (expanded)
            {move || show_more.get().then(|| view! {
                <Toolbar>
                    <ToolbarField label=t_string!(i18n, vendor_resale_filter_max_sale_time_label).to_string()>
                        <input
                            class="input input-sm w-32"
                            placeholder="e.g. 7d 12h"
                            title=t_string!(i18n, analyzer_tooltip_duration_format)
                            prop:value=move || max_predicted_time().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_max_predicted_time(Some(value))
                            }
                        />
                    </ToolbarField>
                </Toolbar>
            })}

            // Results summary
            <div class="panel px-4 py-3 flex flex-col md:flex-row md:items-center gap-3 md:gap-0 md:justify-between">
                <div class="text-sm text-[color:var(--color-text)] flex flex-wrap items-center gap-3">
                    <div>
                        <span class="text-brand-300 font-semibold">{move || sorted_data().len()}</span> " " {t!(i18n, vendor_resale_results)}
                    </div>
                    <RealtimeStatus
                        status=realtime_status
                        last_update=last_update
                    />
                </div>
                <div class="flex flex-wrap gap-2">
                    {move || {
                        let mut chips: Vec<_> = Vec::new();
                        if let Some(p) = minimum_profit() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, vendor_resale_profit_gte)} <Gil amount=p />
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(cat_id) = category_filter() {
                             let cat_name = tracked_data()
                                .item_search_categorys
                                .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| format!("Category {}", cat_id));
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, vendor_resale_category_colon)} {cat_name}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_category_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(sales) = minimum_sales() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, vendor_resale_sales_gte)} {sales}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_sales(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(roi) = minimum_roi() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, vendor_resale_roi_gte)} {format!("{roi}%")}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_roi(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(_ns) = max_predicted_time() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, vendor_resale_next_sale_lte)} {predicted_time_string()}
                                    <button aria-label=t_string!(i18n, aria_remove_filter) class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_predicted_time(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if chips.is_empty() {
                            Either::Left(view! { <span class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, vendor_resale_no_active_filters)}</span> })
                        } else {
                            Either::Right(view! { <>{chips}</> })
                        }
                    }}
                </div>
                <button aria-label=t_string!(i18n, aria_clear_all_filters) class="text-sm text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] self-start md:self-auto" on:click=move |_| {
                    set_minimum_profit(None);
                    set_minimum_roi(None);
                    set_max_predicted_time(None);
                    set_minimum_sales(None);
                    set_category_filter(None);
                }>
                    {t!(i18n, vendor_resale_clear_all)}
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
                                    {t!(i18n, vendor_resale_hq)}
                                </div>
                                <div role="columnheader" class="w-84 p-4">
                                    {t!(i18n, vendor_resale_item)}
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="profit"
                                    >
                                        <div class="flex items-center gap-2">
                                            {t!(i18n, vendor_resale_profit)}
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
                                            {t!(i18n, vendor_resale_roi)}
                                            {move || {
                                                (sort_mode() == Some(SortMode::Roi))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    {t!(i18n, vendor_resale_vendor_price)}
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    {t!(i18n, vendor_resale_market_price)}
                                </div>
                                <div role="columnheader" class="w-30 p-4 hidden md:block">
                                    {t!(i18n, vendor_resale_avg_sale_time)}
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
                                        <span class={roi_badge_class(data.return_on_investment)}>
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
                                            .map(|duration| format_duration_short(duration.as_secs()))
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
    let i18n = use_i18n();
    let params = use_params_map();
    let world = Signal::derive(move || params.with(|p| p.get("world").clone()).unwrap_or_default());

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

    view! {
        <div class="main-content p-2 sm:p-6">
            <MetaTitle title=move || format!("{} - {}", t_string!(i18n, vendor_resale_title), world()) />
            <div class="flex flex-col gap-8">
                <ToolHeader
                    title=t_string!(i18n, vendor_resale).to_string()
                    summary=t_string!(i18n, vendor_resale_tool_summary_v2).to_string()
                    context=t_string!(i18n, vendor_resale_tool_context).to_string()
                    help_href="/help/vendor-resale"
                    help_body=t_string!(i18n, vendor_resale_tool_help).to_string()
                />

                // Controls Section
                <div class="panel p-4 sm:p-6 rounded-2xl">
                    <div class="flex flex-col gap-4">
                        <MetaDescription text=move || {
                            t_string!(i18n, vendor_resale_meta_desc).to_string().replace("%world%", &world())
                        } />

                        // World Navigator
                        <div class="flex flex-col md:flex-row gap-4 items-center">
                            <VendorWorldNavigator />
                        </div>

                        // Preset Filters
                        <div class="flex flex-wrap gap-4">
                            <PresetFilterButton
                                href="?next-sale=7d&roi=100&profit=1000&sort=profit&"
                                label=t_string!(i18n, vendor_resale_preset_100_roi).to_string()
                            />
                            <PresetFilterButton
                                href="?next-sale=1M&roi=500&profit=5000&"
                                label=t_string!(i18n, vendor_resale_preset_500_roi).to_string()
                            />
                            <PresetFilterButton href="?profit=50000" label=t_string!(i18n, vendor_resale_preset_50k_profit).to_string() />
                        </div>
                        <CalculationSummary
                            title=t_string!(i18n, vendor_resale_calc_title).to_string()
                            formula=t_string!(i18n, vendor_resale_calc_formula).to_string()
                            details=t_string!(i18n, vendor_resale_calc_details).to_string()
                        />
                        <div class="flex flex-wrap gap-2">
                            <AssumptionBadge text=t_string!(i18n, vendor_resale_assumption_nq_purchase).to_string() />
                            <AssumptionBadge text=t_string!(i18n, vendor_resale_assumption_hq_excluded).to_string() />
                            <AssumptionBadge text=t_string!(i18n, vendor_resale_assumption_no_vendor_names).to_string() />
                        </div>
                    </div>
                </div>

                // Main Content
                <div class="min-h-screen">
                    <Suspense fallback=BoxSkeleton>
                        {move || {
                            let world_cheapest = world_cheapest_listings.get();
                            let sales = sales.get();
                            match (world_cheapest, sales) {
                                (Some(Ok(w)), Some(Ok(s))) => {
                                    Either::Left(
                                        view! {
                                            <VendorResaleTable
                                                sales=s
                                                world_cheapest_listings=w
                                                world=world
                                            />
                                        },
                                    )
                                }
                                _ => {
                                    Either::Right(
                                        view! {
                                            <div class="text-xl text-[color:var(--color-text)] text-center p-8
                                            bg-brand-900/20 rounded-2xl border border-white/10">
                                                {t!(i18n, vendor_resale_loading_data)}
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
    }
}

#[component]
fn PresetFilterButton(href: &'static str, label: String) -> impl IntoView {
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
    let i18n = use_i18n();
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
                NavigateOptions {
                    scroll: false,
                    ..Default::default()
                },
            );
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, vendor_resale_select_world)}</label>
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
    let i18n = use_i18n();
    view! {
        <MetaTitle title=t_string!(i18n, vendor_resale_meta_title_ultros) />
        <MetaDescription text=t_string!(i18n, vendor_resale_meta_desc_default) />

        <div class="main-content p-2 sm:p-6">
            <div class="flex flex-col gap-8">
                // Hero Section
                <div class="panel p-4 sm:p-8 rounded-2xl">
                    <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                        {t!(i18n, vendor_resale_tool_title)}
                    </h1>
                    <p class="text-xl text-[color:var(--color-text)] leading-relaxed mb-6">
                        {t!(i18n, vendor_resale_tool_desc)}
                    </p>
                    <p class="text-lg text-[color:var(--color-text)]/90 mb-8">
                        {t!(i18n, vendor_resale_tool_select_world)}
                    </p>

                    // World Selection
                    <div class="panel p-6 rounded-xl">
                        <h2 class="text-xl font-semibold text-[color:var(--brand-fg)] mb-4">
                            {t!(i18n, vendor_resale_choose_world)}
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
                        <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, vendor_resale_arbitrage)}</h3>
                        <p class="text-gray-300">
                            {t!(i18n, vendor_resale_arbitrage_desc)}
                        </p>
                    </div>

                    <div class="card p-6 rounded-lg transition-colors duration-200">
                        <Icon
                            attr:class="text-brand-300 mb-4"
                            width="2.5em"
                            height="2.5em"
                            icon=i::FaShopSolid
                        />
                        <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, vendor_resale_vendor_data)}</h3>
                        <p class="text-gray-300">
                            {t!(i18n, vendor_resale_vendor_data_desc)}
                        </p>
                    </div>

                    <div class="card p-6 rounded-lg transition-colors duration-200">
                        <Icon
                            attr:class="text-brand-300 mb-4"
                            width="2.5em"
                            height="2.5em"
                            icon=i::FaFilterSolid
                        />
                        <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, vendor_resale_filters)}</h3>
                        <p class="text-gray-300">
                            {t!(i18n, vendor_resale_filters_desc)}
                        </p>
                    </div>
                </div>
            </div>
        </div>
    }
}
