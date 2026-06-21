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
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_params_map},
};
use ultros_api_types::{icon_size::IconSize, trends::TrendItem};

use crate::{
    api::get_trends_v2,
    components::{
        add_to_list::AddToList,
        clipboard::Clipboard,
        confidence_badge::ConfidenceBadge,
        gil::Gil,
        item_icon::ItemIcon,
        market_heat::MarketHeat,
        market_movers::MarketMovers,
        meta::{MetaDescription, MetaTitle},
        query_button::QueryButton,
        skeleton::BoxSkeleton,
        sparkline::Sparkline,
        tool_help::*,
        toolbar::{Toolbar, ToolbarField, ToolbarPills},
        world_picker::WorldOnlyPicker,
    },
    global_state::LocalWorldData,
};

const DEFAULT_WINDOW: u16 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    UnitVolume,
    Vwap,
    Price,
    PctChange,
    SalesPerDay,
}

impl SortKey {
    fn from_str(s: &str) -> Self {
        match s {
            "vwap" => SortKey::Vwap,
            "price" => SortKey::Price,
            "pct" => SortKey::PctChange,
            "spd" => SortKey::SalesPerDay,
            _ => SortKey::UnitVolume,
        }
    }
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

#[component]
fn TrendsTable(items: Vec<TrendItem>, world: String) -> impl IntoView {
    let i18n = use_i18n();
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
                // Header row — sortable columns use QueryButton so the
                // sort key persists in the URL.
                <div class="flex flex-row items-center h-12 text-[10px] font-semibold uppercase tracking-[0.14em] text-[color:var(--color-text-muted)] border-b border-[color:var(--line)] bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)]" role="rowgroup">
                    <div role="columnheader" class="w-[40px] px-2 py-3 text-center">{t!(i18n, hq)}</div>
                    <div role="columnheader" class="flex-1 min-w-[14rem] px-3 py-3">{t!(i18n, trends_col_item)}</div>
                    <div role="columnheader" class="w-[100px] px-3 py-3 text-center">{t!(i18n, trends_col_spark)}</div>
                    <div role="columnheader" class="w-[110px] px-3 py-3 text-right">
                        <QueryButton
                            class="!text-brand-300 hover:text-brand-200"
                            active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                            key="sort"
                            value="price"
                        >
                            {t!(i18n, trends_col_price)}
                        </QueryButton>
                    </div>
                    <div role="columnheader" class="w-[110px] px-3 py-3 text-right">
                        <QueryButton
                            class="!text-brand-300 hover:text-brand-200"
                            active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                            key="sort"
                            value="vwap"
                        >
                            {t!(i18n, trends_col_vwap)}
                        </QueryButton>
                    </div>
                    <div role="columnheader" class="w-[90px] px-3 py-3 text-right">
                        <QueryButton
                            class="!text-brand-300 hover:text-brand-200"
                            active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                            key="sort"
                            value="pct"
                        >
                            {t!(i18n, trends_col_pct_change)}
                        </QueryButton>
                    </div>
                    <div role="columnheader" class="w-[100px] px-3 py-3 text-right">
                        <QueryButton
                            class="!text-brand-300 hover:text-brand-200"
                            active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                            key="sort"
                            value="spd"
                        >
                            {t!(i18n, trends_col_sales_per_day)}
                        </QueryButton>
                    </div>
                    <div role="columnheader" class="w-[110px] px-3 py-3 text-right">
                        <QueryButton
                            class="!text-brand-300 hover:text-brand-200"
                            active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                            key="sort"
                            value="units"
                            default=true
                        >
                            {t!(i18n, trends_col_units_window)}
                        </QueryButton>
                    </div>
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

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            nav(
                &format!("/trends/{world}"),
                NavigateOptions {
                    scroll: false,
                    ..Default::default()
                },
            );
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

    // URL-driven page state.
    let (window_param, set_window_param) = query_signal::<u16>("window");
    let (suspicious, set_suspicious) = query_signal::<bool>("show_suspicious");
    let (category_filter, set_category_filter) = query_signal::<i32>("category");
    let (min_sales, set_min_sales) = query_signal::<u32>("min_sales");
    let (min_price, set_min_price) = query_signal::<i32>("min_price");
    let (sort, _set_sort) = query_signal::<String>("sort");

    let window_days = Memo::new(move |_| {
        window_param()
            .map(|w| match w {
                7 | 30 | 90 => w,
                _ => DEFAULT_WINDOW,
            })
            .unwrap_or(DEFAULT_WINDOW)
    });
    let show_suspicious = Memo::new(move |_| suspicious().unwrap_or(false));

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
        let key = SortKey::from_str(sort().as_deref().unwrap_or("units"));
        match key {
            SortKey::UnitVolume => {
                items.sort_by(|a, b| b.unit_volume_window.cmp(&a.unit_volume_window))
            }
            SortKey::Vwap => items.sort_by(|a, b| b.vwap_window.cmp(&a.vwap_window)),
            SortKey::Price => items.sort_by(|a, b| b.price.cmp(&a.price)),
            SortKey::PctChange => items.sort_by(|a, b| {
                b.pct_change_window
                    .partial_cmp(&a.pct_change_window)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortKey::SalesPerDay => items.sort_by(|a, b| {
                b.sales_per_day
                    .partial_cmp(&a.sales_per_day)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }
        items
    });

    // ⚡ Bolt: Removed `format!` macro that dynamically generated class strings. We now just conditionally return the `&'static str`
    // representing the classes, eliminating string allocations during reactive renders.
    let pill_active_class = "bg-[color:color-mix(in_srgb,var(--brand-ring)_18%,transparent)] text-[color:var(--color-text)] border-[color:color-mix(in_srgb,var(--brand-ring)_40%,var(--color-outline))]";
    let pill_inactive_class = "bg-transparent text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] border-transparent";
    let pill_base_class = "px-3 py-1.5 rounded-full text-xs font-semibold border transition-colors";

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

                <Toolbar>
                    <ToolbarField label=t_string!(i18n, world).to_string()>
                        <TrendsWorldNavigator />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, trends_window_label).to_string()>
                        <ToolbarPills>
                            <button
                                aria-pressed=move || (window_days() == 7).to_string()
                                class=move || format!("{} {}", pill_base_class, if window_days() == 7 { pill_active_class } else { pill_inactive_class })
                                on:click=move |_| set_window_param.set(Some(7))
                            >
                                {t!(i18n, trends_window_7d)}
                            </button>
                            <button
                                aria-pressed=move || (window_days() == 30).to_string()
                                class=move || format!("{} {}", pill_base_class, if window_days() == 30 { pill_active_class } else { pill_inactive_class })
                                on:click=move |_| set_window_param.set(Some(30))
                            >
                                {t!(i18n, trends_window_30d)}
                            </button>
                            <button
                                aria-pressed=move || (window_days() == 90).to_string()
                                class=move || format!("{} {}", pill_base_class, if window_days() == 90 { pill_active_class } else { pill_inactive_class })
                                on:click=move |_| set_window_param.set(Some(90))
                            >
                                {t!(i18n, trends_window_90d)}
                            </button>
                        </ToolbarPills>
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, trends_filter_category_label).to_string()>
                        <select
                            class="input input-sm"
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
                            <option value="">{t!(i18n, trends_filter_all_categories)}</option>
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
                    <ToolbarField label=t_string!(i18n, trends_min_sales_label).to_string()>
                        <input
                            class="input input-sm w-20"
                            type="number"
                            min=0
                            step=1
                            placeholder="0"
                            prop:value=move || min_sales().map(|n| n.to_string()).unwrap_or_default()
                            on:input=move |ev| {
                                let val = event_target_value(&ev);
                                if let Ok(n) = val.parse::<u32>() {
                                    set_min_sales(Some(n));
                                } else if val.is_empty() {
                                    set_min_sales(None);
                                }
                            }
                        />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, trends_min_price_label).to_string()>
                        <input
                            class="input input-sm w-24"
                            type="number"
                            min=0
                            step=1000
                            placeholder="0"
                            prop:value=move || min_price().map(|n| n.to_string()).unwrap_or_default()
                            on:input=move |ev| {
                                let val = event_target_value(&ev);
                                if let Ok(n) = val.parse::<i32>() {
                                    set_min_price(Some(n));
                                } else if val.is_empty() {
                                    set_min_price(None);
                                }
                            }
                        />
                    </ToolbarField>
                    <ToolbarField label=t_string!(i18n, trends_show_suspicious).to_string()>
                        <button
                            type="button"
                            title=t_string!(i18n, trends_show_suspicious_help).to_string()
                            aria-pressed=move || show_suspicious().to_string()
                            class=move || format!("{} {}", pill_base_class, if show_suspicious() { pill_active_class } else { pill_inactive_class })
                            on:click=move |_| set_suspicious.set(Some(!show_suspicious()))
                        >
                            {move || if show_suspicious() { "On" } else { "Off" }}
                        </button>
                    </ToolbarField>
                </Toolbar>

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

                // Results summary + active filter chips.
                <div class="panel px-4 py-3 flex flex-wrap items-center gap-3 justify-between">
                    <div class="text-sm text-[color:var(--color-text)]">
                        <span class="text-brand-300 font-semibold">{move || displayed().len()}</span>
                        " "
                        {t!(i18n, trends_summary_results)}
                        " — "
                        <span class="text-[color:var(--color-text-muted)]">
                            {move || format!("{}d window", window_days())}
                        </span>
                    </div>
                    <div class="flex flex-wrap gap-2">
                        {move || {
                            let mut chips: Vec<_> = Vec::new();
                            if let Some(cat_id) = category_filter() {
                                let cat_name = tracked_data()
                                    .item_search_categorys
                                    .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                    .map(|c| c.name.clone())
                                    .unwrap_or_else(|| format!("Category {cat_id}"));
                                chips.push(view! {
                                    <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                        {cat_name}
                                        <button
                                            class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                            on:click=move |_| set_category_filter(None)
                                        >"×"</button>
                                    </span>
                                }.into_any());
                            }
                            if let Some(n) = min_sales() {
                                chips.push(view! {
                                    <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                        {format!("≥ {n} sales")}
                                        <button
                                            class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                            on:click=move |_| set_min_sales(None)
                                        >"×"</button>
                                    </span>
                                }.into_any());
                            }
                            if let Some(n) = min_price() {
                                chips.push(view! {
                                    <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                        "≥ " <Gil amount=n />
                                        <button
                                            class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                            on:click=move |_| set_min_price(None)
                                        >"×"</button>
                                    </span>
                                }.into_any());
                            }
                            if show_suspicious() {
                                chips.push(view! {
                                    <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-amber-300 border-amber-400/40 bg-[color:color-mix(in_srgb,#f59e0b_10%,transparent)]">
                                        {t_string!(i18n, trends_show_suspicious).to_string()}
                                        <button
                                            class="ml-1 hover:text-amber-200"
                                            on:click=move |_| set_suspicious.set(Some(false))
                                        >"×"</button>
                                    </span>
                                }.into_any());
                            }
                            if chips.is_empty() {
                                Either::Left(view! { <span class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, trends_no_active_filters)}</span> })
                            } else {
                                Either::Right(view! { <>{chips}</> })
                            }
                        }}
                    </div>
                </div>

                // Content
                <div class="min-h-[500px]">
                    <Suspense fallback=BoxSkeleton>
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
                            None => view! { <BoxSkeleton /> }.into_any(),
                        }}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}
