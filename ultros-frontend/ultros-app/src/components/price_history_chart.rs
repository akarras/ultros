use std::ops::Deref;

use chrono::DateTime;
use itertools::Itertools;
use leptos::{html::Canvas, *};
use plotters::{prelude::*, style::RGBColor};
use plotters_canvas::CanvasBackend;
use ultros_api_types::SaleHistory;

use chrono::Local;

fn short_number(value: i32) -> String {
    match value {
        1000000.. => {
            format!("{:.2}mil", value as f32 / 1000000.0)
        }
        1000..=999999 => {
            format!("{:.2}K", value as f32 / 1000.0)
        }
        _ => value.to_string(),
    }
}

enum DayLabelMode {
    Day,
    Hourly,
    Minute,
}

fn try_draw(backend: CanvasBackend, sales: &[SaleHistory]) -> Option<()> {
    let root = backend.into_drawing_area();
    root.fill(&RGBColor(16, 10, 18).mix(0.93)).ok()?;

    let line = map_sale_history_to_line(sales);

    let max_sale = line.iter().map(|(_, price, _)| price).max()?;
    let (first_sale, last_sale) = line
        .iter()
        .map(|(date, _, _)| date)
        .minmax()
        .into_option()?;
    if first_sale == last_sale {
        return None;
    }
    let time_range = last_sale.signed_duration_since(*first_sale);
    let label = if time_range.num_days() > 2 {
        DayLabelMode::Day
    } else if time_range.num_hours() > 5 {
        DayLabelMode::Hourly
    } else {
        DayLabelMode::Minute
    };
    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(60)
        .y_label_area_size(80)
        .margin(10)
        .caption(
            "Sale History",
            ("Jaldi, sans-serif", 50.0).into_font().color(&WHITE),
        )
        .build_cartesian_2d(*first_sale..*last_sale, 0..*max_sale)
        .ok()?;

    chart
        .configure_mesh()
        .label_style(&WHITE)
        .bold_line_style(&RGBColor(200, 200, 200).mix(0.2))
        .light_line_style(&RGBColor(200, 200, 200).mix(0.02))
        .x_desc("Time")
        .y_desc("Price per unit")
        .x_label_formatter(&move |x| match label {
            DayLabelMode::Day => format!("{}", x.format("%Y-%m-%d")),
            DayLabelMode::Hourly => format!("{}", x.format("%Y-%m-%d %H")),
            DayLabelMode::Minute => format!("{}", x.format("%Y-%m-%d %H:%M")),
        })
        .y_label_formatter(&|y| short_number(*y))
        .x_labels(5)
        .draw()
        .ok()?;

    chart
        .draw_series(line.into_iter().map(|(date, price, quantity)| {
            Circle::new(
                (date, price).into(),
                (quantity as f32 / 50.0 * 5.0).clamp(2.5, 5.0),
                YELLOW.filled(),
            )
        }))
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
        <div class="content-well">
            <div class:hidden=hidden>
                <canvas width="750" height="450" _ref=canvas/>
            </div>
        </div>
    }
}

fn map_sale_history_to_line(sales: &[SaleHistory]) -> Vec<(DateTime<Local>, i32, i32)> {
    sales
        .iter()
        .flat_map(|sale| {
            Some((
                sale.sold_date.and_local_timezone(Local).single()?,
                sale.price_per_item,
                sale.quantity,
            ))
        })
        .collect()
}
