//! Group sales by the narrowest world-hierarchy level that still yields
//! multiple groups — ported from the old plotters chart. One deliberate
//! change: timestamps stay naive-UTC (the old code converted to the
//! server's local timezone, which is UTC in prod anyway; keeping UTC makes
//! output deterministic across environments).

use std::collections::HashSet;

use chrono::NaiveDateTime;
use itertools::Itertools;
use ultros_api_types::world_helper::{AnySelector, WorldHelper};
use ultros_api_types::SaleHistory;

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

/// All sales on one datacenter → one series per world; one region → per
/// datacenter; otherwise per region. Series sort by name for stable colors.
pub fn group_sales_by_scope(world_helper: &WorldHelper, sales: &[SaleHistory]) -> Vec<Series> {
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
    let selectors = if datacenters.len() == 1 {
        world_ids
    } else if regions.len() == 1 {
        datacenters
    } else {
        regions
    };
    selectors
        .into_iter()
        .flat_map(|selector| series_for(world_helper, selector, sales))
        .sorted_by_cached_key(|series| series.name.clone())
        .collect()
}

fn series_for(
    world_helper: &WorldHelper,
    selector: AnySelector,
    sales: &[SaleHistory],
) -> Option<Series> {
    let result = world_helper.lookup_selector(selector)?;
    let mut points: Vec<SalePoint> = sales
        .iter()
        .filter(|sale| {
            world_helper
                .lookup_selector(AnySelector::World(sale.world_id))
                .map(|world| world.is_in(&result))
                .unwrap_or_default()
        })
        .map(|sale| SalePoint {
            ts: sale.sold_date,
            price: sale.price_per_item,
            quantity: sale.quantity,
        })
        .collect();
    points.sort_by_key(|p| p.ts);
    Some(Series {
        name: result.get_name().to_string(),
        points,
    })
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
}
