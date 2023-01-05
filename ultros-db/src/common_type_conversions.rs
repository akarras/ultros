use ultros_api_types::{
    world::{Datacenter, Region, World, WorldData},
    ActiveListing, Retainer, SaleHistory,
};

use crate::{
    entity::{self, datacenter, region},
    world_cache::WorldCache,
};

impl From<entity::active_listing::Model> for ActiveListing {
    fn from(value: entity::active_listing::Model) -> Self {
        let entity::active_listing::Model {
            id,
            world_id,
            item_id,
            retainer_id,
            price_per_unit,
            quantity,
            hq,
            timestamp,
        } = value;
        Self {
            id,
            world_id,
            item_id,
            retainer_id,
            price_per_unit,
            quantity,
            hq,
            timestamp,
        }
    }
}

impl From<entity::sale_history::Model> for SaleHistory {
    fn from(value: entity::sale_history::Model) -> Self {
        let entity::sale_history::Model {
            quantity,
            price_per_item,
            buying_character_id,
            hq,
            sold_item_id,
            sold_date,
            world_id,
            buyer_name,
        } = value;
        Self {
            quantity,
            price_per_item,
            buying_character_id,
            hq,
            sold_item_id,
            sold_date,
            world_id,
            buyer_name,
        }
    }
}

impl From<entity::retainer::Model> for Retainer {
    fn from(value: entity::retainer::Model) -> Self {
        let entity::retainer::Model {
            id,
            world_id,
            name,
            retainer_city_id,
        } = value;
        Self {
            id,
            world_id,
            name,
            retainer_city_id,
        }
    }
}

impl From<&WorldCache> for WorldData {
    fn from(value: &WorldCache) -> Self {
        Self {
            regions: value
                .get_all()
                .iter()
                .map(|(region, datacenters)| {
                    let region::Model { id, name } = region;
                    Region {
                        id: *id,
                        name: name.to_string(),
                        datacenters: datacenters
                            .into_iter()
                            .map(|(dc, worlds)| {
                                let datacenter::Model { id, name, .. } = dc;
                                Datacenter {
                                    id: *id,
                                    name: name.to_string(),
                                    worlds: worlds
                                        .into_iter()
                                        .map(|world| World::from(*world))
                                        .collect(),
                                    region_id: *id,
                                }
                            })
                            .collect(),
                    }
                })
                .collect(),
        }
    }
}

/// World conversion is only possible for world types. Everything else uses world cache
impl From<&entity::world::Model> for World {
    fn from(value: &entity::world::Model) -> Self {
        let entity::world::Model {
            id,
            name,
            datacenter_id,
        } = value;
        Self {
            id: *id,
            name: name.to_string(),
            datacenter_id: *datacenter_id,
        }
    }
}
