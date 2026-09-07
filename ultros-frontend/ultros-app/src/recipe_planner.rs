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

/// Gil-equivalent price of travel. A world hop is one loading screen from the
/// aetheryte; a datacenter hop goes through the main menu and several loading
/// screens. Zero weights reproduce pure gil ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TravelWeights {
    pub world_hop: i64,
    pub dc_hop: i64,
}

impl Default for TravelWeights {
    fn default() -> Self {
        Self {
            world_hop: 2_000,
            dc_hop: 10_000,
        }
    }
}

/// Hops beyond the worlds already on the itinerary. Entering a datacenter
/// lands on one of its worlds, so that first world is part of the DC hop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Travel {
    pub dc_hops: usize,
    pub world_hops: usize,
}

/// Everything route scoring needs besides market data. No UI or network
/// dependencies, and fully value-comparable so it can live in a memo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteContext {
    pub home: i32,
    /// World id to datacenter id. An unmapped world is treated as its own
    /// datacenter so it is charged a DC hop rather than assumed free.
    pub datacenters: BTreeMap<i32, i32>,
    pub weights: TravelWeights,
    /// Item to ticked-off listings, carried as snapshots: a bought listing
    /// leaves the market, and the price paid should not drift afterwards.
    pub locked: BTreeMap<i32, Vec<Offer>>,
    /// `(item, world)` pairs the user reported as not available.
    pub unavailable: BTreeSet<(i32, i32)>,
}

impl RouteContext {
    pub fn dc_of(&self, world: i32) -> i32 {
        self.datacenters.get(&world).copied().unwrap_or(-world)
    }

    /// Home plus every world holding a locked purchase: free to (re)visit.
    pub fn visited(&self) -> BTreeSet<i32> {
        let mut visited = BTreeSet::from([self.home]);
        visited.extend(self.locked.values().flatten().map(|o| o.world));
        visited
    }

    pub fn travel(&self, worlds: &BTreeSet<i32>) -> Travel {
        let visited = self.visited();
        let visited_dcs: BTreeSet<i32> = visited.iter().map(|w| self.dc_of(*w)).collect();
        let new_worlds: Vec<i32> = worlds.difference(&visited).copied().collect();
        let new_dcs: BTreeSet<i32> = new_worlds
            .iter()
            .map(|w| self.dc_of(*w))
            .filter(|dc| !visited_dcs.contains(dc))
            .collect();
        Travel {
            dc_hops: new_dcs.len(),
            world_hops: new_worlds.len() - new_dcs.len(),
        }
    }

    pub fn travel_cost(&self, travel: Travel) -> i64 {
        (travel.dc_hops as i64)
            .saturating_mul(self.weights.dc_hop)
            .saturating_add((travel.world_hops as i64).saturating_mul(self.weights.world_hop))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShoppingPlan {
    pub purchases: BTreeMap<i32, Purchase>,
    /// Every non-home world with a purchase, locked stops included.
    pub worlds: BTreeSet<i32>,
    pub cost: i64,
    pub missing: i64,
    pub approximate: bool,
    pub travel: Travel,
    /// Gil cost plus the gil-equivalent travel cost: the ranking metric.
    pub effective: i64,
}

/// Locked listings are committed purchases: they count against the need at
/// the price they were ticked at, and only the remainder goes to the market.
fn purchase_with_locked(
    needed: i64,
    offers: &[Offer],
    locked: &[Offer],
    vendor: Option<i64>,
) -> Purchase {
    if locked.is_empty() {
        return purchase(needed, offers, vendor);
    }
    let committed: i64 = locked.iter().map(|o| o.quantity).sum();
    let inner = if needed > committed {
        purchase(needed - committed, offers, vendor)
    } else {
        Purchase::default()
    };
    let mut all = locked.to_vec();
    all.extend(inner.offers);
    all.sort_by_key(|o| (o.world, o.id));
    finish(needed, all, vendor.filter(|p| *p > 0), inner.approximate)
}

/// Per-item purchase results keyed by the exact listings the knapsack could
/// see, so route search does not re-solve an item whose visible offers did
/// not change between two world sets.
type PurchaseCache = BTreeMap<(i32, Vec<i32>), Purchase>;

fn shop_cached(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    allowed: &BTreeSet<i32>,
    ctx: &RouteContext,
    cache: &mut PurchaseCache,
) -> ShoppingPlan {
    let visited = ctx.visited();
    let mut plan = ShoppingPlan::default();
    for m in materials
        .iter()
        .filter(|m| m.recipe.is_none() && m.remaining() > 0)
    {
        let locked = ctx.locked.get(&m.item).map(Vec::as_slice).unwrap_or(&[]);
        let locked_ids: BTreeSet<i32> = locked.iter().map(|o| o.id).collect();
        let offers: Vec<_> = market
            .get(&m.item)
            .into_iter()
            .flatten()
            .filter(|o| allowed.contains(&o.world) || visited.contains(&o.world))
            .filter(|o| !ctx.unavailable.contains(&(m.item, o.world)))
            .filter(|o| !locked_ids.contains(&o.id))
            .cloned()
            .collect();
        let key = (m.item, offers.iter().map(|o| o.id).collect::<Vec<_>>());
        let p = cache
            .entry(key)
            .or_insert_with(|| {
                purchase_with_locked(
                    m.remaining(),
                    &offers,
                    locked,
                    vendors.get(&m.item).copied(),
                )
            })
            .clone();
        plan.worlds
            .extend(p.offers.iter().map(|o| o.world).filter(|w| *w != ctx.home));
        plan.cost += p.cost;
        plan.missing += p.missing();
        plan.approximate |= p.approximate;
        plan.purchases.insert(m.item, p);
    }
    plan.travel = ctx.travel(&plan.worlds);
    plan.effective = plan.cost.saturating_add(ctx.travel_cost(plan.travel));
    plan
}

/// Buy every outstanding leaf from `allowed` worlds (plus the worlds already
/// on the itinerary), honouring locked purchases and "not here" reports.
pub fn shop(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    allowed: &BTreeSet<i32>,
    ctx: &RouteContext,
) -> ShoppingPlan {
    shop_cached(
        materials,
        market,
        vendors,
        allowed,
        ctx,
        &mut PurchaseCache::new(),
    )
}

/// Completeness first, then the travel-weighted cost, then plain gil, then
/// the world set itself so ties resolve the same way on every run.
pub fn rank(p: &ShoppingPlan) -> (i64, i64, i64, BTreeSet<i32>) {
    (p.missing, p.effective, p.cost, p.worlds.clone())
}

pub const ROUTE_LIMIT: usize = 5;
const BEAM_WIDTH: usize = 5;
const BEAM_ROUNDS: usize = 4;

/// Best-found routes ranked by `rank`, at most `ROUTE_LIMIT`, pairwise
/// distinct by the worlds actually visited. Single-world additions are
/// exhaustive; larger routes come from a bounded beam plus whole-datacenter
/// and full-scope seeds, so the UI calls these best-found, not optimal. The
/// no-travel plan is always in the pool, so the top route is never worse
/// than staying put.
pub fn compare_routes(
    materials: &[Material],
    market: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    ctx: &RouteContext,
) -> Vec<ShoppingPlan> {
    let visited = ctx.visited();
    let needed: BTreeSet<i32> = materials
        .iter()
        .filter(|m| m.recipe.is_none() && m.remaining() > 0)
        .map(|m| m.item)
        .collect();
    let candidates: BTreeSet<i32> = market
        .iter()
        .filter(|(item, _)| needed.contains(item))
        .flat_map(|(item, offers)| {
            offers
                .iter()
                .filter(move |o| !ctx.unavailable.contains(&(*item, o.world)))
                .map(|o| o.world)
        })
        .filter(|w| !visited.contains(w))
        .collect();
    let mut cache = PurchaseCache::new();
    let mut pool: BTreeMap<BTreeSet<i32>, ShoppingPlan> = BTreeMap::new();
    // Scoped so the closure's borrow of `pool` ends before the collapse below.
    {
        let mut evaluate = |allowed: BTreeSet<i32>| {
            let plan = pool.entry(allowed.clone()).or_insert_with(|| {
                shop_cached(materials, market, vendors, &allowed, ctx, &mut cache)
            });
            rank(plan)
        };
        let mut beam = vec![(evaluate(visited.clone()), visited.clone())];
        for _ in 0..BEAM_ROUNDS {
            let mut next = beam.clone();
            for (_, set) in &beam {
                for world in candidates.difference(set) {
                    let mut allowed = set.clone();
                    allowed.insert(*world);
                    if !next.iter().any(|(_, s)| *s == allowed) {
                        let key = evaluate(allowed.clone());
                        next.push((key, allowed));
                    }
                }
            }
            next.sort();
            next.truncate(BEAM_WIDTH);
            if next == beam {
                break;
            }
            beam = next;
        }
        let mut full = visited.clone();
        full.extend(&candidates);
        evaluate(full);
        let datacenters: BTreeSet<i32> = candidates.iter().map(|w| ctx.dc_of(*w)).collect();
        for dc in datacenters {
            let mut allowed = visited.clone();
            allowed.extend(candidates.iter().filter(|w| ctx.dc_of(**w) == dc));
            evaluate(allowed);
        }
    }
    // Several allowed sets usually collapse onto the same purchase worlds;
    // keep the best plan per route so the cards are distinct.
    let mut best: BTreeMap<BTreeSet<i32>, ShoppingPlan> = BTreeMap::new();
    for plan in pool.into_values() {
        let better = best
            .get(&plan.worlds)
            .is_none_or(|current| rank(&plan) < rank(current));
        if better {
            best.insert(plan.worlds.clone(), plan);
        }
    }
    let mut results: Vec<_> = best.into_values().collect();
    results.sort_by_key(rank);
    results.truncate(ROUTE_LIMIT);
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
    fn ctx(home: i32, datacenters: &[(i32, i32)]) -> RouteContext {
        RouteContext {
            home,
            datacenters: datacenters.iter().copied().collect(),
            ..Default::default()
        }
    }
    fn leaf(item: i32, needed: i64) -> Material {
        Material {
            item,
            needed,
            ..Default::default()
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
            &ctx(1, &[]),
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
        let plans = compare_routes(
            &lines,
            &market,
            &BTreeMap::new(),
            &ctx(1, &[(1, 1), (2, 1)]),
        );
        // Complete plans outrank the cheaper-looking but short home plan.
        assert_eq!((plans[0].cost, plans[0].missing), (40, 0));
        assert_eq!(plans[0].worlds, BTreeSet::from([2]));
        assert_eq!(
            plans[0].travel,
            Travel {
                dc_hops: 0,
                world_hops: 1
            }
        );
        assert_eq!(plans[1].missing, 1);
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
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &ctx(1, &[]));
        // The best route is never worse than the no-travel plan, even when the
        // large-batch greedy path gets worse with more offers to choose from.
        assert_eq!(plans[0].cost, 20_002);
        assert!(plans[0].worlds.is_empty());
        assert!(plans[0].approximate);
    }

    #[test]
    fn travel_counts_a_new_datacenter_as_one_hop_plus_extra_worlds() {
        let c = ctx(1, &[(1, 10), (2, 10), (3, 20), (4, 20)]);
        let travel = |dc_hops, world_hops| Travel {
            dc_hops,
            world_hops,
        };
        assert_eq!(c.travel(&BTreeSet::new()), Travel::default());
        assert_eq!(c.travel(&BTreeSet::from([2])), travel(0, 1));
        assert_eq!(c.travel(&BTreeSet::from([3, 4])), travel(1, 1));
        assert_eq!(c.travel(&BTreeSet::from([2, 3, 4])), travel(1, 2));
        assert_eq!(c.travel_cost(travel(1, 2)), 10_000 + 2 * 2_000);
        // A locked purchase on world 3 means that datacenter is already on the
        // itinerary: its sibling is a plain world hop.
        let mut locked = c.clone();
        locked.locked.insert(9, vec![offer(1, 3, 1, 1)]);
        assert_eq!(locked.travel(&BTreeSet::from([3, 4])), travel(0, 1));
    }

    #[test]
    fn unmapped_world_is_charged_as_its_own_datacenter() {
        let c = ctx(1, &[(1, 10)]);
        assert_eq!(
            c.travel(&BTreeSet::from([7])),
            Travel {
                dc_hops: 1,
                world_hops: 0
            }
        );
    }

    #[test]
    fn datacenter_weight_can_outrank_a_cheaper_foreign_listing() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(1, vec![offer(1, 2, 1, 1_000), offer(2, 3, 1, 500)])]);
        let mut c = ctx(1, &[(1, 10), (2, 10), (3, 20)]);
        c.weights = TravelWeights {
            world_hop: 100,
            dc_hop: 5_000,
        };
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &c);
        assert_eq!(plans[0].worlds, BTreeSet::from([2]));
        assert_eq!((plans[0].cost, plans[0].effective), (1_000, 1_100));
        c.weights = TravelWeights {
            world_hop: 0,
            dc_hop: 0,
        };
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &c);
        assert_eq!(plans[0].worlds, BTreeSet::from([3]));
    }

    #[test]
    fn locked_offer_is_kept_counted_and_makes_its_world_free() {
        let materials = [leaf(1, 2), leaf(2, 1)];
        // Item 1's locked listing is gone from the market: it was bought.
        let market = BTreeMap::from([
            (1, vec![offer(11, 1, 1, 100)]),
            (2, vec![offer(21, 1, 1, 100), offer(22, 3, 1, 95)]),
        ]);
        let mut c = ctx(1, &[(1, 10), (3, 20)]);
        c.locked.insert(1, vec![offer(10, 3, 1, 50)]);
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &c);
        let best = &plans[0];
        let first = &best.purchases[&1];
        assert_eq!((first.quantity, first.cost, first.missing()), (2, 150, 0));
        assert!(first.offers.iter().any(|o| o.id == 10));
        // World 3 is already on the itinerary, so the 5 gil saving is free.
        assert_eq!(best.purchases[&2].offers[0].id, 22);
        assert_eq!(best.worlds, BTreeSet::from([3]));
        assert_eq!(best.travel, Travel::default());
        assert_eq!(best.effective, best.cost);
    }

    #[test]
    fn locked_quantity_covering_the_need_buys_nothing_more() {
        let materials = [leaf(1, 2)];
        let market = BTreeMap::from([(1, vec![offer(11, 1, 5, 1)])]);
        let mut c = ctx(1, &[(1, 10)]);
        c.locked.insert(1, vec![offer(10, 1, 3, 7)]);
        let plan = shop(
            &materials,
            &market,
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        let p = &plan.purchases[&1];
        assert_eq!(
            (p.quantity, p.cost, p.missing(), p.offers.len()),
            (3, 21, 0, 1)
        );
    }

    #[test]
    fn locked_listing_still_on_the_market_is_not_bought_twice() {
        let materials = [leaf(1, 4)];
        let market = BTreeMap::from([(1, vec![offer(10, 1, 2, 5), offer(11, 1, 2, 9)])]);
        let mut c = ctx(1, &[(1, 10)]);
        c.locked.insert(1, vec![offer(10, 1, 2, 5)]);
        let plan = shop(
            &materials,
            &market,
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        let ids: Vec<_> = plan.purchases[&1].offers.iter().map(|o| o.id).collect();
        assert_eq!(ids, vec![10, 11]);
        assert_eq!((plan.cost, plan.missing), (28, 0));
    }

    #[test]
    fn locked_stock_reduces_the_vendor_top_up() {
        let materials = [leaf(1, 5)];
        let mut c = ctx(1, &[(1, 10)]);
        c.locked.insert(1, vec![offer(10, 1, 2, 3)]);
        let plan = shop(
            &materials,
            &BTreeMap::new(),
            &BTreeMap::from([(1, 10)]),
            &BTreeSet::from([1]),
            &c,
        );
        let p = &plan.purchases[&1];
        assert_eq!(
            (p.vendor_quantity, p.quantity, p.cost, p.missing()),
            (3, 5, 36, 0)
        );
    }

    #[test]
    fn unavailable_pair_is_skipped_without_blocking_the_world() {
        let materials = [leaf(1, 1), leaf(2, 1)];
        let market = BTreeMap::from([
            (1, vec![offer(11, 2, 1, 10), offer(12, 1, 1, 100)]),
            (2, vec![offer(21, 2, 1, 10)]),
        ]);
        let mut c = ctx(1, &[(1, 10), (2, 10)]);
        c.unavailable.insert((1, 2));
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &c);
        assert!(
            plans
                .iter()
                .flat_map(|p| p.purchases.get(&1))
                .flat_map(|p| &p.offers)
                .all(|o| o.world != 2)
        );
        assert_eq!(plans[0].purchases[&1].offers[0].id, 12);
        assert_eq!(plans[0].purchases[&2].offers[0].id, 21);
        assert_eq!(plans[0].missing, 0);
    }

    #[test]
    fn unavailable_does_not_drop_a_locked_offer() {
        let materials = [leaf(1, 1)];
        let mut c = ctx(1, &[(1, 10), (2, 10)]);
        c.locked.insert(1, vec![offer(10, 2, 1, 5)]);
        c.unavailable.insert((1, 2));
        let plan = shop(
            &materials,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        assert_eq!((plan.cost, plan.missing), (5, 0));
    }

    #[test]
    fn routes_are_distinct_ranked_and_capped() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(
            1,
            (1..=8)
                .map(|w| offer(w, w, 1, 100 - i64::from(w)))
                .collect::<Vec<_>>(),
        )]);
        let c = ctx(1, &(1..=8).map(|w| (w, 10)).collect::<Vec<_>>());
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &c);
        assert_eq!(plans.len(), ROUTE_LIMIT);
        let sets: BTreeSet<_> = plans.iter().map(|p| p.worlds.clone()).collect();
        assert_eq!(sets.len(), plans.len());
        assert!(plans.windows(2).all(|w| rank(&w[0]) <= rank(&w[1])));
        let home = shop(
            &materials,
            &market,
            &BTreeMap::new(),
            &BTreeSet::from([1]),
            &c,
        );
        assert!(plans[0].effective <= home.effective);
        // A few gil never pays for a 2,000 gil hop.
        assert!(plans[0].worlds.is_empty());
    }

    #[test]
    fn a_single_world_scope_yields_one_route() {
        let materials = [leaf(1, 1)];
        let market = BTreeMap::from([(1, vec![offer(1, 1, 1, 10)])]);
        let plans = compare_routes(&materials, &market, &BTreeMap::new(), &ctx(1, &[(1, 10)]));
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].cost, 10);
    }

    #[test]
    fn ranking_is_deterministic_under_ties_and_input_order() {
        let materials = [leaf(1, 1)];
        let forward = vec![offer(1, 3, 1, 10), offer(2, 2, 1, 10), offer(3, 1, 1, 50)];
        let mut backward = forward.clone();
        backward.reverse();
        let mut c = ctx(1, &[(1, 10), (2, 10), (3, 10)]);
        c.weights = TravelWeights {
            world_hop: 0,
            dc_hop: 0,
        };
        let a = compare_routes(
            &materials,
            &BTreeMap::from([(1, forward)]),
            &BTreeMap::new(),
            &c,
        );
        let b = compare_routes(
            &materials,
            &BTreeMap::from([(1, backward)]),
            &BTreeMap::new(),
            &c,
        );
        assert_eq!(a, b);
        // Equal gil and travel: the lower world set wins.
        assert_eq!(a[0].worlds, BTreeSet::from([2]));
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
            &ctx(1, &[]),
        );
        assert_eq!((plan.cost, plan.missing), (0, 0));
    }
}
