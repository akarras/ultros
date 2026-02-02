use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sales {
    pub price_per_unit: i32,
    pub sale_date: NaiveDateTime,
    #[serde(default)]
    pub quantity: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaleData {
    pub item_id: i32,
    pub hq: bool,
    pub sales: Vec<Sales>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSales {
    pub sales: Vec<SaleData>,
}
