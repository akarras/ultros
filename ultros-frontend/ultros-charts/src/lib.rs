use std::borrow::Cow;
use std::collections::HashSet;

use anyhow::anyhow;
use chrono::DateTime;
use chrono::Local;
use itertools::Itertools;
use plotters::{
    prelude::*,
    style::{
        full_palette::{ORANGE, PURPLE, PURPLE_A400, TEAL},
        RGBColor,
    },
};
use ultros_api_types::{
    world_helper::{AnySelector, WorldHelper},
    SaleHistory,
};
use xiv_gen::ItemId;

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

/// Returns a filter where Some((min, max))
fn get_iqr_filter(sales: &[SaleHistory]) -> Option<(i32, i32)> {
    if sales.len() < 10 {
        return None;
    }
    let sales_prices = sales
        .into_iter()
        .map(|sales| sales.price_per_item)
        .sorted()
        .collect::<Vec<_>>();
    let first_quartile_index = sales_prices.len() / 4;
    let last_quartile_index = sales_prices.len() - first_quartile_index;
    let first_quartile_value = sales_prices.get(first_quartile_index)?;
    let third_quartile_value = sales_prices.get(last_quartile_index)?;
    let interquartile_range = ((third_quartile_value - first_quartile_value) as f32 * 2.5) as i32;
    Some((
        first_quartile_value - interquartile_range,
        third_quartile_value + interquartile_range,
    ))
}

fn filter_outliers<'a>(sales: &'a [SaleHistory]) -> Cow<'a, [SaleHistory]> {
    if let Some((min, max)) = get_iqr_filter(sales) {
        let range = min..=max;
        Cow::Owned(
            sales
                .into_iter()
                .filter(|sales| range.contains(&sales.price_per_item))
                .cloned()
                .collect(),
        )
    } else {
        Cow::Borrowed(sales)
    }
}

pub fn draw_sale_history_scatter_plot<'a, T>(
    backend: T,
    world_helper: &WorldHelper,
    remove_outliers: bool,
    sales: &[SaleHistory],
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'a>>
where
    T: 'a + DrawingBackend,
{
    let sales = if remove_outliers {
        filter_outliers(sales)
    } else {
        Cow::Borrowed(sales)
    };
    let root = backend.into_drawing_area();
    root.fill(&RGBColor(16, 10, 18))?;
    let line = map_sale_history_to_line(world_helper, &sales);
    let item_name = &xiv_gen_db::data()
        .items
        .get(&ItemId(
            sales.first().ok_or(anyhow!("no sales"))?.sold_item_id,
        ))
        .ok_or(anyhow!("no item data"))?
        .name;
    let max_sale = line
        .iter()
        .flat_map(|(_, sales)| sales)
        .map(|(_, price, _)| price)
        .max()
        .copied()
        .ok_or(anyhow!("price hidden"))?;
    let (first_sale, last_sale) = line
        .iter()
        .flat_map(|(_, sales)| sales)
        .map(|(date, _, _)| date)
        .minmax()
        .into_option()
        .ok_or(anyhow!("bad dates"))?;
    if first_sale == last_sale {
        Err(anyhow!("only one sale"))?;
    }
    let time_range = last_sale.signed_duration_since(*first_sale);
    let label = if time_range.num_days() > 2 {
        DayLabelMode::Day
    } else if time_range.num_hours() > 5 {
        DayLabelMode::Hourly
    } else {
        DayLabelMode::Minute
    };
    let pad_top = (max_sale as f32 * 1.5).ceil() as i32;
    let mut chart = ChartBuilder::on(&root)
        .x_label_area_size(60)
        .y_label_area_size(100)
        .margin(10)
        .caption(
            format!("{} - Sale History", item_name),
            ("Jaldi, sans-serif", 25.0).into_font().color(&WHITE),
        )
        .build_cartesian_2d(*first_sale..*last_sale, 0..pad_top)?;

    chart
        .configure_mesh()
        .bold_line_style(RGBColor(200, 200, 200).mix(0.2))
        .light_line_style(RGBColor(200, 200, 200).mix(0.02))
        .x_desc("Time")
        .y_desc("Price per unit")
        .x_label_formatter(&move |x| match label {
            DayLabelMode::Day => format!("{}", x.format("%Y-%m-%d")),
            DayLabelMode::Hourly => format!("{}", x.format("%Y-%m-%d %H")),
            DayLabelMode::Minute => format!("{}", x.format("%Y-%m-%d %H:%M")),
        })
        .y_label_formatter(&|y| short_number(*y))
        .x_labels(5)
        .label_style(("Jaldi, sans-serif", 20.0).into_font().color(&WHITE))
        .draw()?;

    let colors = vec![
        YELLOW.filled(),
        RED.filled(),
        GREEN.filled(),
        BLUE.filled(),
        PURPLE.filled(),
        ORANGE.filled(),
        TEAL.filled(),
        MAGENTA.filled(),
    ];
    for ((series_name, sales), color) in line.into_iter().zip(colors.into_iter()) {
        chart
            .draw_series(sales.into_iter().map(|(date, price, quantity)| {
                Circle::new(
                    (date, price),
                    (quantity as f32 / 50.0 * 5.0).clamp(2.5, 5.0),
                    color,
                )
            }))
            .ok()
            .unwrap()
            .label(series_name)
            .legend(move |l| Circle::new(l, 5.0, color));
    }

    chart
        .configure_series_labels()
        .border_style(PURPLE_A400)
        .position(SeriesLabelPosition::UpperRight)
        .label_font(("Jaldi, sans-serif", 18.0).into_font().color(&WHITE))
        .draw()?;

    // To avoid the IO failure being ignored silently, we manually call the present function
    root.present()?;

    Ok(())
}

pub type LabelSaleData = Option<(String, Vec<(DateTime<Local>, i32, i32)>)>;

fn map_sales_in(
    world_helper: &WorldHelper,
    selector: AnySelector,
    sales: &[SaleHistory],
) -> LabelSaleData {
    let result = world_helper.lookup_selector(selector)?;
    Some((
        result.get_name().to_string(),
        sales
            .iter()
            .filter(|w| {
                world_helper
                    .lookup_selector(AnySelector::World(w.world_id))
                    .map(|w| w.is_in(&result))
                    .unwrap_or_default()
            })
            .flat_map(|sale| {
                Some((
                    sale.sold_date.and_local_timezone(Local).single()?,
                    sale.price_per_item,
                    sale.quantity,
                ))
            })
            .collect(),
    ))
}

pub type UnlabeledSaleData = Vec<(String, Vec<(DateTime<Local>, i32, i32)>)>;

fn map_sale_history_to_line(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
) -> UnlabeledSaleData {
    // figure out whether we want to group these by world or what
    let world_ids: HashSet<_> = sales
        .iter()
        .map(|w| AnySelector::World(w.world_id))
        .collect();
    let datacenters: HashSet<_> = world_ids
        .iter()
        .flat_map(|world| {
            world_helper
                .lookup_selector(*world)
                .and_then(|s| s.as_world())
                .map(|s| AnySelector::Datacenter(s.datacenter_id))
        })
        .collect();
    let regions: HashSet<_> = datacenters
        .iter()
        .flat_map(|dc| {
            world_helper
                .lookup_selector(*dc)
                .and_then(|dc| dc.as_datacenter())
                .map(|dc| AnySelector::Region(dc.region_id))
        })
        .collect();
    let selector_source = if datacenters.len() == 1 {
        world_ids
    } else if regions.len() == 1 {
        datacenters
    } else {
        regions
    };
    selector_source
        .into_iter()
        .flat_map(|w| map_sales_in(world_helper, w, sales))
        .sorted_by_cached_key(|(name, _)| name.clone())
        .collect()
}
