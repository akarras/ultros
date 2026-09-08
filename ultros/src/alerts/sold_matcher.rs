//! Pairs `listings/remove` events with `sales/add` events to infer that a
//! specific retainer's listing sold. Universalis never says which listing a
//! sale came from, so this is inference, tuned to miss a sale rather than
//! report one that did not happen. See
//! `docs/superpowers/specs/2026-09-07-retainer-sold-alert-design.md`.
//!
//! Events only queue; [`SoldMatcher::settle`] does the matching once a sale
//! has waited [`SETTLE_SECONDS`]. A viewer's upload publishes the sale and
//! the removals as separate websocket messages a few milliseconds apart, and
//! the removals themselves can span several messages, so matching on arrival
//! would let the first removal claim a sale that a second retainer's removal,
//! still in flight, makes ambiguous.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};

/// How long a removal or sale waits for its partner (both arrival orders
/// happen; the worst observed gap was 59 s).
pub(crate) const MATCH_WINDOW_SECONDS: i64 = 300;
/// How long a sale must have been pending before it is resolved, so every
/// removal from the same upload has arrived.
pub(crate) const SETTLE_SECONDS: i64 = 10;
/// A sale may carry a game timestamp slightly before the listing's
/// `lastReviewTime` because two different clients stamped them.
pub(crate) const CLOCK_SKEW_SECONDS: i64 = 60;
/// `sales/add` is mostly history backfill; anything older than this at
/// receipt is not something we can attribute to a live removal.
pub(crate) const MAX_SALE_AGE_SECONDS: i64 = 24 * 3600;

/// Everything a sale and a listing have in common.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SaleKey {
    pub world_id: i32,
    pub item_id: i32,
    pub hq: bool,
    pub price_per_unit: i32,
    pub quantity: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovedListing {
    pub key: SaleKey,
    pub retainer_id: i32,
    pub retainer_name: String,
    /// Universalis `lastReviewTime`: the retainer's last touch. A sale of
    /// this listing cannot predate it.
    pub listed_at: NaiveDateTime,
}

/// Reprice signature: the same retainer removing and re-adding the same
/// stack. Price is deliberately absent — that is what a reprice changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddedListing {
    pub world_id: i32,
    pub item_id: i32,
    pub hq: bool,
    pub quantity: i32,
    pub retainer_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedSale {
    pub key: SaleKey,
    pub sold_at: NaiveDateTime,
    pub buyer_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoldEvent {
    pub retainer_id: i32,
    pub retainer_name: String,
    pub key: SaleKey,
    pub sold_at: NaiveDateTime,
    pub buyer_name: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingRemoval {
    listing: RemovedListing,
    received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct PendingSale {
    sale: ObservedSale,
    received_at: DateTime<Utc>,
}

enum Resolution {
    /// No candidate removal yet; the sale stays pending for its partner.
    Wait,
    /// Candidates from different retainers: nobody can say whose sold.
    Ambiguous,
    /// Exactly one retainer among the candidates; consume this index.
    Consume(usize),
}

#[derive(Debug)]
pub(crate) struct SoldMatcher {
    owned: HashSet<i32>,
    window: TimeDelta,
    settle: TimeDelta,
    skew: TimeDelta,
    max_sale_age: TimeDelta,
    /// Removals from *every* retainer: an unowned removal with the same key
    /// is exactly what makes a sale ambiguous.
    pending_removals: HashMap<SaleKey, Vec<PendingRemoval>>,
    pending_sales: HashMap<SaleKey, Vec<PendingSale>>,
}

fn utc(naive: NaiveDateTime) -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
}

impl SoldMatcher {
    pub(crate) fn new(owned: HashSet<i32>) -> Self {
        Self {
            owned,
            window: TimeDelta::seconds(MATCH_WINDOW_SECONDS),
            settle: TimeDelta::seconds(SETTLE_SECONDS),
            skew: TimeDelta::seconds(CLOCK_SKEW_SECONDS),
            max_sale_age: TimeDelta::seconds(MAX_SALE_AGE_SECONDS),
            pending_removals: HashMap::new(),
            pending_sales: HashMap::new(),
        }
    }

    pub(crate) fn set_owned(&mut self, owned: HashSet<i32>) {
        self.owned = owned;
    }

    pub(crate) fn on_removed(&mut self, listing: RemovedListing, now: DateTime<Utc>) {
        self.pending_removals
            .entry(listing.key)
            .or_default()
            .push(PendingRemoval {
                listing,
                received_at: now,
            });
    }

    pub(crate) fn on_added(&mut self, added: AddedListing) {
        self.pending_removals.retain(|key, removals| {
            if key.world_id != added.world_id
                || key.item_id != added.item_id
                || key.hq != added.hq
                || key.quantity != added.quantity
            {
                return true;
            }
            removals.retain(|r| r.listing.retainer_id != added.retainer_id);
            !removals.is_empty()
        });
    }

    pub(crate) fn on_sale(&mut self, sale: ObservedSale, now: DateTime<Utc>) {
        if now - utc(sale.sold_at) > self.max_sale_age {
            return;
        }
        self.pending_sales
            .entry(sale.key)
            .or_default()
            .push(PendingSale {
                sale,
                received_at: now,
            });
    }

    /// Drop everything older than the window, then resolve every sale that
    /// has been pending long enough. Vectors are in arrival order, so index 0
    /// is the earliest received.
    pub(crate) fn settle(&mut self, now: DateTime<Utc>) -> Vec<SoldEvent> {
        self.expire(now);
        let keys: Vec<SaleKey> = self.pending_sales.keys().copied().collect();
        let mut fired = Vec::new();
        for key in keys {
            fired.extend(self.settle_key(key, now));
        }
        fired
    }

    fn expire(&mut self, now: DateTime<Utc>) {
        let window = self.window;
        self.pending_removals.retain(|_, v| {
            v.retain(|r| now - r.received_at <= window);
            !v.is_empty()
        });
        self.pending_sales.retain(|_, v| {
            v.retain(|s| now - s.received_at <= window);
            !v.is_empty()
        });
    }

    fn settle_key(&mut self, key: SaleKey, now: DateTime<Utc>) -> Vec<SoldEvent> {
        let mut fired = Vec::new();
        let mut index = 0;
        loop {
            let Some(sale) = self
                .pending_sales
                .get(&key)
                .and_then(|sales| sales.get(index).cloned())
            else {
                break;
            };
            if now - sale.received_at < self.settle {
                // Sales are in arrival order, so nothing after this one has
                // settled either.
                break;
            }
            match self.resolve(&key, &sale) {
                Resolution::Wait => index += 1,
                Resolution::Ambiguous => {
                    self.remove_sale(&key, index);
                }
                Resolution::Consume(removal_index) => {
                    self.remove_sale(&key, index);
                    let removal = self
                        .pending_removals
                        .get_mut(&key)
                        .map(|r| r.remove(removal_index))
                        .expect("candidate index came from this vec");
                    if self.owned.contains(&removal.listing.retainer_id) {
                        fired.push(SoldEvent {
                            retainer_id: removal.listing.retainer_id,
                            retainer_name: removal.listing.retainer_name,
                            key,
                            sold_at: sale.sale.sold_at,
                            buyer_name: sale.sale.buyer_name,
                        });
                    }
                }
            }
        }
        if self.pending_sales.get(&key).is_some_and(Vec::is_empty) {
            self.pending_sales.remove(&key);
        }
        if self.pending_removals.get(&key).is_some_and(Vec::is_empty) {
            self.pending_removals.remove(&key);
        }
        fired
    }

    fn remove_sale(&mut self, key: &SaleKey, index: usize) {
        if let Some(sales) = self.pending_sales.get_mut(key)
            && index < sales.len()
        {
            sales.remove(index);
        }
    }

    fn resolve(&self, key: &SaleKey, sale: &PendingSale) -> Resolution {
        let Some(removals) = self.pending_removals.get(key) else {
            return Resolution::Wait;
        };
        let sold_at = utc(sale.sale.sold_at);
        let mut candidates = removals.iter().enumerate().filter(|(_, r)| {
            let gap = (sale.received_at - r.received_at).abs();
            gap <= self.window && utc(r.listing.listed_at) <= sold_at + self.skew
        });
        let Some((first_index, first)) = candidates.next() else {
            return Resolution::Wait;
        };
        let retainer = first.listing.retainer_id;
        if candidates.any(|(_, r)| r.listing.retainer_id != retainer) {
            return Resolution::Ambiguous;
        }
        Resolution::Consume(first_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + secs, 0).unwrap()
    }

    /// A settle instant comfortably after events queued around `t(0..3)`.
    const SETTLED: i64 = 20;

    fn key() -> SaleKey {
        SaleKey {
            world_id: 34,
            item_id: 5,
            hq: false,
            price_per_unit: 100,
            quantity: 99,
        }
    }

    fn removed(retainer_id: i32, listed_secs_ago: i64, now: DateTime<Utc>) -> RemovedListing {
        RemovedListing {
            key: key(),
            retainer_id,
            retainer_name: format!("Retainer{retainer_id}"),
            listed_at: (now - TimeDelta::seconds(listed_secs_ago)).naive_utc(),
        }
    }

    fn sale_at(sold: DateTime<Utc>) -> ObservedSale {
        ObservedSale {
            key: key(),
            sold_at: sold.naive_utc(),
            buyer_name: Some("Buyer".into()),
        }
    }

    fn added(retainer_id: i32, quantity: i32) -> AddedListing {
        AddedListing {
            world_id: 34,
            item_id: 5,
            hq: false,
            quantity,
            retainer_id,
        }
    }

    fn matcher(owned: &[i32]) -> SoldMatcher {
        SoldMatcher::new(owned.iter().copied().collect())
    }

    #[test]
    fn removal_then_matching_sale_fires_once_for_the_owner() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_sale(sale_at(t(-5)), t(1));
        let fired = m.settle(t(SETTLED));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].retainer_id, 1);
        assert_eq!(fired[0].retainer_name, "Retainer1");
        assert_eq!(fired[0].key, key());
        assert_eq!(fired[0].buyer_name.as_deref(), Some("Buyer"));
        // Consumed: a second identical sale finds nothing.
        m.on_sale(sale_at(t(-4)), t(SETTLED));
        assert!(m.settle(t(2 * SETTLED)).is_empty());
    }

    #[test]
    fn sale_then_removal_fires_once() {
        let mut m = matcher(&[1]);
        m.on_sale(sale_at(t(-5)), t(0));
        m.on_removed(removed(1, 600, t(3)), t(3));
        assert_eq!(m.settle(t(SETTLED)).len(), 1);
    }

    #[test]
    fn nothing_fires_before_the_sale_has_settled() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_sale(sale_at(t(-5)), t(0));
        assert!(m.settle(t(SETTLE_SECONDS - 1)).is_empty());
        assert_eq!(m.settle(t(SETTLE_SECONDS)).len(), 1);
    }

    #[test]
    fn a_collision_arriving_after_the_sale_still_blocks_it() {
        // The sale lands first, then one removal, then — in a separate
        // message a moment later — a second retainer's removal. Matching on
        // arrival would have let retainer 1 claim the sale.
        let mut m = matcher(&[1, 2]);
        m.on_sale(sale_at(t(-5)), t(0));
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_removed(removed(2, 600, t(1)), t(1));
        assert!(m.settle(t(SETTLED)).is_empty());
    }

    #[test]
    fn unowned_retainer_never_fires_but_still_consumes() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(2, 600, t(0)), t(0));
        m.on_sale(sale_at(t(-5)), t(1));
        assert!(m.settle(t(SETTLED)).is_empty());
        // The sale was consumed by retainer 2, so an owned removal arriving
        // afterwards has nothing to pair with.
        m.on_removed(removed(1, 600, t(SETTLED)), t(SETTLED));
        assert!(m.settle(t(2 * SETTLED)).is_empty());
    }

    #[test]
    fn reprice_by_same_retainer_suppresses() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_added(added(1, 99));
        m.on_sale(sale_at(t(-5)), t(1));
        assert!(m.settle(t(SETTLED)).is_empty());
    }

    #[test]
    fn add_from_a_different_retainer_is_not_a_reprice() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_added(added(7, 99));
        m.on_sale(sale_at(t(-5)), t(1));
        assert_eq!(m.settle(t(SETTLED)).len(), 1);
    }

    #[test]
    fn add_of_a_different_quantity_is_not_a_reprice() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_added(added(1, 50));
        m.on_sale(sale_at(t(-5)), t(1));
        assert_eq!(m.settle(t(SETTLED)).len(), 1);
    }

    #[test]
    fn cross_retainer_collision_drops_the_sale() {
        let mut m = matcher(&[1, 2]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_removed(removed(2, 600, t(0)), t(0));
        m.on_sale(sale_at(t(-5)), t(1));
        assert!(m.settle(t(SETTLED)).is_empty());
        // The removals stay, so a second sale is just as ambiguous.
        m.on_sale(sale_at(t(-4)), t(SETTLED));
        assert!(m.settle(t(2 * SETTLED)).is_empty());
    }

    #[test]
    fn three_listings_one_retainer_two_sales_fire_twice() {
        let mut m = matcher(&[1]);
        for _ in 0..3 {
            m.on_removed(removed(1, 600, t(0)), t(0));
        }
        m.on_sale(sale_at(t(-5)), t(1));
        m.on_sale(sale_at(t(-4)), t(1));
        assert_eq!(m.settle(t(SETTLED)).len(), 2);
        // The third listing is still pending; one more sale fires, a fourth
        // has no partner.
        m.on_sale(sale_at(t(-3)), t(SETTLED));
        m.on_sale(sale_at(t(-2)), t(SETTLED));
        assert_eq!(m.settle(t(2 * SETTLED)).len(), 1);
    }

    #[test]
    fn stale_sale_is_ignored() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 200_000, t(0)), t(0));
        // 25 hours old at receipt.
        m.on_sale(sale_at(t(-25 * 3600)), t(0));
        assert!(m.settle(t(SETTLED)).is_empty());
    }

    #[test]
    fn sale_before_listing_was_touched_is_not_a_match() {
        let mut m = matcher(&[1]);
        // Listing last reviewed 10 s ago; sale happened 5 minutes ago.
        m.on_removed(removed(1, 10, t(0)), t(0));
        m.on_sale(sale_at(t(-300)), t(0));
        assert!(m.settle(t(SETTLED)).is_empty());
        // Within the 60 s skew it still matches.
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 10, t(0)), t(0));
        m.on_sale(sale_at(t(-40)), t(0));
        assert_eq!(m.settle(t(SETTLED)).len(), 1);
    }

    #[test]
    fn a_too_new_removal_does_not_count_as_a_collision() {
        // Retainer 2 listed after the sale happened, so it cannot have been
        // the seller and must not make retainer 1's sale ambiguous.
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_removed(removed(2, 1, t(0)), t(0));
        m.on_sale(sale_at(t(-300)), t(0));
        assert_eq!(m.settle(t(SETTLED)).len(), 1);
    }

    #[test]
    fn removal_outside_window_does_not_pair() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_sale(sale_at(t(300)), t(301));
        assert!(m.settle(t(301 + SETTLED)).is_empty());
    }

    #[test]
    fn expiry_drops_both_sides() {
        let mut m = matcher(&[1]);
        let other = SaleKey {
            price_per_unit: 200,
            ..key()
        };
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_sale(
            ObservedSale {
                key: other,
                sold_at: t(-5).naive_utc(),
                buyer_name: None,
            },
            t(0),
        );
        // Both sit unmatched past the window and are dropped, so partners
        // arriving afterwards find nothing.
        assert!(m.settle(t(301)).is_empty());
        m.on_sale(sale_at(t(-5)), t(302));
        m.on_removed(
            RemovedListing {
                key: other,
                ..removed(1, 600, t(302))
            },
            t(302),
        );
        assert!(m.settle(t(302 + SETTLED)).is_empty());
    }

    #[test]
    fn set_owned_takes_effect_for_later_matches() {
        let mut m = matcher(&[]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_sale(sale_at(t(-5)), t(0));
        m.set_owned([1].into_iter().collect());
        assert_eq!(m.settle(t(SETTLED)).len(), 1);
    }

    /// Replays a trimmed slice of the 2026-09-07 websocket capture. Boards
    /// were chosen by hand: two clean owned pairs (fire), one same-retainer
    /// reprice (no fire), one cross-retainer collision where the sale
    /// arrived before both removals (no fire), one stale-history board (no
    /// fire). Receipt times come from the capture's `t` field; the matcher
    /// is settled after every event and once more at the end.
    #[test]
    fn replay_capture_fires_only_for_clean_owned_pairs() {
        // Universalis retainer ids are decimal strings wider than i32; the
        // fixture truncates them, which is fine because ids are only ever
        // compared for equality.
        const OWNED: &[i32] = &[-2013938895, -604218276, 1247490289, 881058229];
        const EXPECTED_FIRES: usize = 2;
        let raw = include_str!("../../test_data/sold_replay.jsonl");
        let mut m = SoldMatcher::new(OWNED.iter().copied().collect());
        let retainer = |l: &serde_json::Value| {
            l["retainer_id"].as_str().unwrap().parse::<i64>().unwrap() as i32
        };
        // The listener settles from a 5 s tick; replay those ticks between
        // events so a sale that waits a minute for its removal (board 1042)
        // is resolved before it expires, exactly as in production.
        let tick = TimeDelta::seconds(5);
        let mut next_tick: Option<DateTime<Utc>> = None;
        let mut fired = Vec::new();
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            let now = Utc
                .timestamp_opt(v["t"].as_f64().unwrap() as i64, 0)
                .unwrap();
            let mut at = next_tick.unwrap_or(now);
            while at <= now {
                fired.extend(m.settle(at));
                at += tick;
            }
            next_tick = Some(at);
            let world_id = v["world"].as_i64().unwrap() as i32;
            let item_id = v["item"].as_i64().unwrap() as i32;
            match v["event"].as_str().unwrap() {
                "listings/remove" => {
                    for l in v["listings"].as_array().unwrap() {
                        m.on_removed(
                            RemovedListing {
                                key: SaleKey {
                                    world_id,
                                    item_id,
                                    hq: l["hq"].as_bool().unwrap(),
                                    price_per_unit: l["ppu"].as_i64().unwrap() as i32,
                                    quantity: l["qty"].as_i64().unwrap() as i32,
                                },
                                retainer_id: retainer(l),
                                retainer_name: l["retainer"].as_str().unwrap_or("").to_string(),
                                listed_at: Utc
                                    .timestamp_opt(l["last_review"].as_i64().unwrap_or(0), 0)
                                    .unwrap()
                                    .naive_utc(),
                            },
                            now,
                        );
                    }
                }
                "listings/add" => {
                    for l in v["listings"].as_array().unwrap() {
                        m.on_added(AddedListing {
                            world_id,
                            item_id,
                            hq: l["hq"].as_bool().unwrap(),
                            quantity: l["qty"].as_i64().unwrap() as i32,
                            retainer_id: retainer(l),
                        });
                    }
                }
                "sales/add" => {
                    for s in v["sales"].as_array().unwrap() {
                        m.on_sale(
                            ObservedSale {
                                key: SaleKey {
                                    world_id,
                                    item_id,
                                    hq: s["hq"].as_bool().unwrap(),
                                    price_per_unit: s["ppu"].as_i64().unwrap() as i32,
                                    quantity: s["qty"].as_i64().unwrap() as i32,
                                },
                                sold_at: Utc
                                    .timestamp_opt(s["ts"].as_i64().unwrap(), 0)
                                    .unwrap()
                                    .naive_utc(),
                                buyer_name: s["buyer"].as_str().map(str::to_string),
                            },
                            now,
                        );
                    }
                }
                other => panic!("unexpected event {other}"),
            }
        }
        let mut at = next_tick.unwrap();
        for _ in 0..4 {
            fired.extend(m.settle(at));
            at += tick;
        }
        assert_eq!(fired.len(), EXPECTED_FIRES, "{fired:?}");
        let worlds: HashSet<i32> = fired.iter().map(|f| f.key.world_id).collect();
        let expected: HashSet<i32> = [1177, 1042].into_iter().collect();
        assert_eq!(worlds, expected);
    }
}
