//! Shared fixtures for unit tests: a synthetic world tree and PriceSeries rows.

use chrono::NaiveDateTime;
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
