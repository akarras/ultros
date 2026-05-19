pub mod alert;
pub mod bootstrap;
pub mod cheapest_listings;
mod ffxiv_character;
pub mod icon_size;
pub mod item_stats;
pub mod list;
mod listings;
pub mod market_heat;
pub mod market_pulse;
pub mod recent_sales;
pub mod resale_quality;
pub mod result;
pub mod retainer;
mod sale_history;
pub mod search;
pub mod sparklines;
pub mod trends;
pub mod user;
pub mod websocket;
pub mod world;
pub mod world_helper;

pub use ffxiv_character::*;
pub use listings::ActiveListing;
pub use retainer::Retainer;
pub use sale_history::{CompactSale, ExtendedSaleHistory, SaleHistory};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentlyShownItem {
    pub listings: Vec<(ActiveListing, Retainer)>,
    pub sales: Vec<SaleHistory>,
}
