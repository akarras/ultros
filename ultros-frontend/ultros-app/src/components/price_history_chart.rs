use std::cell::RefCell;
use std::rc::Rc;

use cfg_if::cfg_if;
use leptos::html::Div;
use leptos::{html::Canvas, prelude::*};
#[cfg(feature = "hydrate")]
use leptos_use::use_element_size;
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use ultros_charts::ChartOptions;
use ultros_charts::draw_sale_history_scatter_plot;

use crate::components::skeleton::BoxSkeleton;
use crate::global_state::LocalWorldData;
use crate::global_state::theme::use_theme_settings;
use crate::global_state::xiv_data::tracked_data;

#[component]
pub fn PriceHistoryChart(
    #[prop(into)] sales: Signal<Vec<SaleHistory>>,
    #[prop(into)] filter_outliers: Signal<bool>,
) -> impl IntoView {
    let canvas = NodeRef::<Canvas>::new();
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    cfg_if! {
        if #[cfg(feature = "hydrate")] {
            let div = NodeRef::<Div>::new();
            let parent_div_size = use_element_size(div);
            let width = parent_div_size.width;
            let height = parent_div_size.height;
        } else {
            let div = NodeRef::<Div>::new();
            let (width, _set_width) = signal(800.0f64);
            let (height, _set_height) = signal(480.0f64);
        }
    }
    let helper = local_world_data.0.unwrap();
    let theme = use_theme_settings();
    // Optimization: separate color extraction from resize logic to avoid `get_computed_style` on every resize
    let chart_colors = Memo::new(move |_| {
        let _ = theme.mode.get();
        let _ = theme.palette.get();

        #[cfg(feature = "hydrate")]
        fn __parse_css_rgb(value: &str) -> Option<(u8, u8, u8)> {
            let v = value.trim();
            if let Some(hex) = v.strip_prefix('#')
                && hex.len() == 6
            {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some((r, g, b));
            }
            let v = v
                .trim_start_matches("rgb(")
                .trim_start_matches("rgba(")
                .trim_end_matches(')');
            let parts: Vec<_> = v.split(',').map(|s| s.trim()).collect();
            if parts.len() >= 3 {
                let r = parts[0].parse::<u8>().ok()?;
                let g = parts[1].parse::<u8>().ok()?;
                let b = parts[2].parse::<u8>().ok()?;
                return Some((r, g, b));
            }
            None
        }

        #[cfg(feature = "hydrate")]
        {
            let mut text_rgb = None;
            let mut grid_rgb = None;
            if let Some(window) = web_sys::window()
                && let Some(document) = window.document()
                && let Some(root) = document.document_element()
                && let Ok(Some(style)) = window.get_computed_style(&root)
            {
                if let Ok(val) = style.get_property_value("--color-text") {
                    text_rgb = __parse_css_rgb(&val);
                }
                if let Ok(val) = style.get_property_value("--color-outline") {
                    grid_rgb = __parse_css_rgb(&val);
                }
            }
            (text_rgb, grid_rgb)
        }

        #[cfg(not(feature = "hydrate"))]
        (None, None)
    });

    let (show_skeleton, set_show_skeleton) = signal(true);
    Effect::new(move |_| {
        let measured_width = width.get();
        let measured_height = height.get();
        // Subscribe to xiv-gen-db swaps so chart title re-renders on locale change.
        let _ = tracked_data();
        #[cfg_attr(not(feature = "hydrate"), allow(unused_mut))]
        let mut chart_width = if measured_width > 0.0 {
            measured_width
        } else {
            800.0
        };
        #[cfg_attr(not(feature = "hydrate"), allow(unused_mut))]
        let mut chart_height = if measured_height > 0.0 {
            measured_height.min(390.0)
        } else {
            390.0
        };
        let (text_rgb, grid_rgb) = chart_colors.get();
        let remove_outliers = filter_outliers.get();
        #[cfg(not(feature = "hydrate"))]
        let _ = (chart_width, chart_height);

        if let Some(canvas) = canvas.get() {
            #[cfg(feature = "hydrate")]
            {
                let client_width = canvas.client_width();
                let client_height = canvas.client_height();
                if client_width > 0 {
                    chart_width = client_width as f64;
                    canvas.set_width(client_width as u32);
                }
                if client_height > 0 {
                    chart_height = client_height as f64;
                    canvas.set_height(client_height as u32);
                }
            }

            #[cfg(feature = "hydrate")]
            {
                use wasm_bindgen::JsCast;
                if let Ok(Some(ctx)) = canvas
                    .get_context("2d")
                    .map(|c| c.and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok()))
                {
                    ctx.clear_rect(0.0, 0.0, chart_width, chart_height);
                }
            }
            let compact_options = |remove_outliers| ChartOptions {
                remove_outliers,
                text_rgb,
                grid_rgb,
                top_pad_ratio: 0.08,
                show_iqr_band: true,
                show_trendline: true,
                show_caption: false,
                show_legend: false,
                x_label: Some(String::new()),
                y_label: Some(String::new()),
                label_font_size: Some(if chart_width < 500.0 { 12.0 } else { 14.0 }),
                x_labels: Some(if chart_width < 500.0 { 2 } else { 4 }),
                x_label_area_size: Some(if chart_width < 500.0 { 30 } else { 36 }),
                y_label_area_size: Some(if chart_width < 500.0 { 36 } else { 50 }),
                margin: Some(4),
                ..Default::default()
            };

            let is_hidden = sales.with(|sales| {
                let Some(backend) = CanvasBackend::with_canvas_object(canvas.clone()) else {
                    return true;
                };
                let result = draw_sale_history_scatter_plot(
                    Rc::new(RefCell::new(backend)),
                    helper.clone().as_ref(),
                    sales,
                    compact_options(remove_outliers),
                );

                if result.is_ok() || !remove_outliers {
                    return result.is_err();
                }

                #[cfg(feature = "hydrate")]
                {
                    use wasm_bindgen::JsCast;
                    if let Ok(Some(ctx)) = canvas.get_context("2d").map(|c| {
                        c.and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
                    }) {
                        ctx.clear_rect(0.0, 0.0, chart_width, chart_height);
                    }
                }

                let Some(backend) = CanvasBackend::with_canvas_object(canvas.clone()) else {
                    return true;
                };
                draw_sale_history_scatter_plot(
                    Rc::new(RefCell::new(backend)),
                    helper.clone().as_ref(),
                    sales,
                    compact_options(false),
                )
                .is_err()
            });
            set_show_skeleton.set(is_hidden);
        } else {
            set_show_skeleton.set(true);
        }
    });
    view! {
        <div node_ref=div class="relative flex flex-col h-[320px] sm:h-[360px] xl:h-[390px] w-full min-w-0">
            <div class="absolute inset-0" class:hidden=move || !show_skeleton()>
                <BoxSkeleton />
            </div>
            <canvas
                class=move || if show_skeleton() { "opacity-0" } else { "opacity-100" }
                width=move || width.get().max(800.0).round() as u32
                height=390
                style="width: 100%; height: 100%;"
                node_ref=canvas
                role="img"
                aria-label="Scatter plot showing price history over time"
            ></canvas>
        </div>
    }
    .into_any()
}
