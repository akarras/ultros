use leptos::html::Div;
use leptos::prelude::*;
use leptos_use::{UseElementSizeReturn, use_element_size};
use ultros_api_types::price_density::PriceDensity;
use ultros_api_types::price_series::{PriceSeries, SeriesGroup};
use ultros_charts::charts::ChartMode;
use ultros_charts::charts::grid::{GridOptions, GridSort, build_price_grid, nearest_x};
use ultros_charts::charts::price_density::{DensityChartOptions, build_price_density_chart};
use ultros_charts::charts::price_history::{
    PriceChartModel, PriceChartOptions, build_price_history_chart,
};
use ultros_charts::components::{color_attr, scene_view};
use ultros_charts::data::grouping::{GroupLevel, available_group_levels};
use ultros_charts::scale::short_number;
use ultros_charts::theme::Theme;
use web_sys::PointerEvent;
use web_sys::wasm_bindgen::JsCast;

use crate::components::chart_toolbar::{ChartToolbar, ChartView};
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

/// A response with no data at all — used so the chart renders its own empty
/// state instead of unmounting while the `series` resource is still loading
/// or errored.
fn empty_price_series() -> PriceSeries {
    let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
    PriceSeries {
        bucket_seconds: 0,
        group: SeriesGroup::World,
        from: epoch,
        to: epoch,
        series: Vec::new(),
        raw: None,
    }
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

/// Histogram of traded units for the timeline slicer's mini chart. Sums
/// `units` across every series' buckets (grouping doesn't matter for a
/// volume-over-time silhouette) into `bucket_count` display buckets spanning
/// `domain`.
fn timeline_quantity_buckets(
    series: &PriceSeries,
    domain: (i64, i64),
    bucket_count: usize,
) -> Vec<f64> {
    if bucket_count == 0 {
        return Vec::new();
    }
    let has_data = series.series.iter().any(|entry| !entry.buckets.is_empty());
    if !has_data {
        return Vec::new();
    }

    let span = (domain.1 - domain.0).max(1) as f64;
    let mut buckets = vec![0.0; bucket_count];
    for entry in &series.series {
        for bucket in &entry.buckets {
            let ts = bucket.ts.and_utc().timestamp();
            if ts < domain.0 || ts > domain.1 {
                continue;
            }
            let offset = ((ts - domain.0) as f64 / span).clamp(0.0, 1.0);
            let index = ((offset * bucket_count as f64).floor() as usize).min(bucket_count - 1);
            buckets[index] += bucket.units as f64;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::price_series::{PriceBucket, PriceSeriesEntry};

    fn bucket_at(ts: i64, units: i64) -> PriceBucket {
        PriceBucket {
            ts: chrono::DateTime::from_timestamp(ts, 0).unwrap().naive_utc(),
            open: 1000,
            high: 1000,
            low: 1000,
            close: 1000,
            gil: 1000 * units,
            units,
            sales: 1,
            p25: 1000,
            p50: 1000,
            p75: 1000,
        }
    }

    fn single_bucket_entry(id: i32, ts: i64, units: i64) -> PriceSeriesEntry {
        PriceSeriesEntry {
            id,
            buckets: vec![bucket_at(ts, units)],
        }
    }

    fn series_with(entries: Vec<PriceSeriesEntry>) -> PriceSeries {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        PriceSeries {
            bucket_seconds: 100,
            group: SeriesGroup::World,
            from: epoch,
            to: epoch,
            series: entries,
            raw: None,
        }
    }

    #[test]
    fn normalize_time_range_orders_and_clamps() {
        assert_eq!(normalize_time_range(250, 50, (100, 200)), (100, 200));
    }

    #[test]
    fn test_percent_for_ts() {
        // Normal cases within domain
        assert_eq!(percent_for_ts(150, (100, 200)), 50.0);
        assert_eq!(percent_for_ts(125, (100, 200)), 25.0);
        assert_eq!(percent_for_ts(200, (100, 200)), 100.0);
        assert_eq!(percent_for_ts(100, (100, 200)), 0.0);

        // Clamping out of domain
        assert_eq!(percent_for_ts(50, (100, 200)), 0.0);
        assert_eq!(percent_for_ts(250, (100, 200)), 100.0);

        // Zero span
        assert_eq!(percent_for_ts(100, (100, 100)), 0.0);

        // Negative span
        assert_eq!(percent_for_ts(100, (200, 100)), 0.0);
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
    fn timeline_quantity_buckets_sums_units_across_series() {
        let series = series_with(vec![
            single_bucket_entry(1, 0, 3),
            single_bucket_entry(2, 100, 7),
        ]);
        let buckets = timeline_quantity_buckets(&series, (0, 100), 2);
        assert_eq!(buckets, vec![3.0, 7.0]);
    }
}

#[component]
fn TimelineSlicer(
    #[prop(into)] series: Signal<PriceSeries>,
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
        timeline_quantity_buckets(&series.get(), domain, 64)
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
    #[prop(into)] series: Signal<Option<PriceSeries>>,
    #[prop(into)] density: Signal<Option<PriceDensity>>,
    #[prop(into)] scope_name: Signal<String>,
    #[prop(into)] mode: Signal<ChartMode>,
    set_mode: WriteSignal<ChartMode>,
    #[prop(into)] group: Signal<GroupLevel>,
    set_group: WriteSignal<GroupLevel>,
    #[prop(into)] on_range_change: Callback<Option<(i64, i64)>>,
) -> impl IntoView {
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = use_i18n();
    let (show_market_average, set_show_market_average) = signal(true);
    let (show_trend, set_show_trend) = signal(false);
    let (show_quantity, set_show_quantity) = signal(false);
    // Patch milestone bands (spec 4): on by default — under 30 days the LOD
    // tier empties the mark set anyway, so narrow zooms stay clean.
    let (show_patches, set_show_patches) = signal(true);
    // Overlay vs small-multiples grid. Owned here (not item_view): nothing
    // about the view gates a fetch — grid cells re-divide the same payload.
    let (view, set_view) = signal(ChartView::Overlay);
    let (percent_change, set_percent_change) = signal(false);
    let (grid_per_cell_scale, set_grid_per_cell_scale) = signal(false);
    let (grid_sort, set_grid_sort) = signal(GridSort::Name);
    // Lifted so the grid's "+N more" affordance can open the toolbar's
    // world-filter popover.
    let world_filter_open = RwSignal::new(false);
    // Local truth for the slicer's own rendering (handle positions, drag
    // state). Every commit is mirrored out via `on_range_change` below so the
    // caller can debounce it into a refetch (Task 14) — this signal itself
    // stays undebounced so the handles track the pointer at full rate.
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
    // If the scope changes underneath an existing selection (e.g. navigating
    // from a datacenter page to a single-world page) and the current group
    // no longer makes sense, snap it to the narrowest still-valid option
    // rather than requesting a grouping the scope can't offer.
    Effect::new(move |_| {
        let options = color_by_options.get();
        if !options.contains(&group.get_untracked())
            && let Some(first) = options.first()
        {
            set_group.set(*first);
        }
    });

    // Resolved series used for both the model and the slicer's histogram.
    // Falls back to an empty payload while the resource is loading/erroring
    // so the chart renders its own empty state instead of unmounting.
    let resolved_series = Signal::derive(move || series.get().unwrap_or_else(empty_price_series));

    let available_domain = Memo::new(move |_| {
        series
            .get()
            .map(|s| (s.from.and_utc().timestamp(), s.to.and_utc().timestamp()))
    });
    // A domain nested inside the active selection is the echo of our own
    // zoom request (the server may report a slightly narrower "actual data"
    // domain than requested) — leave the selection alone. A domain that
    // *doesn't* fit means the item/world identity changed out from under an
    // active selection, so snap back to full range.
    Effect::new(move |_| {
        let Some(domain) = available_domain.get() else {
            return;
        };
        let stale = selected_range
            .get_untracked()
            .is_some_and(|(start, end)| domain.0 < start || domain.1 > end);
        if stale {
            set_selected_range.set(None);
        }
    });
    // Mirror every commit to the caller so it can (debounced) refetch.
    Effect::new(move |_| {
        on_range_change.run(selected_range.get());
    });
    let selected_domain = Memo::new(move |_| {
        let domain = available_domain.get()?;
        selected_range
            .get()
            .map(|(start, end)| normalize_time_range(start, end, domain))
            .or(Some(domain))
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
    // ── Patch milestones (spec 4) ───────────────────────────────────────
    // The patch calendar the viewed scope follows. At Region grouping the
    // chart can show regions on different patch schedules at once — then
    // there is no correct single calendar, so `None` turns milestones off
    // and the caption says why (picking a winner would silently mislabel
    // half the chart).
    let helper_for_track = helper.clone();
    let milestone_track = Memo::new(move |_| {
        use ultros_api_types::game_history::{PatchTrack, track_for_region};
        use ultros_api_types::world_helper::AnySelector;
        let series_value = resolved_series.get();
        if series_value.group == SeriesGroup::Region {
            let hidden = hidden_series.get();
            let mut tracks: Vec<PatchTrack> = series_value
                .series
                .iter()
                .filter_map(|entry| {
                    let name = helper_for_track
                        .lookup_selector(AnySelector::Region(entry.id))?
                        .get_name()
                        .to_string();
                    (!hidden.contains(&name)).then(|| track_for_region(&name))
                })
                .collect();
            tracks.sort_by_key(|t| t.as_str());
            tracks.dedup();
            return match tracks.len() {
                0 => Some(PatchTrack::Global),
                1 => Some(tracks[0]),
                _ => None,
            };
        }
        // World/DC/unknown scope: a single region — walk the scope up to it.
        // An unresolvable scope falls back to Global, like an unknown region.
        let region_name = helper_for_track
            .lookup_world_by_name(&scope_name.get())
            .and_then(|result| {
                if let Some(region) = result.as_region() {
                    Some(region.name.clone())
                } else if let Some(dc) = result.as_datacenter() {
                    helper_for_track
                        .lookup_selector(AnySelector::Region(dc.region_id))
                        .map(|r| r.get_name().to_string())
                } else if let Some(world) = result.as_world() {
                    helper_for_track
                        .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                        .and_then(|d| d.as_datacenter().map(|d| d.region_id))
                        .and_then(|region_id| {
                            helper_for_track.lookup_selector(AnySelector::Region(region_id))
                        })
                        .map(|r| r.get_name().to_string())
                } else {
                    None
                }
            });
        Some(track_for_region(region_name.as_deref().unwrap_or("")))
    });

    let milestones = Memo::new(move |_| {
        use ultros_charts::charts::MilestoneSpec;
        if !show_patches.get() {
            return Vec::new();
        }
        let Some(track) = milestone_track.get() else {
            return Vec::new();
        };
        let Some((from, to)) = selected_domain.get() else {
            return Vec::new();
        };
        let span = (to - from).max(1);
        // Every LOD-visible patch inside the window, plus the latest one
        // released before it so the leading stretch is tinted — the band
        // layout's documented contract.
        let mut specs: Vec<MilestoneSpec> = Vec::new();
        for patch in ultros_api_types::game_history::visible_patches(track, span) {
            let ts = patch
                .released
                .and_hms_opt(0, 0, 0)
                .expect("midnight is always valid")
                .and_utc()
                .timestamp();
            if ts >= to {
                break;
            }
            if ts <= from {
                specs.clear(); // only the latest pre-window patch survives
            }
            specs.push(MilestoneSpec {
                start: chrono::DateTime::from_timestamp(ts, 0)
                    .expect("seed dates are valid timestamps")
                    .naive_utc(),
                version: patch.version,
                ex_version: patch.ex_version,
            });
        }
        specs
    });

    let model = Memo::new(move |_| {
        let series_value = resolved_series.get();
        let width = chart_width.get();
        let height = (width * 0.56).clamp(300.0, 540.0);
        build_price_history_chart(
            &helper_for_model,
            &series_value,
            &PriceChartOptions {
                width,
                height,
                show_market_average: show_market_average.get(),
                show_trendline: show_trend.get(),
                // Density has no quantity lane (spec: disabled with a
                // reason, and its own layout never draws one anyway).
                show_volume: show_quantity.get() && mode.get() != ChartMode::Density,
                show_legend: false,
                title: None,
                icon_data_uri: None,
                days_range: None,
                group_level: None,
                utc_offset_minutes: utc_offset.get(),
                hidden_series: hidden_series.get(),
                mode: mode.get(),
                milestones: milestones.get(),
                index_to_percent: percent_change.get()
                    && mode.get() == ChartMode::Price
                    && view.get() == ChartView::Overlay,
                theme: Theme::site(),
            },
        )
    });

    // Series names of the current grouping level, grouped for the filter
    // popover. The filter lists whatever the legend lists — hiding a name
    // that isn't a current series name would silently do nothing.
    let helper_for_filter = helper.clone();
    let filter_groups = Memo::new(move |_| {
        let scope = scope_name.get();
        let level = group.get();
        let Some(result) = helper_for_filter.lookup_world_by_name(&scope) else {
            return Vec::<(String, Vec<String>)>::new();
        };
        if let Some(region) = result.as_region() {
            match level {
                GroupLevel::World => region
                    .datacenters
                    .iter()
                    .map(|dc| {
                        (
                            dc.name.clone(),
                            dc.worlds.iter().map(|w| w.name.clone()).collect(),
                        )
                    })
                    .collect(),
                GroupLevel::Datacenter => vec![(
                    region.name.clone(),
                    region
                        .datacenters
                        .iter()
                        .map(|dc| dc.name.clone())
                        .collect(),
                )],
                GroupLevel::Region => Vec::new(),
            }
        } else if let Some(dc) = result.as_datacenter() {
            match level {
                GroupLevel::World => vec![(
                    dc.name.clone(),
                    dc.worlds.iter().map(|w| w.name.clone()).collect(),
                )],
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        }
    });

    let helper_for_grid = helper.clone();
    let grid_model = Memo::new(move |_| {
        let series_value = resolved_series.get();
        // Density never reaches the grid (the view toggle disables it);
        // guard anyway so a stale combination degrades to Price cells.
        let grid_mode = match mode.get() {
            ChartMode::Density => ChartMode::Price,
            m => m,
        };
        build_price_grid(
            &helper_for_grid,
            &series_value,
            &GridOptions {
                mode: grid_mode,
                shared_y: !grid_per_cell_scale.get(),
                sort: grid_sort.get(),
                hidden_series: hidden_series.get(),
                theme: Theme::site(),
                ..Default::default()
            },
        )
    });

    // Built only from the density payload — `None` while the fetch is in
    // flight or the mode is inactive, so the render closure can fall back
    // to the standard empty state.
    let density_model = Memo::new(move |_| {
        let width = chart_width.get();
        let height = (width * 0.56).clamp(300.0, 540.0);
        density.get().map(|d| {
            build_price_density_chart(
                &d,
                &DensityChartOptions {
                    width,
                    height,
                    utc_offset_minutes: utc_offset.get(),
                    milestones: milestones.get(),
                    theme: Theme::site(),
                },
            )
        })
    });

    let stats = Signal::derive(move || model.with(|m| m.stats.clone()));
    let hover_index = RwSignal::new(None::<usize>);

    // Clear stale hover state whenever either model is rebuilt (e.g. after
    // a window resize snaps to a new quantised width or the data changes).
    Effect::new(move |_| {
        model.track();
        density_model.track();
        grid_model.track();
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
        // Grid cells resolve their own pointer position (per-cell svg rects
        // share one x space); the container handler is overlay/density only.
        if view.get_untracked() == ChartView::Grid && mode.get_untracked() != ChartMode::Density {
            return;
        }
        let x_css = evt.client_x() - rect.left();
        let index = if mode.get_untracked() == ChartMode::Density {
            density_model.with_untracked(|m| {
                m.as_ref().and_then(|m| {
                    m.hover
                        .nearest_index((x_css / rect.width()) as f32 * m.scene.width)
                })
            })
        } else {
            model.with_untracked(|m| {
                m.hover
                    .nearest_index((x_css / rect.width()) as f32 * m.scene.width)
            })
        };
        hover_index.set(index);
    };

    view! {
        <div class="flex flex-col gap-3">
            <ChartToolbar
                mode=mode
                set_mode=set_mode
                group_options=color_by_options
                group=group
                set_group=set_group
                show_market_average=show_market_average
                set_show_market_average=set_show_market_average
                show_trend=show_trend
                set_show_trend=set_show_trend
                show_quantity=show_quantity
                set_show_quantity=set_show_quantity
                quantity_disabled=Signal::derive(move || mode.get() == ChartMode::Density)
                show_patches=show_patches
                set_show_patches=set_show_patches
                view=view
                set_view=set_view
                grid_disabled=Signal::derive(move || mode.get() == ChartMode::Density)
                filter_groups=filter_groups
                hidden_series=hidden_series
                filter_open=world_filter_open
                percent_change=percent_change
                set_percent_change=set_percent_change
                percent_disabled=Signal::derive(move || {
                    !(mode.get() == ChartMode::Price && view.get() == ChartView::Overlay)
                })
            />
            // Mode-cap hint: modes that draw fewer series than are visible
            // say so instead of silently dropping data.
            {move || {
                mode.get()
                    .series_cap()
                    .and_then(|cap| {
                        model.with(|m| {
                            let visible: Vec<String> = m
                                .series
                                .iter()
                                .filter(|s| !s.hidden)
                                .map(|s| s.name.clone())
                                .collect();
                            (visible.len() > cap)
                                .then(|| {
                                    let text = if cap == 1 {
                                        let name = visible.first().cloned().unwrap_or_default();
                                        t_string!(i18n, chart_hint_single_series)
                                            .to_string()
                                            .replace("{name}", &name)
                                    } else {
                                        t_string!(i18n, chart_hint_range_limit).to_string()
                                    };
                                    // Grid rescues single-series modes: offer
                                    // it as the hint's action rather than only
                                    // explaining the limitation (spec 3).
                                    let offer_grid = view.get() == ChartView::Overlay
                                        && mode.get() != ChartMode::Density;
                                    view! {
                                        <div class="flex flex-wrap items-center gap-2 text-xs text-amber-200/85">
                                            <span>{text}</span>
                                            {offer_grid
                                                .then(|| {
                                                    view! {
                                                        <button
                                                            type="button"
                                                            class="rounded-md border border-amber-300/40 px-2 py-0.5 text-amber-100 transition-colors hover:bg-amber-500/15"
                                                            on:click=move |_| set_view.set(ChartView::Grid)
                                                        >
                                                            {t_string!(i18n, chart_hint_use_grid).to_string()}
                                                        </button>
                                                    }
                                                })}
                                        </div>
                                    }
                                })
                        })
                    })
            }}
            <TimelineSlicer
                series=resolved_series
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
                    // ── Grid view: small multiples under one crosshair ──
                    if view.get() == ChartView::Grid && mode.get() != ChartMode::Density {
                        let gm = grid_model.get();
                        if gm.cells.is_empty() {
                            let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                            return view! {
                                <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                    {msg}
                                </div>
                            }
                                .into_any();
                        }
                        let bucket_secs =
                            resolved_series.with(|s| s.bucket_seconds.max(1));
                        let hover_x = Signal::derive(move || {
                            hover_index
                                .get()
                                .and_then(|i| grid_model.with(|g| g.xs.get(i).copied()))
                        });
                        let xs_for_move = gm.xs.clone();
                        let cell_width = gm.cell_width;
                        let on_cell_pointer_move = move |evt: web_sys::PointerEvent| {
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
                            hover_index.set(nearest_x(
                                &xs_for_move,
                                (x_css / rect.width()) as f32 * cell_width,
                            ));
                        };
                        return view! {
                            <div class="flex flex-col gap-2">
                                // Grid header: sort + per-cell scaling
                                <div class="flex flex-wrap items-center gap-3 text-xs text-[color:var(--color-text-muted)]">
                                    <select
                                        class="rounded-md border border-[color:var(--color-outline)] bg-transparent px-2 py-1"
                                        on:change=move |event| {
                                            set_grid_sort
                                                .set(
                                                    if event_target_value(&event) == "change" {
                                                        GridSort::Change
                                                    } else {
                                                        GridSort::Name
                                                    },
                                                );
                                        }
                                    >
                                        <option value="name" selected=move || grid_sort.get() == GridSort::Name>
                                            {t_string!(i18n, chart_sort_name).to_string()}
                                        </option>
                                        <option value="change" selected=move || grid_sort.get() == GridSort::Change>
                                            {t_string!(i18n, chart_sort_change).to_string()}
                                        </option>
                                    </select>
                                    <label class="inline-flex cursor-pointer select-none items-center gap-1.5">
                                        <input
                                            type="checkbox"
                                            class="accent-violet-500"
                                            prop:checked=grid_per_cell_scale
                                            on:change=move |event| {
                                                set_grid_per_cell_scale.set(event_target_checked(&event))
                                            }
                                        />
                                        {t_string!(i18n, chart_scale_per_cell).to_string()}
                                    </label>
                                </div>
                                <div
                                    class="grid gap-2"
                                    style="grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));"
                                    on:pointerleave=move |_| hover_index.set(None)
                                >
                                    {gm
                                        .cells
                                        .iter()
                                        .map(|cell| {
                                            let scene = cell.scene.clone();
                                            let name = cell.name.clone();
                                            let color = cell.color;
                                            let handler = on_cell_pointer_move.clone();
                                            view! {
                                                <div class="rounded-md border border-[color:var(--color-outline)]/60 p-1.5">
                                                    <div class="mb-1 flex items-center gap-1.5 text-xs text-[color:var(--color-text)]">
                                                        <span
                                                            class="h-2 w-2 rounded-full"
                                                            style:background-color=color_attr(&color)
                                                        ></span>
                                                        {name}
                                                    </div>
                                                    <svg
                                                        class="block w-full h-auto"
                                                        viewBox=format!(
                                                            "0 0 {:.0} {:.0}",
                                                            scene.width,
                                                            scene.height,
                                                        )
                                                        preserveAspectRatio="none"
                                                        on:pointermove=handler
                                                    >
                                                        {scene_view(&scene)}
                                                        {move || {
                                                            hover_x
                                                                .get()
                                                                .map(|x| {
                                                                    grid_model
                                                                        .with(|g| {
                                                                            view! {
                                                                                <line
                                                                                    x1=px(x)
                                                                                    y1=px(g.plot_top)
                                                                                    x2=px(x)
                                                                                    y2=px(g.plot_bottom)
                                                                                    stroke="#9ca3af"
                                                                                    stroke-opacity="0.45"
                                                                                    stroke-width="1"
                                                                                />
                                                                            }
                                                                        })
                                                                })
                                                        }}
                                                    </svg>
                                                </div>
                                            }
                                        })
                                        .collect_view()}
                                    {(gm.overflow > 0)
                                        .then(|| {
                                            let more = t_string!(i18n, chart_grid_more)
                                                .to_string()
                                                .replace("{n}", &gm.overflow.to_string());
                                            view! {
                                                <button
                                                    type="button"
                                                    class="flex min-h-24 items-center justify-center rounded-md border border-dashed border-[color:var(--color-outline)] text-xs text-[color:var(--color-text-muted)] transition-colors hover:text-[color:var(--color-text)]"
                                                    on:click=move |_| world_filter_open.set(true)
                                                >
                                                    {more}
                                                </button>
                                            }
                                        })}
                                </div>
                                // Single tooltip for the whole grid: every
                                // cell's value at the hovered bucket.
                                {move || {
                                    hover_index
                                        .get()
                                        .and_then(|i| {
                                            grid_model
                                                .with(|g| {
                                                    let ts = g.union.timestamps.get(i)?;
                                                    let label = format_timeline_ts(
                                                        ts.and_utc().timestamp() + bucket_secs / 2,
                                                        utc_offset.get(),
                                                    );
                                                    let rows = g
                                                        .cells
                                                        .iter()
                                                        .filter_map(|cell| {
                                                            let value = (*cell.values.get(i)?)?;
                                                            Some(
                                                                view! {
                                                                    <div class="flex items-center justify-between gap-3">
                                                                        <span class="inline-flex items-center gap-1.5">
                                                                            <span
                                                                                class="inline-block h-2 w-2 rounded-full"
                                                                                style:background-color=color_attr(&cell.color)
                                                                            ></span>
                                                                            <span class="text-[color:var(--color-text-muted)]">
                                                                                {cell.name.clone()}
                                                                            </span>
                                                                        </span>
                                                                        <span class="tabular-nums text-[color:var(--color-text)]">
                                                                            {short_number(value.round() as i32)}
                                                                        </span>
                                                                    </div>
                                                                },
                                                            )
                                                        })
                                                        .collect_view();
                                                    Some(
                                                        view! {
                                                            <div class="pointer-events-none absolute right-2 top-2 z-10 min-w-40 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 px-3 py-2 text-xs shadow-lg">
                                                                <div class="mb-1 font-semibold text-[color:var(--color-text)]">
                                                                    {label}
                                                                </div>
                                                                {rows}
                                                            </div>
                                                        },
                                                    )
                                                })
                                        })
                                }}
                            </div>
                        }
                            .into_any();
                    }
                    let empty_state = || {
                        let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                        view! {
                            <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                {msg}
                            </div>
                        }
                            .into_any()
                    };
                    if mode.get() == ChartMode::Density {
                        // `None` while the density fetch is in flight (or the
                        // endpoint errored) — the standard empty state keeps
                        // the frame instead of unmounting.
                        let Some(dm) = density_model.get() else {
                            return empty_state();
                        };
                        if dm.hover.buckets.is_empty() {
                            return empty_state();
                        }
                        return view! {
                            <svg
                                class="block w-full h-auto"
                                viewBox=format!("0 0 {:.0} {:.0}", dm.scene.width, dm.scene.height)
                                preserveAspectRatio="xMidYMid meet"
                            >
                                {scene_view(&dm.scene)}
                                {move || {
                                    hover_index
                                        .get()
                                        .and_then(|i| {
                                            density_model
                                                .with(|m| {
                                                    let m = m.as_ref()?;
                                                    let b = m.hover.buckets.get(i)?;
                                                    Some(view! {
                                                        <line
                                                            x1=px(b.x)
                                                            y1=px(m.hover.plot_top)
                                                            x2=px(b.x)
                                                            y2=px(m.hover.plot_bottom)
                                                            stroke="#9ca3af"
                                                            stroke-opacity="0.45"
                                                            stroke-width="1"
                                                        />
                                                    })
                                                })
                                        })
                                }}
                            </svg>
                        }
                            .into_any();
                    }
                    let m = model.get();
                    if m.hover.buckets.is_empty() {
                        return empty_state();
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
                // Overlay-only: grid renders its own container tooltip and
                // density's crosshair index doesn't map onto `model.hover`.
                <Show when=move || {
                    view.get() == ChartView::Overlay && mode.get() != ChartMode::Density
                }>
                    <HoverTooltip model=model hover_index=hover_index show_quantity=show_quantity />
                </Show>
            </div>
            // Caption line: the resolved state spelled out once — what makes
            // an icon-only toolbar viable (works on touch, read by screen
            // readers, no icon carries meaning alone). Replaces StatsStrip.
            {move || {
                let s = stats.get();
                let mode_label = match mode.get() {
                    ChartMode::Price => t_string!(i18n, chart_mode_price).to_string(),
                    ChartMode::Candles => t_string!(i18n, chart_mode_candles).to_string(),
                    ChartMode::Range => t_string!(i18n, chart_mode_range).to_string(),
                    ChartMode::Density => t_string!(i18n, chart_mode_density).to_string(),
                };
                let grouped = color_by_options
                    .with(|o| o.len() > 1)
                    .then(|| {
                        let group_label = match group.get() {
                            GroupLevel::Region => t_string!(i18n, chart_color_region).to_string(),
                            GroupLevel::Datacenter => {
                                t_string!(i18n, chart_color_datacenter).to_string()
                            }
                            GroupLevel::World => t_string!(i18n, chart_color_world).to_string(),
                        };
                        t_string!(i18n, chart_caption_grouped_by)
                            .to_string()
                            .replace("{group}", &group_label)
                    });
                let view_label = (view.get() == ChartView::Grid)
                    .then(|| t_string!(i18n, chart_view_grid).to_string());
                let percent_label = (percent_change.get()
                    && mode.get() == ChartMode::Price
                    && view.get() == ChartView::Overlay)
                    .then(|| t_string!(i18n, chart_percent_change).to_string());
                view! {
                    <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs tabular-nums text-[color:var(--color-text)]/70">
                        <span>{mode_label}</span>
                        {view_label.map(|v| view! { <span>"· " {v}</span> })}
                        {percent_label.map(|p| view! { <span>"· " {p}</span> })}
                        {grouped.map(|g| view! { <span>"· " {g}</span> })}
                        {(show_patches.get() && milestone_track.get().is_none())
                            .then(|| {
                                view! {
                                    <span class="text-amber-200/85">
                                        "· "
                                        {t_string!(i18n, chart_milestones_mixed_tracks).to_string()}
                                    </span>
                                }
                            })}
                        {s
                            .as_ref()
                            .map(|s| {
                                let n_label = t_string!(i18n, chart_stat_n_sales)
                                    .to_string()
                                    .replace("{n}", &s.n.to_string());
                                view! { <span>"· " {n_label}</span> }
                            })}
                        {s
                            .as_ref()
                            .and_then(|s| s.market_average)
                            .map(|v| {
                                view! {
                                    <span>
                                        "· " {t_string!(i18n, chart_stat_market_avg).to_string()}
                                        " " {short_number(v)}
                                    </span>
                                }
                            })}
                        {s
                            .as_ref()
                            .and_then(|s| s.median)
                            .map(|v| {
                                view! {
                                    <span>
                                        "· " {t_string!(i18n, chart_stat_median).to_string()} " "
                                        {short_number(v)}
                                    </span>
                                }
                            })}
                    </div>
                }
            }}
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
