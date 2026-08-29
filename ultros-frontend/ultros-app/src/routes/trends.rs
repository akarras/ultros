//! Market Trends — ClickHouse-backed table.
//!
//! The page sources its rows from `item_stats_window` via
//! `get_trends_v2`: a flat list of items the rollup knows about on the
//! current world, with per-row VWAP, sales/day, unit volume, and a 24h
//! sparkline. The user picks a window (7/30/90d), and FE filter/sort
//! state lives in the URL so links are shareable.
//!
//! The MarketHeat band + MarketMovers strip from the home page are
//! reused at the top of the page — they answer "what's hot right now"
//! at a glance; the table below is the deep-dive.

use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::filter_query_signal;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_location, use_navigate, use_params_map, use_query_map},
};
use ultros_api_types::{icon_size::IconSize, trends::TrendItem};

use crate::{
    api::get_trends_v2,
    components::{
        add_to_list::AddToList,
        clipboard::Clipboard,
        confidence_badge::ConfidenceBadge,
        control_bar::{ControlBar, FilterOption},
        filter_chip::FilterChip,
        gil::Gil,
        item_icon::ItemIcon,
        market_heat::MarketHeat,
        market_movers::MarketMovers,
        meta::{MetaDescription, MetaTitle},
        skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton},
        sort_header::{SortColumn, SortDir, SortableHeaderCell},
        sparkline::Sparkline,
        tool_help::*,
        world_picker::WorldOnlyPicker,
    },
    global_state::LocalWorldData,
    routes::world_nav::world_nav_url,
};

const DEFAULT_WINDOW: u16 = 30;

// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract (mirrors the analyzer/currency-exchange convention).
const FILTER_CATEGORY: &str = "category";
const FILTER_MIN_SALES: &str = "min_sales";
const FILTER_MIN_PRICE: &str = "min_price";
const FILTER_SUSPICIOUS: &str = "show_suspicious";

/// Filters the `+ Filter` menu can add, in the old toolbar's left-to-right
/// order.
const ADDABLE_FILTERS: &[&str] = &[
    FILTER_CATEGORY,
    FILTER_MIN_SALES,
    FILTER_MIN_PRICE,
    FILTER_SUSPICIOUS,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    UnitVolume,
    Vwap,
    Price,
    PctChange,
    SalesPerDay,
}

impl std::str::FromStr for SortKey {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "units" => Ok(SortKey::UnitVolume),
            "vwap" => Ok(SortKey::Vwap),
            "price" => Ok(SortKey::Price),
            "pct" => Ok(SortKey::PctChange),
            "spd" => Ok(SortKey::SalesPerDay),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortKey::UnitVolume => "units",
            SortKey::Vwap => "vwap",
            SortKey::Price => "price",
            SortKey::PctChange => "pct",
            SortKey::SalesPerDay => "spd",
        })
    }
}

/// Every column reads biggest-first descending, so the shared default
/// direction applies unchanged.
impl SortColumn for SortKey {
    fn fallback() -> Self {
        SortKey::UnitVolume
    }
}

fn compare_trends(key: SortKey, a: &TrendItem, b: &TrendItem) -> std::cmp::Ordering {
    match key {
        SortKey::UnitVolume => a.unit_volume_window.cmp(&b.unit_volume_window),
        SortKey::Vwap => a.vwap_window.cmp(&b.vwap_window),
        SortKey::Price => a.price.cmp(&b.price),
        SortKey::PctChange => a
            .pct_change_window
            .partial_cmp(&b.pct_change_window)
            .unwrap_or(std::cmp::Ordering::Equal),
        SortKey::SalesPerDay => a
            .sales_per_day
            .partial_cmp(&b.sales_per_day)
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

/// Intern a category id as a `&'static str` token for
/// [`FilterChip`](crate::components::filter_chip::FilterChip)'s
/// `(&'static str, String)` options contract.
///
/// `item_search_categorys` is a small, fixed-size table read from the
/// process-lifetime game data (`xiv_gen_db::data()`), so the set of ids ever
/// asked for here is bounded — each one is leaked exactly once and cached,
/// never per-render, so this cannot grow unbounded over a long session.
fn category_id_token(id: i32) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<i32, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("category token cache poisoned");
    guard
        .entry(id)
        .or_insert_with(|| Box::leak(id.to_string().into_boxed_str()))
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

/// [`TrendsTable`]'s loading state, drawn from the same column geometry.
///
/// Every column here mirrors the corresponding cell class in the table below;
/// keep them in step or the skeleton's columns will drift away from the real
/// ones. Trends has no responsive column hiding — the whole grid sits behind
/// one `min-w-[940px]` — so the skeleton needs no breakpoint classes either.
#[component]
fn TrendsTableSkeleton() -> impl IntoView {
    let columns = vec![
        // HQ, blank on most rows.
        SkeletonColumn::new(
            "px-2 py-2 w-[40px] flex items-center justify-center",
            SkeletonCell::Blank,
        ),
        SkeletonColumn::new(
            "px-3 py-2 flex flex-row flex-1 min-w-[14rem] items-center gap-2",
            SkeletonCell::IconText,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[100px] flex items-center justify-center",
            SkeletonCell::Spark,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[110px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[110px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[90px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[100px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[110px] text-right flex items-center justify-end",
            SkeletonCell::Number,
        ),
        SkeletonColumn::new(
            "px-3 py-2 w-[110px] flex items-center justify-center",
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
fn TrendsTable(items: Vec<TrendItem>, world: String) -> impl IntoView {
    let i18n = use_i18n();
    // Same URL params `Trends` sorts by — the header cells only need to read
    // and rewrite them, so a second `query_signal` on the same keys is fine.
    let (sort_mode, _set_sort_mode) = query_signal::<SortKey>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    let items = Memo::new(move |_| {
        items
            .iter()
            .cloned()
            .enumerate()
            .collect::<Vec<(usize, TrendItem)>>()
    });

    view! {
        <div class="overflow-x-auto content-visible contain-layout contain-paint will-change-scroll forced-layer rounded-lg border border-[color:var(--color-outline)]">
            <div class="min-w-[940px]">
                // Header row — sortable columns use the shared SortableHeaderCell
                // so the sort key and direction persist in the URL.
                <div class="flex flex-row items-center h-12 text-[10px] font-semibold uppercase tracking-[0.14em] text-[color:var(--color-text-muted)] border-b border-[color:var(--line)] bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)]" role="rowgroup">
                    <div role="columnheader" class="w-[40px] px-2 py-3 text-center">{t!(i18n, hq)}</div>
                    <div role="columnheader" class="flex-1 min-w-[14rem] px-3 py-3">{t!(i18n, trends_col_item)}</div>
                    <div role="columnheader" class="w-[100px] px-3 py-3 text-center">{t!(i18n, trends_col_spark)}</div>
                    <SortableHeaderCell
                        mode=SortKey::Price
                        label=t_string!(i18n, trends_col_price).to_string()
                        class="w-[110px] px-3 py-3 text-right"
                        sort_mode
                        sort_dir
                    />
                    <SortableHeaderCell
                        mode=SortKey::Vwap
                        label=t_string!(i18n, trends_col_vwap).to_string()
                        class="w-[110px] px-3 py-3 text-right"
                        sort_mode
                        sort_dir
                    />
                    <SortableHeaderCell
                        mode=SortKey::PctChange
                        label=t_string!(i18n, trends_col_pct_change).to_string()
                        class="w-[90px] px-3 py-3 text-right"
                        sort_mode
                        sort_dir
                    />
                    <SortableHeaderCell
                        mode=SortKey::SalesPerDay
                        label=t_string!(i18n, trends_col_sales_per_day).to_string()
                        class="w-[100px] px-3 py-3 text-right"
                        sort_mode
                        sort_dir
                    />
                    <SortableHeaderCell
                        mode=SortKey::UnitVolume
                        label=t_string!(i18n, trends_col_units_window).to_string()
                        class="w-[110px] px-3 py-3 text-right"
                        sort_mode
                        sort_dir
                    />
                    <div role="columnheader" class="w-[110px] px-3 py-3 text-center">{t!(i18n, trends_col_quality)}</div>
                </div>

                // Rows. No virtual scroller — the response is capped at
                // 500 rows and filters narrow it further. A virtual
                // scroller would buy us less than the wiring cost given
                // we render only what passes filters anyway.
                {move || {
                    let world_for_pass = world.clone();
                    items.get().into_iter().map(move |(index, item): (usize, TrendItem)| {
                        let world = world_for_pass.clone();
                        let item_id = item.item_id;
                        let item_data = tracked_data().items.get(&xiv_gen::ItemId(item_id));
                        let item_name = item_data.map(|i| i.name.as_str()).unwrap_or("Unknown Item").to_string();
                        let icon_loading = if index < 20 { "eager" } else { "" };
                        let classes = "flex flex-row items-center flex-nowrap h-12 border-b border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] transition-colors";
                        let pct = item.pct_change_window;
                        let pct_class = if pct > 0.0 {
                            "text-emerald-300"
                        } else if pct < 0.0 {
                            "text-red-300"
                        } else {
                            "text-[color:var(--color-text-muted)]"
                        };
                        let pct_text = if pct.abs() < 0.05 {
                            "—".to_string()
                        } else if pct >= 0.0 {
                            format!("+{pct:.1}%")
                        } else {
                            format!("{pct:.1}%")
                        };

                        view! {
                            <div class=classes role="row-group">
                                <div role="cell" class="px-2 py-2 w-[40px] flex items-center justify-center">
                                    {if item.hq {
                                        Some(view! {
                                            <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">
                                                {t!(i18n, hq)}
                                            </span>
                                        })
                                    } else { None }}
                                </div>
                                <div role="cell" class="px-3 py-2 flex flex-row flex-1 min-w-[14rem] items-center gap-2">
                                    <a
                                        class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full text-[color:var(--color-text)]"
                                        href=format!("/item/{}/{item_id}", world)
                                    >
                                        <div class="shrink-0">
                                            <ItemIcon item_id icon_size=IconSize::Small loading=icon_loading />
                                        </div>
                                        {item_name.clone()}
                                    </a>
                                    <AddToList item_id />
                                    <Clipboard clipboard_text=item_name />
                                </div>
                                <div role="cell" class="px-3 py-2 w-[100px] flex items-center justify-center">
                                    <Sparkline points=item.sparkline_24h pct_change=pct />
                                </div>
                                <div role="cell" class="px-3 py-2 w-[110px] text-right flex items-center justify-end">
                                    <Gil amount=item.price />
                                </div>
                                <div role="cell" class="px-3 py-2 w-[110px] text-right flex items-center justify-end">
                                    <Gil amount=item.vwap_window />
                                </div>
                                <div role="cell" class=format!("px-3 py-2 w-[90px] text-right flex items-center justify-end text-xs font-mono font-semibold {pct_class}")>
                                    {pct_text}
                                </div>
                                <div role="cell" class="px-3 py-2 w-[100px] text-right flex items-center justify-end text-[color:var(--color-text)] font-mono tabular-nums">
                                    {format!("{:.1}", item.sales_per_day)}
                                </div>
                                <div role="cell" class="px-3 py-2 w-[110px] text-right flex items-center justify-end text-[color:var(--color-text)] font-mono tabular-nums">
                                    {format_volume(item.unit_volume_window)}
                                </div>
                                <div role="cell" class="px-3 py-2 w-[110px] flex items-center justify-center">
                                    <ConfidenceBadge band=item.confidence_band sample_size=item.sample_size_30d />
                                </div>
                            </div>
                        }
                    }).collect_view()
                }}
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_sort_key_round_trips() {
        // Display must produce exactly the token FromStr parses back — the
        // shared SortHeader's hrefs depend on that round trip.
        for key in [
            SortKey::UnitVolume,
            SortKey::Vwap,
            SortKey::Price,
            SortKey::PctChange,
            SortKey::SalesPerDay,
        ] {
            assert_eq!(key.to_string().parse::<SortKey>(), Ok(key));
        }

        // Unknown or empty `?sort=` no longer parses silently — the route
        // falls back through `SortColumn::fallback()` instead.
        assert!("unknown_key".parse::<SortKey>().is_err());
        assert!("".parse::<SortKey>().is_err());
        assert_eq!(SortKey::fallback(), SortKey::UnitVolume);
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
    let query = use_query_map();
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

    // URL-driven page state. `window` is a persistent view mode (like a tab),
    // not a filter, so it stays on plain `query_signal`; the rest use
    // `filter_query_signal` (replace: true, scroll: false) so typing into a
    // chip writes the URL on every keystroke without pushing a history entry
    // or yanking the window to the top.
    let (window_param, set_window_param) = query_signal::<u16>("window");
    let (suspicious, set_suspicious) = filter_query_signal::<bool>(FILTER_SUSPICIOUS);
    let (category_filter, set_category_filter) = filter_query_signal::<i32>(FILTER_CATEGORY);
    let (min_sales, set_min_sales) = filter_query_signal::<u32>(FILTER_MIN_SALES);
    let (min_price, set_min_price) = filter_query_signal::<i32>(FILTER_MIN_PRICE);
    let (sort, _set_sort) = query_signal::<SortKey>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");

    // A filter picked from the `+ Filter` menu but not yet committed — its
    // chip mounts in edit state with an empty input (see currency_exchange.rs
    // for the same pattern). The category select has no "obviously correct"
    // default, so it mounts blank too.
    let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

    let window_days = Memo::new(move |_| {
        window_param()
            .map(|w| match w {
                7 | 30 | 90 => w,
                _ => DEFAULT_WINDOW,
            })
            .unwrap_or(DEFAULT_WINDOW)
    });
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
    let trends_for_displayed = trends.clone();
    let trends_for_view = trends;

    let world_signal: Signal<Option<String>> = Signal::derive(move || {
        let w = world();
        if w.is_empty() { None } else { Some(w) }
    });

    // Filter + sort the loaded payload.
    let displayed = Memo::new(move |_| {
        let data = match trends_for_displayed.get() {
            Some(Ok(Some(d))) => d,
            _ => return Vec::new(),
        };
        let mut items: Vec<TrendItem> = data
            .items
            .into_iter()
            .filter(|it| {
                category_filter()
                    .map(|cat| {
                        tracked_data()
                            .items
                            .get(&xiv_gen::ItemId(it.item_id))
                            .map(|i| i.item_search_category == cat)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(|it| min_sales().map(|m| it.sales_in_window >= m).unwrap_or(true))
            .filter(|it| min_price().map(|m| it.price >= m).unwrap_or(true))
            .collect();
        let key = sort().unwrap_or_else(SortKey::fallback);
        let dir = sort_dir().unwrap_or_else(|| key.default_dir());
        items.sort_by(|a, b| {
            let ord = compare_trends(key, a, b);
            match dir {
                SortDir::Asc => ord,
                SortDir::Desc => ord.reverse(),
            }
        });
        items
    });

    let pill_active_combined_class = "px-3 py-1.5 rounded-full text-xs font-semibold border transition-colors bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--color-text)] border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))]";
    let pill_inactive_combined_class = "px-3 py-1.5 rounded-full text-xs font-semibold border transition-colors bg-transparent text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] border-transparent";

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

    // Filters currently drawn as a chip. Drives the "no active filters" hint
    // and keeps `+ Filter` from offering a second copy of something the user
    // can already see.
    let active_filters = Memo::new(move |_| {
        let mut active: Vec<&'static str> = Vec::new();
        if category_filter().is_some() || pending_filter.get() == Some(FILTER_CATEGORY) {
            active.push(FILTER_CATEGORY);
        }
        if min_sales().is_some() || pending_filter.get() == Some(FILTER_MIN_SALES) {
            active.push(FILTER_MIN_SALES);
        }
        if min_price().is_some() || pending_filter.get() == Some(FILTER_MIN_PRICE) {
            active.push(FILTER_MIN_PRICE);
        }
        if show_suspicious() {
            active.push(FILTER_SUSPICIOUS);
        }
        active
    });

    // Menu label for a filter: the long, explanatory label the old toolbar
    // fields carried.
    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_CATEGORY => t_string!(i18n, trends_filter_category_label).to_string(),
            FILTER_MIN_SALES => t_string!(i18n, trends_min_sales_label).to_string(),
            FILTER_MIN_PRICE => t_string!(i18n, trends_min_price_label).to_string(),
            FILTER_SUSPICIOUS => t_string!(i18n, trends_show_suspicious).to_string(),
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
        FILTER_CATEGORY => pending_filter.set(Some(FILTER_CATEGORY)),
        FILTER_MIN_SALES => pending_filter.set(Some(FILTER_MIN_SALES)),
        FILTER_MIN_PRICE => pending_filter.set(Some(FILTER_MIN_PRICE)),
        // Boolean toggle: the chip's presence *is* the value, so it commits
        // straight to `true` rather than mounting an editable chip.
        FILTER_SUSPICIOUS => set_suspicious(Some(true)),
        _ => {}
    });

    let clear_all = Callback::new(move |_| {
        pending_filter.set(None);
        set_category_filter(None);
        set_min_sales(None);
        set_min_price(None);
        set_suspicious(None);
    });

    view! {
        <MetaTitle title=t_string!(i18n, trends_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, trends_meta_desc).to_string() />

        <div class="main-content p-6">
            <div class="flex flex-col gap-6 max-w-7xl mx-auto">
                <ToolHeader
                    title=t_string!(i18n, market_trends).to_string()
                    summary=t_string!(i18n, trends_tool_summary).to_string()
                    context=t_string!(i18n, trends_tool_context).to_string()
                    help_href="/help/market-trends"
                    help_body=t_string!(i18n, trends_tool_help).to_string()
                />

                <div class="flex flex-col md:flex-row md:items-center gap-3">
                    <div class="flex flex-col md:flex-row items-center gap-2">
                        <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, world)}</label>
                        <div class="w-full md:w-auto">
                            <TrendsWorldNavigator />
                        </div>
                    </div>
                    <div class="flex flex-col gap-1">
                        <span class="toolbar-field-label">{t!(i18n, trends_window_label)}</span>
                        <div class="toolbar-pills">
                            <button
                                aria-pressed=move || (window_days() == 7).to_string()
                                class=move || if window_days() == 7 { pill_active_combined_class } else { pill_inactive_combined_class }
                                on:click=move |_| set_window_param.set(Some(7))
                            >
                                {t!(i18n, trends_window_7d)}
                            </button>
                            <button
                                aria-pressed=move || (window_days() == 30).to_string()
                                class=move || if window_days() == 30 { pill_active_combined_class } else { pill_inactive_combined_class }
                                on:click=move |_| set_window_param.set(Some(30))
                            >
                                {t!(i18n, trends_window_30d)}
                            </button>
                            <button
                                aria-pressed=move || (window_days() == 90).to_string()
                                class=move || if window_days() == 90 { pill_active_combined_class } else { pill_inactive_combined_class }
                                on:click=move |_| set_window_param.set(Some(90))
                            >
                                {t!(i18n, trends_window_90d)}
                            </button>
                        </div>
                    </div>
                </div>

                // Market Heat band (gated on a selected world). Gives a
                // quick read on category-level sentiment before the detail
                // table.
                {move || world_signal.with(|w| w.is_some()).then(|| view! {
                    <MarketHeat world=world_signal />
                })}

                // Market Movers — same component as the home page,
                // complements the detail table below with the 24h
                // rising/falling/units view.
                {move || world_signal.with(|w| w.is_some()).then(|| view! {
                    <MarketMovers world=world_signal />
                })}

                <ControlBar
                    summary=move || {
                        view! {
                            <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                                {move || t!(i18n, trends_summary_results_count, n = move || displayed().len())}
                            </span>
                            <span class="text-xs text-[color:var(--color-text-muted)] whitespace-nowrap truncate">
                                {move || format!("{}d window", window_days())}
                            </span>
                        }
                        .into_any()
                    }
                    available_filters=Signal::derive(filter_options)
                    on_add_filter=add_filter
                    on_clear_all=clear_all
                    empty_label=Signal::derive(move || {
                        t_string!(i18n, trends_no_active_filters).to_string()
                    })
                    is_empty=Signal::derive(move || active_filters().is_empty())
                >
                    {move || {
                        (category_filter().is_some() || pending_filter.get() == Some(FILTER_CATEGORY))
                            .then(|| {
                                let start_editing = pending_filter.get_untracked() == Some(FILTER_CATEGORY);
                                view! {
                                    <FilterChip
                                        label=t_string!(i18n, trends_filter_category_label).to_string()
                                        value=Signal::derive(move || category_filter().map(|c| category_id_token(c).to_string()))
                                        options=category_options()
                                        start_editing=start_editing
                                        on_commit=Callback::new(move |v: Option<String>| {
                                            set_category_filter(v.and_then(|v| v.parse().ok()));
                                            if pending_filter.get_untracked() == Some(FILTER_CATEGORY) {
                                                pending_filter.set(None);
                                            }
                                        })
                                    />
                                }
                            })
                    }}
                    {move || {
                        (min_sales().is_some() || pending_filter.get() == Some(FILTER_MIN_SALES))
                            .then(|| {
                                let start_editing = pending_filter.get_untracked() == Some(FILTER_MIN_SALES);
                                view! {
                                    <FilterChip
                                        label=t_string!(i18n, trends_min_sales_label).to_string()
                                        value=Signal::derive(move || min_sales().map(|v| v.to_string()))
                                        numeric=true
                                        min="0"
                                        step="1"
                                        start_editing=start_editing
                                        on_commit=Callback::new(move |v: Option<String>| {
                                            set_min_sales(v.and_then(|v| v.parse().ok()));
                                            if pending_filter.get_untracked() == Some(FILTER_MIN_SALES) {
                                                pending_filter.set(None);
                                            }
                                        })
                                    />
                                }
                            })
                    }}
                    {move || {
                        (min_price().is_some() || pending_filter.get() == Some(FILTER_MIN_PRICE))
                            .then(|| {
                                let start_editing = pending_filter.get_untracked() == Some(FILTER_MIN_PRICE);
                                view! {
                                    <FilterChip
                                        label=t_string!(i18n, trends_min_price_label).to_string()
                                        value=Signal::derive(move || min_price().map(|v| v.to_string()))
                                        numeric=true
                                        min="0"
                                        step="1000"
                                        start_editing=start_editing
                                        on_commit=Callback::new(move |v: Option<String>| {
                                            set_min_price(v.and_then(|v| v.parse().ok()));
                                            if pending_filter.get_untracked() == Some(FILTER_MIN_PRICE) {
                                                pending_filter.set(None);
                                            }
                                        })
                                    />
                                }
                            })
                    }}
                    {move || {
                        show_suspicious()
                            .then(|| {
                                view! {
                                    <FilterChip
                                        label=t_string!(i18n, trends_show_suspicious).to_string()
                                        readonly=true
                                        value=Signal::derive(|| None::<String>)
                                        on_commit=Callback::new(move |_| set_suspicious(None))
                                    />
                                }
                            })
                    }}
                </ControlBar>

                // Content
                <div class="min-h-[500px]">
                    <Suspense fallback=TrendsTableSkeleton>
                        {move || match trends_for_view.get() {
                            Some(Ok(Some(_))) => {
                                let items = displayed();
                                if items.is_empty() {
                                    view! {
                                        <div class="text-xl text-[color:var(--color-text)] text-center p-8 bg-brand-900/20 rounded-2xl border border-white/10">
                                            {t!(i18n, trends_empty_filtered)}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <TrendsTable items=items world=world() /> }.into_any()
                                }
                            },
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
                    </Suspense>
                </div>
            </div>
        </div>
    }
}
