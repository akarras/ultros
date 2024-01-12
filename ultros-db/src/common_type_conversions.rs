use crate::{
    entity::{
        self, datacenter, final_fantasy_character, list, list_item, owned_retainers, region,
        unknown_final_fantasy_character,
    },
    world_cache::WorldCache,
};
use thiserror::Error;
use ultros_api_types::{
    list::{List, ListItem},
    retainer::Retainer,
    user::OwnedRetainer,
    world::{Datacenter, Region, World, WorldData},
    world_helper::AnySelector,
    ActiveListing, FfxivCharacter, SaleHistory, UnknownCharacter,
};

#[derive(Debug, Error)]
pub enum ApiConversionError {
    #[error("No world was supplied for the list")]
    InvalidListNoWorld,
}

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

pub struct SaleHistoryReturn(
    pub entity::sale_history::Model,
    pub Option<entity::unknown_final_fantasy_character::Model>,
);

impl From<SaleHistoryReturn> for SaleHistory {
    fn from(SaleHistoryReturn(value, character): SaleHistoryReturn) -> Self {
        let entity::sale_history::Model {
            quantity,
            price_per_item,
            buying_character_id,
            hq,
            sold_item_id,
            sold_date,
            world_id,
            id,
            // buyer_name,
        } = value;
        Self {
            id,
            quantity,
            price_per_item,
            buying_character_id,
            hq,
            sold_item_id,
            sold_date,
            world_id,
            buyer_name: character.map(|c| c.name),
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
                .get_inner_data()
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
                            .iter()
                            .map(|(dc, worlds)| {
                                let datacenter::Model { id, name, .. } = dc;
                                Datacenter {
                                    id: *id,
                                    name: name.to_string(),
                                    worlds: worlds
                                        .iter()
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

impl From<&unknown_final_fantasy_character::Model> for UnknownCharacter {
    fn from(value: &unknown_final_fantasy_character::Model) -> Self {
        let unknown_final_fantasy_character::Model { id, name } = value;
        Self {
            id: *id,
            name: name.clone(),
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

impl TryFrom<list::Model> for List {
    type Error = ApiConversionError;
    fn try_from(value: list::Model) -> Result<Self, Self::Error> {
        let list::Model {
            id,
            owner,
            name,
            world_id,
            datacenter_id,
            region_id,
        } = value;
        // there should only ever be one world/region/datacenter but just go in order in the off chance there are duplicates
        Ok(Self {
            id,
            owner,
            name,
            wdr_filter: match (world_id, datacenter_id, region_id) {
                (Some(world_id), _, _) => AnySelector::World(world_id),
                (_, Some(datacenter_id), _) => AnySelector::Datacenter(datacenter_id),
                (_, _, Some(region_id)) => AnySelector::Region(region_id),
                _ => return Err(ApiConversionError::InvalidListNoWorld),
            },
        })
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

impl From<ListItem> for list_item::Model {
    fn from(value: ListItem) -> Self {
        let ListItem {
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

impl From<AnySelector> for crate::world_cache::AnySelector {
    fn from(value: AnySelector) -> Self {
        match value {
            AnySelector::Region(region) => crate::world_cache::AnySelector::Region(region),
            AnySelector::Datacenter(dc) => crate::world_cache::AnySelector::Datacenter(dc),
            AnySelector::World(world) => crate::world_cache::AnySelector::World(world),
        }
    }
}
