use std::cell::RefCell;
use std::rc::Rc;

use leptos::html::Div;
use leptos::{html::Canvas, prelude::*};
use leptos_use::use_element_size;
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use ultros_charts::draw_sale_history_scatter_plot;
use ultros_charts::ChartOptions;

use crate::components::skeleton::BoxSkeleton;
use crate::{components::toggle::Toggle, global_state::LocalWorldData};

#[component]
pub fn PriceHistoryChart(#[prop(into)] sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let canvas = NodeRef::<Canvas>::new();
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let div = NodeRef::<Div>::new();
    let parent_div_size = use_element_size(div);
    let width = parent_div_size.width;
    let height = parent_div_size.height;
    let helper = local_world_data.0.unwrap();
    let (filter_outliers, set_filter_outliers) = signal(true);
    let hidden = Memo::new(move |_| {
        width.track();
        height.track();
        if let Some(canvas) = canvas.get() {
            let backend = CanvasBackend::with_canvas_object(canvas.clone()).unwrap();
            // if there's an error drawing, we should hide the canvas

            #[cfg(all(feature = "hydrate"))]
            fn __parse_css_rgb(value: &str) -> Option<(u8, u8, u8)> {
                let v = value.trim();
                if let Some(hex) = v.strip_prefix('#') {
                    if hex.len() == 6 {
                        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                        return Some((r, g, b));
                    }
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

            #[cfg(all(feature = "hydrate"))]
            let (text_rgb, grid_rgb) = {
                let mut text_rgb = None;
                let mut grid_rgb = None;
                if let Some(window) = web_sys::window() {
                    if let Some(document) = window.document() {
                        if let Some(root) = document.document_element() {
                            if let Ok(Some(style)) = window.get_computed_style(&root) {
                                if let Ok(val) = style.get_property_value("--color-text") {
                                    text_rgb = __parse_css_rgb(&val);
                                }
                                if let Ok(val) = style.get_property_value("--color-outline") {
                                    grid_rgb = __parse_css_rgb(&val);
                                }
                            }
                        }
                    }
                }
                (text_rgb, grid_rgb)
            };

            #[cfg(not(feature = "hydrate"))]
            let (text_rgb, grid_rgb) = (None, None);

            sales.with(|sales| {
                draw_sale_history_scatter_plot(
                    Rc::new(RefCell::new(backend)),
                    helper.clone().as_ref(),
                    sales,
                    ChartOptions {
                        remove_outliers: filter_outliers(),
                        text_rgb,
                        grid_rgb,
                        ..Default::default()
                    },
                )
                .is_err()
            })
        } else {
            true
        }
    });
    view! {
        <div class="mx-auto min-h-[440px]" class:hidden=move || !hidden()>
            <BoxSkeleton />
        </div>
        <div node_ref=div class="flex flex-col min-h-[480px] mx-auto" class:hidden=hidden>
            <canvas
                width=width
                height=move || height.get().min(480.0)
                style=move || {
                    format!("width: {}px; height: {}px", width.get(), height.get().min(480.0))
                }
                node_ref=canvas
            ></canvas>
            <Toggle
                checked=filter_outliers
                set_checked=set_filter_outliers
                checked_label="Filtering outliers"
                unchecked_label="No filter"
            />
        </div>
    }
    .into_any()
}
