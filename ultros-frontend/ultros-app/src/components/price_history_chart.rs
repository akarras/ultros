
use crate::global_state::LocalWorldData;
use crate::global_state::theme::use_theme_settings;
use leptos::html::Div;
use leptos::prelude::*;
use leptos_use::use_element_size;
use ultros_api_types::SaleHistory;
use ultros_charts::{ChartOptions, prepare_chart_data};

#[component]
pub fn PriceHistoryChart(#[prop(into)] sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let container = NodeRef::<Div>::new();
    let size = use_element_size(container);
    let width = size.width;
    let height = size.height;

    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let theme = use_theme_settings();

    let (filter_outliers, set_filter_outliers) = signal(true);
    let (tooltip, set_tooltip) = signal(None::<(f64, f64, String)>);

    let chart_data = Memo::new(move |_| {
        let sales_val = sales.get();
        if sales_val.is_empty() {
            return None;
        }

        let _theme_mode = theme.mode.get(); // Track theme changes if we want to change colors based on theme

        let options = ChartOptions {
            remove_outliers: filter_outliers.get(),
            top_pad_ratio: 0.15,
            show_iqr_band: true,
            show_trendline: true,
            ..Default::default()
        };

        // Basic theme support for text color
        if let Some(w) = web_sys::window()
            && let Some(doc) = w.document()
                && let Some(el) = doc.document_element() {
                    let style = w.get_computed_style(&el).ok().flatten();
                    if let Some(_style) = style {
                        // We are not using text_rgb in prepare_chart_data yet but if we did:
                        // ...
                    }
                }

        prepare_chart_data(&helper, &sales_val, options).ok()
    });

    view! {
        <div node_ref=container class="relative w-full h-[480px] select-none">
            {move || {
                let w = width.get();
                let h = height.get();
                if w == 0.0 || h == 0.0 {
                    return view! { <div>"Loading chart..."</div> }.into_any();
                }

                if let Some(data) = chart_data.get() {
                    let padding = 40.0;
                    let plot_w = w - padding * 2.0;
                    let plot_h = h - padding * 2.0;

                    let x_range = (data.max_x - data.min_x) as f64;
                    let y_range = (data.max_y - data.min_y) as f64;

                    let scale_x = move |val: i64| -> f64 {
                        padding + ((val - data.min_x) as f64 / x_range) * plot_w
                    };

                    let scale_y = move |val: i32| -> f64 {
                        // Y axis is inverted in SVG (0 is top)
                        h - padding - ((val - data.min_y) as f64 / y_range) * plot_h
                    };

                    view! {
                        <svg width=w height=h class="w-full h-full font-sans text-xs">
                            // Grid and Axes
                            <rect x=padding y=padding width=plot_w height=plot_h fill="none" stroke="#ccc" stroke-opacity="0.2" />

                            // IQR Band
                            {move || {
                                if let Some(band) = &data.iqr_band {
                                    let y1 = scale_y(band.max_price);
                                    let y2 = scale_y(band.min_price);
                                    let h_band = (y2 - y1).abs();
                                    view! {
                                        <rect
                                            x=padding
                                            y=y1.min(y2)
                                            width=plot_w
                                            height=h_band
                                            fill="#ccc"
                                            fill-opacity="0.12"
                                        />
                                    }.into_any()
                                } else {
                                    let _: () = view! {};
                                    ().into_any()
                                }
                            }}

                            // Trendline
                            {move || {
                                if let Some(line) = &data.trendline {
                                    if line.len() >= 2 {
                                        let x1 = scale_x(line[0].0);
                                        let y1 = scale_y(line[0].1);
                                        let x2 = scale_x(line[1].0);
                                        let y2 = scale_y(line[1].1);
                                        view! {
                                            <line x1=x1 y1=y1 x2=x2 y2=y2 stroke="#ccc" stroke-opacity="0.4" stroke-width="2" />
                                        }.into_any()
                                    } else {
                                        let _: () = view! {};
                                        ().into_any()
                                    }
                                } else {
                                    let _: () = view! {};
                                    ().into_any()
                                }
                            }}

                            // Points
                            {data.series.iter().map(|series| {
                                let color = series.color.clone();
                                series.points.iter().map(|point| {
                                    let cx = scale_x(point.x);
                                    let cy = scale_y(point.y);
                                    let r = point.r;
                                    let tooltip_text = format!("{}: {} gil", series.name, point.y);
                                    let color_clone = color.clone();

                                    view! {
                                        <circle
                                            cx=cx
                                            cy=cy
                                            r=r
                                            fill=color_clone.clone()
                                            opacity="0.7"
                                            on:mouseenter=move |_ev| {
                                                // Calculate tooltip position
                                                // Simple logic for now
                                                set_tooltip.set(Some((cx, cy, tooltip_text.clone())));
                                            }
                                            on:mouseleave=move |_| {
                                                set_tooltip.set(None);
                                            }
                                        />
                                    }
                                }).collect::<Vec<_>>()
                            }).collect::<Vec<_>>()}

                            // Labels (Simplified)
                             <text x=w/2.0 y=h-10.0 text-anchor="middle" fill="currentColor">{data.x_label.clone()}</text>
                             <text
                                x=10.0
                                y=h/2.0
                                text-anchor="middle"
                                transform=format!("rotate(-90, 10, {})", h/2.0)
                                fill="currentColor"
                             >
                                {data.y_label.clone()}
                             </text>

                             // Min/Max Labels
                             <text x=padding y=h-25.0 text-anchor="start" fill="currentColor">
                                {chrono::DateTime::from_timestamp(data.min_x, 0).unwrap_or_default().format("%Y-%m-%d").to_string()}
                             </text>
                             <text x=w-padding y=h-25.0 text-anchor="end" fill="currentColor">
                                {chrono::DateTime::from_timestamp(data.max_x, 0).unwrap_or_default().format("%Y-%m-%d").to_string()}
                             </text>
                             <text x=padding-5.0 y=scale_y(data.min_y) text-anchor="end" dominant-baseline="middle" fill="currentColor">
                                {data.min_y}
                             </text>
                             <text x=padding-5.0 y=scale_y(data.max_y) text-anchor="end" dominant-baseline="middle" fill="currentColor">
                                {data.max_y}
                             </text>

                        </svg>
                    }.into_any()
                } else {
                    view! { <div>"No data"</div> }.into_any()
                }
            }}

            // Tooltip Overlay
            {move || {
                 if let Some((x, y, text)) = tooltip.get() {
                     view! {
                         <div
                            class="absolute bg-black/80 text-white text-xs p-1 rounded pointer-events-none z-10 whitespace-nowrap"
                            style=format!("left: {}px; top: {}px; transform: translate(-50%, -120%);", x, y)
                         >
                            {text}
                         </div>
                     }.into_any()
                 } else {
                     let _: () = view! {};
                     ().into_any()
                 }
            }}
             <div class="absolute bottom-0 right-0 p-2">
                 <crate::components::toggle::Toggle
                    checked=filter_outliers
                    set_checked=set_filter_outliers
                    checked_label="Filtering outliers"
                    unchecked_label="No filter"
                />
            </div>
        </div>
    }
}
