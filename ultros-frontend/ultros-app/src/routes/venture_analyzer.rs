use crate::components::meta::{MetaDescription, MetaTitle};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::{SalesStats, analyze_sales},
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        control_bar::{ControlBar, FilterOption},
        filter_chip::FilterChip,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortableHeaderCell, sort_and_truncate},
        tool_help::*,
        virtual_scroller::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::{
        LocalWorldData, home_world::use_home_world, region_for_world::use_region_for_world,
    },
    query_defaults::filter_query_signal,
};
use itertools::Itertools;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_query_map},
};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};
use ultros_api_types::{
    cheapest_listings::{CheapestListings, CheapestListingsMap},
    recent_sales::{RecentSales, SaleData},
};

#[derive(Clone, Debug, PartialEq)]
struct VentureProfitData {
    task_level: i32,
    item_id: i32,
    quantity: i32,
    market_price: i32,
    profit: i32,
    avg_price: i32,
    daily_sales: f32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Level,
    UnitPrice,
    AvgPrice,
    DailySales,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "level" => Ok(SortMode::Level),
            "unit-price" => Ok(SortMode::UnitPrice),
            "avg-price" => Ok(SortMode::AvgPrice),
            "daily-sales" => Ok(SortMode::DailySales),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Profit => "profit",
            SortMode::Level => "level",
            SortMode::UnitPrice => "unit-price",
            SortMode::AvgPrice => "avg-price",
            SortMode::DailySales => "daily-sales",
        };
        f.write_str(val)
    }
}

/// Every column reads best-first descending — ventures cost venture coins,
/// not gil, so there is no cost-like column to default ascending.
impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }
}

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_PROFIT: &str = "profit";
const FILTER_OUTLIERS: &str = "filter-outliers";

/// Filters the `+ Filter` menu can add, in menu order.
const ADDABLE_FILTERS: &[&str] = &[FILTER_PROFIT, FILTER_OUTLIERS];

fn compare_ventures(mode: SortMode, a: &VentureProfitData, b: &VentureProfitData) -> Ordering {
    match mode {
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Level => a.task_level.cmp(&b.task_level),
        SortMode::UnitPrice => a.market_price.cmp(&b.market_price),
        SortMode::AvgPrice => a.avg_price.cmp(&b.avg_price),
        SortMode::DailySales => a
            .daily_sales
            .partial_cmp(&b.daily_sales)
            .unwrap_or(Ordering::Equal),
    }
}

#[component]
fn VentureAnalyzerTable(
    global_cheapest_listings: CheapestListings,
    recent_sales: Option<RecentSales>,
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
    let prices = CheapestListingsMap::from(global_cheapest_listings);
    let data = tracked_data();
    let items = &data.items;
    let retainer_tasks = &data.retainer_tasks;
    let retainer_task_normals = &data.retainer_task_normals;

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    // Filter params use `filter_query_signal` (replace: true, scroll: false):
    // typing into a chip writes the URL on every keystroke, and plain
    // `query_signal`'s defaults would push a history entry and yank the
    // window to the top each time.
    let (minimum_profit, set_minimum_profit) = filter_query_signal::<i32>(FILTER_PROFIT);
    let (filter_outliers, set_filter_outliers) = filter_query_signal::<bool>(FILTER_OUTLIERS);
    let query = use_query_map();
    let location = use_location();
    let nav = use_navigate();

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input (see currency_exchange.rs
    // for the same pattern). Booleans commit immediately on add instead, so
    // this only ever holds `FILTER_PROFIT`.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

    let categories = Memo::new(move |_| {
        retainer_tasks
            .values()
            .filter(|t| !t.is_random)
            .map(|t| t.class_job_category)
            .unique()
            .filter_map(|id| {
                data.class_job_categorys
                    .get(&xiv_gen::ClassJobCategoryId(id))
                    .map(|c| (id, c.name.as_str().to_string()))
            })
            .sorted_by(|a, b| a.1.cmp(&b.1))
            .collect::<Vec<_>>()
    });

    let selected_jobs_set = Memo::new(move |_| {
        query.with(|q| {
            q.get("jobs")
                .map(|s| s.split(',').map(|s| s.to_string()).collect::<HashSet<_>>())
                .unwrap_or_default()
        })
    });

    let toggle_job = move |job_name: String| {
        let mut current = selected_jobs_set.get();
        if current.contains(&job_name) {
            current.remove(&job_name);
        } else {
            current.insert(job_name);
        }

        let mut q = query.get_untracked();
        if current.is_empty() {
            q.remove("jobs");
        } else {
            q.insert("jobs".to_string(), current.into_iter().join(","));
        }

        let qs = q.to_query_string();
        nav(
            &format!("{}{}", location.pathname.get(), qs),
            NavigateOptions {
                scroll: false,
                ..Default::default()
            },
        );
    };

    let selected_category_ids = Memo::new(move |_| {
        let selected_names = selected_jobs_set.get();
        if selected_names.is_empty() {
            return None;
        }
        let ids: HashSet<_> = categories
            .get()
            .iter()
            .filter(|(_, name)| selected_names.contains(name))
            .map(|(id, _)| *id)
            .collect();
        Some(ids)
    });

    let computed_data = Memo::new(move |_| {
        let mut results = Vec::new();
        let selected_ids = selected_category_ids.get();
        let filter_outliers = filter_outliers().unwrap_or(false);

        let sales_map: HashMap<i32, Vec<&SaleData>> = if let Some(ref sales) = recent_sales {
            let mut map: HashMap<i32, Vec<&SaleData>> = HashMap::new();
            for sale in &sales.sales {
                map.entry(sale.item_id).or_default().push(sale);
            }
            map
        } else {
            HashMap::new()
        };

        // Iterate over RetainerTasks to find normal ventures
        for (_task_id, task) in retainer_tasks.iter() {
            if task.is_random {
                continue;
            }

            if let Some(ids) = &selected_ids
                && !ids.contains(&task.class_job_category)
            {
                continue;
            }

            // Check if `task.task` (RowId) corresponds to a RetainerTaskNormal
            // We need to cast RowId to RetainerTaskNormalId?
            // Since RowId is just u16 wrapper, and RetainerTaskNormalId is i32 wrapper.
            let normal_id = xiv_gen::RetainerTaskNormalId(task.task);

            if let Some(normal_task) = retainer_task_normals.get(&normal_id) {
                let item_id = normal_task.item;
                if item_id == 0 {
                    continue;
                }

                let quantity = normal_task.quantity_0; // taking base quantity
                if quantity == 0 {
                    continue;
                }

                let task_level = task.retainer_level as i32;

                // Market Price
                let market_price_summary = prices.find_matching_listings(item_id);
                let market_price = market_price_summary.lowest_gil().unwrap_or(0);

                if market_price == 0 {
                    continue;
                }

                let sales_stats = if let Some(item_sales) = sales_map.get(&{ item_id }) {
                    analyze_sales(item_sales, filter_outliers)
                } else {
                    SalesStats {
                        daily_sales: 0.0,
                        avg_price: 0,
                        total_sales: 0,
                    }
                };

                // Ventures cost venture coins (not gil), so "profit" here is gross revenue.
                // If we ever convert ventures to a gil-equivalent cost, subtract it here.
                let revenue = market_price * quantity;
                let profit = revenue;

                if let Some(min) = minimum_profit()
                    && profit < min
                {
                    continue;
                }

                results.push(VentureProfitData {
                    task_level,
                    item_id,
                    quantity,
                    market_price,
                    profit,
                    avg_price: sales_stats.avg_price,
                    daily_sales: sales_stats.daily_sales,
                });
            }
        }

        // Sort
        // ⚡ Bolt: Optimization: In-place filtering and truncation for Top N lists using select_nth_unstable.
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        sort_and_truncate(&mut results, dir, 100, |a, b| compare_ventures(mode, a, b));

        results
            .into_iter()
            .map(Arc::new)
            .enumerate()
            .collect::<Vec<_>>()
    });

    // Filters currently drawn as a chip. Drives the "no active filters" hint
    // and keeps `+ Filter` from offering a second copy of something the user
    // can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        if minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT) {
            active.push(FILTER_PROFIT);
        }
        if filter_outliers().unwrap_or(false) {
            active.push(FILTER_OUTLIERS);
        }
        active
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, venture_analyzer_filter_profit_min_label).to_string(),
            FILTER_OUTLIERS => t_string!(i18n, venture_analyzer_filter_outliers).to_string(),
            _ => String::new(),
        }
    };

    // What the `+ Filter` menu offers: everything addable that is not already
    // on screen as a chip.
    let filter_options = Memo::new(move |_| {
        ADDABLE_FILTERS
            .iter()
            .copied()
            .filter(|id| !active_filters().contains(id))
            .map(|id| FilterOption {
                id,
                label: filter_label(id),
            })
            .collect::<Vec<_>>()
    });

    let add_filter = Callback::new(move |id: &'static str| match id {
        FILTER_PROFIT => pending_filter.set(Some(FILTER_PROFIT)),
        // Boolean toggle: the chip's presence *is* the value, so it commits
        // straight to `true` rather than mounting an editable chip.
        FILTER_OUTLIERS => set_filter_outliers(Some(true)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_minimum_profit(None);
        set_filter_outliers(None);
    });

    view! {
        <div class="flex flex-col gap-6">
            <ControlBar
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, venture_analyzer_result_count, n = move || computed_data().len())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! { <RealtimeStatus status=realtime_status last_update=last_update /> }
                        .into_any()
                }
                available_filters=Signal::derive(filter_options)
                on_add_filter=add_filter
                on_clear_all=clear_all
                empty_label=Signal::derive(move || {
                    t_string!(i18n, venture_analyzer_no_filters_hint).to_string()
                })
                is_empty=Signal::derive(move || active_filters().is_empty())
            >
                {move || {
                    (minimum_profit().is_some() || pending_filter.get() == Some(FILTER_PROFIT))
                        .then(|| {
                            let start_editing = pending_filter.get_untracked() == Some(FILTER_PROFIT);
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, venture_analyzer_chip_profit_min).to_string()
                                    value=Signal::derive(move || minimum_profit().map(|v| v.to_string()))
                                    numeric=true
                                    min="0"
                                    step="1000"
                                    start_editing=start_editing
                                    on_commit=Callback::new(move |v: Option<String>| {
                                        set_minimum_profit(v.and_then(|v| v.parse().ok()));
                                        if pending_filter.get_untracked() == Some(FILTER_PROFIT) {
                                            pending_filter.set(None);
                                        }
                                    })
                                />
                            }
                        })
                }}
                {move || {
                    filter_outliers()
                        .unwrap_or(false)
                        .then(|| {
                            view! {
                                <FilterChip
                                    label=t_string!(i18n, venture_analyzer_filter_outliers).to_string()
                                    readonly=true
                                    value=Signal::derive(|| None::<String>)
                                    on_commit=Callback::new(move |_| set_filter_outliers(None))
                                />
                            }
                        })
                }}
            </ControlBar>

            // Job category multi-select: complex tag-cloud widget, kept as panel
            <div class="panel p-4 flex flex-col w-full bg-[color:var(--color-background-elevated)] bg-opacity-100 z-20">
                <h3 class="font-bold text-base mb-2 text-[color:var(--brand-fg)]">{t!(i18n, venture_analyzer_filter_by_job)}</h3>
                <div class="flex flex-wrap gap-2">
                    {move || {
                        let selected = selected_jobs_set.get();
                        categories
                            .get()
                            .into_iter()
                            .map(|(_id, name)| {
                                let is_selected = selected.contains(&name);
                                let name_clone = name.clone();
                                let toggle_job = toggle_job.clone();
                                view! {
                                    <button
                                        class=move || {
                                            if is_selected {
                                                "px-3 py-1 rounded-full text-xs font-bold bg-brand-600 text-white transition-colors border border-brand-500"
                                            } else {
                                                "px-3 py-1 rounded-full text-xs font-bold bg-[color:var(--color-base)] hover:bg-[color:var(--brand-ring)]/20 text-[color:var(--color-text)] transition-colors border border-[color:var(--color-outline)]"
                                            }
                                        }
                                        on:click=move |_| toggle_job(name_clone.clone())
                                    >
                                        {name}
                                    </button>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </div>

            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                    viewport_height=720.0
                    row_height=60.0
                    overscan=8
                    header_height=64.0
                    variable_height=false
                    header=view! {
                        <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                             <div role="columnheader" class="w-84 p-4">{t!(i18n, venture_analyzer_col_venture_item)}</div>
                             <SortableHeaderCell
                                mode=SortMode::Profit
                                label=t_string!(i18n, venture_analyzer_col_profit).to_string()
                                class="w-30 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::UnitPrice
                                label=t_string!(i18n, venture_analyzer_col_unit_price).to_string()
                                class="w-30 p-4"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::AvgPrice
                                label=t_string!(i18n, venture_analyzer_col_avg_price).to_string()
                                class="w-30 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::DailySales
                                label=t_string!(i18n, venture_analyzer_col_daily_sales).to_string()
                                class="w-30 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                             <SortableHeaderCell
                                mode=SortMode::Level
                                label=t_string!(i18n, venture_analyzer_col_level).to_string()
                                class="w-30 p-4 hidden md:block"
                                sort_mode
                                sort_dir
                             />
                        </div>
                    }.into_any()
                    each=computed_data.into()
                    key=move |(index, data): &(usize, Arc<VentureProfitData>)| (*index, data.item_id)
                    view=move |(index, data): (usize, Arc<VentureProfitData>)| {
                        let item_id = data.item_id;
                        let item = items.get(&xiv_gen::ItemId(item_id)).map(|i| i.name.as_str().to_string()).unwrap_or_else(|| t_string!(i18n, unknown).to_string());

                        let classes = if (index % 2) == 0 {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                        } else {
                            "flex flex-row items-center flex-nowrap h-15 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                        };

                        view! {
                            <div class=classes role="row-group">
                                <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
                                     <a
                                        class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                        href=format!("/item/{}/{}", world(), item_id)
                                    >
                                        <div class="shrink-0">
                                            <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                        </div>
                                        <div class="flex flex-col truncate">
                                            <span class="font-semibold">{item}</span>
                                            <span class="text-xs text-[color:var(--color-text-muted)] truncate">
                                                {t!(i18n, venture_analyzer_quantity_x)} " " {data.quantity}
                                            </span>
                                        </div>
                                    </a>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.profit />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right">
                                    <Gil amount=data.market_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <Gil amount=data.avg_price />
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]">
                                        {t!(i18n, venture_analyzer_sales_per_day, sales = format!("{:.1}", data.daily_sales))}
                                    </span>
                                </div>
                                <div role="cell" class="px-4 py-2 w-30 text-right hidden md:block">
                                    <span class="text-xs text-[color:var(--color-text-muted)]">
                                        {t!(i18n, venture_analyzer_lv)} " " {data.task_level}
                                    </span>
                                </div>
                            </div>
                        }.into_any()
                    }
                />
             </div>
        </div>
    }
}

#[component]
pub fn VentureAnalyzer() -> impl IntoView {
    let i18n = use_i18n();
    let query = use_query_map();
    let (home_world, _) = use_home_world();
    let nav = use_navigate();

    let region = use_region_for_world(move || query.with(|p| p.get("world").clone()));

    let global_cheapest_listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();

    let initial_world = query.with_untracked(|p| {
        let binding = p.get("world");
        let world = binding.as_deref().unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });

    let (selected_world, set_selected_world) = signal(initial_world);

    // If no world is selected initially, try to use home world
    Effect::new(move |_| {
        if selected_world.get_untracked().is_none()
            && let Some(home) = home_world.get()
        {
            set_selected_world(Some(home));
        }
    });

    // When selected world changes, update the URL
    Effect::new(move |_| {
        if let Some(world) = selected_world.get() {
            let world_name = world.name;
            let current_query = query.get_untracked();
            let world_matches = current_query
                .get("world")
                .map(|s| s == world_name)
                .unwrap_or(false);

            if !world_matches {
                let mut query_string = format!("?world={}", world_name);
                for (k, v) in current_query.into_iter() {
                    if k != "world" {
                        query_string.push_str(&format!("&{}={}", k, v));
                    }
                }
                nav(
                    &query_string,
                    NavigateOptions {
                        scroll: false,
                        ..Default::default()
                    },
                );
            }
        }
    });

    let recent_sales = ArcResource::new(selected_world, move |world| async move {
        if let Some(world) = world {
            get_recent_sales_for_world(&world.name).await
        } else {
            Ok(RecentSales { sales: vec![] })
        }
    });

    let recent_sales_clone = recent_sales.clone();
    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title=move || t_string!(i18n, venture_analyzer_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, venture_analyzer_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, venture_analyzer).to_string()
                    summary=t_string!(i18n, venture_analyzer_tool_summary).to_string()
                    context=t_string!(i18n, venture_analyzer_tool_context).to_string()
                    help_href="/help/venture-analyzer"
                    help_body=t_string!(i18n, venture_analyzer_tool_help).to_string()
                />
                <div class="flex flex-row justify-end items-center">
                    <div class="flex flex-row gap-2 items-center">
                        <Suspense fallback=move || view! { <div class="text-brand-300 text-sm animate-pulse">{t!(i18n, venture_analyzer_loading_sales)}</div> }>
                            {move || {
                                recent_sales_clone
                                    .get()
                                    .and_then(|r| r.err())
                                    .map(|_| view! { <div class="text-red-400 text-sm">{t!(i18n, venture_analyzer_error_sales)}</div> })
                            }}
                        </Suspense>
                    </div>
                </div>

                <div class="flex flex-col md:flex-row items-center gap-2">
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, world)}</label>
                    <div class="w-full md:w-auto">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world.into()
                        />
                    </div>
                </div>
                <CalculationSummary
                    title=t_string!(i18n, venture_analyzer_calc_title).to_string()
                    formula=t_string!(i18n, venture_analyzer_calc_formula).to_string()
                    details=t_string!(i18n, venture_analyzer_calc_details).to_string()
                />
                <div class="flex flex-wrap gap-2">
                    <AssumptionBadge text=t_string!(i18n, venture_analyzer_assumption_gross_revenue).to_string() />
                    <AssumptionBadge text="Normal ventures only" />
                    <AssumptionBadge text="Recent sales affect confidence" />
                </div>

                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        let listings = global_cheapest_listings.get();
                        let sales = recent_sales.get();
                        match (listings, sales) {
                            (Some(Ok(listings)), Some(Ok(sales))) => {
                                view! {
                                    <VentureAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=Some(sales)
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            (Some(Ok(listings)), _) => {
                                view! {
                                    <VentureAnalyzerTable
                                        global_cheapest_listings=listings
                                        recent_sales=None
                                        world=region.into()
                                    />
                                }.into_any()
                            }
                            (Some(Err(e)), _) => {
                                view! {
                                    <div class="text-red-400">
                                        {t!(i18n, venture_analyzer_error_listings)} {e.to_string()}
                                    </div>
                                }.into_any()
                            }
                            _ => {
                                view! { <BoxSkeleton /> }.into_any()
                            }
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Display must produce exactly the token FromStr parses back — the
    /// shared SortHeader's hrefs depend on that round trip.
    #[test]
    fn sort_mode_round_trips_through_the_url() {
        for mode in [
            SortMode::Profit,
            SortMode::Level,
            SortMode::UnitPrice,
            SortMode::AvgPrice,
            SortMode::DailySales,
        ] {
            assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
        }
        assert!("bogus".parse::<SortMode>().is_err());
    }

    #[test]
    fn compare_ventures_orders_ascending_by_column() {
        let row = |profit: i32, daily_sales: f32| VentureProfitData {
            task_level: profit,
            item_id: 1,
            quantity: 1,
            market_price: profit,
            profit,
            avg_price: profit,
            daily_sales,
        };
        let low = row(10, 0.5);
        let high = row(20, 2.0);
        for mode in [
            SortMode::Profit,
            SortMode::Level,
            SortMode::UnitPrice,
            SortMode::AvgPrice,
            SortMode::DailySales,
        ] {
            assert_eq!(
                compare_ventures(mode, &low, &high),
                Ordering::Less,
                "{mode:?}"
            );
        }
    }
}
