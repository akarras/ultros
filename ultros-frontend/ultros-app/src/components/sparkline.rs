//! Inline SVG sparkline for the Market Movers list and other surfaces that
//! want a 24h price trace next to each row.
//!
//! Geometry/coloring live in `ultros_charts::charts::sparkline`; this
//! component adds the interactive layer: nothing renders until hover, then
//! a dot on the trace and a micro-tooltip with the value and how long ago
//! that sample was. Sparkline series are hourly VWAP, oldest first.

use leptos::portal::Portal;
use leptos::prelude::*;

use ultros_charts::charts::sparkline::build_sparkline;
use ultros_charts::components::color_attr;
use ultros_charts::scale::short_number;

use crate::i18n::{t_string, use_i18n};

/// Hover state for the tooltip. Carries the sparkline's viewport-space
/// geometry alongside the index so the portalled tooltip — which lives on
/// `<body>`, not next to the `<svg>` — can position itself.
#[derive(Clone, Copy, PartialEq)]
struct SparkHover {
    index: usize,
    left: f64,
    top: f64,
    width: f64,
}

#[component]
pub fn Sparkline(
    /// VWAP series, oldest first. Zeros mean "no trade in this hour" and are
    /// interpolated across.
    points: Vec<u32>,
    /// Drives stroke color. Pass the API's `pct_change_24h`.
    #[prop(default = 0.0)]
    pct_change: f32,
    /// Pixel width of the rendered sparkline. Default 80.
    #[prop(default = 80)]
    width: u32,
    /// Pixel height. Default 24.
    #[prop(default = 24)]
    height: u32,
    /// Hours represented by one point step (all current feeds are hourly).
    #[prop(default = 1)]
    hours_per_point: u32,
) -> impl IntoView {
    let i18n = use_i18n();
    let model = build_sparkline(&points, pct_change, width as f32, height as f32);

    // Empty / all-zero series → render nothing rather than a flat line at
    // the bottom. The page typically shows the price as text anyway.
    if model.is_empty() {
        return view! { <span class="inline-block w-20 h-6" /> }.into_any();
    }

    let path: String = model
        .points
        .iter()
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect::<Vec<_>>()
        .join(" ");
    let stroke = color_attr(&model.color);
    let model = StoredValue::new(model);
    let hover = RwSignal::new(None::<SparkHover>);
    // Split out so the portal mounts/unmounts once per hover rather than on
    // every index change as the pointer travels along the trace.
    let is_hovered = Memo::new(move |_| hover.with(|h| h.is_some()));

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
        let index = model.with_value(|m| m.nearest_index((x_css / rect.width()) as f32 * m.width));
        hover.set(index.map(|index| SparkHover {
            index,
            left: rect.left(),
            top: rect.top(),
            width: rect.width(),
        }));
    };

    view! {
        <span
            class="inline-block align-middle"
            on:pointermove=on_pointer_move
            on:pointerleave=move |_| hover.set(None)
        >
            <svg
                width=width
                height=height
                viewBox=format!("0 0 {width} {height}")
                class="block"
                aria-hidden="true"
            >
                <polyline
                    fill="none"
                    stroke=stroke
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    points=path
                />
                {move || {
                    hover
                        .get()
                        .and_then(|h| {
                            model
                                .with_value(|m| {
                                    let (x, y) = *m.points.get(h.index)?;
                                    Some(view! {
                                        <circle
                                            cx=format!("{x:.1}")
                                            cy=format!("{y:.1}")
                                            r="2.5"
                                            fill=color_attr(&m.color)
                                        />
                                    })
                                })
                        })
                }}
            </svg>
            // Portalled onto <body> rather than absolutely positioned in the
            // cell. The analyzer table's scroll container carries
            // `overflow-x-auto contain-paint`, both of which clip descendants
            // outright — no z-index can escape them, and `contain: paint`
            // even traps `position: fixed`. Only leaving the subtree works.
            {move || {
                is_hovered
                    .get()
                    .then(|| {
                        view! {
                            <Portal>
                                <span
                                    class="pointer-events-none fixed z-50 whitespace-nowrap rounded border border-[color:var(--color-outline)] bg-violet-950/95 px-1.5 py-0.5 text-[10px] tabular-nums text-[color:var(--color-text)] shadow"
                                    style=move || {
                                        hover
                                            .get()
                                            .map(|h| {
                                                let frac = model
                                                    .with_value(|m| {
                                                        if m.points.len() > 1 {
                                                            h.index as f64 / (m.points.len() as f64 - 1.0)
                                                        } else {
                                                            0.5
                                                        }
                                                    });
                                                // Flip the anchor past the midpoint so the tooltip
                                                // grows back over the sparkline instead of off-screen.
                                                let transform = if frac > 0.5 {
                                                    "translate(-100%,-100%)"
                                                } else {
                                                    "translateY(-100%)"
                                                };
                                                let x = h.left + frac * h.width;
                                                format!(
                                                    "left:{:.1}px;top:{:.1}px;transform:{}",
                                                    x,
                                                    h.top,
                                                    transform,
                                                )
                                            })
                                            .unwrap_or_default()
                                    }
                                >
                                    {move || {
                                        hover
                                            .get()
                                            .and_then(|h| {
                                                model
                                                    .with_value(|m| {
                                                        let value = *m.values.get(h.index)?;
                                                        let steps_back = (m.values.len() - 1 - h.index) as u32
                                                            * hours_per_point;
                                                        let when = if steps_back == 0 {
                                                            t_string!(i18n, sparkline_now).to_string()
                                                        } else {
                                                            t_string!(i18n, sparkline_hours_ago)
                                                                .to_string()
                                                                .replace("{n}", &steps_back.to_string())
                                                        };
                                                        Some(
                                                            format!(
                                                                "{} · {}",
                                                                short_number(value.round() as i32),
                                                                when,
                                                            ),
                                                        )
                                                    })
                                            })
                                    }}
                                </span>
                            </Portal>
                        }
                    })
            }}
        </span>
    }
    .into_any()
}
