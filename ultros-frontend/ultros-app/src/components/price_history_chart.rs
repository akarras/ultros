use std::ops::Deref;

use chrono::DateTime;
use itertools::Itertools;
use leptos::{html::Canvas, *};
use plotters::{prelude::*, style::RGBColor};
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use chrono::Local;

fn try_draw(backend: CanvasBackend, sales: &[SaleHistory]) -> Option<()> {
    let root = backend.into_drawing_area();
    root.fill(&RGBColor(16, 10, 18).mix(0.93)).ok()?;

    let line = map_sale_history_to_line(sales);

    let max_sale = line.iter().map(|(_, price)| price).max()?;
    let (first_sale, last_sale) = line.iter().map(|(date, _)| date).minmax().into_option()?;
    if first_sale == last_sale {
        return None;
    }
    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(60)
        .y_label_area_size(60)
        .margin(15)
        .caption(
            "Sale History",
            ("Jaldi, sans-serif", 50.0).into_font().color(&WHITE),
        )
        .build_cartesian_2d(*first_sale..*last_sale, 0..*max_sale)
        .ok()?;

    chart
        .configure_mesh()
        .label_style(&WHITE)
        .light_line_style(&RGBColor(200, 200, 200).mix(0.2))
        .x_label_formatter(&|x| format!("{}", x.format("%Y-%m-%d %H")))
        .x_labels(5)
        .draw()
        .ok()?;

    chart
        .draw_series(
            line.into_iter()
                .map(|coord| Circle::new(coord.into(), 5, YELLOW.filled())),
        )
        .ok()?;

    // To avoid the IO failure being ignored silently, we manually call the present function
    root.present().ok()?;

    Some(())
}

#[component]
pub fn PriceHistoryChart(cx: Scope, sales: MaybeSignal<Vec<SaleHistory>>) -> impl IntoView {
    let canvas = create_node_ref::<Canvas>(cx);

    let hidden = create_memo(cx, move |_| {
        if let Some(canvas) = canvas() {
            let backend = CanvasBackend::with_canvas_object(canvas.deref().clone()).unwrap();
            // if there's an error drawing, we should hide the canvas
            sales.with(|sales| try_draw(backend, sales).is_none())
        } else {
            true
        }
    });
    view! {cx,
        <div class:hidden=hidden style="width:800px; height:500px">
            <canvas width="800" height="500" _ref=canvas/>
        </div>
    }
}

fn map_sale_history_to_line(sales: &[SaleHistory]) -> Vec<(DateTime<Local>, i32)> {
    sales
        .iter()
        .flat_map(|sale| {
            Some((
                sale.sold_date.and_local_timezone(Local).single()?,
                sale.price_per_item,
            ))
        })
        .collect()
}
