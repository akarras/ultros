//! Batch and whole-stack planning, independent of the analyzer's unit estimates.
//! All quantities/costs are integers. Missing supply is never treated as free.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub id: i32,
    pub output: i32,
    pub yield_amount: i64,
    pub ingredients: Vec<(i32, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Material {
    pub item: i32,
    pub needed: i64,
    pub owned: i64,
    pub crafts: i64,
    pub surplus: i64,
    pub depth: usize,
    pub recipe: Option<i32>,
}

impl Material {
    pub fn remaining(&self) -> i64 {
        self.needed - self.owned
    }
}

/// Parent-first order means every demand for a shared intermediate is known
/// before rounding its batch count. Reverse this order for crafting instructions.
pub fn expand(
    root: &Recipe,
    quantity: i64,
    recipes: &BTreeMap<i32, Recipe>,
    choices: &BTreeMap<i32, i32>,
    owned: &BTreeMap<i32, i64>,
    excluded: &BTreeSet<i32>,
) -> Result<Vec<Material>, String> {
    struct Walk<'a> {
        recipes: &'a BTreeMap<i32, Recipe>,
        selected: BTreeMap<i32, i32>,
        excluded: &'a BTreeSet<i32>,
        active: BTreeSet<i32>,
        seen: BTreeSet<i32>,
        order: Vec<(i32, usize)>,
    }
    impl Walk<'_> {
        fn visit(&mut self, item: i32, depth: usize) -> Result<(), String> {
            if self.active.contains(&item) {
                return Err(
                    "These craft choices form a cycle. Choose Buy for one of the ingredients."
                        .into(),
                );
            }
            if self.seen.contains(&item) {
                return Ok(());
            }
            if depth > 12 || self.seen.len() >= 128 {
                return Err(
                    "This crafting plan is too large. Buy some intermediates to simplify it."
                        .into(),
                );
            }
            self.active.insert(item);
            if let Some(id) = self.selected.get(&item) {
                let recipe = self.recipes.get(id).ok_or("Recipe unavailable")?;
                if recipe.output != item || recipe.yield_amount <= 0 {
                    return Err("Invalid recipe choice".into());
                }
                for (child, amount) in &recipe.ingredients {
                    if *amount > 0 && !self.excluded.contains(child) {
                        self.visit(*child, depth + 1)?;
                    }
                }
            }
            self.active.remove(&item);
            self.seen.insert(item);
            self.order.push((item, depth));
            Ok(())
        }
    }
    let mut walk = Walk {
        recipes,
        selected: choices.clone(),
        excluded,
        active: BTreeSet::new(),
        seen: BTreeSet::new(),
        order: Vec::new(),
    };
    walk.selected.insert(root.output, root.id);
    walk.visit(root.output, 0)?;
    let mut demands = BTreeMap::from([(root.output, quantity.clamp(1, 9999))]);
    let mut result = Vec::new();
    for (item, depth) in walk.order.into_iter().rev() {
        let needed = demands.get(&item).copied().unwrap_or(0);
        if needed == 0 {
            continue;
        }
        let owned = if item == root.output {
            0
        } else {
            owned.get(&item).copied().unwrap_or(0).clamp(0, needed)
        };
        let mut line = Material {
            item,
            needed,
            owned,
            depth,
            ..Default::default()
        };
        if let Some(id) = walk.selected.get(&item) {
            let recipe = &recipes[id];
            let crafts = (needed - owned + recipe.yield_amount - 1) / recipe.yield_amount;
            line.recipe = Some(*id);
            line.crafts = crafts;
            line.surplus = crafts * recipe.yield_amount - (needed - owned);
            for (child, amount) in &recipe.ingredients {
                if *amount <= 0 || excluded.contains(child) {
                    continue;
                }
                let added = crafts.checked_mul(*amount).ok_or("Quantity too large")?;
                let demand = demands.entry(*child).or_insert(0_i64);
                *demand = demand
                    .checked_add(added)
                    .filter(|n| *n <= 1_000_000_000)
                    .ok_or("Quantity too large")?;
            }
        }
        result.push(line);
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub id: i32,
    pub world: i32,
    pub quantity: i64,
    pub price: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Purchase {
    pub needed: i64,
    pub quantity: i64,
    pub cost: i64,
    pub vendor_quantity: i64,
    pub offers: Vec<Offer>,
    pub approximate: bool,
}

impl Purchase {
    pub fn missing(&self) -> i64 {
        (self.needed - self.quantity).max(0)
    }
}

fn finish(needed: i64, offers: Vec<Offer>, vendor: Option<i64>, approximate: bool) -> Purchase {
    let quantity = offers.iter().map(|o| o.quantity).sum::<i64>();
    let vendor_quantity = if vendor.is_some() {
        (needed - quantity).max(0)
    } else {
        0
    };
    Purchase {
        needed,
        quantity: quantity + vendor_quantity,
        cost: offers.iter().map(|o| o.quantity * o.price).sum::<i64>()
            + vendor_quantity * vendor.unwrap_or(0),
        offers,
        vendor_quantity,
        approximate,
    }
}

/// Bounded 0/1 knapsack: listings cannot be split or reused. Vendor supply can
/// fill any remaining quantity. Large batches use two greedy candidates and
/// explicitly mark the result as a best-found estimate, never an exact optimum.
pub fn purchase(needed: i64, offers: &[Offer], vendor: Option<i64>) -> Purchase {
    let vendor = vendor.filter(|p| *p > 0);
    let mut seen = BTreeSet::new();
    let offers: Vec<_> = offers
        .iter()
        .filter(|o| o.price > 0 && o.quantity > 0 && seen.insert(o.id))
        .cloned()
        .collect();
    if needed <= 0 {
        return Purchase::default();
    }
    let supply: i64 = offers.iter().map(|o| o.quantity).sum();
    if supply < needed && vendor.is_none() {
        return finish(needed, offers, None, false);
    }
    if needed > 10_000 || needed as usize * offers.len() > 200_000 {
        let mut candidates = Vec::new();
        for by_stack in [false, true] {
            let mut sorted = offers.clone();
            sorted.sort_by_key(|o| {
                (
                    if by_stack {
                        o.price * o.quantity
                    } else {
                        o.price
                    },
                    o.id,
                )
            });
            let mut selected = Vec::new();
            let mut remaining = needed;
            for offer in sorted {
                if remaining <= 0 {
                    break;
                }
                if vendor.is_some_and(|p| {
                    offer.quantity * offer.price >= p * remaining.min(offer.quantity)
                }) {
                    continue;
                }
                remaining -= offer.quantity;
                selected.push(offer);
            }
            candidates.push(finish(needed, selected, vendor, true));
        }
        return candidates
            .into_iter()
            .min_by_key(|p| (p.missing(), p.cost))
            .unwrap();
    }
    let target = needed as usize;
    // Arena-backed paths keep predecessors immutable during descending updates.
    let mut paths: Vec<(usize, Option<usize>)> = Vec::new();
    let mut states = vec![(i64::MAX, None); target + 1];
    states[0] = (0, None);
    for (index, offer) in offers.iter().enumerate() {
        for amount in (0..target).rev() {
            let (cost, path) = states[amount];
            if cost == i64::MAX {
                continue;
            }
            let next = (amount as i64 + offer.quantity).min(needed) as usize;
            let cost = cost + offer.price * offer.quantity;
            if cost < states[next].0 {
                paths.push((index, path));
                states[next] = (cost, Some(paths.len() - 1));
            }
        }
    }
    let best = (0..=target)
        .filter(|n| states[*n].0 != i64::MAX && (*n == target || vendor.is_some()))
        .min_by_key(|n| states[*n].0 + (needed - *n as i64) * vendor.unwrap_or(0))
        .unwrap_or(0);
    let mut path = states[best].1;
    let mut selected = Vec::new();
    while let Some(p) = path {
        let (index, prev) = paths[p];
        selected.push(offers[index].clone());
        path = prev;
    }
    selected.sort_by_key(|o| (o.world, o.id));
    finish(needed, selected, vendor, false)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShoppingPlan {
    pub purchases: BTreeMap<i32, Purchase>,
    pub worlds: BTreeSet<i32>,
    pub cost: i64,
    pub missing: i64,
    pub approximate: bool,
}

pub fn shop(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    allowed: &BTreeSet<i32>,
    home: i32,
) -> ShoppingPlan {
    let mut plan = ShoppingPlan::default();
    for m in materials
        .iter()
        .filter(|m| m.recipe.is_none() && m.remaining() > 0)
    {
        let offers: Vec<_> = market
            .get(&m.item)
            .into_iter()
            .flatten()
            .filter(|o| allowed.contains(&o.world))
            .cloned()
            .collect();
        let p = purchase(m.remaining(), &offers, vendors.get(&m.item).copied());
        plan.worlds
            .extend(p.offers.iter().map(|o| o.world).filter(|w| *w != home));
        plan.cost += p.cost;
        plan.missing += p.missing();
        plan.approximate |= p.approximate;
        plan.purchases.insert(m.item, p);
    }
    plan
}

/// Home / up to 1 / up to 2 / up to 3 additional worlds / full scope.
/// One-world candidates are exhaustive. Larger routes retain a bounded beam;
/// the UI calls these best-found plans rather than promising global optimality.
pub fn compare_routes(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    home: i32,
) -> Vec<ShoppingPlan> {
    let candidates: BTreeSet<i32> = market
        .values()
        .flatten()
        .map(|o| o.world)
        .filter(|w| *w != home)
        .collect();
    let rank = |p: &ShoppingPlan| (p.missing, p.cost, p.worlds.len());
    let home_set = BTreeSet::from([home]);
    let baseline = shop(materials, market, vendors, &home_set, home);
    let mut results = vec![baseline.clone()];
    let mut beam = vec![(home_set, baseline)];
    for _ in 0..3 {
        let mut next = beam.clone();
        let mut visited = BTreeSet::new();
        for (set, _) in &beam {
            for world in &candidates {
                let mut allowed = set.clone();
                allowed.insert(*world);
                if visited.insert(allowed.clone()) {
                    let plan = shop(materials, market, vendors, &allowed, home);
                    next.push((allowed, plan));
                }
            }
        }
        next.sort_by_key(|(set, p)| (rank(p), set.clone()));
        next.dedup_by(|a, b| a.0 == b.0);
        next.truncate(4);
        results.push(next[0].1.clone());
        beam = next;
    }
    let mut all = candidates;
    all.insert(home);
    let unrestricted = shop(materials, market, vendors, &all, home);
    // A wider allowance can always reuse a cheaper narrow plan, even when the
    // large-batch stack heuristic chooses a worse combination from more offers.
    let best = results
        .iter()
        .chain(std::iter::once(&unrestricted))
        .min_by_key(|p| rank(p))
        .unwrap()
        .clone();
    results.push(best);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recipe(id: i32, output: i32, yield_amount: i64, ingredients: &[(i32, i64)]) -> Recipe {
        Recipe {
            id,
            output,
            yield_amount,
            ingredients: ingredients.to_vec(),
        }
    }
    fn offer(id: i32, world: i32, quantity: i64, price: i64) -> Offer {
        Offer {
            id,
            world,
            quantity,
            price,
        }
    }
    #[test]
    fn shared_intermediates_round_once_and_owned_is_consumed_once() {
        let root = recipe(1, 10, 1, &[(20, 1), (30, 1)]);
        let recipes = BTreeMap::from([
            (1, root.clone()),
            (2, recipe(2, 20, 1, &[(40, 2)])),
            (3, recipe(3, 30, 1, &[(40, 2)])),
            (4, recipe(4, 40, 3, &[(50, 9)])),
        ]);
        let choices = BTreeMap::from([(20, 2), (30, 3), (40, 4)]);
        let lines = expand(
            &root,
            1,
            &recipes,
            &choices,
            &BTreeMap::from([(40, 1)]),
            &BTreeSet::new(),
        )
        .unwrap();
        let shared = lines.iter().find(|l| l.item == 40).unwrap();
        assert_eq!(
            (shared.needed, shared.owned, shared.crafts, shared.surplus),
            (4, 1, 1, 0)
        );
        assert_eq!(lines.iter().find(|l| l.item == 50).unwrap().needed, 9);
    }
    #[test]
    fn batch_cost_does_not_amortize_surplus_and_cycles_fail() {
        let root = recipe(1, 10, 1, &[(20, 2)]);
        let mut recipes = BTreeMap::from([(1, root.clone()), (2, recipe(2, 20, 3, &[(30, 9)]))]);
        let choices = BTreeMap::from([(20, 2)]);
        let lines = expand(
            &root,
            1,
            &recipes,
            &choices,
            &BTreeMap::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(lines.iter().find(|l| l.item == 20).unwrap().surplus, 1);
        let plan = shop(
            &lines,
            &BTreeMap::from([(30, vec![offer(1, 1, 9, 10)])]),
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            1,
        );
        assert_eq!(plan.cost, 90);
        recipes.get_mut(&2).unwrap().ingredients = vec![(10, 1)];
        assert!(
            expand(
                &root,
                1,
                &recipes,
                &choices,
                &BTreeMap::new(),
                &BTreeSet::new()
            )
            .is_err()
        );
    }
    #[test]
    fn whole_stacks_choose_lower_spend_not_lower_unit_price() {
        let p = purchase(2, &[offer(1, 1, 99, 1), offer(2, 1, 2, 10)], None);
        assert_eq!((p.quantity, p.cost), (2, 20));
        assert_eq!(p.offers[0].id, 2);
        let p = purchase(5, &[offer(1, 1, 3, 2)], Some(10));
        assert_eq!((p.vendor_quantity, p.cost), (2, 26));
    }
    #[test]
    fn stacks_are_not_reused_and_missing_supply_is_explicit() {
        let p = purchase(5, &[offer(1, 1, 3, 2), offer(1, 1, 3, 2)], None);
        assert_eq!((p.quantity, p.missing(), p.cost), (3, 2, 6));
    }
    #[test]
    fn itinerary_counts_nested_materials_and_prefers_complete_plans() {
        let lines = vec![
            Material {
                item: 1,
                needed: 2,
                ..Default::default()
            },
            Material {
                item: 2,
                needed: 1,
                ..Default::default()
            },
        ];
        let market = BTreeMap::from([
            (1, vec![offer(1, 1, 2, 50), offer(2, 2, 2, 10)]),
            (2, vec![offer(3, 2, 1, 20)]),
        ]);
        let plans = compare_routes(&lines, &market, &BTreeMap::new(), 1);
        assert_eq!(plans[0].missing, 1);
        assert_eq!((plans[1].cost, plans[1].missing), (40, 0));
        assert_eq!(plans[1].worlds, BTreeSet::from([2]));
    }
    #[test]
    fn exact_stack_solver_matches_exhaustive_subsets() {
        let offers = vec![
            offer(1, 1, 3, 7),
            offer(2, 2, 5, 4),
            offer(3, 1, 2, 8),
            offer(4, 2, 8, 3),
        ];
        for need in 1..=20 {
            for vendor in [None, Some(6)] {
                let expected = (0..1 << offers.len())
                    .map(|mask| {
                        finish(
                            need,
                            offers
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| mask & (1 << i) != 0)
                                .map(|(_, o)| o.clone())
                                .collect(),
                            vendor,
                            false,
                        )
                    })
                    .min_by_key(|p| (p.missing(), p.cost))
                    .unwrap();
                let actual = purchase(need, &offers, vendor);
                assert_eq!(
                    (actual.missing(), actual.cost),
                    (expected.missing(), expected.cost)
                );
            }
        }
    }

    #[test]
    fn stack_solver_matches_varied_exhaustive_markets() {
        // A fixed seed keeps failures reproducible. The oracle enumerates all
        // subsets independently, including partial supply and vendor top-ups.
        let mut seed = 358_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed >> 32
        };
        for case in 0..256 {
            let offers: Vec<_> = (0..8)
                .map(|id| {
                    offer(
                        id,
                        1 + (id % 3),
                        1 + (next() % 12) as i64,
                        1 + (next() % 30) as i64,
                    )
                })
                .collect();
            let needed = 1 + (next() % 100) as i64;
            for vendor in [None, Some(1 + (next() % 30) as i64)] {
                let expected = (0..1_u32 << offers.len())
                    .map(|mask| {
                        let mut quantity = 0;
                        let mut cost = 0;
                        for (i, o) in offers.iter().enumerate() {
                            if mask & (1 << i) != 0 {
                                quantity += o.quantity;
                                cost += o.quantity * o.price;
                            }
                        }
                        let missing = (needed - quantity).max(0);
                        match vendor {
                            Some(price) => (0, cost + missing * price),
                            None => (missing, cost),
                        }
                    })
                    .min()
                    .unwrap();
                let actual = purchase(needed, &offers, vendor);
                assert_eq!((actual.missing(), actual.cost), expected, "market {case}");
                assert_eq!(
                    actual
                        .offers
                        .iter()
                        .map(|o| o.id)
                        .collect::<BTreeSet<_>>()
                        .len(),
                    actual.offers.len()
                );
            }
        }
    }

    #[test]
    fn full_scope_can_reuse_a_better_home_plan_for_large_stacks() {
        let materials = [Material {
            item: 1,
            needed: 10_001,
            ..Default::default()
        }];
        let market = BTreeMap::from([(1, vec![offer(1, 1, 10_001, 2), offer(2, 2, 10_000, 1)])]);
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), 1);
        assert_eq!(plans[4].cost, 20_002);
        assert!(plans[4].worlds.is_empty());
        assert!(plans[4].approximate);
    }

    #[test]
    fn desired_output_rounds_up_and_nested_crystals_can_be_excluded() {
        let root = recipe(1, 10, 3, &[(20, 2), (59, 1)]);
        let recipes = BTreeMap::from([
            (1, root.clone()),
            (2, recipe(2, 20, 1, &[(30, 2), (59, 3)])),
        ]);
        let lines = expand(
            &root,
            4,
            &recipes,
            &BTreeMap::from([(20, 2)]),
            &BTreeMap::new(),
            &BTreeSet::from([59]),
        )
        .unwrap();
        assert_eq!((lines[0].crafts, lines[0].surplus), (2, 2));
        assert_eq!(lines.iter().find(|m| m.item == 30).unwrap().needed, 8);
        assert!(!lines.iter().any(|m| m.item == 59));
    }

    #[test]
    fn owned_intermediate_requires_no_child_purchases() {
        let root = recipe(1, 10, 1, &[(20, 2)]);
        let recipes = BTreeMap::from([(1, root.clone()), (2, recipe(2, 20, 3, &[(30, 9)]))]);
        let lines = expand(
            &root,
            1,
            &recipes,
            &BTreeMap::from([(20, 2)]),
            &BTreeMap::from([(20, 2)]),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(lines.iter().find(|m| m.item == 20).unwrap().crafts, 0);
        assert!(!lines.iter().any(|m| m.item == 30));
        let plan = shop(
            &lines,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            1,
        );
        assert_eq!((plan.cost, plan.missing), (0, 0));
    }
}
