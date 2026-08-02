//! Shared fixtures for unit tests: a synthetic world tree and PriceSeries rows.

use chrono::NaiveDateTime;
use ultros_api_types::CompactSale;
use ultros_api_types::price_series::{PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup};
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;

pub(crate) fn ts(secs: i64) -> NaiveDateTime {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap()
        .naive_utc()
}

pub(crate) fn bucket(
    ts_secs: i64,
    open: i32,
    high: i32,
    low: i32,
    close: i32,
    units: i64,
) -> PriceBucket {
    PriceBucket {
        ts: ts(ts_secs),
        open,
        high,
        low,
        close,
        gil: i64::from(close) * units,
        units,
        sales: 3,
        p25: low,
        p50: (low + high) / 2,
        p75: high,
    }
}

/// Two worlds of one datacenter, 10 daily buckets each, gently trending up.
pub(crate) fn two_world_series() -> PriceSeries {
    let entry = |id: i32, base: i32| PriceSeriesEntry {
        id,
        buckets: (0..10)
            .map(|i| {
                let p = base + i * 10;
                bucket(
                    1_700_006_400 + i as i64 * 86_400,
                    p,
                    p + 20,
                    p - 10,
                    p + 5,
                    2,
                )
            })
            .collect(),
    };
    PriceSeries {
        bucket_seconds: 86_400,
        group: SeriesGroup::World,
        from: ts(1_700_006_400),
        to: ts(1_700_006_400 + 9 * 86_400),
        series: vec![entry(1, 1_000), entry(2, 1_200)],
        raw: None,
    }
}

/// Tiny deterministic LCG so fixtures can wander like market data without a
/// `rand` dependency (and without any wall-clock nondeterminism, which the
/// snapshot tests must never see).
pub(crate) struct Lcg(u64);

impl Lcg {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Next value in `0..bound`.
    pub(crate) fn next(&mut self, bound: u64) -> u64 {
        // Numerical Recipes constants; period 2^64.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound.max(1)
    }
}

/// Fixed start for the synthetic series: 2023-11-15 00:00:00 UTC, a bucket
/// boundary at every ladder step.
pub(crate) const SYNTH_START: i64 = 1_700_006_400;

/// A richer synthetic dataset than [`two_world_series`], for snapshot tests:
/// two worlds (Gilgamesh, Adamantoise) over 45 daily buckets. Prices follow
/// a seeded random walk; sale counts vary from 1 (below the wick-only candle
/// threshold) upward; world 2 skips every seventh bucket so gap handling is
/// exercised; and `raw` carries individual sales so the scatter-dot layer
/// draws. Fully deterministic — same scene, same SVG, every run.
pub(crate) fn synthetic_price_series() -> PriceSeries {
    let mut rng = Lcg::new(0x5EED_CAFE);
    let mut raw: Vec<CompactSale> = Vec::new();

    let mut entry = |id: i32, base: i64, skip_every: usize| {
        let mut price = base;
        let buckets = (0..45)
            .filter(|i| skip_every == 0 || (i + 1) % skip_every != 0)
            .map(|i| {
                let ts_secs = SYNTH_START + i as i64 * 86_400;
                // Walk ±6%, floored so the series never collapses to zero.
                let step = (price * (rng.next(13) as i64 - 6)) / 100;
                price = (price + step).max(200);
                let spread = (price / 10).max(4);
                let low = (price - rng.next(spread as u64) as i64).max(1) as i32;
                let high = (price + rng.next(spread as u64) as i64) as i32;
                let open = low + rng.next((high - low + 1) as u64) as i32;
                let close = low + rng.next((high - low + 1) as u64) as i32;
                let sales = 1 + rng.next(7) as u32;
                let units = sales as i64 * (1 + rng.next(3) as i64);
                for s in 0..sales.min(3) {
                    raw.push(CompactSale {
                        quantity: 1 + rng.next(3) as i32,
                        price_per_item: low + rng.next((high - low + 1) as u64) as i32,
                        hq: s % 2 == 0,
                        sold_date: ts(ts_secs + s as i64 * 7_200),
                        world_id: id,
                    });
                }
                PriceBucket {
                    ts: ts(ts_secs),
                    open,
                    high,
                    low,
                    close,
                    gil: price * units,
                    units,
                    sales,
                    p25: low + (price as i32 - low) / 2,
                    p50: price as i32,
                    p75: price as i32 + (high - price as i32) / 2,
                }
            })
            .collect();
        PriceSeriesEntry { id, buckets }
    };

    let series = vec![entry(1, 40_000, 0), entry(2, 55_000, 7)];
    PriceSeries {
        bucket_seconds: 86_400,
        group: SeriesGroup::World,
        from: ts(SYNTH_START),
        to: ts(SYNTH_START + 44 * 86_400),
        series,
        raw: Some(raw),
    }
}

/// [`synthetic_price_series`] with one bucket laundered to roughly twenty
/// times the market — the outlier shape from #1068, where a single sale used
/// to stretch the y axis until the real history rendered as a line along the
/// bottom edge.
pub(crate) fn synthetic_price_series_with_outlier() -> PriceSeries {
    const LAUNDERED: i32 = 1_000_000;
    let mut series = synthetic_price_series();
    let buckets = &mut series.series[0].buckets;
    let index = buckets.len() / 2;
    let target = &mut buckets[index];
    target.open = LAUNDERED;
    target.high = LAUNDERED;
    target.low = LAUNDERED;
    target.close = LAUNDERED;
    target.p25 = LAUNDERED;
    target.p50 = LAUNDERED;
    target.p75 = LAUNDERED;
    target.gil = LAUNDERED as i64 * target.units;
    series
}

/// Two regions; region 1 has two datacenters; datacenter 1 has two worlds.
/// World ids: 1 = Gilgamesh (Aether), 2 = Adamantoise (Aether),
/// 3 = Behemoth (Primal), 4 = Cerberus (Chaos / Europe).
pub(crate) fn world_helper() -> WorldHelper {
    WorldHelper::new(WorldData {
        regions: vec![
            Region {
                id: 1,
                name: "North-America".to_string(),
                datacenters: vec![
                    Datacenter {
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
                    },
                    Datacenter {
                        id: 2,
                        name: "Primal".to_string(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 3,
                            name: "Behemoth".to_string(),
                            datacenter_id: 2,
                        }],
                    },
                ],
            },
            Region {
                id: 2,
                name: "Europe".to_string(),
                datacenters: vec![Datacenter {
                    id: 3,
                    name: "Chaos".to_string(),
                    region_id: 2,
                    worlds: vec![World {
                        id: 4,
                        name: "Cerberus".to_string(),
                        datacenter_id: 3,
                    }],
                }],
            },
        ],
    })
}
