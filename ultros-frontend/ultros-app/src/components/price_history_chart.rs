use std::ops::Deref;

use chrono::{DateTime, NaiveDateTime};
use gloo::console::info;
use itertools::Itertools;
use leptos::*;
use plotters::{prelude::*, style::RGBColor};
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use chrono::{Duration, Local, TimeZone};

fn try_draw(backend: CanvasBackend, sales: &[SaleHistory]) -> Option<()> {
    let root = backend.into_drawing_area();
    root.fill(&RGBColor(16, 10, 18).mix(0.93)).ok()?;

    let line = map_sale_history_to_line(sales);

    let (min_sale, max_sale) = line.iter().map(|(_, price)| price).minmax().into_option()?;
    let (first_sale, last_sale) = line.iter().map(|(date, _)| date).minmax().into_option()?;

    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(60)
        .y_label_area_size(60)
        .margin(15)
        .caption(
            "Sale History",
            ("Jaldi, sans-serif", 50.0).into_font().color(&WHITE),
        )
        .build_cartesian_2d(*first_sale..*last_sale, *min_sale..*max_sale)
        .ok()?;

    chart
        .configure_mesh()
        .label_style(&WHITE)
        .light_line_style(&RGBColor(200, 200, 200).mix(0.2))
        .x_label_formatter(&|x| format!("{}", x.format("%Y-%m-%d %H:%M:%S")))
        .draw()
        .ok()?;

    chart
        .draw_series(LineSeries::new(line.into_iter(), &YELLOW))
        .ok()?;

    // To avoid the IO failure being ignored silently, we manually call the present function
    root.present().ok()?;

    Some(())
}

#[component]
pub fn PriceHistoryChart(cx: Scope, sales: MaybeSignal<Vec<SaleHistory>>) -> impl IntoView {
    let c = NodeRef::<HtmlElement<Canvas>>::new(cx);
    let hidden = create_memo(cx, move |_| {
        if let Some(canvas) = c() {
            let backend = CanvasBackend::with_canvas_object(canvas.deref().clone()).unwrap();
            sales.with(|sales| {
                info!("drawing canvas");
                try_draw(backend, sales).is_none()
            })
        } else {
            true
        }
    });
    view! {cx,
        <div class:hidden=hidden style="width:800px; height:500px">
            <canvas width="800" height="500" _ref=c/>
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
