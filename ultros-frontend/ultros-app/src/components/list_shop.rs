//! Stable, whole-stack shopping trips shared by account and device lists.
use crate::recipe_planner::{self as planner, Material, Offer, RouteContext, ShoppingPlan};
use leptos::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use ultros_api_types::ActiveListing;

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

#[derive(Clone, Debug, Default)]
pub struct ShopInput {
    pub title: String,
    pub rows: Vec<ShopRow>,
    pub home_world: i32,
    pub world_names: BTreeMap<i32, String>,
    pub datacenters: BTreeMap<i32, i32>,
    pub observed_at: Option<String>,
}

#[derive(Clone, Debug)]
struct Trip {
    source: ShopInput,
    plan: ShoppingPlan,
    mode: usize,
    alternatives: Vec<ShoppingPlan>,
}

// Give quality-specific needs first claim on supply, then offer the remaining
// stacks to Any-quality rows. Each physical listing can appear at most once.
// This allocation is conservative; all route choices are labelled best-found.
fn candidates(input: &ShopInput, unavailable: &BTreeSet<i32>) -> Vec<ShoppingPlan> {
    let mut plans = scoped_candidates(input, unavailable);
    let mut home = input.clone();
    for row in &mut home.rows {
        row.listings
            .retain(|listing| listing.world_id == input.home_world);
    }
    // Allocate overlapping quality needs within the fixed home scope: cheaper
    // foreign stacks must never hide feasible supply on the player's world.
    plans[0] = scoped_candidates(&home, unavailable).remove(0);
    if (
        plans[0].missing,
        planner::itinerary(&plans[0]).len(),
        plans[0].cost,
    ) < (
        plans[1].missing,
        planner::itinerary(&plans[1]).len(),
        plans[1].cost,
    ) {
        plans[1] = plans[0].clone();
    }
    if (plans[0].missing, plans[0].cost) < (plans[2].missing, plans[2].cost) {
        plans[2] = plans[0].clone();
    }
    plans
}

fn scoped_candidates(input: &ShopInput, unavailable: &BTreeSet<i32>) -> Vec<ShoppingPlan> {
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
    let comparison = planner::compare_routes(&materials, &market, &BTreeMap::new(), &ctx);
    let home = comparison.cards.first().cloned().unwrap_or_default();
    let fewest = comparison
        .cards
        .iter()
        .min_by_key(|plan| (plan.missing, planner::itinerary(plan).len(), plan.cost))
        .cloned()
        .unwrap_or_default();
    let cheapest = comparison
        .cards
        .iter()
        .min_by_key(|plan| (plan.missing, plan.cost, planner::itinerary(plan).len()))
        .cloned()
        .unwrap_or_default();
    vec![home, fewest, cheapest]
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanionSnapshot {
    title: String,
    world: String,
    progress: String,
    rows: Vec<CompanionRow>,
    can_edit: bool,
    has_next: bool,
}
#[derive(Clone, Debug, Serialize)]
struct CompanionRow {
    key: String,
    name: String,
    quality: String,
    quantity: i64,
    cost: i64,
    done: bool,
    #[serde(rename = "canBuy")]
    can_buy: bool,
}

/// Completed physical listings stay excluded across replans. The acquired
/// threshold lets a document undo restore precisely the corresponding supply.
fn completed_offers(trip: &Trip, live: &ShopInput) -> BTreeMap<i32, (String, i64)> {
    let mut thresholds = BTreeMap::<i32, i64>::new();
    let mut receipts = BTreeMap::new();
    for offers in planner::itinerary(&trip.plan).values() {
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
    let stops = planner::itinerary(&trip.plan);
    let current = stops.iter().nth(stop.min(stops.len().saturating_sub(1)));
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
                });
            }
        }
    }
    CompanionSnapshot {
        title: live.title.clone(),
        world: current
            .map(|(world, _)| {
                trip.source
                    .world_names
                    .get(world)
                    .cloned()
                    .unwrap_or_else(|| format!("World {world}"))
            })
            .unwrap_or_else(|| "No available stacks".into()),
        progress: format!(
            "{done_count} of {total} stacks recorded · {} missing",
            trip.plan.missing
        ),
        has_next: stop + 1 < stops.len() && rows.iter().all(|row| row.done),
        rows,
        can_edit,
    }
}

#[cfg(feature = "hydrate")]
mod browser {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(
        inline_js = "export { openCompanion, updateCompanion, closeCompanion } from '/static/list-companion.mjs';"
    )]
    extern "C" {
        #[wasm_bindgen(js_name = openCompanion)]
        pub fn open(snapshot: &str, callback: &js_sys::Function) -> js_sys::Promise;
        #[wasm_bindgen(js_name = updateCompanion)]
        pub fn update(snapshot: &str);
        #[wasm_bindgen(js_name = closeCompanion)]
        pub fn close();
    }
}

#[component]
pub fn ListShop(
    input: Signal<ShopInput>,
    on_purchase: Callback<(String, i32)>,
    on_undo: Callback<()>,
    can_edit: Signal<bool>,
) -> impl IntoView {
    let trip = RwSignal::new(None::<Trip>);
    let unavailable = RwSignal::new(BTreeSet::<i32>::new());
    let consumed = RwSignal::new(BTreeMap::<i32, (String, i64)>::new());
    let stop = RwSignal::new(0_usize);
    let notice = RwSignal::new(String::new());
    let choose = Callback::new(move |mode: usize| {
        let source = input.get_untracked();
        if let Some(previous) = trip.get_untracked() {
            consumed.update(|receipts| receipts.extend(completed_offers(&previous, &source)));
        }
        let mut excluded = unavailable.get_untracked();
        consumed.with_untracked(|receipts| {
            excluded.extend(receipts.iter().filter_map(|(id, (key, threshold))| {
                source
                    .rows
                    .iter()
                    .find(|row| &row.key == key)
                    .filter(|row| i64::from(row.acquired) >= *threshold)
                    .map(|_| *id)
            }));
        });
        let plans = candidates(&source, &excluded);
        let plan = plans[mode.min(2)].clone();
        trip.set(Some(Trip {
            source,
            plan,
            mode,
            alternatives: plans,
        }));
        stop.set(0);
        notice.set(String::new());
    });
    let live_snapshot = move || {
        trip.get()
            .map(|trip| snapshot(&trip, &input.get(), stop.get(), can_edit.get()))
    };
    let action = Callback::new(move |(action, key, quantity): (String, String, i32)| {
        if action == "next" {
            if live_snapshot().is_some_and(|view| view.has_next) {
                stop.update(|stop| *stop += 1);
            }
            return;
        }
        if !can_edit.get_untracked() {
            return;
        }
        if action == "undo" {
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
        let Some(available) = live_snapshot().and_then(|view| {
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
        if let Some(view) = live_snapshot() {
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
        if let Some(view) = live_snapshot()
            && let Ok(json) = serde_json::to_string(&view)
        {
            // Call before awaiting anything: PiP requires transient activation.
            let promise = browser_callback.with_value(|callback| browser::open(&json, callback));
            leptos::task::spawn_local(async move {
                if wasm_bindgen_futures::JsFuture::from(promise).await.is_err() {
                    let _ = notice.try_set(
                        "Could not open the companion. Allow pop-ups and try again.".into(),
                    );
                }
            });
        }
    };
    view! {
        <section class="space-y-4" aria-label="Shopping trip">
            <div class="rounded-xl border border-white/10 p-4 space-y-3">
                <h2 class="text-xl font-semibold">"Shopping route"</h2>
                <div class="flex flex-wrap gap-2" role="group" aria-label="Shopping route">
                    <button class=move || if trip.get().is_some_and(|t| t.mode == 0) { "btn-primary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" } else { "btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" } aria-pressed=move || trip.get().is_some_and(|t| t.mode == 0).to_string() data-testid="shop-home" disabled=move || input.get().home_world == 0 on:click=move |_| choose.run(0)>"Stay on my world"</button>
                    <button class=move || if trip.get().is_some_and(|t| t.mode == 1) { "btn-primary min-h-11" } else { "btn-secondary min-h-11" } aria-pressed=move || trip.get().is_some_and(|t| t.mode == 1).to_string() data-testid="shop-fewest" on:click=move |_| choose.run(1)>"Fewest stops"</button>
                    <button class=move || if trip.get().is_some_and(|t| t.mode == 2) { "btn-primary min-h-11" } else { "btn-secondary min-h-11" } aria-pressed=move || trip.get().is_some_and(|t| t.mode == 2).to_string() data-testid="shop-cheapest" on:click=move |_| choose.run(2)>"Lowest total cost"</button>
                </div>
                <p class="text-xs text-[color:var(--color-text-muted)]">{move || input.get().observed_at.map(|time| format!("Prices observed {time}")).unwrap_or_else(|| "Price age unavailable".into())}</p>
            </div>
            <p role="status" class:hidden=move || notice.get().is_empty()>{move || notice.get()}</p>
            {move || trip.get().map(|active| {
                let missing = active.plan.missing;
                let cost = active.plan.cost;
                let surplus: i64 = active.plan.purchases.values().map(|purchase| (purchase.quantity - purchase.needed).max(0)).sum();
                view! {
                    <div class="flex flex-wrap items-center justify-between gap-3">
                        <strong>{format!("{cost} gil · {surplus} surplus · {missing} missing")}</strong>
                        <div class="flex flex-wrap gap-2">
                            <button class="btn-secondary min-h-11" data-testid="shop-refresh" on:click=move |_| choose.run(active.mode)>"Replan remaining items"</button>
                            <button class="btn-primary min-h-11" data-testid="open-shopping-companion" on:click=pop_out>"Pop out shopping companion"</button>
                        </div>
                    </div>
                }
            })}
            {move || live_snapshot().map(|view| view! {
                <div class="rounded-xl border border-white/10 p-4 space-y-3">
                    <h3 class="text-lg font-semibold">{view.world}</h3>
                    <p aria-live="polite">{view.progress}</p>
                    {view.rows.into_iter().map(|row| {
                        let key = row.key.clone();
                        let gone_key = row.key.clone();
                        let amount = RwSignal::new(row.quantity.min(i64::from(i32::MAX)) as i32);
                        view! {
                            <div class="rounded-lg border border-white/10 p-3 space-y-2">
                                <div class="flex items-center justify-between gap-3">
                                    <strong>{row.name.clone()}</strong>
                                    <span class="[&_button]:min-h-11 [&_button]:min-w-11"><crate::components::clipboard::Clipboard clipboard_text=Signal::stored(row.name.clone())/></span>
                                </div>
                                <p>{format!("{} · {} remaining in stack · {} gil for whole stack", row.quality, row.quantity, row.cost)}</p>
                                <div class="flex flex-wrap gap-2">
                                    <input class="input max-w-20 min-h-11" type="number" min="1" max=row.quantity aria-label=format!("Purchase quantity for {}", row.name) prop:value=move || amount.get() on:input=move |event| amount.set(event_target_value(&event).parse().unwrap_or(0))/>
                                    <button class="btn-primary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" disabled=move || !row.can_buy || !can_edit.get() on:click=move |_| action.run(("bought".into(),key.clone(),amount.get_untracked()))>{if row.done { "Recorded" } else { "Bought" }}</button>
                                    <button class="btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" disabled=row.done on:click=move |_| {
                                        if let Ok(id) = gone_key.parse() { unavailable.update(|ids| { ids.insert(id); }); }
                                        notice.set("Listing excluded. Replan remaining items to review a replacement from loaded prices.".into());
                                    }>"Listing is gone"</button>
                                </div>
                            </div>
                        }
                    }).collect_view()}
                    <div class="flex flex-wrap gap-2">
                        <button class="btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" disabled=move || !can_edit.get() on:click=move |_| action.run(("undo".into(), String::new(), 0))>"Undo purchase"</button>
                        <button class="btn-secondary min-h-11 disabled:opacity-40 disabled:cursor-not-allowed" disabled= !view.has_next on:click=move |_| action.run(("next".into(), String::new(), 0))>"Next world"</button>
                    </div>
                </div>
            })}
            <details class="rounded-xl border border-white/10 p-4 text-sm">
                <summary class="cursor-pointer font-medium">"Compare routes and shopping details"</summary>
                <div class="space-y-3 pt-3">
                    {move || trip.get().map(|active| {
                        let savings = if active.alternatives[1].missing == active.alternatives[2].missing { format!("Lowest cost saves {} gil versus fewest stops.", (active.alternatives[1].cost - active.alternatives[2].cost).max(0)) } else { "Compare missing supply before comparing costs.".into() };
                        view! {
                            <div class="grid gap-2 sm:grid-cols-3">
                                {active.alternatives.iter().enumerate().map(|(mode, plan)| view! {
                                    <div class="rounded-lg border border-white/10 p-3">
                                        <strong>{["Home world", "Fewest stops", "Lowest cost"][mode]}</strong>
                                        <p>{format!("{} gil · {} stops", plan.cost, planner::itinerary(plan).len())}</p>
                                        <p>{format!("{} missing", plan.missing)}</p>
                                    </div>
                                }).collect_view()}
                            </div>
                            <p>{savings}</p>
                        }
                    })}
                    <p>{move || input.get().observed_at.map(|time| format!("Listings observed {time}. Estimates, not reservations.")).unwrap_or_else(|| "Price observation time unavailable. Look up prices when connected to refresh market data.".into())}</p>
                    <p>"Prices include whole stacks. Routes use the available listings; check travel availability in game. Travel fees are excluded."</p>
                    <p>"Record stacks in order for each item. Your trip stays in place as prices update. Replanning uses the latest loaded listings."</p>
                    <p>"Keep this page open for the companion. Its regular-window fallback is not always on top."</p>
                </div>
            </details>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ShopInput {
            title: "Raid supplies".into(),
            home_world: 1,
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
        }
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
            alternatives: plans,
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
            alternatives: plans,
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
}
