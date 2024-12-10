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
            sales.with(|sales| {
                draw_sale_history_scatter_plot(
                    Rc::new(RefCell::new(backend)),
                    helper.clone().as_ref(),
                    sales,
                    ChartOptions {
                        remove_outliers: filter_outliers(),
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
            <BoxSkeleton/>
        </div>
        <div node_ref=div class="flex flex-col min-h-[480px] mx-auto" class:hidden=hidden>
            <canvas
                width=width
                height=move || height.get().min(480.0)
                style={move || format!("width: {}px; height: {}px", width.get(), height.get().min(480.0))}
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
}
