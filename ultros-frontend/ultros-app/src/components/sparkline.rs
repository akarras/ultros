//! Inline SVG sparkline for the Market Movers list and other surfaces that
//! want a 24h price trace next to each row.
//!
//! Geometry/coloring live in `ultros_charts::charts::sparkline`; this
//! component adds the interactive layer: nothing renders until hover, then
//! a dot on the trace and a micro-tooltip with the value and how long ago
//! that sample was. Sparkline series are hourly VWAP, oldest first.

use leptos::prelude::*;

use ultros_charts::charts::sparkline::build_sparkline;
use ultros_charts::components::color_attr;
use ultros_charts::scale::short_number;

use crate::i18n::{t_string, use_i18n};

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
    let hover = RwSignal::new(None::<usize>);

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
        let index =
            model.with_value(|m| m.nearest_index((x_css / rect.width()) as f32 * m.width));
        hover.set(index);
    };

    view! {
        <span
            class="relative inline-block align-middle"
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
                        .and_then(|i| {
                            model
                                .with_value(|m| {
                                    let (x, y) = *m.points.get(i)?;
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
            {move || {
                hover
                    .get()
                    .and_then(|i| {
                        model
                            .with_value(|m| {
                                let value = *m.values.get(i)?;
                                let steps_back = (m.values.len() - 1 - i) as u32 * hours_per_point;
                                let when = if steps_back == 0 {
                                    t_string!(i18n, sparkline_now).to_string()
                                } else {
                                    t_string!(i18n, sparkline_hours_ago)
                                        .to_string()
                                        .replace("{n}", &steps_back.to_string())
                                };
                                let left_pct = if m.points.len() > 1 {
                                    i as f32 / (m.points.len() as f32 - 1.0) * 100.0
                                } else {
                                    50.0
                                };
                                let style = if left_pct > 50.0 {
                                    format!("left:{left_pct:.0}%;transform:translate(-100%,-100%)")
                                } else {
                                    format!("left:{left_pct:.0}%;transform:translateY(-100%)")
                                };
                                Some(view! {
                                    <span
                                        class="pointer-events-none absolute top-0 z-20 whitespace-nowrap rounded border border-[color:var(--color-outline)] bg-violet-950/95 px-1.5 py-0.5 text-[10px] tabular-nums text-[color:var(--color-text)] shadow"
                                        style=style
                                    >
                                        {format!("{} · {}", short_number(value.round() as i32), when)}
                                    </span>
                                })
                            })
                    })
            }}
        </span>
    }
    .into_any()
}
