//! Shared fixtures for unit tests: a synthetic world tree and SaleHistory rows.

use chrono::NaiveDateTime;
use ultros_api_types::SaleHistory;
use ultros_api_types::world::{Datacenter, Region, World, WorldData};
use ultros_api_types::world_helper::WorldHelper;

pub(crate) fn ts(secs: i64) -> NaiveDateTime {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap()
        .naive_utc()
}

pub(crate) fn sale(price: i32, quantity: i32, world_id: i32, sold: NaiveDateTime) -> SaleHistory {
    SaleHistory {
        id: 0,
        quantity,
        price_per_item: price,
        buying_character_id: 0,
        hq: false,
        sold_item_id: 1,
        sold_date: sold,
        world_id,
        buyer_name: None,
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
