//! One-row control surface for the price chart (spec 2 of the chart
//! revamp): icon-only mode group, group-by dropdown chip, overlays popover
//! with a count badge. The resolved state is spelled out by the caption
//! line under the chart, which is what makes an icon-only toolbar viable —
//! every icon button still carries an aria-label.
//!
//! Slots are deliberately left between the groups for spec 3's view toggle
//! and world filter.

use icondata as i;
use leptos::prelude::*;
use ultros_charts::charts::ChartMode;
use ultros_charts::data::grouping::GroupLevel;

use crate::components::icon::Icon;
use crate::i18n::{t_string, use_i18n};

/// Overlay = one chart with every series overlaid; Grid = one small chart
/// per series with a shared crosshair (spec 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartView {
    #[default]
    Overlay,
    Grid,
}

fn mode_icon(mode: ChartMode) -> icondata_core::Icon {
    match mode {
        ChartMode::Price => i::TbChartLineOutline,
        ChartMode::Candles => i::TbChartCandleOutline,
        ChartMode::Range => i::TbChartAreaLineOutline,
        ChartMode::Density => i::TbChartGridDotsOutline,
    }
}

fn group_icon(level: GroupLevel) -> icondata_core::Icon {
    match level {
        GroupLevel::Region => i::TbStack2Outline,
        GroupLevel::Datacenter => i::TbCirclesOutline,
        GroupLevel::World => i::TbPointFilled,
    }
}

const CHIP: &str = "inline-flex items-center gap-1.5 rounded-md border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] px-2.5 py-1 text-xs text-[color:var(--color-text-muted)] transition-colors hover:text-[color:var(--color-text)]";

#[component]
pub fn ChartToolbar(
    #[prop(into)] mode: Signal<ChartMode>,
    set_mode: WriteSignal<ChartMode>,
    #[prop(into)] group_options: Signal<Vec<GroupLevel>>,
    #[prop(into)] group: Signal<GroupLevel>,
    set_group: WriteSignal<GroupLevel>,
    #[prop(into)] show_market_average: Signal<bool>,
    set_show_market_average: WriteSignal<bool>,
    #[prop(into)] show_trend: Signal<bool>,
    set_show_trend: WriteSignal<bool>,
    #[prop(into)] show_quantity: Signal<bool>,
    set_show_quantity: WriteSignal<bool>,
    /// Density mode has no quantity lane; the toggle stays visible but
    /// disabled with a reason (spec: disabled, never hidden).
    #[prop(into)]
    quantity_disabled: Signal<bool>,
    #[prop(into)] view: Signal<ChartView>,
    set_view: WriteSignal<ChartView>,
    /// Grid is per-series; density's payload is scope-wide, so grid
    /// disables with a reason in density mode.
    #[prop(into)]
    grid_disabled: Signal<bool>,
    /// Every world of the current scope grouped by datacenter, for the
    /// filter popover: `(datacenter name, world names)`.
    #[prop(into)]
    filter_groups: Signal<Vec<(String, Vec<String>)>>,
    /// The same signal the legend writes — the filter is a legend at scale,
    /// so the two can never disagree.
    hidden_series: RwSignal<Vec<String>>,
    /// Popover-open state lifted to the caller so the grid's "+N more"
    /// affordance can open the filter too.
    filter_open: RwSignal<bool>,
    #[prop(into)] percent_change: Signal<bool>,
    set_percent_change: WriteSignal<bool>,
    /// `% change` applies to overlay Price view only.
    #[prop(into)]
    percent_disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (group_open, set_group_open) = signal(false);
    let (overlays_open, set_overlays_open) = signal(false);
    let (filter_query, set_filter_query) = signal(String::new());

    let mode_name = move |m: ChartMode| match m {
        ChartMode::Price => t_string!(i18n, chart_mode_price).to_string(),
        ChartMode::Candles => t_string!(i18n, chart_mode_candles).to_string(),
        ChartMode::Range => t_string!(i18n, chart_mode_range).to_string(),
        ChartMode::Density => t_string!(i18n, chart_mode_density).to_string(),
    };
    let group_name = move |g: GroupLevel| match g {
        GroupLevel::Region => t_string!(i18n, chart_color_region).to_string(),
        GroupLevel::Datacenter => t_string!(i18n, chart_color_datacenter).to_string(),
        GroupLevel::World => t_string!(i18n, chart_color_world).to_string(),
    };
    let overlay_count = Signal::derive(move || {
        [
            show_market_average.get(),
            show_trend.get(),
            show_quantity.get(),
        ]
        .iter()
        .filter(|on| **on)
        .count()
    });

    view! {
        // Wrapping, NOT `overflow-x-auto`: a scroll container computes
        // `overflow-y: auto` too, which clips the absolutely-positioned
        // popovers below into the toolbar's own one-line-high scroll area —
        // every popover (group-by, world filter, overlays) opened invisibly.
        <div class="flex flex-wrap items-center gap-2 text-xs">
            // ── Mode: icon-only segmented group ──
            <div
                role="group"
                aria-label=move || t_string!(i18n, chart_toolbar_mode_group).to_string()
                class="inline-flex shrink-0 overflow-hidden rounded-md border border-[color:var(--color-outline)]"
            >
                {[
                    ChartMode::Price,
                    ChartMode::Candles,
                    ChartMode::Range,
                    ChartMode::Density,
                ]
                    .into_iter()
                    .map(|m| {
                        view! {
                            <button
                                type="button"
                                aria-label=move || mode_name(m)
                                aria-pressed=move || (mode.get() == m).to_string()
                                class=move || {
                                    let active = mode.get() == m;
                                    [
                                        "border-l border-[color:var(--color-outline)] px-2.5 py-1.5 transition-colors first:border-l-0",
                                        if active {
                                            "bg-brand-600/30 text-brand-100"
                                        } else {
                                            "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                        },
                                    ]
                                        .join(" ")
                                }
                                on:click=move |_| set_mode.set(m)
                            >
                                <Icon height="1.1em" width="1.1em" icon=mode_icon(m) />
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            // ── View: overlay / grid segmented group ──
            <div
                role="group"
                aria-label=move || t_string!(i18n, chart_view_group).to_string()
                class="inline-flex shrink-0 overflow-hidden rounded-md border border-[color:var(--color-outline)]"
            >
                {[
                    (ChartView::Overlay, i::LuChartNoAxesCombined),
                    (ChartView::Grid, i::LuLayoutGrid),
                ]
                    .into_iter()
                    .map(|(v, icon)| {
                        let disabled =
                            Signal::derive(move || v == ChartView::Grid && grid_disabled.get());
                        view! {
                            <button
                                type="button"
                                aria-label=move || match v {
                                    ChartView::Overlay => {
                                        t_string!(i18n, chart_view_overlay).to_string()
                                    }
                                    ChartView::Grid => t_string!(i18n, chart_view_grid).to_string(),
                                }
                                aria-pressed=move || (view.get() == v).to_string()
                                prop:disabled=disabled
                                title=move || {
                                    if disabled.get() {
                                        t_string!(i18n, chart_grid_density_unavailable).to_string()
                                    } else {
                                        String::new()
                                    }
                                }
                                class=move || {
                                    let active = view.get() == v;
                                    [
                                        "border-l border-[color:var(--color-outline)] px-2.5 py-1.5 transition-colors first:border-l-0 disabled:cursor-not-allowed disabled:opacity-45",
                                        if active {
                                            "bg-brand-600/30 text-brand-100"
                                        } else {
                                            "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                        },
                                    ]
                                        .join(" ")
                                }
                                on:click=move |_| set_view.set(v)
                            >
                                <Icon height="1.1em" width="1.1em" icon=icon />
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            // ── Group by: dropdown chip ──
            <Show when=move || group_options.with(|o| o.len() > 1)>
                <div class="relative shrink-0">
                    <button
                        type="button"
                        class=CHIP
                        aria-haspopup="menu"
                        aria-expanded=move || group_open.get().to_string()
                        on:click=move |_| set_group_open.update(|open| *open = !*open)
                    >
                        {move || {
                            view! {
                                <Icon height="1.0em" width="1.0em" icon=group_icon(group.get()) />
                            }
                        }}
                        {move || group_name(group.get())}
                    </button>
                    <Show when=move || group_open.get()>
                        <div
                            role="menu"
                            class="absolute left-0 top-full z-20 mt-1 min-w-36 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 py-1 shadow-lg"
                        >
                            {move || {
                                group_options
                                    .get()
                                    .into_iter()
                                    .map(|level| {
                                        view! {
                                            <button
                                                type="button"
                                                role="menuitem"
                                                class=move || {
                                                    let active = group.get() == level;
                                                    [
                                                        "flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-brand-600/20",
                                                        if active {
                                                            "text-brand-100"
                                                        } else {
                                                            "text-[color:var(--color-text-muted)]"
                                                        },
                                                    ]
                                                        .join(" ")
                                                }
                                                on:click=move |_| {
                                                    set_group.set(level);
                                                    set_group_open.set(false);
                                                }
                                            >
                                                <Icon height="1.0em" width="1.0em" icon=group_icon(level) />
                                                {group_name(level)}
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </Show>
                </div>
            </Show>
            // ── World filter: chip + searchable multi-select popover ──
            // Drives the SAME `hidden_series` the legend writes — a legend
            // at scale, so the two stay in sync by construction. Filtering
            // is purely client-side visibility: no refetch, ever.
            <Show when=move || {
                filter_groups.with(|g| g.iter().map(|(_, w)| w.len()).sum::<usize>() > 1)
            }>
                <div class="relative shrink-0">
                    <button
                        type="button"
                        class=CHIP
                        aria-haspopup="menu"
                        aria-expanded=move || filter_open.get().to_string()
                        on:click=move |_| filter_open.update(|open| *open = !*open)
                    >
                        <Icon height="1.0em" width="1.0em" icon=i::TbFilterOutline />
                        {move || t_string!(i18n, chart_world_filter).to_string()}
                        // A hidden filter that silently omits data is a
                        // correctness hazard — the count badge keeps a
                        // non-default filter visible without opening it.
                        <span class="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-brand-600/40 px-1 text-[10px] tabular-nums text-brand-100">
                            {move || {
                                filter_groups
                                    .with(|groups| {
                                        let total: usize =
                                            groups.iter().map(|(_, w)| w.len()).sum();
                                        let hidden = hidden_series
                                            .with(|h| {
                                                groups
                                                    .iter()
                                                    .flat_map(|(_, w)| w.iter())
                                                    .filter(|w| h.contains(w))
                                                    .count()
                                            });
                                        format!("{}/{}", total - hidden, total)
                                    })
                            }}
                        </span>
                    </button>
                    <Show when=move || filter_open.get()>
                        <div class="absolute left-0 top-full z-20 mt-1 max-h-80 min-w-56 overflow-y-auto rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 px-3 py-2 shadow-lg">
                            <input
                                type="text"
                                class="mb-2 w-full rounded-md border border-[color:var(--color-outline)] bg-transparent px-2 py-1 text-xs text-[color:var(--color-text)] placeholder:text-[color:var(--color-text-muted)]"
                                placeholder=move || t_string!(i18n, chart_filter_search).to_string()
                                prop:value=filter_query
                                on:input=move |event| set_filter_query.set(event_target_value(&event))
                            />
                            {move || {
                                let query = filter_query.get().to_lowercase();
                                filter_groups
                                    .get()
                                    .into_iter()
                                    .filter_map(|(dc, worlds)| {
                                        let shown: Vec<String> = worlds
                                            .iter()
                                            .filter(|w| {
                                                query.is_empty()
                                                    || w.to_lowercase().contains(&query)
                                            })
                                            .cloned()
                                            .collect();
                                        if shown.is_empty() {
                                            return None;
                                        }
                                        let all_worlds = worlds.clone();
                                        let none_worlds = worlds.clone();
                                        Some(view! {
                                            <div class="mb-1">
                                                <div class="flex items-center justify-between gap-2 py-1">
                                                    <span class="text-[10px] font-semibold uppercase text-[color:var(--color-text-muted)]">
                                                        {dc}
                                                    </span>
                                                    <span class="flex gap-1">
                                                        <button
                                                            type="button"
                                                            class="rounded px-1.5 py-0.5 text-[10px] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                                            on:click=move |_| {
                                                                hidden_series
                                                                    .update(|list| {
                                                                        list.retain(|n| !all_worlds.contains(n));
                                                                    });
                                                            }
                                                        >
                                                            {t_string!(i18n, chart_filter_all).to_string()}
                                                        </button>
                                                        <button
                                                            type="button"
                                                            class="rounded px-1.5 py-0.5 text-[10px] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                                            on:click=move |_| {
                                                                hidden_series
                                                                    .update(|list| {
                                                                        for w in &none_worlds {
                                                                            if !list.contains(w) {
                                                                                list.push(w.clone());
                                                                            }
                                                                        }
                                                                        list.sort();
                                                                    });
                                                            }
                                                        >
                                                            {t_string!(i18n, chart_filter_none).to_string()}
                                                        </button>
                                                    </span>
                                                </div>
                                                {shown
                                                    .into_iter()
                                                    .map(|world| {
                                                        let toggle_name = world.clone();
                                                        let checked_name = world.clone();
                                                        view! {
                                                            <label class="flex cursor-pointer select-none items-center justify-between gap-3 py-0.5">
                                                                <span class="text-[color:var(--color-text)]">
                                                                    {world}
                                                                </span>
                                                                <input
                                                                    type="checkbox"
                                                                    class="accent-violet-500"
                                                                    prop:checked=move || {
                                                                        hidden_series
                                                                            .with(|h| !h.contains(&checked_name))
                                                                    }
                                                                    on:change=move |_| {
                                                                        hidden_series
                                                                            .update(|list| {
                                                                                if let Some(pos) = list
                                                                                    .iter()
                                                                                    .position(|n| n == &toggle_name)
                                                                                {
                                                                                    list.remove(pos);
                                                                                } else {
                                                                                    list.push(toggle_name.clone());
                                                                                    list.sort();
                                                                                }
                                                                            });
                                                                    }
                                                                />
                                                            </label>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </div>
                                        })
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </Show>
                </div>
            </Show>
            // ── Overlays: chip + count badge + popover ──
            <div class="relative shrink-0">
                <button
                    type="button"
                    class=CHIP
                    aria-haspopup="menu"
                    aria-expanded=move || overlays_open.get().to_string()
                    on:click=move |_| set_overlays_open.update(|open| *open = !*open)
                >
                    <Icon height="1.0em" width="1.0em" icon=i::TbAdjustmentsHorizontalOutline />
                    {move || t_string!(i18n, chart_toolbar_overlays).to_string()}
                    <span class="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-brand-600/40 px-1 text-[10px] tabular-nums text-brand-100">
                        {move || overlay_count.get()}
                    </span>
                </button>
                <Show when=move || overlays_open.get()>
                    <div class="absolute left-0 top-full z-20 mt-1 min-w-52 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 px-3 py-2 shadow-lg">
                        <OverlayRow
                            label=Signal::derive(move || {
                                t_string!(i18n, chart_toggle_market_avg).to_string()
                            })
                            checked=show_market_average
                            set_checked=set_show_market_average
                            disabled=Signal::derive(move || {
                                percent_change.get() && !percent_disabled.get()
                            })
                            disabled_reason=Signal::derive(move || {
                                t_string!(i18n, chart_percent_disables_overlays).to_string()
                            })
                        />
                        <OverlayRow
                            label=Signal::derive(move || {
                                t_string!(i18n, chart_legend_trend).to_string()
                            })
                            checked=show_trend
                            set_checked=set_show_trend
                            disabled=Signal::derive(move || {
                                percent_change.get() && !percent_disabled.get()
                            })
                            disabled_reason=Signal::derive(move || {
                                t_string!(i18n, chart_percent_disables_overlays).to_string()
                            })
                        />
                        <OverlayRow
                            label=Signal::derive(move || {
                                t_string!(i18n, chart_legend_quantity).to_string()
                            })
                            checked=show_quantity
                            set_checked=set_show_quantity
                            disabled=quantity_disabled
                            disabled_reason=Signal::derive(move || {
                                t_string!(i18n, chart_density_quantity_unavailable).to_string()
                            })
                        />
                        <OverlayRow
                            label=Signal::derive(move || {
                                t_string!(i18n, chart_percent_change).to_string()
                            })
                            checked=percent_change
                            set_checked=set_percent_change
                            disabled=percent_disabled
                            disabled_reason=Signal::derive(move || {
                                t_string!(i18n, chart_percent_overlay_only).to_string()
                            })
                        />
                    </div>
                </Show>
            </div>
        </div>
    }
}

/// One labelled checkbox row in the overlays popover. Disabled rows keep
/// their space and carry the reason as a title tooltip — a control that
/// vanishes reads as a bug.
#[component]
fn OverlayRow(
    #[prop(into)] label: Signal<String>,
    #[prop(into)] checked: Signal<bool>,
    set_checked: WriteSignal<bool>,
    #[prop(into)] disabled: Signal<bool>,
    #[prop(into)] disabled_reason: Signal<String>,
) -> impl IntoView {
    view! {
        <label
            class=move || {
                [
                    "flex cursor-pointer select-none items-center justify-between gap-3 py-1",
                    if disabled.get() { "cursor-not-allowed opacity-45" } else { "" },
                ]
                    .join(" ")
            }
            title=move || {
                if disabled.get() { disabled_reason.get() } else { String::new() }
            }
        >
            <span class="text-[color:var(--color-text)]">{label}</span>
            <input
                type="checkbox"
                class="accent-violet-500"
                prop:checked=checked
                prop:disabled=disabled
                on:change=move |event| {
                    set_checked.set(event_target_checked(&event));
                }
            />
        </label>
    }
}
