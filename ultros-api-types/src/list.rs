use crate::world_helper::AnySelector;
/// Lists serve as a way to gather a large amount of items
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i16)]
pub enum ListPermission {
    None = 0,
    Read = 1,
    Write = 2,
    Owner = 3,
}

impl From<i16> for ListPermission {
    fn from(value: i16) -> Self {
        match value {
            1 => ListPermission::Read,
            2 => ListPermission::Write,
            3 => ListPermission::Owner,
            _ => ListPermission::None,
        }
    }
}

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

#[derive(Debug, Deserialize, Serialize, Clone, Default, Eq, PartialEq, PartialOrd, Ord)]
pub struct ListItem {
    pub id: i32,
    pub item_id: i32,
    pub list_id: i32,
    /// None if it doesn't matter whether this item is HQ, otherwise follows value.
    pub hq: Option<bool>,
    pub quantity: Option<i32>,
    pub acquired: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListInvite {
    pub id: String,
    pub list_id: i32,
    pub permission: ListPermission,
    pub max_uses: Option<i32>,
    pub uses: i32,
}
