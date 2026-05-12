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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ListWithPermission {
    pub list: List,
    pub permission: ListPermission,
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
    /// Per-item price target for the list-scoped price alert trigger. When set,
    /// `AlertTrigger::ListItemThreshold` rules fire when a listing meets or
    /// undercuts this price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_price: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListInvite {
    pub id: String,
    pub list_id: i32,
    pub permission: ListPermission,
    pub max_uses: Option<i32>,
    pub uses: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListSharedUser {
    pub list_id: i32,
    pub user_id: i64,
    pub username: String,
    pub permission: ListPermission,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListSharedGroup {
    pub list_id: i32,
    pub group_id: i32,
    pub group_name: String,
    pub permission: ListPermission,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShareListUser {
    pub user_id: i64,
    pub permission: ListPermission,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShareListGroup {
    pub group_id: i32,
    pub permission: ListPermission,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateInvite {
    pub permission: ListPermission,
    pub max_uses: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_permission_from_i16_known_values() {
        assert_eq!(ListPermission::from(0_i16), ListPermission::None);
        assert_eq!(ListPermission::from(1_i16), ListPermission::Read);
        assert_eq!(ListPermission::from(2_i16), ListPermission::Write);
        assert_eq!(ListPermission::from(3_i16), ListPermission::Owner);
    }

    #[test]
    fn list_permission_from_unknown_falls_back_to_none() {
        assert_eq!(ListPermission::from(-1_i16), ListPermission::None);
        assert_eq!(ListPermission::from(4_i16), ListPermission::None);
        assert_eq!(ListPermission::from(i16::MAX), ListPermission::None);
        assert_eq!(ListPermission::from(i16::MIN), ListPermission::None);
    }

    #[test]
    fn list_permission_ord_increases_with_capability() {
        assert!(ListPermission::None < ListPermission::Read);
        assert!(ListPermission::Read < ListPermission::Write);
        assert!(ListPermission::Write < ListPermission::Owner);
    }

    #[test]
    fn list_with_permission_serde_roundtrip() {
        let list = ListWithPermission {
            list: List {
                id: 1,
                owner: 2,
                name: "Shared".into(),
                wdr_filter: AnySelector::World(3),
            },
            permission: ListPermission::Write,
        };
        let s = serde_json::to_string(&list).unwrap();
        let back: ListWithPermission = serde_json::from_str(&s).unwrap();
        assert_eq!(back.list.id, 1);
        assert_eq!(back.permission, ListPermission::Write);
    }

    #[test]
    fn list_item_default_is_all_zero_and_none() {
        let item = ListItem::default();
        assert_eq!(item.id, 0);
        assert_eq!(item.item_id, 0);
        assert_eq!(item.list_id, 0);
        assert!(item.hq.is_none());
        assert!(item.quantity.is_none());
        assert!(item.acquired.is_none());
    }

    #[test]
    fn list_item_serde_roundtrip() {
        let item = ListItem {
            id: 1,
            item_id: 2,
            list_id: 3,
            hq: Some(true),
            quantity: Some(99),
            acquired: Some(50),
        };
        let s = serde_json::to_string(&item).unwrap();
        let back: ListItem = serde_json::from_str(&s).unwrap();
        assert_eq!(item, back);
    }
}
