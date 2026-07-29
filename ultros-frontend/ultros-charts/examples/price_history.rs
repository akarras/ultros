//! Renders a sample chart to sample-chart.svg for design eyeballing.
//! Run: cargo run -p ultros-charts --example price_history

use std::collections::BTreeMap;

use chrono::DateTime;
use ultros_api_types::price_series::{PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup};
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;
use ultros_charts::charts::price_history::{PriceChartOptions, build_price_history_scene};
use ultros_charts::svg::scene_to_svg;

fn lcg(state: &mut u32) -> i32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 16) as i32
}

/// Bucket a flat list of (world_id, ts_secs, price, quantity) sales into a
/// `PriceSeries` grouped by world — a stand-in for the server-side
/// aggregation this example doesn't have access to.
fn bucket_into_series(sales: &[(i32, i64, i32, i32)], bucket_secs: i64) -> PriceSeries {
    let mut by_world: BTreeMap<i32, BTreeMap<i64, Vec<(i32, i32)>>> = BTreeMap::new();
    for &(world_id, ts, price, quantity) in sales {
        let start = ts.div_euclid(bucket_secs) * bucket_secs;
        by_world
            .entry(world_id)
            .or_default()
            .entry(start)
            .or_default()
            .push((price, quantity));
    }
    let series: Vec<PriceSeriesEntry> = by_world
        .into_iter()
        .map(|(id, buckets)| {
            let buckets = buckets
                .into_iter()
                .map(|(start, sales)| {
                    let ts = DateTime::from_timestamp(start, 0).unwrap().naive_utc();
                    let open = sales.first().unwrap().0;
                    let close = sales.last().unwrap().0;
                    let high = sales.iter().map(|(p, _)| *p).max().unwrap();
                    let low = sales.iter().map(|(p, _)| *p).min().unwrap();
                    let gil: i64 = sales.iter().map(|(p, q)| *p as i64 * *q as i64).sum();
                    let units: i64 = sales.iter().map(|(_, q)| *q as i64).sum();
                    let mut prices: Vec<i32> = sales.iter().map(|(p, _)| *p).collect();
                    prices.sort_unstable();
                    let p50 = prices[prices.len() / 2];
                    PriceBucket {
                        ts,
                        open,
                        high,
                        low,
                        close,
                        gil,
                        units,
                        sales: sales.len() as u32,
                        p25: low,
                        p50,
                        p75: high,
                    }
                })
                .collect();
            PriceSeriesEntry { id, buckets }
        })
        .collect();
    let from = sales.iter().map(|s| s.1).min().unwrap_or(0);
    let to = sales.iter().map(|s| s.1).max().unwrap_or(0);
    PriceSeries {
        bucket_seconds: bucket_secs,
        group: SeriesGroup::World,
        from: DateTime::from_timestamp(from, 0).unwrap().naive_utc(),
        to: DateTime::from_timestamp(to, 0).unwrap().naive_utc(),
        series,
        raw: None,
    }
}

fn main() {
    let helper = WorldHelper::new(WorldData {
        regions: vec![Region {
            id: 1,
            name: "North-America".to_string(),
            datacenters: vec![Datacenter {
                id: 1,
                name: "Aether".to_string(),
                region_id: 1,
                worlds: vec![
                    World {
                        id: 1,
                        name: "Gilgamesh".to_string(),
                        datacenter_id: 1,
                    },
                    World {
                        id: 2,
                        name: "Adamantoise".to_string(),
                        datacenter_id: 1,
                    },
                ],
            }],
        }],
    });
    let mut state = 0x1234_5678u32;
    let sales: Vec<(i32, i64, i32, i32)> = (0..200)
        .map(|i| {
            (
                1 + (i % 2),
                1_750_000_000 + i as i64 * 7_200,
                8_000 + lcg(&mut state) % 400 + if i > 120 { 1_500 } else { 0 },
                1 + (lcg(&mut state) % 5).abs(),
            )
        })
        .collect();
    let series = bucket_into_series(&sales, 6 * 3_600);
    let scene = build_price_history_scene(
        &helper,
        &series,
        &PriceChartOptions {
            title: Some("Grade 8 Tincture of Intelligence - Sale History".to_string()),
            show_trendline: true,
            ..Default::default()
        },
    );
    std::fs::write("sample-chart.svg", scene_to_svg(&scene)).unwrap();
    println!("wrote sample-chart.svg");
}
