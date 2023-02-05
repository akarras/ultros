use crate::world_helper::AnySelector;
/// Lists serve as a way to gather a large amount of items
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateList {
    /// Name of the list to be created
    pub name: String,
    /// World/Datacenter/Region that this list should be compared against.
    pub wdr_filter: AnySelector,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct List {
    pub id: i32,
    pub owner: i64,
    pub name: String,
    /// World/Datacenter/Region that this list should be compared against.
    pub wdr_filter: AnySelector,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ListItem {
    pub id: i32,
    pub item_id: i32,
    pub list_id: i32,
    /// None if it doesn't matter whether this item is HQ, otherwise follows value.
    pub hq: Option<bool>,
    pub quantity: Option<i32>,
}
