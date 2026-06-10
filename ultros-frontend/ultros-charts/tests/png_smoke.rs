//! End-to-end smoke test: scene → SVG → resvg rasterization → decodable PNG.
//!
//! This mirrors the production pipeline in `ultros/src/web/item_card.rs`
//! (`svg_to_png`). It lives here rather than in the `ultros` crate because
//! the server crate's test binaries don't currently run anywhere (CI's test
//! step is disabled and they fail to link on Windows), and the property it
//! guards — "usvg can parse every construct our serializer emits" — is a
//! property of this crate's output.

use chrono::DateTime;
use resvg::{
    tiny_skia,
    usvg::{self, Options},
};
use ultros_api_types::SaleHistory;
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;
use ultros_charts::charts::price_history::{PriceChartOptions, build_price_history_scene};
use ultros_charts::svg::scene_to_svg;

fn svg_to_png(svg: &str) -> Vec<u8> {
    let opt = Options::default();
    let tree = usvg::Tree::from_str(svg, &opt).expect("serializer output must parse as SVG");
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).expect("pixmap");
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().expect("png encode")
}

fn helper() -> WorldHelper {
    WorldHelper::new(WorldData {
        regions: vec![Region {
            id: 1,
            name: "Test".to_string(),
            datacenters: vec![Datacenter {
                id: 1,
                name: "DC".to_string(),
                region_id: 1,
                worlds: vec![World {
                    id: 1,
                    name: "World".to_string(),
                    datacenter_id: 1,
                }],
            }],
        }],
    })
}

#[test]
fn full_chart_renders_to_a_decodable_png() {
    let sales: Vec<SaleHistory> = (0..30)
        .map(|i| SaleHistory {
            id: i,
            quantity: 1,
            price_per_item: 1_000 + i * 13,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            sold_date: DateTime::from_timestamp(1_750_000_000 + i as i64 * 7_200, 0)
                .unwrap()
                .naive_utc(),
            world_id: 1,
            buyer_name: None,
        })
        .collect();
    let scene = build_price_history_scene(
        &helper(),
        &sales,
        &PriceChartOptions {
            title: Some("Smoke Test - Sale History".to_string()),
            remove_outliers: true,
            show_trendline: true,
            ..Default::default()
        },
    );
    let png = svg_to_png(&scene_to_svg(&scene));
    let decoded = image::load_from_memory(&png).expect("decodable png");
    assert_eq!((decoded.width(), decoded.height()), (960, 540));
}

#[test]
fn empty_chart_renders_to_a_decodable_png() {
    let scene = build_price_history_scene(&helper(), &[], &PriceChartOptions::default());
    let png = svg_to_png(&scene_to_svg(&scene));
    assert!(image::load_from_memory(&png).is_ok());
}
