//! Vendor Sell: market listings priced below the NPC vendor sell-back price.
//!
//! Spec: docs/superpowers/specs/2026-09-18-vendor-sell-analyzer-design.md

use super::analyzer::ITEM_COLUMN_WIDTH;
use super::world_nav::use_analyzer_world;
use crate::analyzer_kit::calculation::{Calculation, CalculationStrip, CalculationTerm};
use crate::analyzer_kit::filters::{category_id_token, register_filters};
use crate::analyzer_kit::market::{MarketGrid, MarketSubject, use_market_data};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::term_badge::TermRole;
use crate::components::virtual_grid::metrics::{FilterOp, GridMetric, GridValue};
use crate::components::virtual_grid::registry::FilterAlias;
use crate::components::virtual_grid::saved_views::{
    GridPresetView, GridSavedViews, provide_grid_saved_views,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::{filter_query_signal, query_signal};
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::roi_badge_class,
    api::get_cheapest_listings,
    components::{
        add_to_list::AddToList,
        clipboard::*,
        control_bar::ControlBar,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortableHeaderCell},
        tool_help::*,
        virtual_grid::{ColumnFilter, GridColumn},
        world_picker::WorldOnlyPicker,
    },
    global_state::{LocalWorldData, region_for_world::use_region_for_world},
};
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
use thousands::Separable;
use ultros_api_types::cheapest_listings::CheapestListings;
use ultros_calc::formula::vendor_sell_line;
use xiv_gen::ItemId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VendorSellRow {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub listing: i32,
    pub tax: i32,
    pub cost: i32,
    pub vendor_price: i32,
    pub profit: i32,
    pub margin: i32,
}

/// `item_id -> price_low` for every item a player can both buy on the board
/// and sell to an NPC.
fn vendor_prices_from_data() -> HashMap<i32, i32> {
    tracked_data()
        .items
        .iter()
        .filter(|(_, item)| item.item_search_category != 0 && item.price_low > 0)
        .map(|(id, item)| (id.0, item.price_low as i32))
        .collect()
}

/// Join listings against vendor prices, drop anything that does not profit,
/// and return a deterministic (item id, then NQ before HQ) order.
fn build_rows(
    listings: &CheapestListings,
    vendor_prices: &HashMap<i32, i32>,
) -> Vec<Arc<VendorSellRow>> {
    let mut rows: Vec<Arc<VendorSellRow>> = listings
        .cheapest_listings
        .iter()
        .filter_map(|listing| {
            let vendor_price = *vendor_prices.get(&listing.item_id)?;
            let line = vendor_sell_line(listing.cheapest_price, vendor_price);
            (line.profit > 0).then(|| {
                Arc::new(VendorSellRow {
                    item_id: listing.item_id,
                    hq: listing.hq,
                    world_id: listing.world_id,
                    listing: line.listing,
                    tax: line.tax,
                    cost: line.cost,
                    vendor_price: line.vendor_price,
                    profit: line.profit,
                    margin: line.margin,
                })
            })
        })
        .collect();
    rows.sort_by_key(|row| (row.item_id, row.hq));
    rows
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Margin,
    Listing,
    Tax,
    VendorPrice,
    World,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "margin" => Ok(SortMode::Margin),
            "listing" => Ok(SortMode::Listing),
            "tax" => Ok(SortMode::Tax),
            "vendor-price" => Ok(SortMode::VendorPrice),
            "world" => Ok(SortMode::World),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortMode::Profit => "profit",
            SortMode::Margin => "margin",
            SortMode::Listing => "listing",
            SortMode::Tax => "tax",
            SortMode::VendorPrice => "vendor-price",
            SortMode::World => "world",
        })
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Money columns read best-first descending; the cost-like columns
    /// (listing, tax) and world read ascending.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::Profit | SortMode::Margin | SortMode::VendorPrice => SortDir::Desc,
            SortMode::Listing | SortMode::Tax | SortMode::World => SortDir::Asc,
        }
    }
}

fn compare_rows(mode: SortMode, a: &VendorSellRow, b: &VendorSellRow) -> Ordering {
    match mode {
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Margin => a.margin.cmp(&b.margin),
        SortMode::Listing => a.listing.cmp(&b.listing),
        SortMode::Tax => a.tax.cmp(&b.tax),
        SortMode::VendorPrice => a.vendor_price.cmp(&b.vendor_price),
        SortMode::World => a.world_id.cmp(&b.world_id),
    }
    .then_with(|| a.item_id.cmp(&b.item_id))
    .then_with(|| a.hq.cmp(&b.hq))
}

fn sort_rows(rows: &mut [Arc<VendorSellRow>], mode: SortMode, dir: SortDir) {
    rows.sort_by(|a, b| {
        let order = compare_rows(mode, a, b);
        if dir == SortDir::Asc {
            order
        } else {
            order.reverse()
        }
    });
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract.
const FILTER_PROFIT: &str = "profit";
const FILTER_CATEGORY: &str = "category";

/// Queries only: labels live in [`vendor_sell_presets`] because `t_string!`
/// needs a literal key. Every key used here is pinned by a test below.
const PRESET_QUERIES: [&str; 2] = ["?sort=profit", "?sort=margin"];

fn vendor_sell_presets(i18n: I18nContext<Locale, I18nKeys>) -> Vec<GridPresetView> {
    [
        t_string!(i18n, vendor_sell_preset_best_profit).to_string(),
        t_string!(i18n, vendor_sell_preset_best_margin).to_string(),
    ]
    .into_iter()
    .zip(PRESET_QUERIES)
    .map(|(label, query)| GridPresetView {
        label,
        query: query.to_string(),
    })
    .collect()
}

#[cfg(test)]
const LEGACY_PRESET_FILTER_KEYS: &[&str] = &[FILTER_PROFIT, FILTER_CATEGORY];

/// `world_id -> world name` for the World column.
fn world_names() -> Arc<HashMap<i32, String>> {
    Arc::new(
        use_context::<LocalWorldData>()
            .and_then(|v| v.0.ok())
            .map(|helper| {
                helper
                    .get_inner_data()
                    .regions
                    .iter()
                    .flat_map(|r| r.datacenters.iter())
                    .flat_map(|dc| dc.worlds.iter())
                    .map(|w| (w.id, w.name.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

type Row = (usize, Arc<VendorSellRow>);

fn vendor_sell_metrics(worlds: Arc<HashMap<i32, String>>) -> Vec<GridMetric<Row>> {
    let items = &tracked_data().items;
    vec![
        GridMetric::text("item", move |(_, row): &Row| {
            GridValue::Text(
                items
                    .get(&ItemId(row.item_id))
                    .map(|item| item.name.clone())
                    .unwrap_or_default(),
            )
        }),
        GridMetric::text("hq", |(_, row): &Row| {
            GridValue::Text(if row.hq { "HQ" } else { "NQ" }.into())
        }),
        GridMetric::text("world", move |(_, row): &Row| {
            GridValue::Text(worlds.get(&row.world_id).cloned().unwrap_or_default())
        }),
        GridMetric::number("listing", |(_, row): &Row| {
            GridValue::Number(row.listing as f64)
        }),
        GridMetric::number("tax", |(_, row): &Row| GridValue::Number(row.tax as f64)),
        GridMetric::number("vendor-price", |(_, row): &Row| {
            GridValue::Number(row.vendor_price as f64)
        }),
        GridMetric::number("profit", |(_, row): &Row| {
            GridValue::Number(row.profit as f64)
        }),
        GridMetric::number("margin", |(_, row): &Row| {
            GridValue::Number(row.margin as f64)
        }),
    ]
}

#[component]
fn VendorSellTable(listings: CheapestListings, region: Signal<String>) -> impl IntoView {
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
    let market = use_market_data(region);
    let worlds = world_names();
    let worlds_measure = worlds.clone();
    let worlds_view = worlds.clone();
    let items = &tracked_data().items;

    let all_rows = build_rows(&listings, &vendor_prices_from_data());

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    let (category_filter, _set_category_filter) = filter_query_signal::<i32>(FILTER_CATEGORY);

    let sorted_rows = Memo::new(move |_| {
        let mut rows: Vec<Arc<VendorSellRow>> = all_rows
            .iter()
            .filter(|row| {
                category_filter()
                    .map(|cat_id| {
                        items
                            .get(&ItemId(row.item_id))
                            .map(|item| item.item_search_category == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        sort_rows(&mut rows, mode, dir);
        rows.into_iter().enumerate().collect::<Vec<Row>>()
    });

    let category_options = move || {
        let mut categories = tracked_data()
            .item_search_categorys
            .iter()
            .filter(|(_, cat)| !cat.name.is_empty())
            .map(|(id, cat)| (id.0, cat.name.clone()))
            .collect::<Vec<_>>();
        categories.sort_by(|a, b| a.1.cmp(&b.1));
        categories
            .into_iter()
            .map(|(id, name)| (category_id_token(id), name))
            .collect::<Vec<_>>()
    };

    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, vendor_sell_filter_profit_min_label).to_string(),
            FILTER_CATEGORY => t_string!(i18n, vendor_sell_filter_category_label).to_string(),
            _ => String::new(),
        }
    };

    let filters = register_filters(
        vec![FilterAlias::integer(FILTER_PROFIT, "profit", FilterOp::Gte)],
        Signal::derive(move || {
            vec![
                ColumnFilter::new(FILTER_PROFIT, filter_label(FILTER_PROFIT), true),
                {
                    let mut f =
                        ColumnFilter::new(FILTER_CATEGORY, filter_label(FILTER_CATEGORY), false);
                    f.options = category_options();
                    f
                },
            ]
        }),
    );

    let presets = Signal::derive(move || vendor_sell_presets(i18n));

    let calculation = Calculation::provide(
        filters,
        vec![
            CalculationTerm::fixed(
                TermRole::Result,
                t_string!(i18n, vendor_sell_col_profit).to_string(),
                Some("profit"),
            ),
            CalculationTerm::fixed(
                TermRole::Revenue,
                t_string!(i18n, vendor_sell_col_vendor_price).to_string(),
                Some("vendor-price"),
            ),
            CalculationTerm::fixed(
                TermRole::Cost,
                t_string!(i18n, vendor_sell_col_listing).to_string(),
                Some("listing"),
            ),
            CalculationTerm::fixed(
                TermRole::Tax,
                t_string!(i18n, vendor_sell_col_tax).to_string(),
                Some("tax"),
            ),
        ],
        None,
    );

    let sorted = move |m: SortMode| {
        (
            sort_mode.get().unwrap_or_else(SortMode::fallback) == m,
            sort_dir.get().unwrap_or_else(|| m.default_dir()) == SortDir::Asc,
        )
    };
    let hq_badge_class = "px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]";

    view! {
        <div class="flex flex-col gap-6">
            <div class="flex flex-wrap items-start gap-3">
                <CalculationStrip calculation window=market.window />
            </div>

            <ControlBar sticky=false
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, vendor_sell_result_count, n = move || filters.row_count())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! {
                        <RealtimeStatus status=realtime_status last_update=last_update />
                        <GridSavedViews id="vendor-sell-grid" presets=presets />
                    }
                    .into_any()
                }
                empty_label=Signal::derive(move || {
                    t_string!(i18n, vendor_sell_no_filters_hint).to_string()
                })
            />

            <div>
                <MarketGrid show_saved_views=false id="vendor-sell-grid"
                    label=t_string!(i18n, vendor_sell_col_item).to_string()
                    row_height=40.0
                    market
                    metrics=vendor_sell_metrics(worlds.clone())
                    subject=Arc::new(move |(_, row): &Row| {
                        let mut subject = MarketSubject::new(row.item_id, row.hq, row.world_id);
                        subject.listing_price = Some(row.listing);
                        subject
                    })
                    columns=Signal::derive(move || {
                        let (profit_on, profit_asc) = sorted(SortMode::Profit);
                        let (margin_on, margin_asc) = sorted(SortMode::Margin);
                        let (listing_on, listing_asc) = sorted(SortMode::Listing);
                        let (tax_on, tax_asc) = sorted(SortMode::Tax);
                        let (vendor_on, vendor_asc) = sorted(SortMode::VendorPrice);
                        let (world_on, world_asc) = sorted(SortMode::World);
                        vec![
                            GridColumn::new("hq", t_string!(i18n, vendor_sell_col_hq).to_string(), 60.0, true, true),
                            // Same fixed width as Flip Finder: item names run long, so
                            // auto-fitting this column shoved the numbers off screen.
                            GridColumn::new("item", t_string!(i18n, vendor_sell_col_item).to_string(), ITEM_COLUMN_WIDTH, false, true).fixed_width(),
                            GridColumn::new("world", t_string!(i18n, vendor_sell_col_world).to_string(), 130.0, false, true).sorted(world_on, world_asc),
                            GridColumn::new("listing", t_string!(i18n, vendor_sell_col_listing).to_string(), 120.0, true, true).sorted(listing_on, listing_asc),
                            GridColumn::new("tax", t_string!(i18n, vendor_sell_col_tax).to_string(), 90.0, true, true).sorted(tax_on, tax_asc),
                            GridColumn::new("vendor-price", t_string!(i18n, vendor_sell_col_vendor_price).to_string(), 130.0, true, true).sorted(vendor_on, vendor_asc),
                            {
                                let mut col = GridColumn::new("profit", t_string!(i18n, vendor_sell_col_profit).to_string(), 130.0, true, true).sorted(profit_on, profit_asc);
                                col.filters.push(ColumnFilter::new(FILTER_PROFIT, filter_label(FILTER_PROFIT), true));
                                col
                            },
                            GridColumn::new("margin", t_string!(i18n, vendor_sell_col_margin).to_string(), 100.0, true, true).sorted(margin_on, margin_asc),
                        ]
                    })
                    header=move |id| {
                        match id {
                            "hq" => view! { <div class="text-center w-full min-w-0">{t!(i18n, vendor_sell_col_hq)}</div> }.into_any(),
                            "item" => view! { <div class="w-full min-w-0">{t!(i18n, vendor_sell_col_item)}</div> }.into_any(),
                            "world" => view! { <SortableHeaderCell embedded=true mode=SortMode::World label=t_string!(i18n, vendor_sell_col_world).to_string() class="w-full min-w-0" sort_mode sort_dir /> }.into_any(),
                            "listing" => view! { <SortableHeaderCell embedded=true mode=SortMode::Listing label=t_string!(i18n, vendor_sell_col_listing).to_string() class="w-full min-w-0" sort_mode sort_dir /> }.into_any(),
                            "tax" => view! { <SortableHeaderCell embedded=true mode=SortMode::Tax label=t_string!(i18n, vendor_sell_col_tax).to_string() class="w-full min-w-0" sort_mode sort_dir /> }.into_any(),
                            "vendor-price" => view! { <SortableHeaderCell embedded=true mode=SortMode::VendorPrice label=t_string!(i18n, vendor_sell_col_vendor_price).to_string() class="w-full min-w-0" sort_mode sort_dir /> }.into_any(),
                            "profit" => view! { <SortableHeaderCell embedded=true mode=SortMode::Profit label=t_string!(i18n, vendor_sell_col_profit).to_string() class="w-full min-w-0" sort_mode sort_dir /> }.into_any(),
                            "margin" => view! { <SortableHeaderCell embedded=true mode=SortMode::Margin label=t_string!(i18n, vendor_sell_col_margin).to_string() class="w-full min-w-0" sort_mode sort_dir /> }.into_any(),
                            _ => ().into_any(),
                        }
                    }
                    each=sorted_rows
                    key=move |(_, row): &Row| (row.item_id, row.hq)
                    measure=move |(_, row): &Row, id| {
                        match id {
                            "hq" => ("HQ".to_string(), 42.0),
                            "item" => (items.get(&ItemId(row.item_id)).map(|i| i.name.as_str()).unwrap_or_default().to_string(), 110.0),
                            "world" => (worlds_measure.get(&row.world_id).cloned().unwrap_or_default(), 42.0),
                            "listing" => (row.listing.separate_with_commas(), 42.0),
                            "tax" => (row.tax.separate_with_commas(), 42.0),
                            "vendor-price" => (row.vendor_price.separate_with_commas(), 42.0),
                            "profit" => (row.profit.separate_with_commas(), 42.0),
                            "margin" => (format!("{}%", row.margin), 30.0),
                            _ => (String::new(), 0.0),
                        }
                    }
                    view=move |(index, row): Row, id| {
                        let item_id = row.item_id;
                        let item = items.get(&ItemId(item_id)).map(|i| i.name.as_str()).unwrap_or_default();
                        let icon_loading = if index < 20 { "eager" } else { "" };
                        let world_name = worlds_view.get(&row.world_id).cloned().unwrap_or_default();
                        let item_href = if world_name.is_empty() {
                            format!("/item/{item_id}")
                        } else {
                            format!("/item/{world_name}/{item_id}")
                        };
                        match id {
                            "hq" => view! {
                                <div class="flex items-center justify-center w-full min-w-0">
                                    {row.hq.then(|| view! { <span class=hq_badge_class>{t!(i18n, vendor_sell_col_hq)}</span> })}
                                </div>
                            }.into_any(),
                            "item" => view! {
                                <div class="flex flex-row items-center gap-2 w-full min-w-0">
                                    <a class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip min-w-0"
                                       href=item_href>
                                        <div class="shrink-0"><ItemIcon item_id icon_size=IconSize::Small loading=icon_loading /></div>
                                        {item}
                                    </a>
                                    <AddToList item_id />
                                    <Clipboard clipboard_text=item.to_string() />
                                </div>
                            }.into_any(),
                            "world" => view! { <div class="truncate w-full min-w-0">{world_name}</div> }.into_any(),
                            "listing" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.listing /></div> }.into_any(),
                            "tax" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.tax /></div> }.into_any(),
                            "vendor-price" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.vendor_price /></div> }.into_any(),
                            "profit" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.profit /></div> }.into_any(),
                            "margin" => view! {
                                <div class="text-right flex items-center justify-end w-full min-w-0">
                                    <span class=roi_badge_class(row.margin)>{format!("{}%", row.margin)}</span>
                                </div>
                            }.into_any(),
                            _ => ().into_any(),
                        }
                    }
                />
            </div>
        </div>
    }
}

#[component]
pub fn VendorSell() -> impl IntoView {
    provide_grid_saved_views("vendor-sell-grid");
    let i18n = use_i18n();
    let (selected_world, set_selected_world) = use_analyzer_world("/vendor-sell");
    let region = use_region_for_world(move || selected_world.get().map(|world| world.name));

    let listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title=move || t_string!(i18n, vendor_sell_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, vendor_sell_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, vendor_sell).to_string()
                    summary=t_string!(i18n, vendor_sell_tool_summary).to_string()
                    context=t_string!(i18n, vendor_sell_tool_context).to_string()
                    help_href="/help/vendor-sell"
                    help_body=t_string!(i18n, vendor_sell_tool_help).to_string()
                    calculation=ToolCalculation::new(
                        t_string!(i18n, vendor_sell_calc_title).to_string(),
                        t_string!(i18n, vendor_sell_calc_formula).to_string(),
                        t_string!(i18n, vendor_sell_calc_details).to_string(),
                    )
                    assumptions=vec![
                        t_string!(i18n, vendor_sell_assumption_tax).to_string(),
                        t_string!(i18n, vendor_sell_assumption_hq).to_string(),
                        t_string!(i18n, vendor_sell_assumption_per_unit).to_string(),
                    ]
                >
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, world)}</label>
                    <div data-testid="analyzer-world-picker">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world
                        />
                    </div>
                    <span class="text-sm text-[color:var(--color-text-muted)]" data-testid="analyzer-market-scope">
                        {t!(i18n, market_scope)} ": " {move || region.get()}
                    </span>
                </ToolHeader>
                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        match listings.get() {
                            Some(Ok(listings)) => view! {
                                <VendorSellTable listings region=region.into() />
                            }.into_any(),
                            Some(Err(e)) => view! {
                                <div class="text-red-400">
                                    {t!(i18n, vendor_sell_error_listings)} {e.to_string()}
                                </div>
                            }.into_any(),
                            None => view! { <BoxSkeleton /> }.into_any(),
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use ultros_api_types::cheapest_listings::CheapestListingItem;

    fn listing(item_id: i32, hq: bool, cheapest_price: i32, world_id: i32) -> CheapestListingItem {
        CheapestListingItem {
            item_id,
            hq,
            cheapest_price,
            world_id,
        }
    }

    fn listings(items: Vec<CheapestListingItem>) -> CheapestListings {
        CheapestListings {
            cheapest_listings: items,
        }
    }

    #[test]
    fn items_without_a_vendor_price_never_become_rows() {
        // item 2 is absent from the catalog: unmarketable or price_low == 0.
        let rows = build_rows(
            &listings(vec![listing(1, false, 10, 7), listing(2, false, 10, 7)]),
            &[(1, 100)].into_iter().collect(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 1);
    }

    #[test]
    fn unprofitable_rows_are_dropped() {
        // 100 + 5 tax = 105 cost. Vendor 105 → profit 0 → dropped.
        let rows = build_rows(
            &listings(vec![listing(1, false, 100, 7), listing(2, false, 100, 7)]),
            &[(1, 105), (2, 106)].into_iter().collect(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 2);
        assert_eq!(rows[0].profit, 1);
    }

    #[test]
    fn hq_listing_is_valued_at_nq_price_and_keeps_its_flag_and_world() {
        let rows = build_rows(
            &listings(vec![listing(1, true, 101, 42), listing(1, false, 200, 43)]),
            &[(1, 120)].into_iter().collect(),
        );
        // NQ row at 200 costs 210 > 120 → dropped; HQ row survives.
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.hq);
        assert_eq!(row.world_id, 42);
        assert_eq!(row.listing, 101);
        assert_eq!(row.tax, 6);
        assert_eq!(row.cost, 107);
        assert_eq!(row.vendor_price, 120);
        assert_eq!(row.profit, 13);
        assert_eq!(row.margin, 12);
    }

    #[test]
    fn build_rows_order_is_item_then_nq_before_hq() {
        let rows = build_rows(
            &listings(vec![
                listing(5, true, 1, 1),
                listing(3, false, 1, 1),
                listing(5, false, 1, 1),
            ]),
            &[(3, 100), (5, 100)].into_iter().collect(),
        );
        let keys: Vec<_> = rows.iter().map(|r| (r.item_id, r.hq)).collect();
        assert_eq!(keys, vec![(3, false), (5, false), (5, true)]);
    }

    #[test]
    fn default_sort_is_profit_descending_and_asc_reverses() {
        let mut rows = build_rows(
            &listings(vec![
                listing(1, false, 10, 1),
                listing(2, false, 10, 1),
                listing(3, false, 10, 1),
            ]),
            &[(1, 50), (2, 500), (3, 20)].into_iter().collect(),
        );
        let mode = SortMode::fallback();
        assert_eq!(mode, SortMode::Profit);
        assert_eq!(mode.default_dir(), SortDir::Desc);
        sort_rows(&mut rows, mode, mode.default_dir());
        let ids: Vec<_> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![2, 1, 3]);
        sort_rows(&mut rows, mode, SortDir::Asc);
        let ids: Vec<_> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![3, 1, 2]);
    }

    #[test]
    fn sort_tokens_round_trip() {
        for token in [
            "profit",
            "margin",
            "listing",
            "tax",
            "vendor-price",
            "world",
        ] {
            assert_eq!(SortMode::from_str(token).unwrap().to_string(), token);
        }
        assert!(SortMode::from_str("roi").is_err());
    }

    #[test]
    fn every_preset_query_is_a_clean_query_string() {
        for query in PRESET_QUERIES {
            assert!(query.starts_with('?'), "{query}");
            assert!(!query.ends_with('&'), "{query}");
            assert!(!query.contains("&&"), "{query}");
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                assert!(!key.is_empty(), "{query}");
                assert!(!value.is_empty(), "{query}");
            }
        }
    }

    #[test]
    fn preset_queries_only_use_keys_this_page_still_reads() {
        for query in PRESET_QUERIES {
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').expect("key=value");
                match key {
                    "sort" => assert!(SortMode::from_str(value).is_ok(), "{query}"),
                    "dir" => assert!(SortDir::from_str(value).is_ok(), "{query}"),
                    other => assert!(LEGACY_PRESET_FILTER_KEYS.contains(&other), "{query}"),
                }
            }
        }
    }
}
