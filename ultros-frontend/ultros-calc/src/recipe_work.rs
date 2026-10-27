//! Owned, serializable recipe-planning work. Both SSR and the browser worker
//! call this same engine, including pinned routes and output comparisons.

use crate::recipe_planner::{
    self as planner, Material, Offer, Purchase, RouteContext, ShoppingPlan,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishedInput {
    pub item: i32,
    pub quantity: i64,
    pub offers: Vec<Offer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub materials: Vec<Material>,
    pub offers: BTreeMap<i32, Vec<Offer>>,
    pub vendors: BTreeMap<i32, i64>,
    pub context: RouteContext,
    pub route: Option<BTreeSet<i32>>,
    pub visits: Option<usize>,
    pub finished: Option<FinishedInput>,
    /// Present for buying the finished item; absent when the selected plan
    /// already represents crafting materials.
    pub craft_materials: Option<Vec<Material>>,
    pub buy_output: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cards {
    pub plans: Vec<ShoppingPlan>,
    pub pinned: Option<usize>,
    pub best_value: Option<usize>,
    pub cheapest: Option<usize>,
    pub basis: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub cards: Cards,
    pub selected_index: Option<usize>,
    pub finished: Option<Purchase>,
    pub craft_comparison: Option<ShoppingPlan>,
}

pub fn legacy_visits(plans: &[ShoppingPlan], visits: usize) -> Option<usize> {
    plans
        .iter()
        .enumerate()
        .filter(|(_, p)| visits >= 4 || p.worlds.len() <= visits)
        .min_by_key(|(_, p)| planner::rank(p))
        .map(|(i, _)| i)
}

pub fn cheapest_badge(plans: &[ShoppingPlan], candidate: Option<usize>) -> Option<usize> {
    let i = candidate?;
    let cost = plans.get(i)?.cost;
    plans
        .iter()
        .enumerate()
        .all(|(j, p)| j == i || cost <= p.cost)
        .then_some(i)
}

/// Reuse the expensive frontier when only selection or its comparisons change.
/// Each worker retains exactly one input and frontier.
#[derive(Default)]
pub struct Planner {
    previous: Option<(Request, planner::RouteComparison)>,
    #[cfg(test)]
    comparison_runs: usize,
}

impl Planner {
    pub fn compute(&mut self, request: Request) -> Output {
        let comparison = match &self.previous {
            Some((previous, comparison))
                if previous.materials == request.materials
                    && previous.offers == request.offers
                    && previous.vendors == request.vendors
                    && previous.context == request.context =>
            {
                comparison.clone()
            }
            _ => {
                #[cfg(test)]
                {
                    self.comparison_runs += 1;
                }
                let comparison = compare(&request);
                self.previous = Some((request.clone(), comparison.clone()));
                comparison
            }
        };
        assemble(request, comparison)
    }
}

fn compare(request: &Request) -> planner::RouteComparison {
    planner::compare_routes(
        &request.materials,
        &request.offers,
        &request.vendors,
        &request.context,
    )
}

pub fn compute(request: Request) -> Output {
    let comparison = compare(&request);
    assemble(request, comparison)
}

/// Selection is cheap and can update immediately while a new worker job runs.
pub fn selected_index(
    cards: &Cards,
    route: Option<&BTreeSet<i32>>,
    visits: Option<usize>,
) -> Option<usize> {
    if cards.plans.is_empty() {
        return None;
    }
    let default = cards.best_value.or(Some(0));
    if let Some(route) = route {
        cards
            .plans
            .iter()
            .position(|p| p.worlds == *route)
            .or(cards.pinned)
            .or(default)
    } else {
        visits
            .and_then(|v| legacy_visits(&cards.plans, v))
            .or(default)
    }
}

fn assemble(request: Request, comparison: planner::RouteComparison) -> Output {
    let Request {
        materials,
        offers,
        vendors,
        context,
        route,
        visits,
        finished,
        craft_materials,
        buy_output,
    } = request;
    let best_worlds = comparison
        .best_value()
        .map(|i| comparison.cards[i].worlds.clone());
    let cheapest_worlds = comparison.cheapest().map(|p| p.worlds.clone());
    let mut plans = comparison.cards;
    let mut pinned = None;
    if let Some(mut allowed) = route
        .clone()
        .filter(|set| !plans.is_empty() && !plans.iter().any(|p| p.worlds == *set))
    {
        allowed.insert(context.home);
        let plan = planner::shop(&materials, &offers, &vendors, &allowed, &context);
        let key = (context.weights.distance(plan.travel), plan.travel);
        let at = plans.partition_point(|p| (context.weights.distance(p.travel), p.travel) <= key);
        plans.insert(at, plan);
        pinned = Some(at);
    }
    let find = |worlds: Option<BTreeSet<i32>>| {
        worlds.and_then(|w| plans.iter().position(|p| p.worlds == w))
    };
    let (best_value, cheapest) = if plans.len() > 1 {
        (
            find(best_worlds),
            cheapest_badge(&plans, find(cheapest_worlds)),
        )
    } else {
        (None, None)
    };
    let basis = planner::savings_basis(&plans);
    let cards = Cards {
        plans,
        pinned,
        best_value,
        cheapest,
        basis,
    };
    let selected_index = selected_index(&cards, route.as_ref(), visits);
    let selected = selected_index.and_then(|i| cards.plans.get(i)).cloned();
    let mut allowed = selected
        .as_ref()
        .map(|p| p.worlds.clone())
        .unwrap_or_default();
    allowed.insert(context.home);
    // Completed ingredient purchases do not count as finished-item purchases,
    // nor as already-owned craft materials in the alternative comparison.
    let unlocked = RouteContext {
        locked: BTreeMap::new(),
        ..context
    };
    let finished = finished.and_then(|input| {
        planner::shop(
            &[Material {
                item: input.item,
                needed: input.quantity.clamp(1, 9999),
                ..Default::default()
            }],
            &BTreeMap::from([(input.item, input.offers)]),
            &vendors,
            &allowed,
            &unlocked,
        )
        .purchases
        .remove(&input.item)
    });
    let craft_comparison = if buy_output {
        craft_materials.and_then(|demand| {
            selected
                .as_ref()
                .map(|_| planner::shop(&demand, &offers, &vendors, &allowed, &unlocked))
        })
    } else {
        selected
    };
    Output {
        cards,
        selected_index,
        finished,
        craft_comparison,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_planner::TravelWeights;

    #[test]
    fn wire_round_trip_preserves_locked_inventory_vendor_and_pinned_route_rules() {
        let locked = Offer {
            id: 9_007_199_254_740_993,
            world: 1,
            quantity: 1,
            price: 100,
        };
        let request = Request {
            materials: vec![Material {
                item: 10,
                needed: 5,
                owned: 1,
                ..Default::default()
            }],
            offers: BTreeMap::from([(
                10,
                vec![
                    Offer {
                        id: 1,
                        world: 1,
                        quantity: 4,
                        price: 100,
                    },
                    Offer {
                        id: 2,
                        world: 2,
                        quantity: 4,
                        price: 10,
                    },
                ],
            )]),
            vendors: BTreeMap::from([(10, 30)]),
            context: RouteContext {
                home: 1,
                datacenters: BTreeMap::from([(1, 1), (2, 1)]),
                weights: TravelWeights {
                    world_hop: 5,
                    dc_hop: 20,
                },
                locked: BTreeMap::from([(10, vec![locked.clone()])]),
                unavailable: BTreeSet::from([(10, 2)]),
            },
            route: Some(BTreeSet::from([2])),
            visits: Some(0),
            finished: Some(FinishedInput {
                item: 20,
                quantity: 2,
                offers: vec![Offer {
                    id: 20,
                    world: 1,
                    quantity: 2,
                    price: 70,
                }],
            }),
            buy_output: true,
            craft_materials: Some(vec![Material {
                item: 10,
                needed: 7,
                owned: 2,
                ..Default::default()
            }]),
        };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: Request = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, request);
        let output = compute(decoded);
        assert_eq!(output.selected_index, output.cards.pinned);
        let selected = &output.cards.plans[output.selected_index.unwrap()];
        let purchase = &selected.purchases[&10];
        assert_eq!(purchase.needed, 4);
        assert_eq!(purchase.offers, vec![locked]);
        assert_eq!(purchase.vendor_quantity, 3);
        assert_eq!(purchase.cost, 190);
        assert_eq!(output.finished.as_ref().unwrap().cost, 140);
        assert_eq!(output.craft_comparison.as_ref().unwrap().cost, 150);
        let encoded = serde_json::to_string(&output).unwrap();
        assert_eq!(serde_json::from_str::<Output>(&encoded).unwrap(), output);
    }

    #[test]
    fn ordinary_frontier_and_default_selection_match_the_original_engine() {
        let materials = vec![Material {
            item: 1,
            needed: 2,
            ..Default::default()
        }];
        let offers = BTreeMap::from([(
            1,
            vec![
                Offer {
                    id: 1,
                    world: 1,
                    quantity: 2,
                    price: 100,
                },
                Offer {
                    id: 2,
                    world: 2,
                    quantity: 2,
                    price: 10,
                },
            ],
        )]);
        let context = RouteContext {
            home: 1,
            ..Default::default()
        };
        let original = planner::compare_routes(&materials, &offers, &BTreeMap::new(), &context);
        let result = compute(Request {
            materials,
            offers,
            context,
            vendors: BTreeMap::new(),
            route: None,
            visits: None,
            finished: None,
            craft_materials: None,
            buy_output: false,
        });
        assert_eq!(result.cards.plans, original.cards);
        assert_eq!(result.selected_index, result.cards.best_value.or(Some(0)));
        assert_eq!(
            result.craft_comparison,
            result.selected_index.map(|i| result.cards.plans[i].clone())
        );
    }
    #[test]
    fn invalid_craft_alternative_does_not_block_buying_the_finished_item() {
        let output = compute(Request {
            materials: vec![Material {
                item: 20,
                needed: 2,
                ..Default::default()
            }],
            offers: BTreeMap::from([(
                20,
                vec![Offer {
                    id: 1,
                    world: 1,
                    quantity: 2,
                    price: 50,
                }],
            )]),
            vendors: BTreeMap::new(),
            context: RouteContext {
                home: 1,
                ..Default::default()
            },
            route: None,
            visits: None,
            finished: None,
            craft_materials: None,
            buy_output: true,
        });
        assert!(!output.cards.plans.is_empty());
        assert_eq!(output.cards.plans[output.selected_index.unwrap()].cost, 100);
        assert!(output.craft_comparison.is_none());
    }
    #[test]
    fn worker_reuses_the_frontier_for_selection_and_invalidates_every_search_input() {
        let mut request = Request {
            materials: vec![Material {
                item: 1,
                needed: 2,
                ..Default::default()
            }],
            offers: BTreeMap::from([(
                1,
                vec![
                    Offer {
                        id: 1,
                        world: 1,
                        quantity: 2,
                        price: 100,
                    },
                    Offer {
                        id: 2,
                        world: 2,
                        quantity: 2,
                        price: 10,
                    },
                ],
            )]),
            vendors: BTreeMap::new(),
            context: RouteContext {
                home: 1,
                ..Default::default()
            },
            route: None,
            visits: None,
            finished: None,
            craft_materials: None,
            buy_output: false,
        };
        let mut worker = Planner::default();
        assert_eq!(worker.compute(request.clone()), compute(request.clone()));
        request.route = Some(BTreeSet::from([2]));
        assert_eq!(worker.compute(request.clone()), compute(request.clone()));
        request.route = None;
        request.visits = Some(0);
        assert_eq!(worker.compute(request.clone()), compute(request.clone()));
        assert_eq!(
            worker.comparison_runs, 1,
            "card selection must reuse the frontier"
        );
        for change in 0..4 {
            match change {
                0 => request.materials[0].needed += 1,
                1 => request.offers.get_mut(&1).unwrap()[0].price += 1,
                2 => {
                    request.vendors.insert(1, 30);
                }
                _ => request.context.weights.world_hop += 1,
            }
            assert_eq!(worker.compute(request.clone()), compute(request.clone()));
            assert_eq!(worker.comparison_runs, change + 2);
        }
    }
}
