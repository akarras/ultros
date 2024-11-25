use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use html::Div;
use leptos::{html::Canvas, *};
use leptos_use::use_element_size;
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use ultros_charts::draw_sale_history_scatter_plot;
use ultros_charts::ChartOptions;

use crate::components::skeleton::BoxSkeleton;
use crate::{components::toggle::Toggle, global_state::LocalWorldData};

#[component]
pub fn PriceHistoryChart(sales: MaybeSignal<Vec<SaleHistory>>) -> impl IntoView {
    let canvas = create_node_ref::<Canvas>();
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let div = create_node_ref::<Div>();
    let parent_div_size = use_element_size(div);
    let width = parent_div_size.width;
    let height = parent_div_size.height;
    let helper = local_world_data.0.unwrap();
    let (filter_outliers, set_filter_outliers) = create_signal(true);
    let hidden = create_memo(move |_| {
        width.track();
        height.track();
        if let Some(canvas) = canvas() {
            let backend = CanvasBackend::with_canvas_object(canvas.deref().clone()).unwrap();
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
                _ref=canvas
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
