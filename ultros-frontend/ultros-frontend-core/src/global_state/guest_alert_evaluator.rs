//! Pure client-side matcher for guest price-alert rules against realtime
//! listing events.
//!
//! [`evaluate_rules`] is the counterpart, for a guest without an account, of
//! the server's `price_alert_tracker` — both ultimately call the same
//! [`threshold_listing_matches`] predicate so a rule behaves identically
//! whether it lives in `localStorage` or in the `alert` table. The reactive
//! wiring (websocket subscription, `Inbox`/`GuestAlerts` mutation, toast +
//! browser notification) lives in
//! [`crate::components::guest_alert_evaluator`] — this module only holds the
//! logic that's worth testing without a reactive runtime.

use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use ultros_api_types::ActiveListing;
use ultros_api_types::alert::threshold_listing_matches;
use ultros_api_types::websocket::ListingEventData;
use ultros_api_types::world_helper::WorldHelper;

use crate::global_state::guest_alerts::{GuestAlertRule, cooldown_elapsed};

/// Ring buffer of the most recently fired `"{rule_id}:{listing_id}"` keys,
/// so a listing that fires an `Added` event and then an `Updated` event for
/// the same underlying row (common right after the initial subscribe
/// handshake replays current state) doesn't produce two inbox hits.
///
/// Capacity is fixed at 256: generous for the number of distinct
/// rule/listing pairs a guest's rule set plausibly matches in one realtime
/// session, small enough that a browser tab never notices the memory.
pub struct DedupeWindow {
    seen: VecDeque<String>,
    capacity: usize,
}

impl DedupeWindow {
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            seen: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Returns `true` and records `key` if it hasn't been seen before;
    /// returns `false` (and leaves the window untouched) if it has.
    fn insert_if_new(&mut self, key: String) -> bool {
        if self.seen.contains(&key) {
            return false;
        }
        if self.seen.len() >= self.capacity {
            self.seen.pop_front();
        }
        self.seen.push_back(key);
        true
    }
}

impl Default for DedupeWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// One guest rule firing against a specific listing.
#[derive(Clone, Debug, PartialEq)]
pub struct GuestHit {
    pub rule_id: String,
    pub listing: ActiveListing,
}

fn dedupe_key(rule_id: &str, listing_id: i32) -> String {
    format!("{rule_id}:{listing_id}")
}

/// Evaluates every enabled rule in `rules` against the listings in `event`,
/// returning at most one [`GuestHit`] per rule.
///
/// For each enabled rule whose `item_id` matches `event.item_id` and that is
/// off cooldown, every listing in `event.listings` is checked with
/// [`threshold_listing_matches`]; among the listings that match, the one
/// with the lowest `price_per_unit` wins (so a guest sees the best available
/// deal, not just the first one the server happened to list). A hit whose
/// `"{rule_id}:{listing_id}"` key is already in `dedupe` is skipped and the
/// window is left untouched for that key (it was already recorded on the
/// firing that put it there); every new hit's key is recorded before it's
/// returned.
///
/// The caller is expected to only pass `Added`/`Updated` events — a listing
/// `Removed` from the market can't be "matched" against.
pub fn evaluate_rules(
    rules: &[GuestAlertRule],
    event: &ListingEventData,
    worlds: &WorldHelper,
    now: DateTime<Utc>,
    dedupe: &mut DedupeWindow,
) -> Vec<GuestHit> {
    let mut hits = Vec::new();

    for rule in rules {
        if !rule.enabled || rule.item_id != event.item_id {
            continue;
        }
        if !cooldown_elapsed(rule.last_fired_at, rule.cooldown_seconds, now) {
            continue;
        }

        let threshold_rule = rule.to_threshold_rule();
        let best = event
            .listings
            .iter()
            .map(|(listing, _retainer)| listing)
            .filter(|listing| threshold_listing_matches(&threshold_rule, listing, worlds, now))
            .min_by_key(|listing| listing.price_per_unit);

        let Some(listing) = best else {
            continue;
        };

        let key = dedupe_key(&rule.id, listing.id);
        if !dedupe.insert_if_new(key) {
            continue;
        }

        hits.push(GuestHit {
            rule_id: rule.id.clone(),
            listing: listing.clone(),
        });
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::retainer::Retainer;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};
    use ultros_api_types::world_helper::AnySelector;

    fn worlds() -> WorldHelper {
        WorldData {
            regions: vec![Region {
                id: 1,
                name: "NA".into(),
                datacenters: vec![
                    Datacenter {
                        id: 10,
                        name: "Aether".into(),
                        region_id: 1,
                        worlds: vec![
                            World {
                                id: 100,
                                name: "Adamantoise".into(),
                                datacenter_id: 10,
                            },
                            World {
                                id: 101,
                                name: "Cactuar".into(),
                                datacenter_id: 10,
                            },
                        ],
                    },
                    Datacenter {
                        id: 20,
                        name: "Crystal".into(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 200,
                            name: "Balmung".into(),
                            datacenter_id: 20,
                        }],
                    },
                ],
            }],
        }
        .into()
    }

    fn t(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH + chrono::Duration::seconds(seconds)
    }

    fn rule(id: &str, item_id: i32, price_threshold: i32, enabled: bool) -> GuestAlertRule {
        GuestAlertRule {
            id: id.to_string(),
            item_id,
            world_selector: AnySelector::World(100),
            price_threshold,
            hq_only: false,
            cooldown_seconds: 3600,
            last_fired_at: None,
            enabled,
            created_at: t(0),
        }
    }

    fn listing(id: i32, world_id: i32, item_id: i32, price: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id,
            world_id,
            item_id,
            retainer_id: 7,
            price_per_unit: price,
            quantity: 1,
            hq,
            timestamp: chrono::NaiveDateTime::default(),
        }
    }

    fn retainer(world_id: i32) -> Retainer {
        Retainer {
            id: 7,
            world_id,
            name: "Bob".into(),
            retainer_city_id: 1,
        }
    }

    fn event(item_id: i32, world_id: i32, listings: Vec<ActiveListing>) -> ListingEventData {
        ListingEventData {
            item_id,
            world_id,
            listings: listings
                .into_iter()
                .map(|l| {
                    let r = retainer(l.world_id);
                    (l, r)
                })
                .collect(),
        }
    }

    #[test]
    fn hit_when_price_at_or_below_threshold_on_matching_world() {
        let w = worlds();
        let rules = vec![rule("r1", 42, 1000, true)];
        let ev = event(42, 100, vec![listing(1, 100, 42, 900, false)]);
        let mut dedupe = DedupeWindow::new();
        let hits = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rule_id, "r1");
        assert_eq!(hits[0].listing.id, 1);
    }

    #[test]
    fn no_hit_on_other_datacenter_world() {
        let w = worlds();
        // r1's selector is World(100), which is in the Aether datacenter;
        // a listing on Balmung (Crystal datacenter) must not match.
        let rules = vec![rule("r1", 42, 1000, true)];
        let ev = event(42, 200, vec![listing(1, 200, 42, 500, false)]);
        let mut dedupe = DedupeWindow::new();
        let hits = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert!(hits.is_empty());
    }

    #[test]
    fn hq_only_skips_nq_listings() {
        let w = worlds();
        let mut r = rule("r1", 42, 1000, true);
        r.hq_only = true;
        let ev = event(
            42,
            100,
            vec![
                listing(1, 100, 42, 500, false),
                listing(2, 100, 42, 600, true),
            ],
        );
        let mut dedupe = DedupeWindow::new();
        let hits = evaluate_rules(&[r], &ev, &w, t(1000), &mut dedupe);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].listing.id, 2);
    }

    #[test]
    fn cooldown_suppresses_second_hit() {
        let w = worlds();
        let mut r = rule("r1", 42, 1000, true);
        r.last_fired_at = Some(t(1000) - chrono::Duration::seconds(60));
        let ev = event(42, 100, vec![listing(1, 100, 42, 500, false)]);
        let mut dedupe = DedupeWindow::new();
        let hits = evaluate_rules(&[r], &ev, &w, t(1000), &mut dedupe);
        assert!(hits.is_empty());
    }

    #[test]
    fn lowest_price_listing_wins_per_rule() {
        let w = worlds();
        let rules = vec![rule("r1", 42, 1000, true)];
        let ev = event(
            42,
            100,
            vec![
                listing(1, 100, 42, 900, false),
                listing(2, 100, 42, 500, false),
                listing(3, 100, 42, 700, false),
            ],
        );
        let mut dedupe = DedupeWindow::new();
        let hits = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].listing.id, 2);
        assert_eq!(hits[0].listing.price_per_unit, 500);
    }

    #[test]
    fn dedupe_window_blocks_added_then_updated_same_listing() {
        let w = worlds();
        let rules = vec![rule("r1", 42, 1000, true)];
        let ev = event(42, 100, vec![listing(1, 100, 42, 500, false)]);
        let mut dedupe = DedupeWindow::new();

        let first = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert_eq!(first.len(), 1);

        // Same rule, same listing id, arriving again as an Updated event —
        // must not fire twice.
        let second = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert!(second.is_empty());
    }

    #[test]
    fn disabled_rule_never_fires() {
        let w = worlds();
        let rules = vec![rule("r1", 42, 1000, false)];
        let ev = event(42, 100, vec![listing(1, 100, 42, 500, false)]);
        let mut dedupe = DedupeWindow::new();
        let hits = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert!(hits.is_empty());
    }

    #[test]
    fn dedupe_window_evicts_oldest_past_capacity() {
        let mut dedupe = DedupeWindow::with_capacity(2);
        assert!(dedupe.insert_if_new("a".to_string()));
        assert!(dedupe.insert_if_new("b".to_string()));
        // Capacity 2, both "a" and "b" now recorded: window is [a, b].
        assert!(!dedupe.insert_if_new("a".to_string()));
        assert!(!dedupe.insert_if_new("b".to_string()));

        // Inserting "c" evicts the oldest ("a"): window becomes [b, c].
        assert!(dedupe.insert_if_new("c".to_string()));
        // "b" is still in the window, so it stays blocked (and this check
        // does not itself mutate the window, since it's already a dup).
        assert!(!dedupe.insert_if_new("b".to_string()));
        // "a" was evicted, so it's allowed to fire again.
        assert!(dedupe.insert_if_new("a".to_string()));
    }
}
