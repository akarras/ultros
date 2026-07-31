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
) -> impl IntoView {
    let i18n = use_i18n();
    let (group_open, set_group_open) = signal(false);
    let (overlays_open, set_overlays_open) = signal(false);

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
        <div class="flex items-center gap-2 overflow-x-auto text-xs">
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
            // (slot: spec 3 view toggle)
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
            // (slot: spec 3 world filter)
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
                            disabled=Signal::derive(|| false)
                            disabled_reason=Signal::derive(String::new)
                        />
                        <OverlayRow
                            label=Signal::derive(move || {
                                t_string!(i18n, chart_legend_trend).to_string()
                            })
                            checked=show_trend
                            set_checked=set_show_trend
                            disabled=Signal::derive(|| false)
                            disabled_reason=Signal::derive(String::new)
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
            title=move || disabled.get().then(|| disabled_reason.get()).unwrap_or_default()
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
