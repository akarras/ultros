//! Static SVG snapshots of every chart mode over one synthetic dataset.
//!
//! Each test builds a scene from [`crate::test_util::synthetic_price_series`]
//! (a seeded, fully deterministic fixture) and compares the serialized SVG
//! byte-for-byte against a file under `src/charts/snapshots/`. Any change to
//! layout, theming, or scene emission shows up as a snapshot diff — cheap
//! regression cover for "the chart silently stopped drawing X".
//!
//! To (re)generate after an intentional rendering change:
//!
//! ```text
//! UPDATE_SNAPSHOTS=1 cargo test -p ultros-charts snapshot
//! ```
//!
//! then review the .svg diff like any other code change (they render in a
//! browser for eyeballing).

use crate::charts::ChartMode;
use crate::charts::grid::{GridOptions, build_price_grid};
use crate::charts::price_density::{DensityChartOptions, build_price_density_chart};
use crate::charts::price_history::{PriceChartOptions, build_price_history_chart};
use crate::svg::scene_to_svg;
use crate::test_util::{Lcg, SYNTH_START, synthetic_price_series, ts, world_helper};
use crate::theme::Theme;
use ultros_api_types::price_density::{DensityCell, PriceDensity};

fn assert_snapshot(name: &str, svg: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/charts/snapshots")
        .join(format!("{name}.svg"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, svg).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {} — run `UPDATE_SNAPSHOTS=1 cargo test -p ultros-charts snapshot` to create it",
            path.display()
        )
    });
    assert_eq!(
        expected, svg,
        "snapshot `{name}` drifted — if the rendering change is intentional, \
         rerun with UPDATE_SNAPSHOTS=1 and review the .svg diff"
    );
}

/// Web-chart shaped options (no title row / built-in legend, site theme),
/// matching how `price_history_chart.rs` calls the layout.
fn web_options(mode: ChartMode) -> PriceChartOptions {
    PriceChartOptions {
        width: 960.0,
        height: 540.0,
        show_legend: false,
        show_market_average: true,
        show_volume: true,
        mode,
        theme: Theme::site(),
        ..Default::default()
    }
}

#[test]
fn snapshot_price_mode_with_raw_dots() {
    let model = build_price_history_chart(
        &world_helper(),
        &synthetic_price_series(),
        &web_options(ChartMode::Price),
    );
    assert_snapshot("price", &scene_to_svg(&model.scene));
}

#[test]
fn snapshot_candles_mode() {
    let model = build_price_history_chart(
        &world_helper(),
        &synthetic_price_series(),
        &web_options(ChartMode::Candles),
    );
    assert_snapshot("candles", &scene_to_svg(&model.scene));
}

#[test]
fn snapshot_range_mode() {
    let model = build_price_history_chart(
        &world_helper(),
        &synthetic_price_series(),
        &web_options(ChartMode::Range),
    );
    assert_snapshot("range", &scene_to_svg(&model.scene));
}

#[test]
fn snapshot_grid_view() {
    let grid = build_price_grid(
        &world_helper(),
        &synthetic_price_series(),
        &GridOptions::default(),
    );
    assert_eq!(grid.cells.len(), 2, "one cell per synthetic world");
    // One file with every cell, labelled, so the shared-domain alignment
    // across cells is part of the snapshot too.
    let combined: String = grid
        .cells
        .iter()
        .map(|cell| format!("<!-- {} -->\n{}\n", cell.name, scene_to_svg(&cell.scene)))
        .collect();
    assert_snapshot("grid", &combined);
}

#[test]
fn snapshot_density_mode() {
    // Sparse seeded time × price grid, same flavor as the series fixture.
    let mut rng = Lcg::new(0xD05E);
    let mut cells = Vec::new();
    for bucket in 0..45u16 {
        for bin in 0..24u16 {
            // ~1 in 3 cells populated, banded toward the middle bins.
            if rng.next(3) == 0 {
                let center_bias = 12u32.abs_diff(bin as u32);
                cells.push(DensityCell {
                    ts: ts(SYNTH_START + bucket as i64 * 86_400),
                    bin,
                    n: 1 + rng.next(9) as u32 + (12 - center_bias.min(12)),
                });
            }
        }
    }
    let density = PriceDensity {
        bucket_seconds: 86_400,
        from: ts(SYNTH_START),
        to: ts(SYNTH_START + 45 * 86_400),
        price_lo: 38_000,
        bin_width: 1_500.0,
        price_bins: 24,
        cells,
    };
    let model = build_price_density_chart(
        &density,
        &DensityChartOptions {
            width: 960.0,
            height: 540.0,
            utc_offset_minutes: 0,
            theme: Theme::site(),
            ..Default::default()
        },
    );
    assert_snapshot("density", &scene_to_svg(&model.scene));
}
