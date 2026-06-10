//! Group sales by the narrowest world-hierarchy level that still yields
//! multiple groups — ported from the old plotters chart. One deliberate
//! change: timestamps stay naive-UTC (the old code converted to the
//! server's local timezone, which is UTC in prod anyway; keeping UTC makes
//! output deterministic across environments).

use std::collections::{BTreeMap, HashSet};

use chrono::NaiveDateTime;
use itertools::Itertools;
use ultros_api_types::SaleHistory;
use ultros_api_types::world_helper::{AnySelector, WorldHelper};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SalePoint {
    /// Naive UTC, matching `SaleHistory::sold_date`.
    pub ts: NaiveDateTime,
    pub price: i32,
    pub quantity: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    /// Sorted by timestamp ascending.
    pub points: Vec<SalePoint>,
}

/// Which level of the world hierarchy to roll sales up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLevel {
    Region,
    Datacenter,
    World,
}

impl GroupLevel {
    /// Stable identifier (list keys / debugging); user-facing names come
    /// from the app's i18n layer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Region => "Region",
            Self::Datacenter => "Datacenter",
            Self::World => "World",
        }
    }
}

/// Group sales at an explicit hierarchy level. Sales whose world id isn't in
/// the helper are dropped. Series sort by name; points by timestamp.
pub fn group_sales_by_level(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    level: GroupLevel,
) -> Vec<Series> {
    let mut groups = BTreeMap::<AnySelector, Series>::new();
    for sale in sales {
        let Some(world) = world_helper
            .lookup_selector(AnySelector::World(sale.world_id))
            .and_then(|r| r.as_world())
        else {
            continue;
        };
        let selector = match level {
            GroupLevel::World => AnySelector::World(world.id),
            GroupLevel::Datacenter => AnySelector::Datacenter(world.datacenter_id),
            GroupLevel::Region => {
                let Some(datacenter) = world_helper
                    .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                    .and_then(|r| r.as_datacenter())
                else {
                    continue;
                };
                AnySelector::Region(datacenter.region_id)
            }
        };
        let Some(result) = world_helper.lookup_selector(selector) else {
            continue;
        };
        groups
            .entry(selector)
            .or_insert_with(|| Series {
                name: result.get_name().to_string(),
                points: Vec::new(),
            })
            .points
            .push(SalePoint {
                ts: sale.sold_date,
                price: sale.price_per_item,
                quantity: sale.quantity,
            });
    }
    let mut series: Vec<Series> = groups
        .into_values()
        .sorted_by_cached_key(|series| series.name.clone())
        .collect();
    for series in &mut series {
        series.points.sort_by_key(|p| p.ts);
    }
    series
}

/// The narrowest level that still yields multiple groups — the old
/// `group_sales_by_scope` cascade.
pub fn auto_group_level(world_helper: &WorldHelper, sales: &[SaleHistory]) -> GroupLevel {
    let world_ids: HashSet<_> = sales
        .iter()
        .map(|s| AnySelector::World(s.world_id))
        .collect();
    let datacenters: HashSet<_> = world_ids
        .iter()
        .flat_map(|world| {
            world_helper
                .lookup_selector(*world)
                .and_then(|s| s.as_world())
                .map(|w| AnySelector::Datacenter(w.datacenter_id))
        })
        .collect();
    let regions: HashSet<_> = datacenters
        .iter()
        .flat_map(|dc| {
            world_helper
                .lookup_selector(*dc)
                .and_then(|dc| dc.as_datacenter())
                .map(|dc| AnySelector::Region(dc.region_id))
        })
        .collect();
    if datacenters.len() <= 1 {
        GroupLevel::World
    } else if regions.len() <= 1 {
        GroupLevel::Datacenter
    } else {
        GroupLevel::Region
    }
}

/// Which grouping levels make sense for the scope page being viewed —
/// ported from the web UI (a world page only offers World; a DC page offers
/// DC + World; a region page or unknown scope offers everything).
pub fn available_group_levels(world_helper: &WorldHelper, scope_name: &str) -> Vec<GroupLevel> {
    match world_helper.lookup_world_by_name(scope_name) {
        Some(result) if result.as_world().is_some() => vec![GroupLevel::World],
        Some(result) if result.as_datacenter().is_some() => {
            vec![GroupLevel::Datacenter, GroupLevel::World]
        }
        _ => vec![
            GroupLevel::Region,
            GroupLevel::Datacenter,
            GroupLevel::World,
        ],
    }
}

/// Auto-picked grouping (the PNG path): the narrowest level that still
/// yields multiple groups.
pub fn group_sales_by_scope(world_helper: &WorldHelper, sales: &[SaleHistory]) -> Vec<Series> {
    group_sales_by_level(world_helper, sales, auto_group_level(world_helper, sales))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sale, ts, world_helper};

    fn names(series: &[Series]) -> Vec<&str> {
        series.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn single_datacenter_groups_by_world() {
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 2, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Adamantoise", "Gilgamesh"]);
    }

    #[test]
    fn single_region_groups_by_datacenter() {
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 3, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Aether", "Primal"]);
    }

    #[test]
    fn multiple_regions_group_by_region() {
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 4, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Europe", "North-America"]);
    }

    #[test]
    fn points_are_sorted_by_time() {
        let sales = vec![sale(100, 1, 1, ts(100)), sale(200, 1, 1, ts(50))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(series.len(), 1);
        assert_eq!(
            series[0].points.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![ts(50), ts(100)]
        );
    }

    #[test]
    fn unknown_worlds_are_dropped() {
        // world_id 999 isn't in the fixture; it must vanish rather than
        // panic or leak into another series.
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 999, ts(10))];
        let series = group_sales_by_scope(&world_helper(), &sales);
        assert_eq!(names(&series), vec!["Gilgamesh"]);
        assert_eq!(series[0].points.len(), 1);
    }

    #[test]
    fn explicit_level_overrides_auto() {
        // Sales on one DC would auto-group by world; force datacenter level.
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 2, ts(10))];
        let series = group_sales_by_level(&world_helper(), &sales, GroupLevel::Datacenter);
        assert_eq!(names(&series), vec!["Aether"]);
        assert_eq!(series[0].points.len(), 2);
        let series = group_sales_by_level(&world_helper(), &sales, GroupLevel::Region);
        assert_eq!(names(&series), vec!["North-America"]);
    }

    #[test]
    fn auto_level_matches_scope_cascade() {
        let h = world_helper();
        // one DC → world level
        assert_eq!(
            auto_group_level(&h, &[sale(1, 1, 1, ts(0)), sale(1, 1, 2, ts(0))]),
            GroupLevel::World
        );
        // two DCs, one region → datacenter level
        assert_eq!(
            auto_group_level(&h, &[sale(1, 1, 1, ts(0)), sale(1, 1, 3, ts(0))]),
            GroupLevel::Datacenter
        );
        // two regions → region level
        assert_eq!(
            auto_group_level(&h, &[sale(1, 1, 1, ts(0)), sale(1, 1, 4, ts(0))]),
            GroupLevel::Region
        );
    }

    #[test]
    fn scope_grouping_equals_explicit_level_at_the_auto_level() {
        let h = world_helper();
        let scenarios = [
            vec![sale(100, 1, 1, ts(0)), sale(200, 1, 2, ts(10))], // one DC
            vec![sale(100, 1, 1, ts(0)), sale(200, 1, 3, ts(10))], // one region
            vec![sale(100, 1, 1, ts(0)), sale(200, 1, 4, ts(10))], // two regions
        ];
        for sales in scenarios {
            let auto = group_sales_by_scope(&h, &sales);
            let explicit = group_sales_by_level(&h, &sales, auto_group_level(&h, &sales));
            assert_eq!(auto, explicit);
        }
    }

    #[test]
    fn available_levels_follow_the_viewed_scope() {
        let h = world_helper();
        assert_eq!(
            available_group_levels(&h, "Gilgamesh"),
            vec![GroupLevel::World]
        );
        assert_eq!(
            available_group_levels(&h, "Aether"),
            vec![GroupLevel::Datacenter, GroupLevel::World]
        );
        assert_eq!(
            available_group_levels(&h, "North-America"),
            vec![
                GroupLevel::Region,
                GroupLevel::Datacenter,
                GroupLevel::World
            ]
        );
        assert_eq!(
            available_group_levels(&h, "Not A Scope"),
            vec![
                GroupLevel::Region,
                GroupLevel::Datacenter,
                GroupLevel::World
            ]
        );
    }
}
