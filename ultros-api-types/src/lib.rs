mod listings;
mod retainer;
mod sale_history;
pub mod world;
pub mod world_helper;

pub use listings::ActiveListing;
pub use retainer::Retainer;
pub use sale_history::SaleHistory;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentlyShownItem {
    pub listings: Vec<(ActiveListing, Retainer)>,
    pub sales: Vec<SaleHistory>,
}
