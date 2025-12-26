use serde::{Deserialize, Serialize};

use crate::world_helper::AnySelector;

#[derive(Debug, Deserialize, Serialize)]
pub enum AlertsRx {
    Undercuts {
        margin: i32,
    },
    CreatePriceAlert {
        item_id: i32,
        travel_amount: AnySelector,
        price_threshold: i32,
    },
    Ping(Vec<u8>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AlertsTx {
    RetainerUndercut {
        item_id: i32,
        item_name: String,
        /// List of all the retainers that were just undercut
        undercut_retainers: Vec<UndercutRetainer>,
    },
    PriceAlert {
        world_id: i32,
        item_id: i32,
        item_name: String,
        price: i32,
    },
}

#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize, Clone)]
pub struct UndercutRetainer {
    pub id: i32,
    pub name: String,
    pub undercut_amount: i32,
}
