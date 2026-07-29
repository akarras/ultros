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
use ultros_api_types::price_series::{PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup};
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

/// 30 daily buckets on world 1, one sale each — enough to exercise the full
/// chart (lines, volume, trendline) without a real bucketing pipeline.
fn thirty_day_series() -> PriceSeries {
    let bucket_secs = 6 * 3_600;
    let buckets: Vec<PriceBucket> = (0..30)
        .map(|i| {
            let price = 1_000 + i * 13;
            let start = 1_750_000_000 + i as i64 * 7_200;
            let start = start.div_euclid(bucket_secs) * bucket_secs;
            PriceBucket {
                ts: DateTime::from_timestamp(start, 0).unwrap().naive_utc(),
                open: price,
                high: price,
                low: price,
                close: price,
                gil: price as i64,
                units: 1,
                sales: 1,
                p25: price,
                p50: price,
                p75: price,
            }
        })
        .collect();
    let from = buckets.first().unwrap().ts;
    let to = buckets.last().unwrap().ts;
    PriceSeries {
        bucket_seconds: bucket_secs,
        group: SeriesGroup::World,
        from,
        to,
        series: vec![PriceSeriesEntry { id: 1, buckets }],
        raw: None,
    }
}

#[test]
fn full_chart_renders_to_a_decodable_png() {
    let scene = build_price_history_scene(
        &helper(),
        &thirty_day_series(),
        &PriceChartOptions {
            title: Some("Smoke Test - Sale History".to_string()),
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
    let empty = PriceSeries {
        bucket_seconds: 86_400,
        group: SeriesGroup::World,
        from: DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
        to: DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
        series: Vec::new(),
        raw: None,
    };
    let scene = build_price_history_scene(&helper(), &empty, &PriceChartOptions::default());
    let png = svg_to_png(&scene_to_svg(&scene));
    assert!(image::load_from_memory(&png).is_ok());
}
