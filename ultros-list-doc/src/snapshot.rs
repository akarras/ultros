//! Plain-value views of a document, and the diff between two of them.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use ultros_api_types::world_helper::AnySelector;

use crate::key::{KeyError, RowKey, TargetKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSnapshot {
    pub key: RowKey,
    pub need: i64,
    pub acquired: i64,
    pub target: Option<i64>,
}

/// How an intermediate item in a crafting project is sourced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CraftMode {
    #[default]
    Craft,
    Buy,
}

impl CraftMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CraftMode::Craft => "craft",
            CraftMode::Buy => "buy",
        }
    }
}

impl fmt::Display for CraftMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CraftMode {
    type Err = KeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "craft" => Ok(CraftMode::Craft),
            "buy" => Ok(CraftMode::Buy),
            other => Err(KeyError::Shape(other.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetSnapshot {
    pub key: TargetKey,
    pub count: i64,
    pub completed: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CraftChoiceSnapshot {
    pub item_id: i32,
    pub mode: CraftMode,
    pub recipe_id: Option<i32>,
    pub on_hand: i64,
    pub crafted: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CraftOptionsSnapshot {
    pub exclude_crystals: bool,
    pub include_vendors: bool,
}

/// `scope` is `None` only for a document nobody wrote a scope into; consumers
/// treat that as "leave the list's scope alone".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaSnapshot {
    pub name: String,
    pub scope: Option<AnySelector>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetChange {
    Added(TargetSnapshot),
    Removed(TargetSnapshot),
    Updated {
        before: TargetSnapshot,
        after: TargetSnapshot,
    },
}

impl TargetChange {
    pub fn key(&self) -> TargetKey {
        match self {
            TargetChange::Added(t) | TargetChange::Removed(t) => t.key,
            TargetChange::Updated { after, .. } => after.key,
        }
    }
}

pub fn diff_targets(before: &[TargetSnapshot], after: &[TargetSnapshot]) -> Vec<TargetChange> {
    let before: BTreeMap<TargetKey, TargetSnapshot> = before.iter().map(|t| (t.key, *t)).collect();
    let after: BTreeMap<TargetKey, TargetSnapshot> = after.iter().map(|t| (t.key, *t)).collect();
    let mut changes = Vec::new();
    for (key, b) in &before {
        match after.get(key) {
            None => changes.push(TargetChange::Removed(*b)),
            Some(a) if a != b => changes.push(TargetChange::Updated {
                before: *b,
                after: *a,
            }),
            Some(_) => {}
        }
    }
    for (key, a) in &after {
        if !before.contains_key(key) {
            changes.push(TargetChange::Added(*a));
        }
    }
    changes.sort_by_key(|c| c.key());
    changes
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CraftChoiceChange {
    Added(CraftChoiceSnapshot),
    Removed(CraftChoiceSnapshot),
    Updated {
        before: CraftChoiceSnapshot,
        after: CraftChoiceSnapshot,
    },
}

impl CraftChoiceChange {
    pub fn item_id(&self) -> i32 {
        match self {
            CraftChoiceChange::Added(c) | CraftChoiceChange::Removed(c) => c.item_id,
            CraftChoiceChange::Updated { after, .. } => after.item_id,
        }
    }
}

pub fn diff_craft_choices(
    before: &[CraftChoiceSnapshot],
    after: &[CraftChoiceSnapshot],
) -> Vec<CraftChoiceChange> {
    let before: BTreeMap<i32, CraftChoiceSnapshot> =
        before.iter().map(|c| (c.item_id, *c)).collect();
    let after: BTreeMap<i32, CraftChoiceSnapshot> = after.iter().map(|c| (c.item_id, *c)).collect();
    let mut changes = Vec::new();
    for (item_id, b) in &before {
        match after.get(item_id) {
            None => changes.push(CraftChoiceChange::Removed(*b)),
            Some(a) if a != b => changes.push(CraftChoiceChange::Updated {
                before: *b,
                after: *a,
            }),
            Some(_) => {}
        }
    }
    for (item_id, a) in &after {
        if !before.contains_key(item_id) {
            changes.push(CraftChoiceChange::Added(*a));
        }
    }
    changes.sort_by_key(|c| c.item_id());
    changes
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowChange {
    Added(RowSnapshot),
    Removed(RowSnapshot),
    Updated {
        before: RowSnapshot,
        after: RowSnapshot,
    },
}

impl RowChange {
    pub fn key(&self) -> RowKey {
        match self {
            RowChange::Added(row) | RowChange::Removed(row) => row.key,
            RowChange::Updated { after, .. } => after.key,
        }
    }
}

/// Every row that differs between two snapshots, ordered by key. Inputs may be
/// in any order.
pub fn diff_rows(before: &[RowSnapshot], after: &[RowSnapshot]) -> Vec<RowChange> {
    let before: BTreeMap<RowKey, RowSnapshot> = before.iter().map(|r| (r.key, *r)).collect();
    let after: BTreeMap<RowKey, RowSnapshot> = after.iter().map(|r| (r.key, *r)).collect();
    let mut changes = Vec::new();
    for (key, b) in &before {
        match after.get(key) {
            None => changes.push(RowChange::Removed(*b)),
            Some(a) if a != b => changes.push(RowChange::Updated {
                before: *b,
                after: *a,
            }),
            Some(_) => {}
        }
    }
    for (key, a) in &after {
        if !before.contains_key(key) {
            changes.push(RowChange::Added(*a));
        }
    }
    changes.sort_by_key(|c| c.key());
    changes
}

pub fn encode_scope(scope: AnySelector) -> String {
    match scope {
        AnySelector::World(id) => format!("world:{id}"),
        AnySelector::Datacenter(id) => format!("datacenter:{id}"),
        AnySelector::Region(id) => format!("region:{id}"),
    }
}

pub fn parse_scope(text: &str) -> Option<AnySelector> {
    let (kind, id) = text.split_once(':')?;
    let id: i32 = id.parse().ok()?;
    match kind {
        "world" => Some(AnySelector::World(id)),
        "datacenter" => Some(AnySelector::Datacenter(id)),
        "region" => Some(AnySelector::Region(id)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(item: i32, need: i64, acquired: i64, target: Option<i64>) -> RowSnapshot {
        RowSnapshot {
            key: RowKey::new(item, None),
            need,
            acquired,
            target,
        }
    }

    #[test]
    fn diff_classifies_added_removed_and_updated_in_key_order() {
        let before = [
            row(3, 1, 0, None),
            row(1, 2, 0, None),
            row(2, 5, 5, Some(10)),
        ];
        let after = [
            row(2, 5, 6, Some(10)),
            row(1, 2, 0, None),
            row(4, 1, 0, None),
        ];
        let changes = diff_rows(&before, &after);
        assert_eq!(
            changes,
            vec![
                RowChange::Updated {
                    before: row(2, 5, 5, Some(10)),
                    after: row(2, 5, 6, Some(10)),
                },
                RowChange::Removed(row(3, 1, 0, None)),
                RowChange::Added(row(4, 1, 0, None)),
            ]
        );
    }

    #[test]
    fn diff_is_empty_for_equal_snapshots_in_different_orders() {
        let a = [row(1, 1, 0, None), row(2, 1, 0, None)];
        let b = [row(2, 1, 0, None), row(1, 1, 0, None)];
        assert!(diff_rows(&a, &b).is_empty());
    }

    #[test]
    fn scope_encoding_round_trips_every_variant_and_rejects_junk() {
        for scope in [
            AnySelector::World(79),
            AnySelector::Datacenter(5),
            AnySelector::Region(1),
        ] {
            assert_eq!(parse_scope(&encode_scope(scope)), Some(scope));
        }
        assert_eq!(encode_scope(AnySelector::World(79)), "world:79");
        assert_eq!(parse_scope("planet:1"), None);
        assert_eq!(parse_scope("world:x"), None);
        assert_eq!(parse_scope("world"), None);
    }
}
