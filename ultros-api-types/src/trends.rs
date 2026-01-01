use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TrendItem {
    pub item_id: i32,
    pub hq: bool,
    pub price: i32,
    pub world_id: i32,
    pub average_sale_price: f32,
    pub sales_per_week: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrendsData {
    pub high_velocity: Vec<TrendItem>,
    pub rising_price: Vec<TrendItem>,
    pub falling_price: Vec<TrendItem>,
}
