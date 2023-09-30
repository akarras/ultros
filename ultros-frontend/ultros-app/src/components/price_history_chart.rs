use std::ops::Deref;

use leptos::{html::Canvas, *};
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use ultros_charts::draw_sale_history_scatter_plot;

use crate::{components::toggle::Toggle, global_state::LocalWorldData};

#[component]
pub fn PriceHistoryChart(sales: MaybeSignal<Vec<SaleHistory>>) -> impl IntoView {
    let canvas = create_node_ref::<Canvas>();
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let (filter_outliers, set_filter_outliers) = create_signal(true);
    let hidden = create_memo(move |_| {
        if let Some(canvas) = canvas() {
            let backend = CanvasBackend::with_canvas_object(canvas.deref().clone()).unwrap();
            // if there's an error drawing, we should hide the canvas
            sales.with(|sales| {
                draw_sale_history_scatter_plot(
                    backend,
                    helper.clone().as_ref(),
                    filter_outliers(),
                    sales,
                )
                .is_err()
            })
        } else {
            true
        }
    });
    view! {
        <div class="flex flex-col max-h-[480px] mx-auto" class:hidden=hidden>
            <canvas width="750" height="440" style="width: 750px; height: 440px" _ref=canvas/>
            <Toggle checked=filter_outliers set_checked=set_filter_outliers
                checked_label="Filtering outliers" unchecked_label="No filter" />
        </div>
    }
}
