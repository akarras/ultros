//! Renders a sample chart to sample-chart.svg for design eyeballing.
//! Run: cargo run -p ultros-charts --example price_history

use chrono::DateTime;
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;
use ultros_api_types::SaleHistory;
use ultros_charts::charts::price_history::{build_price_history_scene, PriceChartOptions};
use ultros_charts::svg::scene_to_svg;

fn lcg(state: &mut u32) -> i32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 16) as i32
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
                    World { id: 1, name: "Gilgamesh".to_string(), datacenter_id: 1 },
                    World { id: 2, name: "Adamantoise".to_string(), datacenter_id: 1 },
                ],
            }],
        }],
    });
    let mut state = 0x1234_5678u32;
    let sales: Vec<SaleHistory> = (0..200)
        .map(|i| SaleHistory {
            id: i,
            quantity: 1 + (lcg(&mut state) % 5).abs(),
            price_per_item: 8_000 + lcg(&mut state) % 400 + if i > 120 { 1_500 } else { 0 },
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            sold_date: DateTime::from_timestamp(1_750_000_000 + i as i64 * 7_200, 0)
                .unwrap()
                .naive_utc(),
            world_id: 1 + (i % 2),
            buyer_name: None,
        })
        .collect();
    let scene = build_price_history_scene(
        &helper,
        &sales,
        &PriceChartOptions {
            title: Some("Grade 8 Tincture of Intelligence - Sale History".to_string()),
            show_trendline: true,
            remove_outliers: true,
            ..Default::default()
        },
    );
    std::fs::write("sample-chart.svg", scene_to_svg(&scene)).unwrap();
    println!("wrote sample-chart.svg");
}
