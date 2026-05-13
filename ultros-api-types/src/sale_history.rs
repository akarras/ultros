use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// Relates to the sale history stored in ultros_db, but is a clean type
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaleHistory {
    pub id: i32,
    pub quantity: i32,
    pub price_per_item: i32,
    pub buying_character_id: i32,
    pub hq: bool,
    pub sold_item_id: i32,
    pub sold_date: NaiveDateTime,
    pub world_id: i32,
    pub buyer_name: Option<String>,
}

/// Lean projection of a sale event for charting. Drops buyer/id metadata so the
/// payload is small enough to ship thousands of rows for the extended history view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactSale {
    pub quantity: i32,
    pub price_per_item: i32,
    pub hq: bool,
    pub sold_date: NaiveDateTime,
    pub world_id: i32,
}

/// Response payload for the extended sale history endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedSaleHistory {
    pub sales: Vec<CompactSale>,
}
