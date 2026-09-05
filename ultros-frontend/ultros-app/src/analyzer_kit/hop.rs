//! Hop gain / unit and Worlds to visit: is the trip to another world worth
//! it? Buy side only — revenue stays the sell world (2026-08-30 decision).

use std::collections::BTreeSet;

use crate::components::crafting_cost::{CostBreakdown, PriceSource};

use super::formula::per_unit_cost;

/// Home cost minus buy-scope cost per unit: signed, never clamped.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HopGain {
    Gain(i32),
    /// The home run has an ingredient with no home listing and no vendor:
    /// the trip is not optional.
    Needed,
    /// The scope run has unpriced lines, or the buy scope IS the home world.
    Unavailable,
}

/// Distinct non-home worlds holding the cheapest listing of a top-level
/// ingredient, in first-appearance order, and the datacenters they span.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WorldsToVisit {
    /// `(world id, ingredient lines priced there)`.
    pub worlds: Vec<(i32, u16)>,
    pub dcs: u8,
}

/// `home` is `compute_cost` over the sell-world listings alone (not layered
/// over the buy scope: an ingredient with no home listing would otherwise
/// be priced at the scope price and zero the gain for exactly the
/// ingredients that force the trip); `scope` is the page's normal
/// buy-scope run under the same cost signal.
pub fn hop_gain(
    home: &CostBreakdown,
    scope: &CostBreakdown,
    amount_result: i32,
    scope_is_home: bool,
) -> HopGain {
    if scope_is_home || scope.unpriced_market_lines > 0 {
        return HopGain::Unavailable;
    }
    if home.unpriced_market_lines > 0 {
        return HopGain::Needed;
    }
    HopGain::Gain(
        per_unit_cost(home.cost, amount_result) - per_unit_cost(scope.cost, amount_result),
    )
}

/// Over the *listing-min* scope run's top-level market lines only: vendor
/// lines and sub-craft lines carry world 0 and are skipped, so sub-craft
/// materials are never counted.
pub fn worlds_to_visit<'a>(
    scope_listing_run: &CostBreakdown,
    home_world: i32,
    dc_of: &dyn Fn(i32) -> Option<&'a str>,
) -> WorldsToVisit {
    let mut worlds: Vec<(i32, u16)> = Vec::new();
    for line in &scope_listing_run.ingredient_lines {
        if line.source != PriceSource::Market
            || line.used_from_market == 0
            || line.world_id == 0
            || line.world_id == home_world
        {
            continue;
        }
        match worlds.iter_mut().find(|(w, _)| *w == line.world_id) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => worlds.push((line.world_id, 1)),
        }
    }
    let dcs: BTreeSet<&str> = worlds.iter().filter_map(|(w, _)| dc_of(*w)).collect();
    WorldsToVisit {
        worlds,
        dcs: dcs.len() as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::crafting_cost::IngredientLine;
    use xiv_gen::ItemId;

    fn breakdown(cost: i32, unpriced: u16, lines: Vec<IngredientLine>) -> CostBreakdown {
        CostBreakdown {
            cost,
            shard_cost: 0,
            on_hand_savings: 0,
            ingredient_lines: lines,
            sub_crafts: vec![],
            unpriced_market_lines: unpriced,
        }
    }

    fn line(item: i32, source: PriceSource, world_id: i32) -> IngredientLine {
        IngredientLine {
            item_id: ItemId(item),
            needed_total: 1,
            used_from_on_hand: 0,
            used_from_market: 1,
            unit_price: if world_id == 0 { 0 } else { 100 },
            is_shard: false,
            source,
            world_id,
        }
    }

    #[test]
    fn hop_gain_is_home_cost_minus_scope_cost_signed() {
        let home = breakdown(13_450, 0, vec![]);
        let scope = breakdown(11_300, 0, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, false), HopGain::Gain(2_150));
        // Negative means stay home; nothing is clamped.
        assert_eq!(hop_gain(&scope, &home, 1, false), HopGain::Gain(-2_150));
        // Per unit of output.
        assert_eq!(
            hop_gain(&home, &scope, 2, false),
            HopGain::Gain(6_725 - 5_650)
        );
    }

    #[test]
    fn hop_is_needed_when_home_has_unpriced_lines() {
        let home = breakdown(100, 1, vec![]);
        let scope = breakdown(300, 0, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, false), HopGain::Needed);
    }

    #[test]
    fn hop_is_unavailable_when_scope_has_unpriced_lines_or_world_scope() {
        let home = breakdown(100, 0, vec![]);
        let scope = breakdown(300, 2, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, false), HopGain::Unavailable);
        let scope = breakdown(300, 0, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, true), HopGain::Unavailable);
        // Unavailable outranks Needed.
        let home = breakdown(100, 1, vec![]);
        assert_eq!(hop_gain(&home, &scope, 1, true), HopGain::Unavailable);
    }

    #[test]
    fn hop_worlds_counts_distinct_non_home_listing_worlds_and_dcs() {
        let run = breakdown(
            0,
            0,
            vec![
                line(1, PriceSource::Market, 5),
                line(2, PriceSource::Market, 7),
                line(3, PriceSource::Market, 5),
                line(4, PriceSource::Vendor, 0),
                line(5, PriceSource::Market, 3), // the home world
                line(6, PriceSource::Subcraft, 0),
                line(7, PriceSource::Market, 0), // unpriced
            ],
        );
        let same_dc = |w: i32| match w {
            5 | 7 | 3 => Some("Aether"),
            _ => None,
        };
        let got = worlds_to_visit(&run, 3, &same_dc);
        assert_eq!(
            got.worlds,
            vec![(5, 2), (7, 1)],
            "first-appearance order, counts per world"
        );
        assert_eq!(got.dcs, 1);
        let two_dcs = |w: i32| match w {
            5 => Some("Aether"),
            7 => Some("Primal"),
            _ => None,
        };
        assert_eq!(worlds_to_visit(&run, 3, &two_dcs).dcs, 2);
        // A line whose world is on hand entirely is not a trip.
        let mut on_hand = line(8, PriceSource::Market, 9);
        on_hand.used_from_market = 0;
        let run = breakdown(0, 0, vec![on_hand]);
        assert_eq!(worlds_to_visit(&run, 3, &two_dcs), WorldsToVisit::default());
    }
}
