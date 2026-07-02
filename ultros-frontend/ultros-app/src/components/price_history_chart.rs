use leptos::html::Div;
use leptos::prelude::*;
use leptos_use::{UseElementSizeReturn, use_element_size};
use ultros_api_types::SaleHistory;
use ultros_charts::charts::price_history::{
    ChartStats, PriceChartModel, PriceChartOptions, build_price_history_chart,
};
use ultros_charts::components::{color_attr, scene_view};
use ultros_charts::data::grouping::{GroupLevel, available_group_levels};
use ultros_charts::scale::short_number;
use ultros_charts::theme::Theme;
use web_sys::PointerEvent;
use web_sys::wasm_bindgen::JsCast;

use crate::global_state::LocalWorldData;
use crate::i18n::{t, t_string, use_i18n};

fn px(v: f32) -> String {
    format!("{v:.1}")
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TimelineDrag {
    Start,
    End,
    New { anchor_ts: i64 },
}

fn sales_time_domain(sales: &[SaleHistory]) -> Option<(i64, i64)> {
    let min_ts = sales
        .iter()
        .map(|sale| sale.sold_date.and_utc().timestamp())
        .min()?;
    let max_ts = sales
        .iter()
        .map(|sale| sale.sold_date.and_utc().timestamp())
        .max()?;
    Some((min_ts, max_ts))
}

fn normalize_time_range(a: i64, b: i64, domain: (i64, i64)) -> (i64, i64) {
    let (domain_start, domain_end) = domain;
    if domain_start >= domain_end {
        return (domain_start, domain_end);
    }

    let mut start = a.min(b).clamp(domain_start, domain_end);
    let mut end = a.max(b).clamp(domain_start, domain_end);
    let min_span = ((domain_end - domain_start) / 200).max(1);

    if end - start < min_span {
        let center = start + ((end - start) / 2);
        start = (center - (min_span / 2)).clamp(domain_start, domain_end - min_span);
        end = (start + min_span).clamp(domain_start + min_span, domain_end);
    }

    (start, end)
}

fn percent_for_ts(ts: i64, domain: (i64, i64)) -> f64 {
    let span = domain.1 - domain.0;
    if span <= 0 {
        return 0.0;
    }
    (((ts - domain.0) as f64 / span as f64) * 100.0).clamp(0.0, 100.0)
}

fn format_timeline_ts(ts: i64, utc_offset_minutes: i32) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| {
            (dt + chrono::TimeDelta::minutes(utc_offset_minutes as i64))
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn timeline_quantity_buckets(
    sales: &[SaleHistory],
    domain: (i64, i64),
    bucket_count: usize,
) -> Vec<f64> {
    if sales.is_empty() || bucket_count == 0 {
        return Vec::new();
    }

    let span = (domain.1 - domain.0).max(1) as f64;
    let mut buckets = vec![0.0; bucket_count];
    for sale in sales {
        let ts = sale.sold_date.and_utc().timestamp();
        if ts < domain.0 || ts > domain.1 {
            continue;
        }
        let offset = ((ts - domain.0) as f64 / span).clamp(0.0, 1.0);
        let index = ((offset * bucket_count as f64).floor() as usize).min(bucket_count - 1);
        buckets[index] += sale.quantity.max(0) as f64;
    }
    buckets
}

fn timestamp_from_pointer(
    track_ref: NodeRef<Div>,
    event: &PointerEvent,
    domain: (i64, i64),
) -> Option<i64> {
    let node = track_ref.get()?;
    let rect = node.get_bounding_client_rect();
    let width = rect.width();
    if width <= 0.0 {
        return None;
    }

    let x = (event.client_x() - rect.left()).clamp(0.0, width);
    let pct = x / width;
    Some(domain.0 + ((domain.1 - domain.0) as f64 * pct).round() as i64)
}

// ── Sub-components ────────────────────────────────────────────────────────────

#[component]
fn StatsStrip(stats: Signal<Option<ChartStats>>) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        {move || {
            stats
                .get()
                .map(|s| {
                    let n_label = t_string!(i18n, chart_stat_n_sales)
                        .to_string()
                        .replace("{n}", &s.n.to_string());
                    let market_average_label =
                        t_string!(i18n, chart_stat_market_avg).to_string();
                    let median_label = t_string!(i18n, chart_stat_median).to_string();
                    let min_label = t_string!(i18n, chart_stat_min).to_string();
                    let max_label = t_string!(i18n, chart_stat_max).to_string();
                    view! {
                        <div class="flex flex-wrap gap-x-4 gap-y-1 text-sm tabular-nums text-[color:var(--color-text)]/70 mb-3">
                            <span>{n_label}</span>
                            {s
                                .market_average
                                .map(|v| {
                                    view! {
                                        <span>
                                            {market_average_label.clone()} " " {short_number(v)}
                                        </span>
                                    }
                                })}
                            {s
                                .median
                                .map(|v| {
                                    view! {
                                        <span>
                                            {median_label} " " {short_number(v)}
                                        </span>
                                    }
                                })}
                            <span>{min_label} " " {short_number(s.min)}</span>
                            <span>{max_label} " " {short_number(s.max)}</span>
                        </div>
                    }
                        .into_any()
                })
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sale(quantity: i32, ts: i64) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity,
            price_per_item: 1000,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            sold_date: chrono::Utc.timestamp_opt(ts, 0).unwrap().naive_utc(),
            world_id: 1,
            buyer_name: None,
        }
    }

    #[test]
    fn sales_time_domain_uses_all_available_sales() {
        let sales = vec![sale(1, 200), sale(1, 100), sale(1, 300)];
        assert_eq!(sales_time_domain(&sales), Some((100, 300)));
    }

    #[test]
    fn normalize_time_range_orders_and_clamps() {
        assert_eq!(normalize_time_range(250, 50, (100, 200)), (100, 200));
    }

    #[test]
    fn test_format_timeline_ts() {
        // 1609459200 is 2021-01-01 00:00:00 UTC
        assert_eq!(format_timeline_ts(1609459200, 0), "01-01 00:00");
        // offset of +60 minutes
        assert_eq!(format_timeline_ts(1609459200, 60), "01-01 01:00");
        // offset of -120 minutes
        assert_eq!(format_timeline_ts(1609459200, -120), "12-31 22:00");
        // different time 2021-07-04 15:30:00 UTC = 1625412600
        assert_eq!(format_timeline_ts(1625412600, 0), "07-04 15:30");
    }

    #[test]
    fn timeline_quantity_buckets_sums_quantities() {
        let sales = vec![sale(3, 0), sale(7, 100)];
        let buckets = timeline_quantity_buckets(&sales, (0, 100), 2);
        assert_eq!(buckets, vec![3.0, 7.0]);
    }
}

#[component]
fn TimelineSlicer(
    #[prop(into)] sales: Signal<Vec<SaleHistory>>,
    #[prop(into)] available_domain: Signal<Option<(i64, i64)>>,
    #[prop(into)] selected_domain: Signal<Option<(i64, i64)>>,
    #[prop(into)] selected_range: Signal<Option<(i64, i64)>>,
    #[prop(into)] utc_offset_minutes: Signal<i32>,
    set_selected_range: WriteSignal<Option<(i64, i64)>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let track_ref = NodeRef::<Div>::new();
    let (dragging, set_dragging) = signal::<Option<TimelineDrag>>(None);

    let buckets = Memo::new(move |_| {
        let Some(domain) = available_domain.get() else {
            return Vec::new();
        };
        timeline_quantity_buckets(&sales.get(), domain, 64)
    });
    let bucket_items =
        Memo::new(move |_| buckets.get().into_iter().enumerate().collect::<Vec<_>>());

    let selected_style = move || {
        let Some(domain) = available_domain.get() else {
            return "left: 0%; width: 0%;".to_string();
        };
        let (start, end) = selected_domain.get().unwrap_or(domain);
        let start_pct = percent_for_ts(start, domain);
        let end_pct = percent_for_ts(end, domain);
        format!(
            "left: {:.4}%; width: {:.4}%;",
            start_pct,
            (end_pct - start_pct).max(0.35)
        )
    };
    let start_handle_style = move || {
        let Some(domain) = available_domain.get() else {
            return "left: 0%;".to_string();
        };
        let (start, _) = selected_domain.get().unwrap_or(domain);
        format!("left: {:.4}%;", percent_for_ts(start, domain))
    };
    let end_handle_style = move || {
        let Some(domain) = available_domain.get() else {
            return "left: 100%;".to_string();
        };
        let (_, end) = selected_domain.get().unwrap_or(domain);
        format!("left: {:.4}%;", percent_for_ts(end, domain))
    };
    let range_label = move || {
        selected_domain
            .get()
            .map(|(start, end)| {
                let offset = utc_offset_minutes.get();
                format!(
                    "{} - {}",
                    format_timeline_ts(start, offset),
                    format_timeline_ts(end, offset)
                )
            })
            .unwrap_or_default()
    };

    let update_drag = move |event: &PointerEvent| {
        let Some(mode) = dragging.get() else {
            return;
        };
        let Some(domain) = available_domain.get() else {
            return;
        };
        let Some(ts) = timestamp_from_pointer(track_ref, event, domain) else {
            return;
        };
        let current = selected_domain.get().unwrap_or(domain);
        let next = match mode {
            TimelineDrag::Start => normalize_time_range(ts, current.1, domain),
            TimelineDrag::End => normalize_time_range(current.0, ts, domain),
            TimelineDrag::New { anchor_ts } => normalize_time_range(anchor_ts, ts, domain),
        };
        set_selected_range.set(Some(next));
    };

    let capture_pointer = move |event: &PointerEvent| {
        if let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        {
            let _ = target.set_pointer_capture(event.pointer_id());
        }
    };
    let release_pointer = move |event: &PointerEvent| {
        if let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        {
            let _ = target.release_pointer_capture(event.pointer_id());
        }
    };

    view! {
        <Show when=move || available_domain.get().is_some()>
            <div class="rounded-md border border-[color:var(--color-outline)]/80 bg-[color:color-mix(in_srgb,_var(--color-text)_3%,_transparent)] px-3 py-2">
                <div class="mb-2 flex items-center justify-between gap-3">
                    <div class="min-w-0">
                        <div class="text-xs font-semibold uppercase text-[color:var(--color-text-muted)]">
                            {t!(i18n, chart_timeline_label)}
                        </div>
                        <div class="truncate text-xs tabular-nums text-[color:var(--color-text)]/75">
                            {range_label}
                        </div>
                    </div>
                    <button
                        type="button"
                        class="shrink-0 rounded-md border border-[color:var(--color-outline)] px-2.5 py-1 text-xs text-[color:var(--color-text-muted)] transition-colors hover:text-[color:var(--color-text)] disabled:cursor-not-allowed disabled:opacity-45"
                        disabled=move || selected_range.get().is_none()
                        on:click=move |_| set_selected_range.set(None)
                    >
                        {t!(i18n, chart_timeline_full_range)}
                    </button>
                </div>
                <div
                    node_ref=track_ref
                    role="group"
                    aria-label=move || t_string!(i18n, chart_timeline_track_label).to_string()
                    class="relative h-14 cursor-crosshair overflow-hidden rounded-md border border-[color:var(--color-outline)]/70 bg-[color:color-mix(in_srgb,_var(--color-background)_72%,_black)]"
                    style="touch-action: none; user-select: none;"
                    on:pointerdown=move |event: PointerEvent| {
                        if event.button() != 0 {
                            return;
                        }
                        let Some(domain) = available_domain.get() else {
                            return;
                        };
                        let Some(ts) = timestamp_from_pointer(track_ref, &event, domain) else {
                            return;
                        };
                        event.prevent_default();
                        capture_pointer(&event);
                        set_dragging.set(Some(TimelineDrag::New { anchor_ts: ts }));
                        set_selected_range.set(Some(normalize_time_range(ts, ts, domain)));
                    }
                    on:pointermove=move |event: PointerEvent| {
                        event.prevent_default();
                        update_drag(&event);
                    }
                    on:pointerup=move |event: PointerEvent| {
                        release_pointer(&event);
                        set_dragging.set(None);
                    }
                    on:pointercancel=move |event: PointerEvent| {
                        release_pointer(&event);
                        set_dragging.set(None);
                    }
                >
                    <div class="pointer-events-none absolute inset-x-2 bottom-2 top-3 flex items-end gap-px">
                        <For
                            each=move || bucket_items.get()
                            key=|(index, _)| *index
                            children=move |(_, value)| {
                                let height = move || {
                                    let max_value = buckets
                                        .with(|values| values.iter().copied().fold(0.0, f64::max));
                                    if max_value <= 0.0 {
                                        "height: 0%;".to_string()
                                    } else {
                                        let pct = (value / max_value * 100.0).clamp(6.0, 100.0);
                                        format!("height: {pct:.2}%;")
                                    }
                                };
                                view! {
                                    <span
                                        class="min-w-0 flex-1 rounded-t-sm bg-emerald-500/55"
                                        style=height
                                    ></span>
                                }
                            }
                        />
                    </div>
                    <div
                        class="pointer-events-none absolute inset-y-0 rounded-sm bg-brand-500/18 ring-1 ring-brand-300/35"
                        style=selected_style
                    ></div>
                    <button
                        type="button"
                        aria-label=move || t_string!(i18n, chart_timeline_start_handle).to_string()
                        class="absolute top-1/2 h-8 w-3 -translate-x-1/2 -translate-y-1/2 cursor-ew-resize rounded-full border border-brand-200 bg-brand-500 shadow-sm shadow-black/30"
                        style=start_handle_style
                        on:pointerdown=move |event: PointerEvent| {
                            if event.button() != 0 {
                                return;
                            }
                            event.stop_propagation();
                            event.prevent_default();
                            capture_pointer(&event);
                            set_dragging.set(Some(TimelineDrag::Start));
                        }
                    ></button>
                    <button
                        type="button"
                        aria-label=move || t_string!(i18n, chart_timeline_end_handle).to_string()
                        class="absolute top-1/2 h-8 w-3 -translate-x-1/2 -translate-y-1/2 cursor-ew-resize rounded-full border border-brand-200 bg-brand-500 shadow-sm shadow-black/30"
                        style=end_handle_style
                        on:pointerdown=move |event: PointerEvent| {
                            if event.button() != 0 {
                                return;
                            }
                            event.stop_propagation();
                            event.prevent_default();
                            capture_pointer(&event);
                            set_dragging.set(Some(TimelineDrag::End));
                        }
                    ></button>
                </div>
            </div>
        </Show>
    }
}

#[component]
fn ChartOverlayToggle(
    label: String,
    #[prop(into)] checked: Signal<bool>,
    set_checked: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <label
            class=move || {
                [
                    "inline-flex cursor-pointer select-none items-center gap-1.5 rounded-md border px-2.5 py-1 transition-colors",
                    if checked.get() {
                        "border-brand-500/60 bg-brand-700/30 text-brand-100"
                    } else {
                        "border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)]"
                    },
                ]
                    .join(" ")
            }
        >
            <input
                class="sr-only"
                type="checkbox"
                prop:checked=checked
                on:change=move |event| set_checked.set(event_target_checked(&event))
            />
            <span
                class=move || {
                    [
                        "h-2 w-2 rounded-full",
                        if checked.get() { "bg-brand-300" } else { "bg-[color:var(--color-text-muted)]/45" },
                    ]
                        .join(" ")
                }
            ></span>
            {label}
        </label>
    }
}

#[component]
fn ColorByControl(
    #[prop(into)] options: Signal<Vec<GroupLevel>>,
    #[prop(into)] selected: Signal<GroupLevel>,
    set_selected: WriteSignal<GroupLevel>,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <Show when=move || options.with(|options| options.len() > 1)>
            <div class="flex flex-wrap items-center gap-2 text-xs">
                <span class="font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                    {t!(i18n, chart_color_by)}
                </span>
                <div class="inline-flex overflow-hidden rounded-md border border-[color:var(--color-outline)]">
                <For
                    each=move || options.get()
                    key=|option| option.label()
                    children=move |option| {
                        view! {
                            <button
                                type="button"
                                class=move || {
                                    let active = selected.get() == option;
                                    [
                                        "border-l border-[color:var(--color-outline)] px-2.5 py-1 transition-colors first:border-l-0",
                                        if active {
                                            "bg-brand-600/30 text-brand-100"
                                        } else {
                                            "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                        },
                                    ]
                                        .join(" ")
                                }
                                on:click=move |_| set_selected.set(option)
                            >
                                {match option {
                                    GroupLevel::Region => t_string!(i18n, chart_color_region).to_string(),
                                    GroupLevel::Datacenter => t_string!(i18n, chart_color_datacenter).to_string(),
                                    GroupLevel::World => t_string!(i18n, chart_color_world).to_string(),
                                }}
                            </button>
                        }
                    }
                />
                </div>
            </div>
        </Show>
    }
}

/// Crosshair + per-series dots at the hovered bucket. Lives INSIDE the
/// chart's `<svg>` so it shares the viewBox coordinate space.
#[component]
fn HoverLayer(model: Memo<PriceChartModel>, hover_index: RwSignal<Option<usize>>) -> impl IntoView {
    move || {
        hover_index.get().and_then(|i| {
            model.with(|m| {
                let bucket = m.hover.buckets.get(i)?;
                let dots = bucket
                    .series_values
                    .iter()
                    .enumerate()
                    .filter_map(|(series_index, value)| {
                        let (y, _) = (*value)?;
                        let color = m.series.get(series_index)?.color;
                        Some(view! {
                            <circle
                                cx=px(bucket.x)
                                cy=px(y)
                                r="4"
                                fill=color_attr(&color)
                                stroke="#16131f"
                                stroke-width="1.5"
                            />
                        })
                    })
                    .collect_view();
                Some(view! {
                    <g class="pointer-events-none">
                        <line
                            x1=px(bucket.x)
                            y1=px(m.hover.plot_top)
                            x2=px(bucket.x)
                            y2=px(m.hover.plot_bottom)
                            stroke="#9ca3af"
                            stroke-opacity="0.45"
                            stroke-width="1"
                        />
                        {dots}
                    </g>
                })
            })
        })
    }
}

/// HTML tooltip positioned over the chart container; flips to the left of
/// the crosshair past the midpoint so it never clips on the right edge.
#[component]
fn HoverTooltip(
    model: Memo<PriceChartModel>,
    hover_index: RwSignal<Option<usize>>,
    #[prop(into)] show_quantity: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    move || {
        hover_index.get().and_then(|i| {
            model.with(|m| {
                let bucket = m.hover.buckets.get(i)?.clone();
                let series = m.series.clone();
                let left_pct = (bucket.x / m.scene.width * 100.0).clamp(0.0, 100.0);
                let style = if left_pct > 55.0 {
                    format!("left:calc({left_pct:.1}% - 12px);transform:translateX(-100%)")
                } else {
                    format!("left:calc({left_pct:.1}% + 12px)")
                };
                Some(view! {
                    <div
                        class="pointer-events-none absolute top-2 z-10 min-w-36 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 px-3 py-2 text-xs shadow-lg"
                        style=style
                    >
                        <div class="mb-1 font-semibold text-[color:var(--color-text)]">
                            {bucket.label.clone()}
                        </div>
                        {series
                            .iter()
                            .enumerate()
                            .filter_map(|(series_index, info)| {
                                let (_, vwap) =
                                    bucket.series_values.get(series_index).copied().flatten()?;
                                Some(view! {
                                    <div class="flex items-center justify-between gap-3">
                                        <span class="inline-flex items-center gap-1.5">
                                            <span
                                                class="inline-block h-2 w-2 rounded-full"
                                                style:background-color=color_attr(&info.color)
                                            ></span>
                                            <span class="text-[color:var(--color-text-muted)]">
                                                {info.name.clone()}
                                            </span>
                                        </span>
                                        <span class="tabular-nums text-[color:var(--color-text)]">
                                            {short_number(vwap.round() as i32)}
                                        </span>
                                    </div>
                                })
                            })
                            .collect_view()}
                        {show_quantity
                            .get()
                            .then(|| {
                                view! {
                                    <div class="mt-1 flex items-center justify-between gap-3 border-t border-[color:var(--color-outline)]/60 pt-1">
                                        <span class="text-[color:var(--color-text-muted)]">
                                            {t!(i18n, chart_legend_quantity)}
                                        </span>
                                        <span class="tabular-nums text-[color:var(--color-text)]">
                                            {bucket.volume}
                                        </span>
                                    </div>
                                }
                            })}
                    </div>
                })
            })
        })
    }
}

// ── Main component ────────────────────────────────────────────────────────────

#[component]
pub fn PriceHistoryChart(
    #[prop(into)] sales: Signal<Vec<SaleHistory>>,
    #[prop(into)] filter_outliers: Signal<bool>,
    #[prop(into)] scope_name: Signal<String>,
) -> impl IntoView {
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = use_i18n();
    let (show_market_average, set_show_market_average) = signal(true);
    let (show_trend, set_show_trend) = signal(false);
    let (show_quantity, set_show_quantity) = signal(false);
    let (color_by, set_color_by) = signal(GroupLevel::Region);
    let (selected_range, set_selected_range) = signal::<Option<(i64, i64)>>(None);
    // Series the user hid by clicking legend chips. Stored as a sorted Vec so
    // the model memo's PartialEq sees a stable value.
    let hidden_series = RwSignal::new(Vec::<String>::new());

    // Viewer timezone for axis/tooltip LABELS only. SSR and the first client
    // render agree on 0 (UTC); this effect shifts the labels after hydration
    // — same idea as ChartWrapper's `hydrated` gate, so tachys never sees
    // divergent markup. Bucketing/geometry are timezone-independent.
    let utc_offset = RwSignal::new(0i32);
    Effect::new(move |_| {
        utc_offset.set(chrono::Local::now().offset().local_minus_utc() / 60);
    });

    // Responsive: rebuild the scene at the measured container width so text
    // renders at natural size instead of scaling down. Unmeasured (SSR and
    // first client render) falls back to 960, and leptos-use only updates
    // the signal post-mount — hydration-safe for the same reason as above.
    // use_element_size is ResizeObserver-only (no scroll listener), so page
    // scroll does not trigger model rebuilds.
    let container = NodeRef::<Div>::new();
    let UseElementSizeReturn {
        width: container_width,
        ..
    } = use_element_size(container);

    let helper_for_options = helper.clone();
    let color_by_options =
        Memo::new(move |_| available_group_levels(&helper_for_options, &scope_name.get()));
    let effective_color_by = Memo::new(move |_| {
        let selected = color_by.get();
        let options = color_by_options.get();
        if options.contains(&selected) {
            selected
        } else {
            *options.first().unwrap_or(&GroupLevel::World)
        }
    });

    let available_domain = Memo::new(move |_| sales_time_domain(&sales.get()));
    let last_available_domain = RwSignal::new(None::<(i64, i64)>);
    Effect::new(move |_| {
        let next_domain = available_domain.get();
        if next_domain != last_available_domain.get_untracked() {
            last_available_domain.set(next_domain);
            set_selected_range.set(None);
        }
    });
    let selected_domain = Memo::new(move |_| {
        let domain = available_domain.get()?;
        selected_range
            .get()
            .map(|(start, end)| normalize_time_range(start, end, domain))
            .or(Some(domain))
    });
    let visible_sales = Memo::new(move |_| {
        let sales = sales.get();
        let Some((start, end)) = selected_domain.get() else {
            return sales;
        };
        sales
            .into_iter()
            .filter(|sale| {
                let ts = sale.sold_date.and_utc().timestamp();
                ts >= start && ts <= end
            })
            .collect::<Vec<_>>()
    });

    // Quantise measured width to 16 px steps so resize-dragging doesn't
    // rebuild the full multi-thousand-node scene on every pixel change.
    // Memo's PartialEq deduplicates sub-step changes automatically.
    let chart_width = Memo::new(move |_| {
        let measured = container_width.get() as f32;
        if measured > 0.0 {
            ((measured / 16.0).round() * 16.0).clamp(320.0, 1600.0)
        } else {
            960.0
        }
    });

    let helper_for_model = helper.clone();
    let model = Memo::new(move |_| {
        let sales = visible_sales.get();
        let width = chart_width.get();
        let height = (width * 0.56).clamp(300.0, 540.0);
        build_price_history_chart(
            &helper_for_model,
            &sales,
            &PriceChartOptions {
                width,
                height,
                remove_outliers: filter_outliers.get(),
                show_market_average: show_market_average.get(),
                show_trendline: show_trend.get(),
                show_volume: show_quantity.get(),
                show_legend: false,
                title: None,
                icon_data_uri: None,
                days_range: None,
                group_level: Some(effective_color_by.get()),
                utc_offset_minutes: utc_offset.get(),
                hidden_series: hidden_series.get(),
                theme: Theme::site(),
            },
        )
    });

    let stats = Signal::derive(move || model.with(|m| m.stats.clone()));
    let hover_index = RwSignal::new(None::<usize>);

    // Clear stale hover state whenever the model is rebuilt (e.g. after a
    // window resize snaps to a new quantised width or the data changes).
    Effect::new(move |_| {
        model.track();
        hover_index.set(None);
    });

    let on_pointer_move = move |evt: web_sys::PointerEvent| {
        use web_sys::wasm_bindgen::JsCast;
        let Some(target) = evt
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let rect = target.get_bounding_client_rect();
        if rect.width() <= 0.0 {
            return;
        }
        let x_css = evt.client_x() - rect.left();
        let index = model.with_untracked(|m| {
            m.hover
                .nearest_index((x_css / rect.width()) as f32 * m.scene.width)
        });
        hover_index.set(index);
    };

    view! {
        <div class="flex flex-col gap-3">
            <StatsStrip stats=stats />
            <div class="flex flex-wrap items-center gap-2 text-xs">
                <ChartOverlayToggle
                    label=t_string!(i18n, chart_toggle_market_avg).to_string()
                    checked=show_market_average
                    set_checked=set_show_market_average
                />
                <ChartOverlayToggle
                    label=t_string!(i18n, chart_legend_trend).to_string()
                    checked=show_trend
                    set_checked=set_show_trend
                />
                <ChartOverlayToggle
                    label=t_string!(i18n, chart_legend_quantity).to_string()
                    checked=show_quantity
                    set_checked=set_show_quantity
                />
            </div>
            <ColorByControl options=color_by_options selected=effective_color_by set_selected=set_color_by />
            <TimelineSlicer
                sales=sales
                available_domain=available_domain
                selected_domain=selected_domain
                selected_range=selected_range
                utc_offset_minutes=utc_offset
                set_selected_range=set_selected_range
            />
            <div
                role="img"
                aria-label=move || {
                    let n = stats.get().map(|s| s.n).unwrap_or(0);
                    let (from, to) = selected_domain
                        .get()
                        .map(|(start, end)| {
                            let offset = utc_offset.get();
                            (
                                format_timeline_ts(start, offset),
                                format_timeline_ts(end, offset),
                            )
                        })
                        .unwrap_or_else(|| {
                            model.with(|m| {
                                (
                                    m.hover
                                        .buckets
                                        .first()
                                        .map(|b| b.label.clone())
                                        .unwrap_or_default(),
                                    m.hover
                                        .buckets
                                        .last()
                                        .map(|b| b.label.clone())
                                        .unwrap_or_default(),
                                )
                            })
                        });
                    t_string!(i18n, chart_aria_label)
                        .to_string()
                        .replace("{n}", &n.to_string())
                        .replace("{from}", &from)
                        .replace("{to}", &to)
                }
                class="price-history-chart relative w-full overflow-visible"
                node_ref=container
                on:pointermove=on_pointer_move
                on:pointerleave=move |_| hover_index.set(None)
            >
                {move || {
                    let m = model.get();
                    if m.hover.buckets.is_empty() {
                        let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                        return view! {
                            <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                {msg}
                            </div>
                        }
                            .into_any();
                    }
                    view! {
                        <svg
                            class="block w-full h-auto"
                            viewBox=format!("0 0 {:.0} {:.0}", m.scene.width, m.scene.height)
                            preserveAspectRatio="xMidYMid meet"
                        >
                            {scene_view(&m.scene)}
                            <HoverLayer model=model hover_index=hover_index />
                        </svg>
                    }
                        .into_any()
                }}
                <HoverTooltip model=model hover_index=hover_index show_quantity=show_quantity />
            </div>
            {move || {
                let m = model.get();
                (!m.series.is_empty())
                    .then(|| {
                        let toggleable = m.series.len() > 1;
                        view! {
                            <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[color:var(--color-text-muted)]">
                                {m
                                    .series
                                    .iter()
                                    .take(10)
                                    .map(|info| {
                                        let name = info.name.clone();
                                        let toggle_name = info.name.clone();
                                        let hidden = info.hidden;
                                        view! {
                                            <button
                                                type="button"
                                                disabled=!toggleable
                                                class=[
                                                    "inline-flex items-center gap-1.5 transition-opacity",
                                                    if toggleable { "cursor-pointer" } else { "cursor-default" },
                                                    if hidden { "opacity-40 line-through" } else { "" },
                                                ]
                                                    .join(" ")
                                                on:click=move |_| {
                                                    if !toggleable {
                                                        return;
                                                    }
                                                    hidden_series
                                                        .update(|hidden_list| {
                                                            if let Some(pos) = hidden_list
                                                                .iter()
                                                                .position(|n| n == &toggle_name)
                                                            {
                                                                hidden_list.remove(pos);
                                                            } else {
                                                                hidden_list.push(toggle_name.clone());
                                                                hidden_list.sort();
                                                            }
                                                        });
                                                }
                                            >
                                                <span
                                                    class="h-2.5 w-2.5 rounded-full ring-1 ring-blue-100/70"
                                                    style:background-color=color_attr(&info.color)
                                                ></span>
                                                {name}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                                {(m.series.len() > 10).then(|| {
                                    let hidden = m.series.len() - 10;
                                    let more = t_string!(i18n, chart_legend_more)
                                        .to_string()
                                        .replace("{n}", &hidden.to_string());
                                    view! {
                                        <span class="inline-flex items-center gap-1.5 text-[color:var(--color-text-muted)]/85">
                                            {more}
                                        </span>
                                    }
                                })}
                                {show_market_average
                                    .get()
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span class="h-0.5 w-5 bg-[#facc15]"></span>
                                                {t!(i18n, chart_legend_market_avg)}
                                            </span>
                                        }
                                    })}
                                {show_trend
                                    .get()
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span class="h-0.5 w-5 bg-[#94a3b8]"></span>
                                                {t!(i18n, chart_legend_trend)}
                                            </span>
                                        }
                                    })}
                                {show_quantity
                                    .get()
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span class="h-2.5 w-3 rounded-sm bg-[#22c55e]"></span>
                                                {t!(i18n, chart_legend_quantity)}
                                            </span>
                                        }
                                    })}
                            </div>
                        }
                    })
            }}
        </div>
    }
}
