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
//!
//! [`DedupeWindow`] only guards one within-one-fire-window race: two tabs
//! for the same guest can both evaluate the same realtime batch and both
//! decide a rule fires before either has written the new `last_fired_at`
//! back to `GuestAlerts` (that write only happens in the reactive component,
//! not here). This is an accepted, unguarded race at this layer —
//! [`crate::global_state::notifications::merge_inbox`] collapses the
//! resulting duplicate `InboxItem`s on display by their shared
//! `"{rule_id}:{listing_id}"` `source_key`, same as it does for a local hit
//! and its later server-persisted counterpart.

use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use ultros_api_types::ActiveListing;
use ultros_api_types::alert::threshold_listing_matches;
use ultros_api_types::websocket::ListingEventData;
use ultros_api_types::world_helper::WorldHelper;

use crate::global_state::guest_alerts::{GuestAlertRule, cooldown_elapsed};

/// Ring buffer of the most recently fired `"{rule_id}:{listing_id}:{last_fired_at}"`
/// keys, so a listing that fires an `Added` event and then an `Updated`
/// event for the same underlying row within one fire window doesn't produce
/// two inbox hits.
///
/// Including the rule's `last_fired_at` (at the moment it fired) in the key,
/// rather than just `"{rule_id}:{listing_id}"`, matters once the rule goes
/// off cooldown again: `last_fired_at` has since moved forward, so the key
/// for the *same* listing is now different and the rule is free to fire on
/// it again — a plain `rule_id:listing_id` key would otherwise permanently
/// block that one listing for the rest of the session, even though the
/// server-side equivalent would fire on it every time the rule comes off
/// cooldown.
///
/// Capacity is fixed at 256: generous for the number of distinct
/// rule/listing/fire-window keys a guest's rule set plausibly matches in one
/// realtime session, small enough that a browser tab never notices the
/// memory.
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

    /// Returns whether `key` is currently recorded in the window.
    fn contains(&self, key: &str) -> bool {
        self.seen.iter().any(|seen| seen == key)
    }

    /// Records `key` unconditionally, evicting the oldest entry first once
    /// at capacity. Callers that care about not re-recording an
    /// already-present key should check [`contains`][Self::contains] first —
    /// [`evaluate_rules`] does exactly that, checking every candidate
    /// listing's key up front and only recording the eventual winner's.
    fn insert(&mut self, key: String) {
        if self.seen.len() >= self.capacity {
            self.seen.pop_front();
        }
        self.seen.push_back(key);
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

/// Builds this rule/listing pairing's dedupe key as of `last_fired_at` (the
/// rule's `last_fired_at` at evaluation time — `None` before it has ever
/// fired). See the [`DedupeWindow`] docs for why `last_fired_at` is part of
/// the key.
fn dedupe_key(rule_id: &str, listing_id: i64, last_fired_at: Option<DateTime<Utc>>) -> String {
    match last_fired_at {
        Some(last_fired_at) => format!(
            "{rule_id}:{listing_id}:{}",
            last_fired_at.timestamp_millis()
        ),
        None => format!("{rule_id}:{listing_id}:none"),
    }
}

/// Evaluates every enabled rule in `rules` against the listings in `event`,
/// returning at most one [`GuestHit`] per rule.
///
/// For each enabled rule whose `item_id` matches `event.item_id` and that is
/// off cooldown, every listing in `event.listings` is checked with
/// [`threshold_listing_matches`], and any listing whose dedupe key is
/// already recorded in `dedupe` is excluded *before* picking a winner — so a
/// listing that already produced a hit this fire window doesn't block a
/// different, still-matching listing from firing. Among what's left, the one
/// with the lowest `price_per_unit` wins (so a guest sees the best available
/// deal, not just the first one the server happened to list). The winning
/// listing's key is then recorded in `dedupe`.
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
            .filter(|listing| {
                !dedupe.contains(&dedupe_key(&rule.id, listing.id, rule.last_fired_at))
            })
            .min_by_key(|listing| listing.price_per_unit);

        let Some(listing) = best else {
            continue;
        };

        dedupe.insert(dedupe_key(&rule.id, listing.id, rule.last_fired_at));

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

    fn listing(id: i64, world_id: i32, item_id: i32, price: i32, hq: bool) -> ActiveListing {
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

        // Same rule, same listing id, `last_fired_at` unchanged (nothing in
        // this test calls `touch_fired` between the two evaluations) —
        // arriving again as an Updated event within the same fire window
        // must not fire twice: the key (which now also includes
        // `last_fired_at`) is identical on both calls.
        let second = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        assert!(second.is_empty());
    }

    #[test]
    fn off_cooldown_same_cheapest_listing_fires_again() {
        let w = worlds();
        let mut r = rule("r1", 42, 1000, true);
        // Off cooldown as of `now` (fired long enough ago).
        // The rule already fired once, long enough ago that it's off
        // cooldown again as of `now`.
        let previous_fire = t(1000) - chrono::Duration::seconds(7200);
        r.last_fired_at = Some(previous_fire);
        let ev = event(42, 100, vec![listing(1, 100, 42, 500, false)]);

        let mut dedupe = DedupeWindow::new();
        // The ring holds the key from *that* firing, computed with the
        // rule's `last_fired_at` as it stood right before it fired — here,
        // its very first fire, so `None`. `touch_fired` only updates
        // `last_fired_at` to `previous_fire` afterwards, in the reactive
        // component that calls `evaluate_rules`, not inside this function.
        dedupe.insert(dedupe_key("r1", 1, None));

        // A plain `rule_id:listing_id` key would still find that entry and
        // block this fire forever; because the key also carries the rule's
        // *current* `last_fired_at` (now `previous_fire`, not `None`), this
        // is a fresh key and the off-cooldown rule is free to fire again on
        // the very same listing.
        let hits = evaluate_rules(&[r], &ev, &w, t(1000), &mut dedupe);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].listing.id, 1);
    }

    #[test]
    fn deduped_min_listing_does_not_block_other_matching_listing() {
        let w = worlds();
        let rules = vec![rule("r1", 42, 1000, true)];
        let ev = event(
            42,
            100,
            vec![
                listing(1, 100, 42, 500, false), // cheapest, already fired
                listing(2, 100, 42, 700, false), // also matches, pricier
            ],
        );
        let mut dedupe = DedupeWindow::new();
        // Listing 1 (the cheapest match) already fired this window.
        dedupe.insert(dedupe_key("r1", 1, None));

        let hits = evaluate_rules(&rules, &ev, &w, t(1000), &mut dedupe);
        // Listing 1 is excluded before the price comparison, so listing 2 —
        // the next-cheapest still-matching listing — wins instead of the
        // rule being skipped entirely because its overall cheapest match had
        // already fired.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].listing.id, 2);
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
        // `evaluate_rules` never re-records an already-`contains`ed key, so
        // exercise that same check-then-insert pairing here directly.
        fn try_insert(dedupe: &mut DedupeWindow, key: &str) -> bool {
            if dedupe.contains(key) {
                return false;
            }
            dedupe.insert(key.to_string());
            true
        }

        let mut dedupe = DedupeWindow::with_capacity(2);
        assert!(try_insert(&mut dedupe, "a"));
        assert!(try_insert(&mut dedupe, "b"));
        // Capacity 2, both "a" and "b" now recorded: window is [a, b].
        assert!(!try_insert(&mut dedupe, "a"));
        assert!(!try_insert(&mut dedupe, "b"));

        // Inserting "c" evicts the oldest ("a"): window becomes [b, c].
        assert!(try_insert(&mut dedupe, "c"));
        // "b" is still in the window, so it stays blocked (and this check
        // does not itself mutate the window, since it's already a dup).
        assert!(!try_insert(&mut dedupe, "b"));
        // "a" was evicted, so it's allowed to fire again.
        assert!(try_insert(&mut dedupe, "a"));
    }
}
