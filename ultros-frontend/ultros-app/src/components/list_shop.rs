//! Stable, whole-stack shopping trips shared by account and device lists.
use crate::components::world_picker::WorldOnlyPicker;
use crate::global_state::home_world::use_home_world;
use crate::i18n::{t_string, use_i18n};
use crate::recipe_planner::{self as planner, Material, Offer, RouteContext, ShoppingPlan};
use leptos::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use thousands::Separable;
use ultros_api_types::ActiveListing;
use ultros_calc::list_estimate::{CartEstimate, MissingReason, PriceFeed};
use ultros_calc::list_travel::TravelPolicy;

#[derive(Clone, Debug)]
pub struct ShopRow {
    pub key: String,
    pub name: String,
    pub item_id: i32,
    pub hq: Option<bool>,
    pub needed: i32,
    pub acquired: i32,
    pub listings: Vec<ActiveListing>,
}

#[derive(Clone, Debug)]
pub struct ShopInput {
    pub title: String,
    pub rows: Vec<ShopRow>,
    pub home_world: i32,
    pub world_names: BTreeMap<i32, String>,
    pub datacenters: BTreeMap<i32, i32>,
    pub observed_at: Option<String>,
    /// The same served-price state as Build; frozen with a selected trip.
    pub price_feed: PriceFeed,
    /// Actual Build result, including per-item lookup coverage. Shopping rows
    /// never reconstruct it: their whole-stack purpose differs.
    pub build_estimate: CartEstimate,
    pub estimate_available: bool,
}

impl Default for ShopInput {
    fn default() -> Self {
        Self {
            title: String::new(),
            rows: Vec::new(),
            home_world: 0,
            world_names: BTreeMap::new(),
            datacenters: BTreeMap::new(),
            observed_at: None,
            price_feed: PriceFeed::Missing(MissingReason::NotRequested),
            build_estimate: CartEstimate::default(),
            estimate_available: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Trip {
    source: ShopInput,
    plan: ShoppingPlan,
    mode: usize,
    frontier: Vec<ShoppingPlan>,
    policy: TravelPolicy,
}

impl Trip {
    /// The source and plan are frozen at adoption. Home is first only when
    /// the trip buys there; the remaining world order and each world's
    /// physical-stack order stay stable until a replacement is adopted.
    /// Receipts must use this same sequence when assigning acquired units.
    fn itinerary(&self) -> Vec<(i32, Vec<(i32, Offer)>)> {
        let mut stops = planner::itinerary(&self.plan);
        let home = stops.remove(&self.source.home_world);
        home.into_iter()
            .map(|offers| (self.source.home_world, offers))
            .chain(stops)
            .collect()
    }
}

/// Stacks already recorded against physical listings, keyed by listing id.
type Receipts = BTreeMap<i32, (String, i64)>;

/// A refreshed trip the player has not adopted yet. The active trip keeps
/// rendering unchanged until the player accepts this one or dismisses it.
#[derive(Clone, Debug)]
struct Review {
    source: ShopInput,
    receipts: Receipts,
    plans: Vec<ShoppingPlan>,
    mode: usize,
    frontier: Vec<ShoppingPlan>,
    policy: TravelPolicy,
    unavailable: BTreeSet<i32>,
}

/// A review authorizes exactly the cart and settings it displays. Purchases,
/// undo, quality edits or a newly unavailable stack require a fresh proposal.
fn review_is_current(
    review: &Review,
    live: &ShopInput,
    policy: &TravelPolicy,
    unavailable: &BTreeSet<i32>,
) -> bool {
    review.policy == *policy
        && review.unavailable == *unavailable
        && review.source.home_world == live.home_world
        && review.source.rows.len() == live.rows.len()
        && review
            .source
            .rows
            .iter()
            .zip(&live.rows)
            .all(|(before, after)| {
                let mut old_offers: Vec<_> = before.listings.iter().map(listing_shape).collect();
                let mut new_offers: Vec<_> = after.listings.iter().map(listing_shape).collect();
                old_offers.sort_unstable();
                new_offers.sort_unstable();
                before.key == after.key
                    && before.item_id == after.item_id
                    && before.hq == after.hq
                    && before.needed == after.needed
                    && before.acquired == after.acquired
                    && old_offers == new_offers
            })
}

/// Choosing a route only needs a review once there is progress to protect.
/// Until a stack has been recorded the player is previewing options, so a
/// different world or quick pick replaces the trip outright (#1480 follow-up).
fn choice_needs_review(active: bool, live: &ShopInput) -> bool {
    active && live.rows.iter().any(|row| row.acquired > 0)
}

/// How the live cart differs from the cart an active trip was planned from.
/// Purchase progress is not drift: the trip tracks acquired counts itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CartDrift {
    pub added: usize,
    pub removed: usize,
    pub quantity_changed: usize,
    pub prices_changed: bool,
}

impl CartDrift {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn listing_shape(listing: &ActiveListing) -> (i32, i32, bool, i32, i32) {
    (
        listing.id,
        listing.world_id,
        listing.hq,
        listing.quantity,
        listing.price_per_unit,
    )
}

fn remaining(row: &ShopRow) -> i64 {
    i64::from(row.needed.max(0)) - i64::from(row.acquired.max(0))
}

/// Compares the cart a trip was planned from with the cart as it is now.
/// Price changes only count for rows that still had something to buy when
/// the trip was planned; a finished row's listings cannot change the trip.
pub fn cart_drift(planned: &ShopInput, live: &ShopInput) -> CartDrift {
    let mut drift = CartDrift::default();
    for row in &planned.rows {
        let Some(now) = live.rows.iter().find(|candidate| candidate.key == row.key) else {
            drift.removed += 1;
            continue;
        };
        if now.needed != row.needed {
            drift.quantity_changed += 1;
        }
        if remaining(row) > 0 {
            let mut before: Vec<_> = row.listings.iter().map(listing_shape).collect();
            let mut after: Vec<_> = now.listings.iter().map(listing_shape).collect();
            before.sort_unstable();
            after.sort_unstable();
            if before != after {
                drift.prices_changed = true;
            }
        }
    }
    drift.added = live
        .rows
        .iter()
        .filter(|row| !planned.rows.iter().any(|planned| planned.key == row.key))
        .count();
    drift
}

/// The same quantity-aware shared-supply allocation as Build. Callers use
/// the live input for the handoff and the frozen trip source for its reference;
/// whole-stack purchases and surplus remain separate in `TripTotals`.
pub fn build_estimate(input: &ShopInput) -> CartEstimate {
    input.build_estimate.clone()
}

/// Whole-stack accounting for one plan: the parts of a trip total that the
/// Build estimate cannot see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TripTotals {
    pub cost: i64,
    pub surplus: i64,
    pub missing: i64,
    pub stops: usize,
}

fn trip_totals(plan: &ShoppingPlan) -> TripTotals {
    TripTotals {
        cost: plan.cost,
        surplus: plan
            .purchases
            .values()
            .map(|purchase| (purchase.quantity - purchase.needed).max(0))
            .sum(),
        missing: plan.missing,
        stops: planner::itinerary(plan).len(),
    }
}

// Give quality-specific needs first claim on supply, then offer the remaining
// stacks to Any-quality rows. Each physical listing can appear at most once.
// This allocation is conservative; all route choices are labelled best-found.
#[cfg(test)]
fn candidates(input: &ShopInput, unavailable: &BTreeSet<i32>) -> Vec<ShoppingPlan> {
    let frontier = candidate_frontier(input, unavailable);
    quick_candidates(&frontier)
}

fn quick_candidates(frontier: &[ShoppingPlan]) -> Vec<ShoppingPlan> {
    let home = frontier.first().cloned().unwrap_or_default();
    let fewest = frontier
        .iter()
        .min_by_key(|plan| (plan.missing, planner::itinerary(plan).len(), plan.cost))
        .cloned()
        .unwrap_or_default();
    let cheapest = frontier
        .iter()
        .min_by_key(|plan| (plan.missing, plan.cost, planner::itinerary(plan).len()))
        .cloned()
        .unwrap_or_default();
    vec![home, fewest, cheapest]
}

fn choice_plan<'a>(
    quick: &'a [ShoppingPlan],
    frontier: &'a [ShoppingPlan],
    mode: usize,
) -> &'a ShoppingPlan {
    if mode < 3 {
        quick.get(mode)
    } else {
        frontier.get(mode - 3)
    }
    .unwrap_or(&quick[2])
}

/// Frontier indices are presentation positions, not route identities. A
/// refreshed route follows the same set of purchased worlds when possible;
/// otherwise the complete cheapest replacement is offered for review.
fn refreshed_mode(
    previous: Option<&[ShoppingPlan]>,
    frontier: &[ShoppingPlan],
    mode: usize,
) -> usize {
    if mode < 3 {
        return mode;
    }
    let Some(old) = previous.and_then(|plans| plans.get(mode - 3)) else {
        return 2;
    };
    let worlds: Vec<_> = planner::itinerary(old).into_keys().collect();
    frontier
        .iter()
        .position(|plan| planner::itinerary(plan).into_keys().collect::<Vec<_>>() == worlds)
        .map(|index| index + 3)
        .unwrap_or(2)
}

/// Keep the planner's complete frontier for marginal route comparisons.
/// Input listings have already been narrowed by the editor's travel policy;
/// neither this search nor its separate home allocation may expand that scope.
fn candidate_frontier(input: &ShopInput, unavailable: &BTreeSet<i32>) -> Vec<ShoppingPlan> {
    let plans = scoped_frontier(input, unavailable);
    let mut home = input.clone();
    for row in &mut home.rows {
        row.listings
            .retain(|listing| listing.world_id == input.home_world);
    }
    // Allocate overlapping quality needs within the fixed home scope: cheaper
    // foreign stacks must never hide feasible supply on the player's world.
    let home_plan = scoped_frontier(&home, unavailable)
        .into_iter()
        .next()
        .unwrap_or_default();
    planner::frontier(
        plans.into_iter().chain(std::iter::once(home_plan)),
        &planner::TravelWeights::default(),
        planner::ROUTE_LIMIT,
    )
}

fn scoped_frontier(input: &ShopInput, unavailable: &BTreeSet<i32>) -> Vec<ShoppingPlan> {
    let mut materials = Vec::new();
    let mut market = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut order: Vec<_> = input.rows.iter().enumerate().collect();
    order.sort_by_key(|(index, row)| (row.hq.is_none(), *index));
    for (index, row) in order {
        let remaining = i64::from(row.needed.max(0)) - i64::from(row.acquired.max(0));
        if remaining <= 0 {
            continue;
        }
        let offers: Vec<_> = row
            .listings
            .iter()
            .filter(|listing| listing.item_id == row.item_id)
            .filter(|listing| row.hq.is_none_or(|hq| hq == listing.hq))
            .filter(|listing| !used.contains(&listing.id) && !unavailable.contains(&listing.id))
            .map(|listing| Offer {
                id: listing.id,
                world: listing.world_id,
                quantity: i64::from(listing.quantity),
                price: i64::from(listing.price_per_unit),
            })
            .collect();
        let allocation = planner::purchase(remaining, &offers, None);
        // Reserve only the stacks actually needed, preserving other quality
        // supply for the later row. The solver may choose a different world,
        // so give this row its reserved stacks when other rows overlap.
        let overlaps = input.rows.iter().enumerate().any(|(other, candidate)| {
            other != index
                && candidate.item_id == row.item_id
                && (candidate.hq.is_none() || row.hq.is_none() || candidate.hq == row.hq)
        });
        let assigned = if overlaps { allocation.offers } else { offers };
        used.extend(assigned.iter().map(|offer| offer.id));
        materials.push(Material {
            item: index as i32,
            needed: remaining,
            ..Default::default()
        });
        market.insert(index as i32, assigned);
    }
    let ctx = RouteContext {
        home: input.home_world,
        datacenters: input.datacenters.clone(),
        ..Default::default()
    };
    planner::compare_routes(&materials, &market, &BTreeMap::new(), &ctx).cards
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanionSnapshot {
    title: String,
    world: String,
    progress: String,
    rows: Vec<CompanionRow>,
    can_edit: bool,
    can_undo_purchase: bool,
    has_next: bool,
    labels: BTreeMap<String, String>,
    #[serde(skip)]
    done_count: usize,
    #[serde(skip)]
    total: usize,
    #[serde(skip)]
    unknown_world: Option<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct CompanionRow {
    key: String,
    name: String,
    quality: String,
    quantity: i64,
    cost: i64,
    done: bool,
    #[serde(rename = "canBuy")]
    can_buy: bool,
    description: String,
    #[serde(rename = "quantityLabel")]
    quantity_label: String,
    #[serde(rename = "invalidQuantity")]
    invalid_quantity: String,
}

/// Receipts and candidate plans for a fresh trip over `source`, honouring the
/// stacks the previous trip already recorded. Pure: the caller decides whether
/// to adopt the result (choosing a route) or only to show it (a review).
fn replan(
    previous: Option<&Trip>,
    source: &ShopInput,
    unavailable: &BTreeSet<i32>,
    consumed: &Receipts,
) -> (Receipts, Vec<ShoppingPlan>, Vec<ShoppingPlan>) {
    let mut receipts = consumed.clone();
    if let Some(previous) = previous {
        receipts.extend(completed_offers(previous, source));
    }
    let mut excluded = unavailable.clone();
    excluded.extend(receipts.iter().filter_map(|(id, (key, threshold))| {
        source
            .rows
            .iter()
            .find(|row| &row.key == key)
            .filter(|row| i64::from(row.acquired) >= *threshold)
            .map(|_| *id)
    }));
    let frontier = candidate_frontier(source, &excluded);
    let quick = quick_candidates(&frontier);
    (receipts, quick, frontier)
}

/// Completed physical listings stay excluded across replans. The acquired
/// threshold lets a document undo restore precisely the corresponding supply.
fn completed_offers(trip: &Trip, live: &ShopInput) -> Receipts {
    let mut thresholds = BTreeMap::<i32, i64>::new();
    let mut receipts = BTreeMap::new();
    for (_, offers) in &trip.itinerary() {
        for (index, offer) in offers {
            let row = &trip.source.rows[*index as usize];
            let threshold = thresholds.entry(*index).or_insert(i64::from(row.acquired));
            *threshold += offer.quantity;
            if live
                .rows
                .iter()
                .any(|live| live.key == row.key && i64::from(live.acquired) >= *threshold)
            {
                receipts.insert(offer.id, (row.key.clone(), *threshold));
            }
        }
    }
    receipts
}

fn snapshot(trip: &Trip, live: &ShopInput, stop: usize, can_edit: bool) -> CompanionSnapshot {
    let stops = trip.itinerary();
    let current = stops.get(stop.min(stops.len().saturating_sub(1)));
    let mut completed = BTreeMap::new();
    for (index, row) in trip.source.rows.iter().enumerate() {
        let now = live.rows.iter().find(|candidate| candidate.key == row.key);
        completed.insert(
            index as i32,
            now.map(|r| i64::from(r.acquired) - i64::from(row.acquired))
                .unwrap_or(i64::MAX)
                .max(0),
        );
    }
    let mut rows = Vec::new();
    let mut done_count = 0;
    let mut total = 0;
    let mut pending_rows = BTreeSet::new();
    for (world, offers) in &stops {
        for (index, offer) in offers {
            let row = &trip.source.rows[*index as usize];
            let bought = completed.entry(*index).or_default();
            let left = (offer.quantity - *bought).max(0);
            *bought = (*bought - offer.quantity).max(0);
            total += 1;
            if left == 0 {
                done_count += 1;
            }
            let can_buy = left > 0 && pending_rows.insert(*index);
            if current.is_some_and(|(current_world, _)| current_world == world) {
                let hq = row
                    .listings
                    .iter()
                    .find(|listing| listing.id == offer.id)
                    .is_some_and(|listing| listing.hq);
                rows.push(CompanionRow {
                    key: offer.id.to_string(),
                    name: row.name.clone(),
                    quality: if hq { "HQ" } else { "NQ" }.into(),
                    quantity: left,
                    cost: offer.quantity * offer.price,
                    done: left == 0,
                    can_buy,
                    description: String::new(),
                    quantity_label: String::new(),
                    invalid_quantity: String::new(),
                });
            }
        }
    }
    CompanionSnapshot {
        can_undo_purchase: false,
        title: live.title.clone(),
        world: current
            .map(|(world, _)| {
                trip.source
                    .world_names
                    .get(world)
                    .cloned()
                    .unwrap_or_default()
            })
            .unwrap_or_default(),
        progress: String::new(),
        labels: BTreeMap::new(),
        done_count,
        total,
        unknown_world: current.and_then(|(world, _)| {
            (!trip.source.world_names.contains_key(world)).then_some(*world)
        }),
        has_next: stop + 1 < stops.len() && rows.iter().all(|row| row.done),
        rows,
        can_edit,
    }
}

#[cfg(feature = "hydrate")]
mod browser {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(module = "/../../ultros/static/list-companion.mjs")]
    extern "C" {
        #[wasm_bindgen(js_name = openCompanion)]
        pub fn open(snapshot: &str, callback: &js_sys::Function) -> js_sys::Promise;
        #[wasm_bindgen(js_name = updateCompanion)]
        pub fn update(snapshot: &str);
        #[wasm_bindgen(js_name = closeCompanion)]
        pub fn close();
        #[wasm_bindgen(js_name = watchShopFocus)]
        pub fn watch_focus(root: &web_sys::HtmlElement) -> js_sys::Function;
    }
}

/// The "Cheapest" badge goes to the most complete card only when no other
/// card costs less, so a cheap partial route never reads as the bargain.
fn cheapest_index(plans: &[ShoppingPlan]) -> Option<usize> {
    let i = plans.len().checked_sub(1)?;
    let cost = plans[i].cost;
    plans
        .iter()
        .enumerate()
        .all(|(j, p)| j == i || cost <= p.cost)
        .then_some(i)
}

#[component]
pub fn ListShop(
    input: Signal<ShopInput>,
    on_purchase: Callback<(String, i32)>,
    on_undo: Callback<()>,
    can_undo_purchase: Signal<bool>,
    can_edit: Signal<bool>,
    #[prop(optional)] travel_policy: Option<Signal<TravelPolicy>>,
    /// Told whether a trip is being followed. A host that fetches prices on
    /// its own uses it to stop refreshing under an adopted trip.
    #[prop(optional)]
    on_trip_active: Option<Callback<bool>>,
) -> impl IntoView {
    let i18n = use_i18n();
    // The Starting world picker needs the cookie jar and the world catalog;
    // a bare render (tests, a degraded SSR owner) simply omits it.
    let home_picker = (use_context::<crate::global_state::cookies::Cookies>().is_some()
        && use_context::<crate::global_state::LocalWorldData>().is_some())
    .then(use_home_world);
    let trip = RwSignal::new(None::<Trip>);
    if let Some(on_trip_active) = on_trip_active {
        Effect::new(move |_| on_trip_active.run(trip.with(|trip| trip.is_some())));
    }
    let unavailable = RwSignal::new(BTreeSet::<i32>::new());
    let consumed = RwSignal::new(Receipts::new());
    let review = RwSignal::new(None::<Review>);
    let stop = RwSignal::new(0_usize);
    let notice = RwSignal::new(String::new());
    let policy = Signal::derive(move || {
        travel_policy.map(|policy| policy.get()).unwrap_or_else(|| {
            input.with(|source| TravelPolicy {
                home_world: (source.home_world > 0).then_some(source.home_world),
                datacenters: source.datacenters.clone(),
                ..Default::default()
            })
        })
    });
    let adopt = move |source: ShopInput,
                      receipts: Receipts,
                      plans: Vec<ShoppingPlan>,
                      frontier: Vec<ShoppingPlan>,
                      adopted_policy: TravelPolicy,
                      mode: usize| {
        let plan = choice_plan(&plans, &frontier, mode).clone();
        consumed.set(receipts);
        trip.set(Some(Trip {
            source,
            plan,
            mode,
            frontier,
            policy: adopted_policy,
        }));
        review.set(None);
        stop.set(0);
        notice.set(String::new());
    };
    // The first choice starts a trip. Once a stack has been recorded, any
    // replacement, including a different shortcut or frontier card, is
    // reviewed before changing the active trip; before that the player is
    // previewing options and the choice simply replaces the trip.
    let choose = Callback::new(move |mode: usize| {
        let source = input.get_untracked();
        let (receipts, plans, frontier) = trip.with_untracked(|previous| {
            replan(
                previous.as_ref(),
                &source,
                &unavailable.get_untracked(),
                &consumed.get_untracked(),
            )
        });
        let next_policy = policy.get_untracked();
        let mode = trip.with_untracked(|previous| {
            refreshed_mode(
                previous.as_ref().map(|trip| trip.frontier.as_slice()),
                &frontier,
                mode,
            )
        });
        if choice_needs_review(trip.with_untracked(|active| active.is_some()), &source) {
            review.set(Some(Review {
                source,
                receipts,
                plans,
                mode,
                frontier,
                policy: next_policy,
                unavailable: unavailable.get_untracked(),
            }));
        } else {
            adopt(source, receipts, plans, frontier, next_policy, mode);
        }
    });
    // Refreshing an active trip against newer prices or cart edits is
    // reviewable: the candidate is shown beside the current trip first.
    let refresh = Callback::new(move |()| {
        let Some(mode) = trip.with_untracked(|trip| trip.as_ref().map(|trip| trip.mode)) else {
            return;
        };
        let source = input.get_untracked();
        let (receipts, plans, frontier) = trip.with_untracked(|previous| {
            replan(
                previous.as_ref(),
                &source,
                &unavailable.get_untracked(),
                &consumed.get_untracked(),
            )
        });
        let mode = trip.with_untracked(|previous| {
            refreshed_mode(
                previous.as_ref().map(|trip| trip.frontier.as_slice()),
                &frontier,
                mode,
            )
        });
        review.set(Some(Review {
            source,
            receipts,
            plans,
            mode,
            frontier,
            policy: policy.get_untracked(),
            unavailable: unavailable.get_untracked(),
        }));
    });
    let apply_review = move |_| {
        if let Some(pending) = review.get_untracked() {
            if !review_is_current(
                &pending,
                &input.get_untracked(),
                &policy.get_untracked(),
                &unavailable.get_untracked(),
            ) {
                let source = input.get_untracked();
                let (receipts, plans, frontier) = trip.with_untracked(|previous| {
                    replan(
                        previous.as_ref(),
                        &source,
                        &unavailable.get_untracked(),
                        &consumed.get_untracked(),
                    )
                });
                let mode = refreshed_mode(Some(&pending.frontier), &frontier, pending.mode);
                review.set(Some(Review {
                    source,
                    receipts,
                    plans,
                    mode,
                    frontier,
                    policy: policy.get_untracked(),
                    unavailable: unavailable.get_untracked(),
                }));
                notice.set(t_string!(i18n, list_travel_review_updated).to_string());
                return;
            }
            adopt(
                pending.source,
                pending.receipts,
                pending.plans,
                pending.frontier,
                pending.policy,
                pending.mode,
            );
        }
    };
    let drift = Signal::derive(move || {
        trip.with(|trip| {
            trip.as_ref()
                .map(|trip| cart_drift(&trip.source, &input.get()))
                .unwrap_or_default()
        })
    });
    let drift_text = move || {
        let drift = drift.get();
        if drift.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        if drift.added > 0 {
            parts.push(t_string!(i18n, list_shop_drift_added, count = drift.added).to_string());
        }
        if drift.removed > 0 {
            parts.push(t_string!(i18n, list_shop_drift_removed, count = drift.removed).to_string());
        }
        if drift.quantity_changed > 0 {
            parts.push(
                t_string!(
                    i18n,
                    list_shop_drift_quantity,
                    count = drift.quantity_changed
                )
                .to_string(),
            );
        }
        if drift.prices_changed {
            parts.push(t_string!(i18n, list_shop_drift_prices).to_string());
        }
        format!(
            "{} {}",
            t_string!(i18n, list_shop_drift_intro),
            parts.join(" · ")
        )
    };
    let totals_text = move |totals: &TripTotals| {
        t_string!(
            i18n,
            list_shop_review_totals,
            cost = totals.cost,
            stops = totals.stops,
            missing = totals.missing
        )
        .to_string()
    };
    let live_snapshot = Memo::new(move |_| {
        let source = input.get();
        if !source.estimate_available {
            return None;
        }
        trip.get().map(|trip| {
            let mut view = snapshot(&trip, &source, stop.get(), can_edit.get());
            view.can_undo_purchase = can_edit.get() && can_undo_purchase.get();
            if let Some(world) = view.unknown_world {
                view.world = t_string!(i18n, list_shop_world, world = world).to_string();
            } else if view.world.is_empty() {
                view.world = t_string!(i18n, list_shop_no_stacks).to_string();
            }
            view.progress = t_string!(
                i18n,
                list_shop_progress,
                done = view.done_count,
                total = view.total,
                missing = trip.plan.missing
            )
            .to_string();
            view.labels = BTreeMap::from([
                (
                    "shoppingList".into(),
                    t_string!(i18n, list_shop_shopping_list).to_string(),
                ),
                (
                    "shoppingCompanion".into(),
                    t_string!(i18n, list_shop_companion).to_string(),
                ),
                (
                    "keepOpen".into(),
                    t_string!(i18n, list_shop_keep_open).to_string(),
                ),
                (
                    "anyQuality".into(),
                    t_string!(i18n, list_shop_any_quality).to_string(),
                ),
                (
                    "copyName".into(),
                    t_string!(i18n, list_shop_copy_name).to_string(),
                ),
                (
                    "copied".into(),
                    t_string!(i18n, list_shop_copied).to_string(),
                ),
                (
                    "clipboardUnavailable".into(),
                    t_string!(i18n, list_shop_clipboard_unavailable).to_string(),
                ),
                (
                    "bought".into(),
                    t_string!(i18n, list_shop_bought).to_string(),
                ),
                ("undo".into(), t_string!(i18n, list_shop_undo).to_string()),
                (
                    "nextWorld".into(),
                    t_string!(i18n, list_shop_next).to_string(),
                ),
                (
                    "earlierStack".into(),
                    t_string!(i18n, list_shop_earlier_stack).to_string(),
                ),
                (
                    "nothingLeft".into(),
                    t_string!(i18n, list_shop_nothing_left).to_string(),
                ),
                (
                    "updateFailed".into(),
                    t_string!(i18n, list_shop_update_failed).to_string(),
                ),
                (
                    "regularWindow".into(),
                    t_string!(i18n, list_shop_regular_window).to_string(),
                ),
                (
                    "blocked".into(),
                    t_string!(i18n, list_shop_open_failed).to_string(),
                ),
                (
                    "closed".into(),
                    t_string!(i18n, list_shop_closed).to_string(),
                ),
                (
                    "invalidData".into(),
                    t_string!(i18n, list_shop_invalid_data).to_string(),
                ),
            ]);
            for row in &mut view.rows {
                row.description = t_string!(
                    i18n,
                    list_shop_stack,
                    quality = row.quality.clone(),
                    quantity = row.quantity,
                    cost = row.cost
                )
                .to_string();
                if row.done {
                    row.description.push_str(" · ");
                    row.description
                        .push_str(t_string!(i18n, list_shop_recorded));
                }
                row.quantity_label =
                    t_string!(i18n, list_shop_quantity, name = row.name.clone()).to_string();
                row.invalid_quantity =
                    t_string!(i18n, list_shop_invalid_quantity, quantity = row.quantity)
                        .to_string();
            }
            view
        })
    });
    let action = Callback::new(move |(action, key, quantity): (String, String, i32)| {
        // A queued popup callback can arrive after the document became unreadable.
        // Permission alone can still reflect the last successful REST response.
        if !input.get_untracked().estimate_available {
            return;
        }
        if action == "next" {
            if live_snapshot.get().is_some_and(|view| view.has_next) {
                stop.update(|stop| *stop += 1);
            }
            return;
        }
        if !can_edit.get_untracked() {
            return;
        }
        if action == "undo" {
            if !can_undo_purchase.get_untracked() {
                return;
            }
            on_undo.run(());
            stop.set(0);
            return;
        }
        if action != "bought" || quantity <= 0 {
            return;
        }
        let Some(active) = trip.get_untracked() else {
            return;
        };
        let Some(available) = live_snapshot.get().and_then(|view| {
            view.rows
                .into_iter()
                .find(|row| row.key == key && row.can_buy)
        }) else {
            return;
        };
        for (index, purchase) in &active.plan.purchases {
            if purchase
                .offers
                .iter()
                .any(|offer| offer.id.to_string() == key)
            {
                let row = &active.source.rows[*index as usize];
                if input
                    .get_untracked()
                    .rows
                    .iter()
                    .any(|live| live.key == row.key)
                {
                    on_purchase.run((
                        row.key.clone(),
                        quantity.min(available.quantity.min(i64::from(i32::MAX)) as i32),
                    ));
                }
                return;
            }
        }
    });
    #[cfg(feature = "hydrate")]
    let browser_callback = {
        use wasm_bindgen::{JsCast, closure::Closure};
        let callback = Closure::<dyn Fn(String, String, i32)>::new(move |kind, key, quantity| {
            action.run((kind, key, quantity))
        });
        let function: js_sys::Function = callback
            .as_ref()
            .unchecked_ref::<js_sys::Function>()
            .clone();
        let stored = StoredValue::new_local(callback);
        on_cleanup(move || {
            browser::close();
            stored.update_value(|_| {});
        });
        StoredValue::new_local(function)
    };
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if !input.get().estimate_available {
            browser::close();
            return;
        }
        if let Some(view) = live_snapshot.get() {
            let changed_list = trip.get().is_some_and(|active| {
                let live = input.get();
                active.source.title != live.title
                    || active
                        .source
                        .rows
                        .iter()
                        .any(|old| !live.rows.iter().any(|row| row.key == old.key))
            });
            if !view.can_edit || changed_list {
                browser::close();
            } else if let Ok(json) = serde_json::to_string(&view) {
                browser::update(&json);
            }
        }
    });
    let pop_out = move |_| {
        #[cfg(feature = "hydrate")]
        if let Some(view) = live_snapshot.get()
            && let Ok(json) = serde_json::to_string(&view)
        {
            // Call before awaiting anything: PiP requires transient activation.
            let promise = browser_callback.with_value(|callback| browser::open(&json, callback));
            leptos::task::spawn_local(async move {
                if wasm_bindgen_futures::JsFuture::from(promise).await.is_err() {
                    let _ = notice.try_set(t_string!(i18n, list_shop_open_failed).to_string());
                }
            });
        }
    };
    let stacks = NodeRef::<leptos::html::Div>::new();
    #[cfg(feature = "hydrate")]
    {
        let cleanup = StoredValue::new_local(None::<js_sys::Function>);
        let available = Memo::new(move |_| input.get().estimate_available);
        Effect::new(move |_| {
            let root = if available.get() { stacks.get() } else { None };
            cleanup.update_value(|cleanup| {
                if let Some(previous) = cleanup.take() {
                    let _ = previous.call0(&wasm_bindgen::JsValue::NULL);
                }
                *cleanup = root.map(|root| browser::watch_focus(&root));
            });
        });
        on_cleanup(move || {
            cleanup.with_value(|cleanup| {
                if let Some(cleanup) = cleanup {
                    let _ = cleanup.call0(&wasm_bindgen::JsValue::NULL);
                }
            })
        });
    }
    view! {
        <Show when=move || input.get().estimate_available>
        <section class="space-y-4" aria-label=move || t_string!(i18n, list_shop_trip).to_string()>
            <div class="rounded-xl border border-white/10 p-4 space-y-2" data-testid="shop-handoff">
                <p>{move || t_string!(i18n, list_shop_handoff_intro)}</p>
                <p class="font-medium" data-testid="shop-cart-summary">{move || {
                    let source = input.get();
                    let estimate = build_estimate(&source);
                    let remaining: i64 = estimate.lines.iter().map(|line| i64::from(line.remaining)).sum();
                    let priced = if source.price_feed.has_prices() { estimate.lines_priced } else { 0 };
                    t_string!(i18n, list_shop_cart_summary, items = estimate.lines_needing_units(), units = remaining, priced = priced).to_string()
                }}</p>
                <p class="text-sm text-[color:var(--color-text-muted)]">{move || {
                    let input = input.get();
                    match input.world_names.get(&input.home_world) {
                        Some(name) => t_string!(i18n, list_shop_home_world, world = name.clone()).to_string(),
                        None if input.home_world > 0 => t_string!(i18n, list_shop_home_world, world = input.home_world.to_string()).to_string(),
                        None => t_string!(i18n, list_shop_home_unset).to_string(),
                    }
                }}</p>
                <p role="status" class="text-sm" data-testid="shop-no-prices" class:hidden=move || input.with(|input| input.rows.iter().any(|row| !row.listings.is_empty()))>{move || t_string!(i18n, list_shop_no_prices)}</p>
            </div>
            <div class="rounded-xl border border-white/10 p-4 space-y-3" data-testid="shop-route-picker">
                <div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
                    <div class="min-w-0">
                        <h2 class="text-xl font-semibold">{move || t_string!(i18n, recipe_planner_route_heading)}</h2>
                        <p class="text-sm text-[color:var(--color-text-muted)]">{move || t_string!(i18n, recipe_planner_route_subheading)}</p>
                    </div>
                    {home_picker.map(|(home_world, set_home_world)| view! {
                        <label class="flex flex-wrap items-center gap-2 text-sm" data-testid="shop-starting-world">
                            <span class="text-[color:var(--color-text-muted)]">{move || t_string!(i18n, list_shop_starting_world)}</span>
                            <WorldOnlyPicker current_world=home_world set_current_world=set_home_world />
                        </label>
                    })}
                </div>
                // One card per route on the travel frontier, shortest trip first,
                // exactly like the recipe tool. Before a trip is adopted the
                // frontier follows the live cart; once adopted it is frozen with
                // the trip, and choosing another card goes through Review.
                <div class="grid grid-cols-2 gap-2 xl:grid-cols-5" role="group" aria-label=move || t_string!(i18n, list_shop_route).to_string() data-testid="shop-route-comparison">
                    {move || {
                        let source = input.get();
                        let active = trip.get();
                        let frontier = active.as_ref().map(|active| active.frontier.clone()).unwrap_or_else(|| {
                            replan(None, &source, &unavailable.get(), &consumed.get()).2
                        });
                        // `cheapest_card` also marks a lone card, so the
                        // e2e harness can always find the route to adopt.
                        let cheapest_card = cheapest_index(&frontier);
                        let (best_value, cheapest) = if frontier.len() > 1 {
                            (planner::RouteComparison { cards: frontier.clone() }.best_value(), cheapest_card)
                        } else {
                            (None, None)
                        };
                        frontier.iter().enumerate().map(|(index, plan)| {
                            let selected = active.as_ref().is_some_and(|active| active.plan == *plan);
                            let saving = match planner::marginal_saving_line(&frontier, index) {
                                Some(planner::SavingLine::Saved(amount)) => Some(t_string!(i18n, list_travel_saved_previous, amount = amount).to_string()),
                                Some(planner::SavingLine::Same) => Some(t_string!(i18n, list_travel_saved_previous, amount = 0).to_string()),
                                _ => None,
                            };
                            let stops = planner::itinerary(plan);
                            let worlds = stops.keys().map(|world| world.to_string()).collect::<Vec<_>>().join(",");
                            let stop_names = stops.keys().map(|world| source.world_names.get(world).cloned().unwrap_or_else(|| world.to_string())).collect::<Vec<_>>().join(" · ");
                            let label = match (plan.travel.dc_hops, plan.travel.world_hops) {
                                (0, 0) => t_string!(i18n, recipe_planner_route_stay_home).to_string(),
                                (0, count) => t_string!(i18n, recipe_planner_route_world_hops, count = count).to_string(),
                                (dcs, worlds) => t_string!(i18n, recipe_planner_route_dc_and_world_hops, dcs = dcs, worlds = worlds).to_string(),
                            };
                            let badge = match (best_value == Some(index), cheapest == Some(index)) {
                                (true, true) => Some((format!("{} · {}", t_string!(i18n, recipe_planner_route_best_value), t_string!(i18n, recipe_planner_route_cheapest)), true)),
                                (true, false) => Some((t_string!(i18n, recipe_planner_route_best_value).to_string(), true)),
                                (false, true) => Some((t_string!(i18n, recipe_planner_route_cheapest).to_string(), false)),
                                (false, false) => None,
                            };
                            let incomplete = plan.missing > 0;
                            // A partial route is hatched and dashed so its lower total reads as "not comparable", not as a saving.
                            let card_class = if incomplete { "panel panel-incomplete rounded-xl p-3 text-left space-y-1 min-h-11 hover:border-brand-500 focus-visible:ring-2 focus-visible:ring-brand-400" } else { "panel rounded-xl p-3 text-left space-y-1 min-h-11 hover:border-brand-500 focus-visible:ring-2 focus-visible:ring-brand-400" };
                            let cost = plan.cost;
                            let missing = plan.missing;
                            let title = stop_names.clone();
                            view! {
                                <button type="button" class=card_class class:border-brand-400=selected aria-pressed=selected.to_string() title=title
                                    data-testid="shop-route-option" data-route-index=index data-route-worlds=worlds data-route-cost=cost data-route-missing=missing data-incomplete=incomplete.to_string() data-route-cheapest=(cheapest_card == Some(index)).to_string()
                                    on:click=move |_| choose.run(index + 3)>
                                    <span class="flex flex-wrap items-center justify-between gap-2">
                                        <span class="text-sm text-[color:var(--color-text-muted)]">{label}</span>
                                        {badge.map(|(text, brand)| {
                                            let badge_class = if brand { "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide bg-brand-500/20 text-brand-300" } else { "rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide bg-[color:var(--color-background-elevated)] text-[color:var(--color-text-muted)]" };
                                            view! { <span class=badge_class data-testid="shop-route-badge">{text}</span> }
                                        })}
                                    </span>
                                    <strong class="block text-xl tabular-nums">{t_string!(i18n, lists_workspace_gil, price = cost.separate_with_commas())}</strong>
                                    <span class="block text-xs text-[color:var(--color-text-muted)]">{t_string!(i18n, list_shop_route_stops, count = stops.len())}</span>
                                    {incomplete.then(|| view! { <span class="block text-xs text-amber-300">{t_string!(i18n, list_shop_route_partial, count = missing)}</span> })}
                                    <span class="block text-xs" data-testid="shop-route-saving">{saving}</span>
                                    <span class="sr-only">{stop_names}</span>
                                </button>
                            }
                        }).collect_view()
                    }}
                </div>
                <p class="text-xs text-[color:var(--color-text-muted)]">{move || t_string!(i18n, list_travel_comparison_note)}</p>
                <p class="text-xs text-[color:var(--color-text-muted)]">{move || input.get().observed_at.map(|time| t_string!(i18n, list_shop_observed, time = time).to_string()).unwrap_or_else(|| t_string!(i18n, list_shop_age_unknown).to_string())}</p>
            </div>
            <p role="status" data-testid="shop-notice" class:hidden=move || notice.get().is_empty()>{move || notice.get()}</p>
            {move || trip.get().map(|active| {
                let totals = trip_totals(&active.plan);
                let estimate = build_estimate(&active.source);
                let feed = active.source.price_feed;
                let estimate_cost = super::list_estimate_summary::displayed_total(feed, &estimate)
                    .map(|total| t_string!(i18n, lists_workspace_gil, price = total.separate_with_commas()).to_string())
                    .unwrap_or_else(|| "—".to_string());
                let estimate_coverage = super::list_estimate_summary::status_text(i18n, feed, &estimate);
                let incomplete = feed.has_prices() && estimate.is_incomplete();
                let priced = if feed.has_prices() { estimate.lines_priced } else { 0 };
                let refresh_failed = matches!(feed, PriceFeed::Observed { refresh_failed: true, .. });
                view! {
                    <div class="flex flex-wrap items-center justify-between gap-3">
                        <strong data-testid="shop-totals">{t_string!(i18n, list_shop_totals, cost = totals.cost, surplus = totals.surplus, missing = totals.missing)}</strong>
                        <div class="flex flex-wrap gap-2">
                            <button class="btn-secondary min-h-11" data-testid="shop-refresh" on:click=move |_| refresh.run(())>{move || t_string!(i18n, list_shop_replan)}</button>
                            <button class="btn-primary min-h-11" data-testid="open-shopping-companion" on:click=pop_out>{move || t_string!(i18n, list_shop_popout)}</button>
                        </div>
                    </div>
                    <details class="rounded-lg border border-white/10 p-3 text-sm" data-testid="shop-estimate">
                        <summary class="cursor-pointer font-medium">{t_string!(i18n, list_shop_estimate_summary)}</summary>
                        <ul class="space-y-1 pt-2">
                            <li data-testid="shop-build-reference" data-incomplete=incomplete.to_string() data-refresh-failed=refresh_failed.to_string()>{t_string!(i18n, list_shop_estimate_build, cost = estimate_cost, priced = priced, rows = estimate.lines_needing_units())}</li>
                            <li data-testid="shop-build-coverage">{estimate_coverage}</li>
                            <Show when=move || refresh_failed><li data-testid="shop-build-refresh-failed">{t_string!(i18n, lists_estimate_refresh_failed)}</li></Show>
                            <li>{t_string!(i18n, list_shop_estimate_trip, cost = totals.cost, stops = totals.stops)}</li>
                            <li>{t_string!(i18n, list_shop_estimate_surplus, units = totals.surplus)}</li>
                            <li>{t_string!(i18n, list_shop_estimate_missing, units = totals.missing)}</li>
                        </ul>
                        <p class="pt-2 text-[color:var(--color-text-muted)]">{t_string!(i18n, list_shop_estimate_explainer)}</p>
                    </details>
                }
            })}
            <p role="status" class="text-sm" data-testid="shop-drift" class:hidden=move || drift.get().is_empty()>{drift_text}</p>
            <p role="status" class="text-sm" data-testid="shop-travel-drift" class:hidden=move || !trip.with(|active| active.as_ref().is_some_and(|active| active.policy != policy.get()))>{move || t_string!(i18n, list_travel_changed)}</p>
            {move || review.get().map(|pending| {
                let current = trip.with_untracked(|trip| trip.as_ref().map(|trip| trip_totals(&trip.plan))).unwrap_or_default();
                let next = trip_totals(choice_plan(&pending.plans, &pending.frontier, pending.mode));
                view! {
                    <div class="rounded-xl border border-brand-400/60 p-4 space-y-2" role="group" aria-label=t_string!(i18n, list_shop_review_label).to_string() data-testid="shop-review">
                        <p>{t_string!(i18n, list_shop_review_intro)}</p>
                        <p>{t_string!(i18n, list_shop_review_current)}" "{totals_text(&current)}</p>
                        <p data-testid="shop-review-next">{t_string!(i18n, list_shop_review_next)}" "{totals_text(&next)}</p>
                        <div class="flex flex-wrap gap-2">
                            <button class="btn-primary min-h-11" data-testid="shop-review-apply" on:click=apply_review>{t_string!(i18n, list_shop_review_apply)}</button>
                            <button class="btn-secondary min-h-11" data-testid="shop-review-keep" on:click=move |_| review.set(None)>{t_string!(i18n, list_shop_review_keep)}</button>
                        </div>
                    </div>
                }
            })}
            <div node_ref=stacks class="rounded-xl border border-white/10 p-4 space-y-3" class:hidden=move || live_snapshot.get().is_none()>
                <h3 class="text-lg font-semibold" tabindex="-1" data-testid="shop-stop-title">{move || live_snapshot.get().map(|view| view.world)}</h3>
                <p aria-live="polite" data-testid="shop-progress">{move || live_snapshot.get().map(|view| view.progress)}</p>
                <For
                    each=move || live_snapshot.get().map(|view| view.rows).unwrap_or_default()
                    key=|row| row.key.clone()
                    children=move |initial| {
                        let key = initial.key.clone();
                        let find_key = key.clone();
                        let gone_key = key.clone();
                        // The row's owner and draft outlive snapshots. Only its
                        // live values change when another purchase arrives.
                        let row = Memo::new(move |_| live_snapshot.get().and_then(|view| view.rows.into_iter().find(|row| row.key == find_key)).unwrap_or_else(|| initial.clone()));
                        let limit = Memo::new(move |_| row.get().quantity.min(i64::from(i32::MAX)));
                        let amount = RwSignal::new(limit.get_untracked().to_string());
                        let dirty = RwSignal::new(false);
                        Effect::new(move |_| {
                            let quantity = limit.get();
                            // A completed stack no longer has an actionable
                            // purchase draft. Retire it so a disabled editor
                            // cannot hold a Make online transition open.
                            if row.get().done {
                                dirty.set(false);
                                amount.set(quantity.to_string());
                            } else if !dirty.get() {
                                amount.set(quantity.to_string());
                            }
                        });
                        let buy_key = key.clone();
                        view! {
                            <div class="rounded-lg border border-white/10 p-3 space-y-2" data-testid="shop-stack" data-shop-key=key>
                                <div class="flex items-center justify-between gap-3">
                                    <strong>{move || row.get().name}</strong>
                                    <span class="[&_button]:min-h-11 [&_button]:min-w-11"><crate::components::clipboard::Clipboard clipboard_text=Signal::derive(move || row.get().name)/></span>
                                </div>
                                <p data-testid="shop-stack-description">{move || row.get().description}</p>
                                <div class="flex flex-wrap gap-2">
                                    <input class="input max-w-20 min-h-11" type="number" min="1" max=move || limit.get() aria-label=move || row.get().quantity_label data-testid="shop-stack-quantity" data-handoff-committed=move || limit.get() disabled=move || row.get().done || !can_edit.get() prop:value=move || amount.get() on:input=move |event| { dirty.set(true); amount.set(event_target_value(&event)); } on:keydown=move |event| {
                                        if event.key() == "Escape" {
                                            event.prevent_default();
                                            event.stop_propagation();
                                            dirty.set(false);
                                            amount.set(limit.get_untracked().to_string());
                                        }
                                    }/>
                                    <button class="btn-primary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" data-testid="shop-stack-bought" disabled=move || !row.get().can_buy || !can_edit.get() on:click=move |_| {
                                        let live = row.get_untracked();
                                        match amount.get_untracked().parse::<i32>() {
                                            Ok(quantity) if quantity > 0 && i64::from(quantity) <= live.quantity => {
                                                dirty.set(false);
                                                action.run(("bought".into(), buy_key.clone(), quantity));
                                            }
                                            _ => notice.set(live.invalid_quantity),
                                        }
                                    }>{move || if row.get().done { t_string!(i18n, list_shop_recorded).to_string() } else { t_string!(i18n, list_shop_bought).to_string() }}</button>
                                    <button class="btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" data-testid="shop-stack-gone" disabled=move || row.get().done on:click=move |_| {
                                        if let Ok(id) = gone_key.parse() { unavailable.update(|ids| { ids.insert(id); }); }
                                        notice.set(t_string!(i18n, list_shop_excluded).to_string());
                                    }>{move || t_string!(i18n, list_shop_gone)}</button>
                                </div>
                            </div>
                        }
                    }
                />
                <div class="flex flex-wrap gap-2">
                    <button data-testid="shop-undo-purchase" class="btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" disabled=move || !can_edit.get() || !can_undo_purchase.get() on:click=move |_| action.run(("undo".into(), String::new(), 0))>{move || t_string!(i18n, list_shop_undo_purchase)}</button>
                    <button data-testid="shop-next-world" class="btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" disabled=move || !live_snapshot.get().is_some_and(|view| view.has_next) on:click=move |_| action.run(("next".into(), String::new(), 0))>{move || t_string!(i18n, list_shop_next)}</button>
                </div>
            </div>
        </section>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_calc::list_estimate::Coverage;

    // Simulate the route publishing a new result from the shared production
    // estimator. Shop itself must never do this from its shopping rows.
    fn update_build(source: &mut ShopInput) {
        use ultros_calc::list_estimate::{LineRequest, estimate_cart};
        source.build_estimate =
            estimate_cart(source.rows.iter().enumerate().map(|(index, row)| {
                (
                    LineRequest {
                        row_id: index as i32,
                        item_id: row.item_id,
                        hq: row.hq,
                        requested: row.needed,
                        acquired: row.acquired,
                    },
                    row.listings.as_slice(),
                )
            }));
    }

    fn listing(id: i32, world: i32, quantity: i32, price: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id,
            world_id: world,
            item_id: 42,
            retainer_id: 1,
            quantity,
            price_per_unit: price,
            hq,
            timestamp: "2026-09-10T12:00:00".parse().unwrap(),
        }
    }
    fn input() -> ShopInput {
        let mut source = ShopInput {
            title: "Raid supplies".into(),
            home_world: 1,
            price_feed: PriceFeed::observed("2026-09-16T00:00:00Z".parse().unwrap()),
            estimate_available: true,
            rows: vec![ShopRow {
                key: "row:1".into(),
                name: "Potion".into(),
                item_id: 42,
                hq: None,
                needed: 3,
                acquired: 0,
                listings: vec![listing(1, 1, 99, 1, false), listing(2, 2, 3, 10, false)],
            }],
            ..Default::default()
        };
        update_build(&mut source);
        source
    }
    #[test]
    fn actual_stack_cost_changes_cheapest_world() {
        let plans = candidates(&input(), &BTreeSet::new());
        assert_eq!(plans[0].cost, 99);
        assert_eq!(plans[0].purchases[&0].quantity, 99);
        assert_eq!(plans[2].cost, 30);
        assert_eq!(plans[2].purchases[&0].quantity, 3);
    }

    #[test]
    fn higher_id_home_is_first_for_progress_receipts_and_replanning() {
        let mut source = input();
        source.home_world = 9;
        source.world_names = BTreeMap::from([(1, "Foreign".into()), (9, "Home".into())]);
        source.rows[0].needed = 6;
        source.rows[0].listings = vec![listing(1, 1, 3, 1, false), listing(2, 9, 3, 10, false)];
        let plans = candidates(&source, &BTreeSet::new());
        let trip = Trip {
            source: source.clone(),
            plan: plans[2].clone(),
            mode: 2,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        assert_eq!(
            trip.itinerary()
                .iter()
                .map(|(world, _)| *world)
                .collect::<Vec<_>>(),
            vec![9, 1],
        );
        let first = snapshot(&trip, &source, 0, true);
        assert_eq!(first.world, "Home");
        assert_eq!(first.rows[0].key, "2");
        assert!(!first.has_next);

        // An uncompleted home stack retains first claim on purchased units.
        source.rows[0].acquired = 1;
        assert_eq!(snapshot(&trip, &source, 0, true).rows[0].quantity, 2);
        assert!(!snapshot(&trip, &source, 1, true).rows[0].can_buy);
        assert!(completed_offers(&trip, &source).is_empty());
        source.rows[0].acquired = 3;
        assert!(snapshot(&trip, &source, 0, true).has_next);
        let next = snapshot(&trip, &source, 1, true);
        assert_eq!(next.world, "Foreign");
        assert_eq!(next.rows[0].key, "1");
        assert!(next.rows[0].can_buy);
        let receipts = completed_offers(&trip, &source);
        assert_eq!(receipts, BTreeMap::from([(2, ("row:1".into(), 3))]));
        let (_, reviewed, _) = replan(Some(&trip), &source, &BTreeSet::new(), &Receipts::new());
        assert_eq!(reviewed[2].purchases[&0].offers[0].id, 1);
        assert_eq!(reviewed[2].cost, 3);

        // Neither a changed home nor a changed feed rewrites the active trip.
        source.home_world = 1;
        source.rows[0].listings.clear();
        assert_eq!(snapshot(&trip, &source, 0, true).world, "Home");
        assert_eq!(snapshot(&trip, &source, 1, true).world, "Foreign");
        assert_eq!(completed_offers(&trip, &source), receipts);
        source.rows[0].acquired = 0;
        assert!(completed_offers(&trip, &source).is_empty());
        assert_eq!(snapshot(&trip, &source, 0, true).rows[0].quantity, 3);
        assert!(!snapshot(&trip, &source, 1, true).rows[0].can_buy);
    }

    #[test]
    fn home_first_order_does_not_invent_a_home_stop() {
        let mut source = input();
        source.home_world = 99;
        let plans = candidates(&source, &BTreeSet::new());
        let trip = Trip {
            source,
            plan: plans[2].clone(),
            mode: 2,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        assert_eq!(
            trip.itinerary()
                .iter()
                .map(|(world, _)| *world)
                .collect::<Vec<_>>(),
            vec![2],
        );
        let empty = Trip {
            plan: ShoppingPlan::default(),
            ..trip
        };
        assert!(empty.itinerary().is_empty());
    }

    fn travel_ladder_input() -> ShopInput {
        let mut source = input();
        source.home_world = 9;
        source.datacenters = BTreeMap::from([(1, 10), (9, 10), (20, 30)]);
        source.rows[0].needed = 1;
        source.rows[0].listings = vec![listing(1, 9, 1, 100, false), listing(2, 1, 1, 60, false)];
        let mut second = source.rows[0].clone();
        second.key = "row:2".into();
        second.item_id = 43;
        second.listings = vec![listing(3, 9, 1, 100, false), listing(4, 20, 1, 20, false)];
        for offer in &mut second.listings {
            offer.item_id = 43;
        }
        source.rows.push(second);
        source
    }

    #[test]
    fn candidate_frontier_keeps_every_complete_marginal_step() {
        let source = travel_ladder_input();
        let frontier = candidate_frontier(&source, &BTreeSet::new());
        assert_eq!(
            frontier
                .iter()
                .map(|plan| (plan.cost, plan.missing))
                .collect::<Vec<_>>(),
            vec![(200, 0), (160, 0), (120, 0), (80, 0)]
        );
        assert_eq!(
            frontier
                .iter()
                .map(|plan| (plan.travel.world_hops, plan.travel.dc_hops))
                .collect::<Vec<_>>(),
            vec![(0, 0), (1, 0), (0, 1), (1, 1)]
        );
        for index in 1..frontier.len() {
            assert_eq!(
                planner::marginal_saving_line(&frontier, index),
                Some(planner::SavingLine::Saved(40))
            );
        }
        // The middle routes are alternatives, not a nested chain of worlds.
        assert_eq!(
            planner::itinerary(&frontier[1])
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![1, 9]
        );
        assert_eq!(
            planner::itinerary(&frontier[2])
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![9, 20]
        );
    }

    #[test]
    fn frontier_refresh_tracks_worlds_instead_of_card_position() {
        let frontier = candidate_frontier(&travel_ladder_input(), &BTreeSet::new());
        let shortened = frontier[1..].to_vec();
        assert_eq!(refreshed_mode(Some(&frontier), &shortened, 4), 3);
        assert_eq!(
            choice_plan(&quick_candidates(&shortened), &shortened, 3).cost,
            160
        );
        assert_eq!(refreshed_mode(Some(&frontier), &frontier[2..], 4), 2);
        assert_eq!(refreshed_mode(Some(&frontier), &shortened, 1), 1);
    }

    #[test]
    fn a_review_is_invalidated_by_purchases_quality_prices_or_policy() {
        let source = travel_ladder_input();
        let policy = TravelPolicy {
            home_world: Some(source.home_world),
            datacenters: source.datacenters.clone(),
            ..Default::default()
        };
        let (receipts, plans, frontier) = replan(None, &source, &BTreeSet::new(), &Receipts::new());
        let review = Review {
            source: source.clone(),
            receipts,
            plans,
            mode: 2,
            frontier,
            policy: policy.clone(),
            unavailable: BTreeSet::new(),
        };
        assert!(review_is_current(
            &review,
            &source,
            &policy,
            &BTreeSet::new()
        ));
        let mut live = source.clone();
        live.rows[0].acquired += 1;
        assert!(!review_is_current(
            &review,
            &live,
            &policy,
            &BTreeSet::new()
        ));
        live = source.clone();
        live.rows[0].hq = Some(true);
        assert!(!review_is_current(
            &review,
            &live,
            &policy,
            &BTreeSet::new()
        ));
        live = source.clone();
        live.rows[0].listings[0].price_per_unit += 1;
        assert!(!review_is_current(
            &review,
            &live,
            &policy,
            &BTreeSet::new()
        ));
        live = source.clone();
        live.rows[0].listings.reverse();
        assert!(
            review_is_current(&review, &live, &policy, &BTreeSet::new()),
            "transport ordering alone does not change the proposal"
        );
        let mut changed_policy = policy.clone();
        changed_policy.excluded_worlds.insert(999);
        assert!(
            !review_is_current(&review, &source, &changed_policy, &BTreeSet::new()),
            "setting identity matters even when its current offers are unchanged"
        );
        assert!(!review_is_current(
            &review,
            &source,
            &policy,
            &BTreeSet::from([1])
        ));
        live.home_world = 1;
        assert!(!review_is_current(
            &review,
            &live,
            &policy,
            &BTreeSet::new()
        ));
    }

    #[test]
    fn constrained_frontier_cannot_reintroduce_excluded_supply() {
        use ultros_calc::list_travel::{TravelLimit, TravelPolicy};
        let original = travel_ladder_input();
        let mut source = original.clone();
        let mut policy = TravelPolicy {
            limit: TravelLimit::Datacenter,
            home_world: Some(source.home_world),
            datacenters: source.datacenters.clone(),
            ..Default::default()
        };
        let apply = |source: &mut ShopInput, policy: &TravelPolicy| {
            for row in &mut source.rows {
                row.listings = policy.filter_listings(&row.listings);
            }
        };
        apply(&mut source, &policy);
        let frontier = candidate_frontier(&source, &BTreeSet::new());
        assert_eq!(
            frontier.iter().map(|plan| plan.cost).collect::<Vec<_>>(),
            vec![200, 160]
        );
        assert!(frontier.iter().all(|plan| {
            planner::itinerary(plan)
                .keys()
                .all(|world| policy.allows_world(*world))
        }));

        source = original.clone();
        policy.excluded_worlds.insert(1);
        apply(&mut source, &policy);
        assert_eq!(candidate_frontier(&source, &BTreeSet::new())[0].cost, 200);
        source = original;
        // Only the foreign DC can supply row2: the constrained source must
        // retain that missing row, even though its offer list becomes empty.
        source.rows[1].listings.retain(|offer| offer.world_id == 20);
        policy.excluded_worlds.clear();
        apply(&mut source, &policy);
        let partial = candidate_frontier(&source, &BTreeSet::new());
        assert!(partial.iter().all(|plan| plan.missing == 1));
        assert_eq!(planner::savings_basis(&partial), None);
        assert_eq!(planner::marginal_saving_line(&partial, 1), None);
        assert!(partial.iter().all(|plan| {
            planner::itinerary(plan)
                .keys()
                .all(|world| policy.allows_world(*world))
        }));
    }
    #[test]
    fn no_supply_is_explicitly_missing() {
        let mut source = input();
        source.rows[0].listings.clear();
        assert!(
            candidates(&source, &BTreeSet::new())
                .iter()
                .all(|plan| plan.missing == 3)
        );
    }
    #[test]
    fn overlapping_quality_rows_cannot_buy_the_same_listing_twice() {
        let mut source = input();
        source.rows[0].listings = vec![listing(1, 1, 3, 10, true)];
        let mut specific = source.rows[0].clone();
        specific.key = "row:2".into();
        specific.hq = Some(true);
        source.rows.push(specific);
        let plans = candidates(&source, &BTreeSet::new());
        assert_eq!(plans[2].missing, 3);
        assert_eq!(
            plans[2].purchases.values().flat_map(|p| &p.offers).count(),
            1
        );
    }
    #[test]
    fn live_prices_do_not_move_active_trip_and_partial_purchase_updates_it() {
        let source = input();
        let plans = candidates(&source, &BTreeSet::new());
        let trip = Trip {
            source: source.clone(),
            plan: plans[2].clone(),
            mode: 2,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        let mut live = source;
        live.rows[0].listings.clear();
        live.rows[0].acquired = 1;
        let view = snapshot(&trip, &live, 0, true);
        assert_eq!(view.rows[0].quantity, 2);
        assert_eq!(view.rows[0].cost, 30);
        assert!(!view.rows[0].done);
        live.rows[0].acquired = 3;
        assert!(snapshot(&trip, &live, 0, true).rows[0].done);
    }

    #[test]
    fn later_stack_is_disabled_until_earlier_stack_is_recorded() {
        let mut source = input();
        source.rows[0].needed = 8;
        source.rows[0].listings = vec![listing(1, 1, 3, 10, false), listing(2, 1, 5, 10, false)];
        let plans = candidates(&source, &BTreeSet::new());
        let trip = Trip {
            source: source.clone(),
            plan: plans[0].clone(),
            mode: 0,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        let view = snapshot(&trip, &source, 0, true);
        assert!(view.rows[0].can_buy);
        assert!(!view.rows[1].can_buy);
        source.rows[0].acquired = 3;
        let view = snapshot(&trip, &source, 0, true);
        assert!(view.rows[0].done);
        assert!(view.rows[1].can_buy);
        assert_eq!(view.rows[1].quantity, 5);
        let receipts = completed_offers(&trip, &source);
        assert_eq!(receipts.get(&1), Some(&("row:1".into(), 3)));
        let replanned = candidates(&source, &receipts.keys().copied().collect());
        assert_eq!(replanned[0].purchases[&0].offers[0].id, 2);
        source.rows[0].acquired = 0;
        assert!(completed_offers(&trip, &source).is_empty());
    }

    #[test]
    fn foreign_quality_offer_does_not_hide_feasible_home_supply() {
        let mut source = input();
        source.rows[0].needed = 1;
        source.rows[0].listings = vec![
            listing(1, 1, 1, 10, true),
            listing(2, 1, 1, 1, false),
            listing(3, 2, 1, 1, true),
        ];
        let mut hq = source.rows[0].clone();
        hq.key = "row:2".into();
        hq.hq = Some(true);
        source.rows.push(hq);
        let plans = candidates(&source, &BTreeSet::new());
        assert_eq!(plans[0].missing, 0);
        assert_eq!(plans[0].cost, 11);
        assert_eq!(planner::itinerary(&plans[1]).len(), 1);
    }

    #[test]
    fn drift_reports_cart_and_price_changes_but_not_purchases() {
        let planned = input();
        let mut live = planned.clone();
        assert!(cart_drift(&planned, &live).is_empty());
        live.rows[0].acquired = 2;
        assert!(
            cart_drift(&planned, &live).is_empty(),
            "recording a purchase is progress, not drift"
        );
        live.rows[0].needed = 5;
        assert_eq!(cart_drift(&planned, &live).quantity_changed, 1);
        live.rows[0].listings[0].price_per_unit = 2;
        assert!(cart_drift(&planned, &live).prices_changed);
        let mut extra = planned.rows[0].clone();
        extra.key = "row:9".into();
        live.rows.push(extra);
        assert_eq!(cart_drift(&planned, &live).added, 1);
        live.rows.remove(0);
        let drift = cart_drift(&planned, &live);
        assert_eq!(
            (drift.removed, drift.added, drift.quantity_changed),
            (1, 1, 0)
        );
    }

    #[test]
    fn finished_rows_do_not_report_price_drift() {
        let mut planned = input();
        planned.rows[0].acquired = 3;
        let mut live = planned.clone();
        live.rows[0].listings.clear();
        assert!(cart_drift(&planned, &live).is_empty());
    }

    #[test]
    fn build_estimate_prices_remaining_units_and_counts_unpriced_rows() {
        let mut source = input();
        source.rows[0].acquired = 1;
        let mut unpriced = source.rows[0].clone();
        unpriced.key = "row:2".into();
        unpriced.item_id = 43;
        unpriced.listings.clear();
        source.rows.push(unpriced);
        update_build(&mut source);
        let estimate = build_estimate(&source);
        assert_eq!(estimate.total, 2);
        assert_eq!(estimate.lines_needing_units(), 2);
        assert_eq!(estimate.lines_priced, 1);
        assert_eq!(estimate.unpriced_units, 2);
        assert_eq!(estimate.coverage, Coverage::Partial);
        assert_eq!(
            estimate
                .lines
                .iter()
                .map(|line| line.remaining)
                .sum::<i32>(),
            4
        );
    }

    #[test]
    fn whole_stack_totals_explain_the_difference_from_the_build_estimate() {
        let source = input();
        let plans = candidates(&source, &BTreeSet::new());
        assert_eq!(
            trip_totals(&plans[0]),
            TripTotals {
                cost: 99,
                surplus: 96,
                missing: 0,
                stops: 1,
            }
        );
        assert_eq!(build_estimate(&source).total, 3);
    }

    fn overlapping_build_fixture() -> ShopInput {
        let mut source = input();
        let offers = vec![
            listing(1, 1, 2, 10, false),
            listing(2, 1, 2, 20, true),
            listing(3, 2, 3, 12, false),
            listing(4, 3, 4, 25, true),
        ];
        source.rows = [(901, None, 4), (302, Some(true), 2), (703, Some(false), 2)]
            .into_iter()
            .map(|(row_id, hq, needed)| ShopRow {
                key: format!("action:{row_id}"),
                name: "Potion".into(),
                item_id: 42,
                hq,
                needed,
                acquired: 0,
                listings: offers.clone(),
            })
            .collect();
        update_build(&mut source);
        source
    }

    #[test]
    fn active_build_reference_stays_on_the_frozen_trip_source() {
        let source = overlapping_build_fixture();
        let plans = candidates(&source, &BTreeSet::new());
        let trip = Trip {
            source: source.clone(),
            plan: plans[2].clone(),
            mode: 2,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        let mut live = source;
        for row in &mut live.rows {
            row.acquired = row.needed;
            row.listings.clear();
        }
        assert_eq!(
            build_estimate(&live).total,
            121,
            "raw shopping rows cannot reconstruct or overwrite the carried Build result"
        );
        update_build(&mut live);
        assert_eq!(build_estimate(&live).total, 0);
        assert_eq!(build_estimate(&trip.source).total, 121);
        assert_eq!(trip.plan, plans[2]);
    }

    #[test]
    fn build_reference_freezes_feed_and_keeps_cached_failure_prices() {
        use super::super::list_estimate_summary::displayed_total;
        let source = overlapping_build_fixture();
        let plans = candidates(&source, &BTreeSet::new());
        let trip = Trip {
            source: source.clone(),
            plan: plans[2].clone(),
            mode: 2,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        let mut live = source;
        live.price_feed = live.price_feed.after_fetch(None);
        assert!(matches!(
            live.price_feed,
            PriceFeed::Observed {
                refresh_failed: true,
                ..
            }
        ));
        assert!(matches!(
            trip.source.price_feed,
            PriceFeed::Observed {
                refresh_failed: false,
                ..
            }
        ));
        assert_eq!(
            live.price_feed.fetched_at(),
            trip.source.price_feed.fetched_at()
        );
        assert_eq!(
            displayed_total(live.price_feed, &build_estimate(&live)),
            Some(121)
        );
        assert_eq!(
            displayed_total(trip.source.price_feed, &build_estimate(&trip.source)),
            Some(121)
        );
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn unreadable_document_does_not_present_a_known_empty_shopping_cart() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>();
            provide_context(i18n);
            let source = ShopInput {
                price_feed: PriceFeed::observed(chrono::Utc::now()),
                ..Default::default()
            };
            let html = view! {
                <ListShop input=Signal::stored(source) on_purchase=Callback::new(|_| ())
                    on_undo=Callback::new(|_| ()) can_undo_purchase=Signal::stored(false)
                    can_edit=Signal::stored(false) />
            }
            .to_html();
            assert!(!html.contains("shop-cart-summary"));
            assert!(!html.contains("shop-route-option"));
        });
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn build_reference_matches_unknown_feed_status_and_acquired_zero() {
        use super::super::list_estimate_summary::{displayed_total, status_text};
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>();
            provide_context(i18n);
            for (feed, message) in [
                (PriceFeed::Loading, "Loading prices"),
                (
                    PriceFeed::Missing(MissingReason::NotRequested),
                    "Look up prices",
                ),
                (
                    PriceFeed::Missing(MissingReason::Failed),
                    "Prices unavailable",
                ),
            ] {
                let mut source = overlapping_build_fixture();
                source.price_feed = feed;
                let estimate = build_estimate(&source);
                assert_eq!(displayed_total(source.price_feed, &estimate), None);
                assert!(status_text(i18n, source.price_feed, &estimate).contains(message));
                for row in &mut source.rows {
                    row.acquired = row.needed;
                }
                update_build(&mut source);
                let acquired = build_estimate(&source);
                // #1471's shared presentation helpers make a fully owned cart
                // zero/complete even if no price request has ever succeeded.
                assert_eq!(displayed_total(source.price_feed, &acquired), Some(0));
                assert!(
                    status_text(i18n, source.price_feed, &acquired).contains("Nothing left to buy")
                );
            }
        });
    }

    #[test]
    fn a_route_choice_is_reviewed_only_once_a_stack_is_recorded() {
        let mut live = input();
        assert!(
            !choice_needs_review(false, &live),
            "the first choice starts a trip"
        );
        assert!(
            !choice_needs_review(true, &live),
            "an active trip with nothing bought is a preview and is replaced outright"
        );
        live.rows[0].acquired = 1;
        assert!(
            choice_needs_review(true, &live),
            "recorded progress is protected behind a review"
        );
        assert!(!choice_needs_review(false, &live));
    }

    #[test]
    fn a_review_leaves_the_active_trip_untouched_until_adopted() {
        let source = input();
        let (receipts, plans, _) = replan(None, &source, &BTreeSet::new(), &Receipts::new());
        assert!(receipts.is_empty());
        let trip = Trip {
            source: source.clone(),
            plan: plans[2].clone(),
            mode: 2,
            frontier: Vec::new(),
            policy: TravelPolicy::default(),
        };
        let mut live = source;
        live.rows[0].listings = vec![listing(3, 1, 3, 1, false)];
        let (_, refreshed, _) = replan(Some(&trip), &live, &BTreeSet::new(), &Receipts::new());
        assert_eq!(refreshed[2].cost, 3);
        assert_eq!(trip.plan.cost, 30);
        assert_eq!(snapshot(&trip, &live, 0, true).rows[0].cost, 30);
    }
}
