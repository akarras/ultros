use leptos::prelude::*;
use leptos_use::{UseElementBoundingReturn, use_element_bounding};
use ultros_api_types::SaleHistory;
use ultros_charts::charts::price_history::{
    ChartStats, PriceChartModel, PriceChartOptions, build_price_history_chart,
};
use ultros_charts::components::{color_attr, scene_view};
use ultros_charts::data::grouping::{GroupLevel, available_group_levels};
use ultros_charts::scale::short_number;
use ultros_charts::theme::Theme;

use crate::global_state::LocalWorldData;
use crate::i18n::{t, t_string, use_i18n};

fn px(v: f32) -> String {
    format!("{v:.1}")
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
fn HoverLayer(
    model: Memo<PriceChartModel>,
    hover_index: RwSignal<Option<usize>>,
) -> impl IntoView {
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
    /// Selected days window from the parent (7 / 30 / 90 / 0 for All).
    #[prop(into)]
    days_range: Signal<i32>,
) -> impl IntoView {
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = use_i18n();
    let (show_market_average, set_show_market_average) = signal(true);
    let (show_trend, set_show_trend) = signal(false);
    let (show_quantity, set_show_quantity) = signal(false);
    let (color_by, set_color_by) = signal(GroupLevel::World);
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
    let container = NodeRef::<leptos::html::Div>::new();
    let UseElementBoundingReturn {
        left: container_left,
        width: container_width,
        ..
    } = use_element_bounding(container);

    let helper_for_options = helper.clone();
    let color_by_options =
        Memo::new(move |_| available_group_levels(&helper_for_options, &scope_name.get()));
    let effective_color_by = Memo::new(move |_| {
        let selected = color_by.get();
        let options = color_by_options.get();
        if options.contains(&selected) {
            selected
        } else {
            *options.last().unwrap_or(&GroupLevel::World)
        }
    });

    let helper_for_model = helper.clone();
    let model = Memo::new(move |_| {
        let sales = sales.get();
        let measured = container_width.get() as f32;
        let width = if measured > 0.0 {
            measured.clamp(320.0, 1600.0)
        } else {
            960.0
        };
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
                days_range: Some(days_range.get()),
                group_level: Some(effective_color_by.get()),
                utc_offset_minutes: utc_offset.get(),
                hidden_series: hidden_series.get(),
                theme: Theme::site(),
            },
        )
    });

    let stats = Signal::derive(move || model.with(|m| m.stats.clone()));
    let hover_index = RwSignal::new(None::<usize>);

    let on_pointer_move = move |evt: web_sys::PointerEvent| {
        let width = container_width.get_untracked();
        if width <= 0.0 {
            return;
        }
        let x_css = evt.client_x() as f64 - container_left.get_untracked();
        let index = model.with_untracked(|m| {
            m.hover
                .nearest_index((x_css / width) as f32 * m.scene.width)
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
            <div
                role="img"
                aria-label=move || {
                    let n = stats.get().map(|s| s.n).unwrap_or(0);
                    let (from, to) = model.with(|m| {
                        (
                            m.hover.buckets.first().map(|b| b.label.clone()).unwrap_or_default(),
                            m.hover.buckets.last().map(|b| b.label.clone()).unwrap_or_default(),
                        )
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
