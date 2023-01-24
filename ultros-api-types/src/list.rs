use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct List {
    pub id: i32,
    pub owner: i64,
    pub name: String,
    pub world_id: Option<i32>,
    pub datacenter_id: Option<i32>,
    pub region_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ListItem {
    pub id: i32,
    pub item_id: i32,
    pub list_id: i32,
    pub hq: Option<bool>,
    pub quantity: Option<i32>,
}
