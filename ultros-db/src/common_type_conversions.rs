use ultros_api_types::{
    list::{List, ListItem},
    user::OwnedRetainer,
    world::{Datacenter, Region, World, WorldData},
    ActiveListing, FfxivCharacter, Retainer, SaleHistory,
};

use crate::{
    entity::{self, datacenter, final_fantasy_character, list, list_item, owned_retainers, region},
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
                    let region::Model {
                        id: region_id,
                        name,
                    } = region;
                    Region {
                        id: *region_id,
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
                                    region_id: *region_id,
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

impl From<final_fantasy_character::Model> for FfxivCharacter {
    fn from(value: final_fantasy_character::Model) -> Self {
        let final_fantasy_character::Model {
            id,
            first_name,
            last_name,
            world_id,
        } = value;
        Self {
            id,
            first_name,
            last_name,
            world_id,
        }
    }
}

impl From<owned_retainers::Model> for OwnedRetainer {
    fn from(value: owned_retainers::Model) -> Self {
        let owned_retainers::Model {
            id,
            retainer_id,
            discord_id,
            character_id,
            weight,
        } = value;
        Self {
            id,
            retainer_id,
            discord_id,
            character_id,
            weight,
        }
    }
}

impl From<list::Model> for List {
    fn from(value: list::Model) -> Self {
        let list::Model {
            id,
            owner,
            name,
            world_id,
            datacenter_id,
            region_id,
        } = value;
        Self {
            id,
            owner,
            name,
            world_id,
            datacenter_id,
            region_id,
        }
    }
}

impl From<list_item::Model> for ListItem {
    fn from(value: list_item::Model) -> Self {
        let list_item::Model {
            id,
            item_id,
            list_id,
            hq,
            quantity,
        } = value;
        Self {
            id,
            item_id,
            list_id,
            hq,
            quantity,
        }
    }
}
