//! Market Trends — the shared analyzer grid over `item_stats_window`.
//!
//! The page sources its rows from `get_trends_v2`: a flat list (capped at
//! 500 server-side) of items the rollup knows about on the current world,
//! with per-row VWAP, sales/day, unit volume, and a 24h sparkline for the
//! selected window. Those native statistics come from the deep-scan's
//! *cleaned* sample — the same noise filter the suspicious-sales control
//! toggles — so they are registered as their own metrics rather than
//! replaced by the shared `sale_stats` columns, whose aggregation counts
//! every recorded sale. The shared columns stay available from the Columns
//! picker beside them.
//!
//! Sorting and filtering only ever rank the returned candidate set; this is
//! not a full-market query. Old `?sort=units|vwap|price|pct|spd` and
//! `?min_sales=` / `?min_price=` links keep working through registry aliases.
//!
//! The MarketHeat band from the home page is reused at the top of the
//! page — it answers "what's hot right now" at a glance; the grid below
//! is the deep-dive.

use std::{collections::HashSet, sync::Arc};

use crate::analyzer_kit::{
    filters::{register_filters, toggle_control},
    market::{MarketGrid, MarketSubject, use_market_data_with_window},
    stat_columns::{
        Window, market_picker_options, shared_cols_in, toggle_shared_col, window_label,
    },
    window::{MarketWindow, MarketWindowControl},
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::{filter_query_signal, query_signal_or_default};
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{use_location, use_navigate, use_params_map},
};
use ultros_api_types::{
    icon_size::IconSize,
    trends::{ConfidenceBand, TrendItem},
};

use crate::components::app_link::use_query_map_or_default;
use crate::components::virtual_grid::{
    ColumnFilter, GridColumn,
    metrics::{FilterOp, GridMetric, GridValue},
    query_grid::MetricSortHeader,
    registry::{FilterAlias, SortAlias},
    saved_views::GridSavedViews,
};
use crate::{
    api::get_trends_v2,
    components::{
        add_to_list::AddToList,
        clipboard::Clipboard,
        confidence_badge::ConfidenceBadge,
        control_bar::{ColumnOption, ControlBar, PickerHeading, parse_visible_cols},
        gil::Gil,
        item_icon::ItemIcon,
        market_heat::MarketHeat,
        meta::{MetaDescription, MetaTitle},
        skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton},
        sparkline::Sparkline,
        tool_help::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::LocalWorldData,
    routes::world_nav::world_nav_url,
};

/// One candidate row; `Arc` so the grid clones it per cell without copying the sparkline.
type Row = Arc<TrendItem>;

// --- URL contract ----------------------------------------------------------
// Calculation inputs read before the request or the row set is built. They
// stay registered controls rather than becoming post-calculation metric
// filters: `category` narrows the candidate set and `show_suspicious`
// changes what the server returns.
const FILTER_CATEGORY: &str = "category";
const FILTER_SUSPICIOUS: &str = "show_suspicious";
/// Retired chip parameters, now aliases onto metric filters.
const LEGACY_MIN_SALES: &str = "min_sales";
const LEGACY_MIN_PRICE: &str = "min_price";

/// Trends keeps its narrower window choices and 30-day default.
const TRENDS_WINDOWS: &[Window] = &[Window::D7, Window::D30, Window::D90];

// --- Column ids ------------------------------------------------------------
// Native ids are a bookmark/saved-view contract; keep them stable.
const COL_ITEM: &str = "item";
const COL_TREND: &str = "trend";
const COL_VWAP: &str = "vwap";
const COL_PCT: &str = "pct";
const COL_SALES_PER_DAY: &str = "sales-per-day";
const COL_UNITS: &str = "units";
const COL_SALES: &str = "sales";
const COL_CONFIDENCE: &str = "confidence";
/// Shared identity columns the page places itself: exact quality and the
/// observed listing price each row was scanned against.
const COL_QUALITY: &str = "market-quality";
const COL_LISTING: &str = "market-listing";

/// Every optional column in table order; `?cols=` lists the visible subset.
const OPTIONAL_COLS: &[&str] = &[
    COL_QUALITY,
    COL_TREND,
    COL_LISTING,
    COL_VWAP,
    COL_PCT,
    COL_SALES_PER_DAY,
    COL_UNITS,
    COL_SALES,
    COL_CONFIDENCE,
];

/// The original table's columns. `sales` (the cleaned sale count behind
/// sales/day and the old Min sales chip) is filterable but hidden by default.
const DEFAULT_COLS: &[&str] = &[
    COL_QUALITY,
    COL_TREND,
    COL_LISTING,
    COL_VWAP,
    COL_PCT,
    COL_SALES_PER_DAY,
    COL_UNITS,
    COL_CONFIDENCE,
];

fn default_cols_query() -> String {
    DEFAULT_COLS.join(",")
}

/// Old `?sort=` tokens and the metric column each now selects. `price` was
/// the observed listing, which the shared listing column carries.
const SORT_ALIASES: [SortAlias; 5] = [
    SortAlias::new("units", COL_UNITS),
    SortAlias::new("vwap", COL_VWAP),
    SortAlias::new("price", COL_LISTING),
    SortAlias::new("pct", COL_PCT),
    SortAlias::new("spd", COL_SALES_PER_DAY),
];

fn filter_aliases() -> Vec<FilterAlias> {
    vec![
        FilterAlias::integer(LEGACY_MIN_SALES, COL_SALES, FilterOp::Gte),
        FilterAlias::integer(LEGACY_MIN_PRICE, COL_LISTING, FilterOp::Gte),
    ]
}

/// Ids currently on in `?cols=`: native optional columns plus any shared
/// sale-history column, so the picker checkboxes reflect both.
fn visible_cols(raw: Option<&str>) -> HashSet<&'static str> {
    let defaults = default_cols_query();
    let raw = raw.unwrap_or(&defaults);
    let mut set = parse_visible_cols(Some(raw), OPTIONAL_COLS, DEFAULT_COLS);
    set.extend(shared_cols_in(Some(raw)));
    set
}

fn format_volume(v: u64) -> String {
    if v >= 1_000_000 {
        format!("{:.1}M", v as f64 / 1_000_000.0)
    } else if v >= 1_000 {
        format!("{:.1}K", v as f64 / 1_000.0)
    } else {
        v.to_string()
    }
}

fn format_pct(pct: f32) -> String {
    if pct.abs() < 0.05 {
        "—".to_string()
    } else if pct >= 0.0 {
        format!("+{pct:.1}%")
    } else {
        format!("{pct:.1}%")
    }
}

fn pct_class(pct: f32) -> &'static str {
    if pct > 0.0 {
        "text-emerald-300"
    } else if pct < 0.0 {
        "text-red-300"
    } else {
        "text-[color:var(--color-text-muted)]"
    }
}

fn item_name(item_id: i32) -> Option<String> {
    tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
}

/// The deep-scan band as filter text. `Unknown` is a missing verdict, not a
/// band, matching the shared confidence column's convention.
fn confidence_value(band: ConfidenceBand) -> GridValue {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match band {
        ConfidenceBand::Unknown => GridValue::Missing,
        ConfidenceBand::High => GridValue::Text(t_string!(i18n, confidence_band_high).to_string()),
        ConfidenceBand::Medium => {
            GridValue::Text(t_string!(i18n, confidence_band_medium).to_string())
        }
        ConfidenceBand::Low => GridValue::Text(t_string!(i18n, confidence_band_low).to_string()),
        ConfidenceBand::Unusable => {
            GridValue::Text(t_string!(i18n, confidence_band_unusable).to_string())
        }
    }
}

fn number(value: f64) -> GridValue {
    if value.is_finite() {
        GridValue::Number(value)
    } else {
        GridValue::Missing
    }
}

/// Native metrics: the deep-scan's cleaned-sample statistics for the
/// selected window. The listing price and quality come from the shared
/// subject metrics; the sparkline is display-only.
fn trend_metrics() -> Vec<GridMetric<Arc<TrendItem>>> {
    vec![
        GridMetric::text(COL_ITEM, |row: &Arc<TrendItem>| {
            GridValue::Text(item_name(row.item_id).unwrap_or_default())
        }),
        GridMetric::number(COL_VWAP, |row: &Arc<TrendItem>| {
            if row.vwap_window > 0 {
                GridValue::Number(f64::from(row.vwap_window))
            } else {
                GridValue::Missing
            }
        }),
        GridMetric::number(COL_PCT, |row: &Arc<TrendItem>| {
            number(f64::from(row.pct_change_window))
        }),
        GridMetric::number(COL_SALES_PER_DAY, |row: &Arc<TrendItem>| {
            number(f64::from(row.sales_per_day))
        }),
        GridMetric::number(COL_UNITS, |row: &Arc<TrendItem>| {
            GridValue::Number(row.unit_volume_window as f64)
        }),
        GridMetric::number(COL_SALES, |row: &Arc<TrendItem>| {
            GridValue::Number(f64::from(row.sales_in_window))
        }),
        GridMetric::text(COL_CONFIDENCE, |row: &Arc<TrendItem>| {
            confidence_value(row.confidence_band)
        }),
    ]
}

/// The grid's loading state, drawn from the default column geometry.
#[component]
fn TrendsTableSkeleton() -> impl IntoView {
    let columns = vec![
        SkeletonColumn::new(
            "px-3 py-2 flex flex-row flex-1 min-w-[14rem] items-center gap-2",
            SkeletonCell::IconText,
        ),
        SkeletonColumn::new(
            "px-2 py-2 w-[90px] flex items-center justify-center",
            SkeletonCell::Blank,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[120px] flex items-center justify-center",
            SkeletonCell::Spark,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[130px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[130px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[100px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[120px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[120px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[130px] flex items-center justify-center",
            SkeletonCell::Badge,
        ),
    ];
    view! {
        <TableSkeleton
            columns
            rows=12
            class="rounded-lg border border-[color:var(--color-outline)]"
            row_class="min-w-[940px] border-b border-[color:var(--line)]"
            row_height="h-12"
            header_height="h-12"
            striped=false
        />
    }
}

#[component]
fn TrendsWorldNavigator() -> impl IntoView {
    let nav = use_navigate();
    let params = use_params_map();
    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0;

    let initial_world = params.with_untracked(|p| {
        let world = p.get_str("world").unwrap_or_default();
        if let Ok(w_data) = &worlds {
            w_data
                .lookup_world_by_name(world)
                .and_then(|w| w.as_world().cloned())
        } else {
            None
        }
    });

    let (current_world, set_current_world) = signal(initial_world);
    let query = use_query_map_or_default();
    let location = use_location();

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            // This effect also runs on mount, where the world already matches
            // the path. Navigating anyway — and without the query — wiped every
            // filter out of a shared link as it hydrated (issue #1053).
            let url = world_nav_url(
                "/trends",
                &world.name,
                &location.pathname.get_untracked(),
                &query.get_untracked(),
            );
            if let Some(url) = url {
                nav(
                    &url,
                    NavigateOptions {
                        scroll: false,
                        ..Default::default()
                    },
                );
            }
        }
    });

    view! {
        <WorldOnlyPicker
            current_world=current_world.into()
            set_current_world=set_current_world.into()
        />
    }
}

#[component]
pub fn Trends() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let world = move || params.with(|params| params.get("world").unwrap_or_default());
    let world_scope: Signal<String> = Signal::derive(world);

    // URL-driven page state. `window` and `cols` are persistent view modes
    // (like a tab), so they navigate without scrolling but do push history;
    // the calculation inputs use `filter_query_signal` (replace: true,
    // scroll: false) like every registered control.
    let window = MarketWindow::new(Window::D30, TRENDS_WINDOWS);
    let market = use_market_data_with_window(world_scope, window, None);
    let (suspicious, _set_suspicious) = filter_query_signal::<bool>(FILTER_SUSPICIOUS);
    let (category_filter, _set_category_filter) = filter_query_signal::<i32>(FILTER_CATEGORY);
    let (cols_param, set_cols_param) = query_signal_or_default::<String>(
        "cols",
        NavigateOptions {
            scroll: false,
            ..Default::default()
        },
    );

    let window_days = Memo::new(move |_| window.selected.get().days());
    let show_suspicious = Signal::derive(move || suspicious().unwrap_or(false));

    let trends = ArcResource::new(
        move || (world(), window_days(), show_suspicious()),
        move |(w, win, sus)| async move {
            if w.is_empty() {
                return Ok(None);
            }
            get_trends_v2(&w, win, sus).await.map(Some)
        },
    );
    // ArcResource is Clone — split the handle so neither the Memo nor
    // the view closure consumes the same binding.
    let trends_for_rows = trends.clone();
    let trends_for_view = trends.clone();
    let trends_for_visibility = trends;

    let world_signal: Signal<Option<String>> = Signal::derive(move || {
        let w = world();
        if w.is_empty() { None } else { Some(w) }
    });

    // The candidate set the grid queries: the server's 500-row cap narrowed
    // by the category control. Everything else — metric filters, aliases,
    // header sorting — is the grid's, over exactly these rows.
    let rows = Memo::new(move |_| {
        let data = match trends_for_rows.get() {
            Some(Ok(Some(d))) => d,
            _ => return Vec::new(),
        };
        let category = category_filter();
        data.items
            .into_iter()
            .filter(|it| {
                category
                    .map(|cat| {
                        tracked_data()
                            .items
                            .get(&xiv_gen::ItemId(it.item_id))
                            .map(|i| i.item_search_category == cat)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .map(Arc::new)
            .collect::<Vec<_>>()
    });

    let category_choices = Memo::new(move |_| {
        let mut categories = tracked_data()
            .item_search_categorys
            .iter()
            .filter(|(_, cat)| !cat.name.is_empty())
            .map(|(id, cat)| (id.0.to_string(), cat.name.clone()))
            .collect::<Vec<_>>();
        categories.sort_by(|a, b| a.1.cmp(&b.1));
        categories
    });

    let filters = register_filters(
        filter_aliases(),
        Signal::derive(move || {
            vec![
                {
                    let mut control = ColumnFilter::new(
                        FILTER_CATEGORY,
                        t_string!(i18n, trends_filter_category_label).to_string(),
                        false,
                    );
                    control.choices = category_choices.get();
                    control
                },
                toggle_control(
                    FILTER_SUSPICIOUS,
                    t_string!(i18n, trends_show_suspicious).to_string(),
                ),
            ]
        }),
    );
    filters.register_sort_aliases(SORT_ALIASES.to_vec());
    filters.register_default_sort(COL_UNITS);

    // Columns picker: native columns in table order under their own heading,
    // then every shared sale-history column grouped by window.
    let native_label = move |id: &str| -> String {
        match id {
            COL_QUALITY => t_string!(i18n, market_quality).to_string(),
            COL_TREND => t_string!(i18n, trends_col_spark).to_string(),
            COL_LISTING => t_string!(i18n, market_listing).to_string(),
            COL_VWAP => t_string!(i18n, trends_col_vwap).to_string(),
            COL_PCT => t_string!(i18n, trends_col_pct_change).to_string(),
            COL_SALES_PER_DAY => t_string!(i18n, trends_col_sales_per_day).to_string(),
            COL_UNITS => t_string!(i18n, trends_col_units_window).to_string(),
            COL_SALES => t_string!(i18n, trends_col_sales).to_string(),
            COL_CONFIDENCE => t_string!(i18n, trends_col_confidence).to_string(),
            _ => String::new(),
        }
    };
    let column_options = Memo::new(move |_| {
        let mut options = OPTIONAL_COLS
            .iter()
            .map(|col| ColumnOption {
                id: col,
                label: native_label(col),
                group: Some(PickerHeading {
                    label: t_string!(i18n, trends_picker_group).to_string(),
                    title: None,
                }),
                disabled: false,
                hint: None,
            })
            .collect::<Vec<_>>();
        options.extend(market_picker_options(window.selected.get()));
        options
    });
    let picker_visible = Memo::new(move |_| visible_cols(cols_param.get().as_deref()));
    let toggle_column = Callback::new(move |col: &'static str| {
        let previous = cols_param.get_untracked();
        set_cols_param.set(Some(toggle_shared_col(
            previous.as_deref(),
            &default_cols_query(),
            col,
        )));
    });

    let trend_group = Signal::derive(move || t_string!(i18n, trends_picker_group).to_string());
    let columns = Signal::derive(move || {
        let native = |id: &'static str, width: f64, visible: bool| {
            let mut col = GridColumn::new(id, native_label(id), width, true, visible);
            col.picker_group = Some(trend_group.get());
            col
        };
        vec![
            GridColumn::new(
                COL_ITEM,
                t_string!(i18n, trends_col_item).to_string(),
                320.0,
                false,
                true,
            ),
            // Shared columns keep their own labels and groups; MarketGrid
            // fills them in. Placing them here fixes their position and
            // default visibility.
            GridColumn::new(COL_QUALITY, String::new(), 90.0, true, true),
            native(COL_TREND, 120.0, true).fixed_width(),
            GridColumn::new(COL_LISTING, String::new(), 130.0, true, true),
            native(COL_VWAP, 130.0, true),
            native(COL_PCT, 100.0, true),
            native(COL_SALES_PER_DAY, 120.0, true),
            native(COL_UNITS, 120.0, true),
            native(COL_SALES, 110.0, false),
            native(COL_CONFIDENCE, 130.0, true),
        ]
    });

    view! {
        <MetaTitle title=t_string!(i18n, trends_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, trends_meta_desc).to_string() />

        <div class="main-content p-2 sm:p-6">
            <div class="flex flex-col gap-6">
                <ToolHeader
                    title=t_string!(i18n, market_trends).to_string()
                    summary=t_string!(i18n, trends_tool_summary).to_string()
                    context=t_string!(i18n, trends_tool_context).to_string()
                    help_href="/help/market-trends"
                    help_body=t_string!(i18n, trends_tool_help).to_string()
                >
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, world)}</label>
                    <TrendsWorldNavigator />
                </ToolHeader>

                <div class="flex flex-col md:flex-row md:items-center gap-3">
                    <MarketWindowControl window />
                </div>

                // Market Heat band (gated on a selected world). Gives a
                // quick read on category-level sentiment before the detail
                // grid.
                {move || world_signal.with(|w| w.is_some()).then(|| view! {
                    <MarketHeat world=world_signal />
                })}

                <ControlBar
                    summary=move || {
                        view! {
                            <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                                {move || t!(i18n, trends_summary_results_count, n = move || filters.row_count())}
                            </span>
                            <span class="text-xs text-[color:var(--color-text-muted)] whitespace-nowrap truncate">
                                {move || format!("{}: {}", t_string!(i18n, trends_window_label), window_label(window.selected.get()))}
                            </span>
                        }
                        .into_any()
                    }
                    actions=move || {
                        view! { <GridSavedViews id="trends-grid" /> }.into_any()
                    }
                    columns=column_options
                    visible_columns=picker_visible
                    on_toggle_column=toggle_column
                    on_reset_columns=Callback::new(move |_| set_cols_param.set(None))
                    empty_label=Signal::derive(move || {
                        t_string!(i18n, trends_no_active_filters).to_string()
                    })
                />

                // Content
                <div class="min-h-[500px]">
                    <Suspense fallback=TrendsTableSkeleton>
                        {move || match trends_for_view.get() {
                            Some(Ok(Some(_))) => ().into_any(),
                            Some(Ok(None)) => view! {
                                <div class="text-xl text-[color:var(--color-text)] text-center p-8 bg-brand-900/20 rounded-2xl border border-white/10">
                                    {t!(i18n, trends_select_valid_world)}
                                </div>
                            }.into_any(),
                            Some(Err(e)) => view! {
                                <div class="text-xl text-red-400 text-center p-8 bg-red-950/20 rounded-2xl border border-red-500/30">
                                    {format!("Error loading trends: {}", e)}
                                </div>
                            }.into_any(),
                            None => view! { <TrendsTableSkeleton /> }.into_any(),
                        }}
                        // Keep the registry's column/count providers alive across
                        // empty categories, loading and errors. Only visibility changes.
                        <div class:hidden=move || !matches!(trends_for_visibility.get(), Some(Ok(Some(_))))>
                            <TrendsGrid rows market columns />
                        </div>
                    </Suspense>
                </div>
            </div>
        </div>
    }
}

#[component]
fn TrendsGrid(
    rows: Memo<Vec<Row>>,
    market: crate::analyzer_kit::market::MarketData,
    columns: Signal<Vec<GridColumn>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let world = market.scope;
    let subject = Arc::new(move |row: &Row| {
        let mut subject = MarketSubject::new(row.item_id, row.hq, row.world_id);
        subject.listing_price = Some(row.price);
        subject.label = item_name(row.item_id).unwrap_or_default();
        subject
    });
    let header_label = move |id: &'static str| -> String {
        match id {
            COL_ITEM => t_string!(i18n, trends_col_item).to_string(),
            COL_TREND => t_string!(i18n, trends_col_spark).to_string(),
            COL_VWAP => t_string!(i18n, trends_col_vwap).to_string(),
            COL_PCT => t_string!(i18n, trends_col_pct_change).to_string(),
            COL_SALES_PER_DAY => t_string!(i18n, trends_col_sales_per_day).to_string(),
            COL_UNITS => t_string!(i18n, trends_col_units_window).to_string(),
            COL_SALES => t_string!(i18n, trends_col_sales).to_string(),
            COL_CONFIDENCE => t_string!(i18n, trends_col_confidence).to_string(),
            _ => String::new(),
        }
    };
    let key = |row: &Row| (row.item_id, row.hq);
    // The item and the sparkline have no ranking; every other native column
    // sorts through the shared header.
    let header = move |id: &'static str| match id {
        COL_ITEM | COL_TREND => view! {
            <div class="w-full min-w-0">{header_label(id)}</div>
        }
        .into_any(),
        _ => view! {
            <MetricSortHeader column=id label=Signal::derive(move || header_label(id)) />
        }
        .into_any(),
    };
    let measure = move |row: &Row, id: &'static str| match id {
        COL_ITEM => (item_name(row.item_id).unwrap_or_default(), 110.0),
        COL_TREND => (String::new(), 96.0),
        COL_VWAP => (row.vwap_window.to_string(), 42.0),
        COL_PCT => (format_pct(row.pct_change_window), 24.0),
        COL_SALES_PER_DAY => (format!("{:.1}", row.sales_per_day), 24.0),
        COL_UNITS => (format_volume(row.unit_volume_window), 24.0),
        COL_SALES => (row.sales_in_window.to_string(), 24.0),
        COL_CONFIDENCE => (String::new(), 110.0),
        _ => (String::new(), 0.0),
    };
    let cell = move |row: Row, id: &'static str| {
        let item_id = row.item_id;
        match id {
            COL_ITEM => {
                let name = item_name(item_id).unwrap_or_else(|| t_string!(i18n, unknown).to_string());
                view! {
                    <div class="flex flex-row items-center gap-2 w-full min-w-0 px-3">
                        <a
                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full text-[color:var(--color-text)]"
                            href=format!("/item/{}/{item_id}", world.get())
                        >
                            <div class="shrink-0">
                                <ItemIcon item_id icon_size=IconSize::Small />
                            </div>
                            {name.clone()}
                        </a>
                        <AddToList item_id />
                        <Clipboard clipboard_text=name />
                    </div>
                }
                .into_any()
            }
            COL_TREND => view! {
                <div class="flex h-full w-full items-center justify-center px-3">
                    <Sparkline points=row.sparkline_24h.clone() pct_change=row.pct_change_window />
                </div>
            }
            .into_any(),
            COL_VWAP => view! {
                <div class="flex h-full w-full min-w-0 items-center justify-end px-3"><Gil amount=row.vwap_window /></div>
            }
            .into_any(),
            COL_PCT => view! {
                <div class=format!("flex h-full w-full min-w-0 items-center justify-end px-3 text-xs font-mono font-semibold {}", pct_class(row.pct_change_window))>
                    {format_pct(row.pct_change_window)}
                </div>
            }
            .into_any(),
            COL_SALES_PER_DAY => view! {
                <div class="flex h-full w-full min-w-0 items-center justify-end px-3 font-mono tabular-nums">{format!("{:.1}", row.sales_per_day)}</div>
            }
            .into_any(),
            COL_UNITS => view! {
                <div class="flex h-full w-full min-w-0 items-center justify-end px-3 font-mono tabular-nums">{format_volume(row.unit_volume_window)}</div>
            }
            .into_any(),
            COL_SALES => view! {
                <div class="flex h-full w-full min-w-0 items-center justify-end px-3 font-mono tabular-nums">{row.sales_in_window}</div>
            }
            .into_any(),
            COL_CONFIDENCE => view! {
                <div class="flex h-full w-full items-center justify-center px-3">
                    <ConfidenceBadge band=row.confidence_band sample_size=row.sample_size_30d />
                </div>
            }
            .into_any(),
            _ => ().into_any(),
        }
    };
    view! {
        <MarketGrid
            show_saved_views=false
            market
            subject
            metrics=trend_metrics()
            id="trends-grid"
            label=t_string!(i18n, market_trends).to_string()
            row_height=48.0
            columns
            each=rows
            key
            header
            measure
            view=cell
        />
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::virtual_grid::registry::{resolve_filters, resolve_sort};
    use leptos_router::params::ParamsMap;

    fn item(units: u64, sales: u32, price: i32) -> Arc<TrendItem> {
        Arc::new(TrendItem {
            item_id: 5,
            hq: false,
            price,
            world_id: 63,
            average_sale_price: 0.0,
            sales_per_week: 0.0,
            vwap_30d: 0,
            price_percentile_30d: 0,
            confidence_band: ConfidenceBand::Unknown,
            sample_size_30d: 0,
            launder_suspicion: 0.0,
            window_days: 30,
            vwap_window: 900,
            sales_in_window: sales,
            unit_volume_window: units,
            gil_volume_window: 0,
            sales_per_day: sales as f32 / 30.0,
            pct_change_window: 12.5,
            sparkline_24h: vec![0; 24],
        })
    }

    #[test]
    fn test_format_volume() {
        assert_eq!(format_volume(0), "0");
        assert_eq!(format_volume(999), "999");
        assert_eq!(format_volume(1_000), "1.0K");
        assert_eq!(format_volume(10_500), "10.5K");
        assert_eq!(format_volume(999_999), "1000.0K");
        assert_eq!(format_volume(1_000_000), "1.0M");
        assert_eq!(format_volume(1_500_000), "1.5M");
        assert_eq!(format_volume(999_999_999), "1000.0M");
    }

    #[test]
    fn every_legacy_sort_token_names_a_registered_metric() {
        // The old div table accepted exactly these tokens; each must still
        // rank the grid, through a native metric or the shared listing column.
        let native: Vec<_> = trend_metrics().into_iter().map(|m| m.id).collect();
        for (token, column) in [
            ("units", COL_UNITS),
            ("vwap", COL_VWAP),
            ("price", COL_LISTING),
            ("pct", COL_PCT),
            ("spd", COL_SALES_PER_DAY),
        ] {
            assert_eq!(
                resolve_sort(Some(token), &SORT_ALIASES).as_deref(),
                Some(column),
                "{token}"
            );
            assert!(
                native.contains(&column) || column == COL_LISTING,
                "{column} has no metric"
            );
        }
        assert_eq!(resolve_sort(Some("unknown_key"), &SORT_ALIASES), None);
        assert_eq!(resolve_sort(Some(""), &SORT_ALIASES), None);
    }

    #[test]
    fn legacy_chip_parameters_alias_the_metrics_they_measured() {
        let mut query = ParamsMap::new();
        query.insert("min_sales", "12".to_string());
        query.insert("min_price", "1000".to_string());
        let filters = resolve_filters(&query, &filter_aliases());
        // Min sales compared the cleaned sale count, not sales/day.
        let sales = &filters[COL_SALES];
        assert_eq!(sales.op, FilterOp::Gte);
        let metric = trend_metrics()
            .into_iter()
            .find(|m| m.id == COL_SALES)
            .unwrap();
        assert_eq!(
            sales.matches(&(metric.value)(&item(1, 12, 0)), false),
            Some(true)
        );
        assert_eq!(
            sales.matches(&(metric.value)(&item(1, 11, 0)), false),
            Some(false)
        );
        // Min price compared the observed listing, which the shared column carries.
        assert_eq!(filters[COL_LISTING].value, "1000");
        assert!(!filters.contains_key("price"));
    }

    #[test]
    fn native_metrics_expose_the_cleaned_sample_statistics() {
        // The confidence label is localized, so the metrics read i18n context.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            native_metrics_assertions();
        });
    }

    fn native_metrics_assertions() {
        let row = item(2_500, 90, 500);
        let value = |id: &str| {
            let metric = trend_metrics().into_iter().find(|m| m.id == id).unwrap();
            (metric.value)(&row)
        };
        assert_eq!(value(COL_UNITS), GridValue::Number(2_500.0));
        assert_eq!(value(COL_SALES), GridValue::Number(90.0));
        assert_eq!(value(COL_SALES_PER_DAY), GridValue::Number(3.0));
        assert_eq!(value(COL_VWAP), GridValue::Number(900.0));
        assert_eq!(value(COL_PCT), GridValue::Number(12.5));
        // No deep-scan verdict is a missing value, never a filterable band.
        assert_eq!(value(COL_CONFIDENCE), GridValue::Missing);
        let mut suspicious = (*row).clone();
        suspicious.confidence_band = ConfidenceBand::Unusable;
        let metric = trend_metrics()
            .into_iter()
            .find(|m| m.id == COL_CONFIDENCE)
            .unwrap();
        assert!(matches!(
            (metric.value)(&Arc::new(suspicious)),
            GridValue::Text(_)
        ));
    }

    #[test]
    fn column_picker_state_round_trips_native_and_shared_ids() {
        for col in DEFAULT_COLS {
            assert!(OPTIONAL_COLS.contains(col), "{col}");
        }
        assert_eq!(
            visible_cols(None),
            DEFAULT_COLS.iter().copied().collect::<HashSet<_>>()
        );
        let raw = "trend,market-sale-median,unknown";
        assert_eq!(
            visible_cols(Some(raw)),
            HashSet::from([COL_TREND, "market-sale-median"])
        );
        let toggled = toggle_shared_col(None, &default_cols_query(), COL_SALES);
        assert!(toggled.split(',').any(|id| id == COL_SALES));
        assert!(toggled.split(',').any(|id| id == COL_TREND));
        assert!(
            visible_cols(Some(&toggle_shared_col(
                Some(&toggled),
                &default_cols_query(),
                COL_TREND
            )))
            .contains(COL_SALES)
        );
    }

    #[test]
    fn pct_formatting_matches_the_old_table() {
        assert_eq!(format_pct(0.0), "—");
        assert_eq!(format_pct(0.04), "—");
        assert_eq!(format_pct(12.34), "+12.3%");
        assert_eq!(format_pct(-3.0), "-3.0%");
        assert_eq!(pct_class(1.0), "text-emerald-300");
        assert_eq!(pct_class(-1.0), "text-red-300");
    }
}
