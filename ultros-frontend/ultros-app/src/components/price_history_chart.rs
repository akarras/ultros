use std::ops::Deref;

use leptos::{html::Canvas, *};
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use ultros_charts::draw_sale_history_scatter_plot;

use crate::global_state::LocalWorldData;

#[component]
pub fn PriceHistoryChart(cx: Scope, sales: MaybeSignal<Vec<SaleHistory>>) -> impl IntoView {
    let canvas = create_node_ref::<Canvas>(cx);
    let local_world_data = use_context::<LocalWorldData>(cx).unwrap();
    let helper = local_world_data.0.unwrap();
    let hidden = create_memo(cx, move |_| {
        if let Some(canvas) = canvas() {
            let backend = CanvasBackend::with_canvas_object(canvas.deref().clone()).unwrap();
            // if there's an error drawing, we should hide the canvas
            sales.with(|sales| {
                !draw_sale_history_scatter_plot(backend, helper.clone().as_ref(), sales).is_ok()
            })
        } else {
            true
        }
    });
    view! {cx,
        <div class="content-well">
            <div class:hidden=hidden>
                <canvas width="750" height="450" _ref=canvas/>
            </div>
        </div>
    }
}
