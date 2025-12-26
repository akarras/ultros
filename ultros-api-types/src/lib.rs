pub mod alerts;
pub mod cheapest_listings;
mod ffxiv_character;
pub mod icon_size;
pub mod list;
mod listings;
pub mod recent_sales;
pub mod result;
pub mod retainer;
mod sale_history;
pub mod search;
pub mod user;
pub mod websocket;
pub mod world;
pub mod world_helper;

pub use ffxiv_character::*;
pub use listings::ActiveListing;
pub use retainer::Retainer;
pub use sale_history::SaleHistory;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentlyShownItem {
    pub listings: Vec<(ActiveListing, Retainer)>,
    pub sales: Vec<SaleHistory>,
}
