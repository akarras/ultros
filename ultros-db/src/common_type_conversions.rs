use ultros_api_types::{ActiveListing, Retainer, SaleHistory};

use crate::entity;

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
