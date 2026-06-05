use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude;
use tokio::sync::Mutex;
use tracing::{error, info, instrument, warn};
use ultros_api_types::{
    ActiveListing,
    websocket::{ListEventData, ListingEventData},
    world_helper::AnySelector as ApiAnySelector,
};
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_item_threshold, alert_list_threshold},
    world_data::world_cache::{AnySelector as DbAnySelector, WorldCache},
};

use crate::alerts::delivery::dispatch_alert;
use crate::event::{EventBus, EventType};

/// True when an alert with the given `last_fired_at` is free to fire again given `cooldown_seconds`
/// as of the reference timestamp `now`. `None` (never fired) is always off cooldown.
pub(crate) fn is_off_cooldown_at(
    last_fired_at: Option<DateTime<Utc>>,
    cooldown_seconds: i32,
    now: DateTime<Utc>,
) -> bool {
    match last_fired_at {
        None => true,
        Some(t) => now.signed_duration_since(t).num_seconds() >= cooldown_seconds as i64,
    }
}

/// Returns true if `listing` satisfies every condition of `rule` and the rule is off
/// cooldown at `now`. Pure: no DB calls, no `Utc::now()`.
pub(crate) fn rule_matches_listing(
    rule: &ActiveRule,
    listing: &ActiveListing,
    now: DateTime<Utc>,
) -> bool {
    if !rule.world_id_set.contains(&listing.world_id) {
        return false;
    }
    if rule.hq_only && !listing.hq {
        return false;
    }
    if listing.price_per_unit > rule.price_threshold {
        return false;
    }
    is_off_cooldown_at(rule.last_fired_at, rule.cooldown_seconds, now)
}

/// Build the Discord embed title + body for a threshold-alert firing. Pure.
pub(crate) fn format_threshold_alert_message(
    item_name: &str,
    item_id: i32,
    matched_price: i32,
    price_threshold: i32,
) -> (String, String) {
    let title = format!("🎯 {item_name} dropped to {matched_price} gil");
    let body = format!("Threshold: {price_threshold} gil\nhttps://ultros.app/item/{item_id}");
    (title, body)
}

/// Build the Discord embed title + body for a list-threshold alert firing. Pure.
/// Format mirrors the item-threshold one so the delivery shape stays familiar,
/// but the title is prefixed with the list name to disambiguate when a user
/// subscribes to multiple lists.
pub(crate) fn format_list_threshold_alert_message(
    list_name: &str,
    list_id: i32,
    item_name: &str,
    item_id: i32,
    matched_price: i32,
    target_price: i64,
) -> (String, String) {
    let title = format!("📋 {list_name}: {item_name} at {matched_price} gil");
    let body = format!(
        "Target: {target_price} gil\nItem: https://ultros.app/item/{item_id}\nList: https://ultros.app/list/{list_id}"
    );
    (title, body)
}

/// Returns true if `listing` satisfies every condition of `rule` and the rule is
/// off cooldown at `now`. Mirrors `rule_matches_listing` but for list-scoped
/// rules. v1 ignores HQ (lists may carry hq=Some(true), but the trigger fires
/// on any listing that meets the price target — documented behavior).
pub(crate) fn list_rule_matches_listing(
    rule: &ListActiveRule,
    listing: &ActiveListing,
    now: DateTime<Utc>,
) -> bool {
    if !rule.world_id_set.contains(&listing.world_id) {
        return false;
    }
    if (listing.price_per_unit as i64) > rule.target_price {
        return false;
    }
    is_off_cooldown_at(rule.last_fired_at, rule.cooldown_seconds, now)
}

/// Look up an item's name in the embedded xiv-gen data, falling back to `"Item {id}"` if missing.
pub(crate) fn resolve_item_name(item_id: i32) -> String {
    xiv_gen_db::data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| format!("Item {item_id}"))
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRule {
    pub(crate) alert_id: i32,
    pub(crate) item_id: i32,
    pub(crate) price_threshold: i32,
    pub(crate) hq_only: bool,
    pub(crate) cooldown_seconds: i32,
    pub(crate) last_fired_at: Option<DateTime<Utc>>,
    /// Pre-resolved set of world IDs this rule applies to.
    pub(crate) world_id_set: HashSet<i32>,
}

/// A pre-computed (alert, list_item) pair the price-alert tracker fires when a
/// listing meets the per-row `target_price`. One per (list-threshold alert ×
/// list_item-with-target). The list's name and the precomputed world id set
/// are folded in so the dispatch path stays O(1).
#[derive(Debug, Clone)]
pub(crate) struct ListActiveRule {
    pub(crate) alert_id: i32,
    pub(crate) list_id: i32,
    pub(crate) item_id: i32,
    pub(crate) target_price: i64,
    pub(crate) cooldown_seconds: i32,
    pub(crate) last_fired_at: Option<DateTime<Utc>>,
    pub(crate) world_id_set: HashSet<i32>,
    pub(crate) list_name: String,
}

#[derive(Debug, Default)]
struct TrackerState {
    by_item: HashMap<i32, Vec<ActiveRule>>,
    /// Same shape as `by_item` but for list-scoped alerts. Each
    /// (item_id) -> Vec<ListActiveRule> entry is one row per (alert × priced
    /// list_item) so the incoming-listing path doesn't have to do any DB
    /// queries.
    by_item_list_rules: HashMap<i32, Vec<ListActiveRule>>,
}

impl TrackerState {
    fn refresh_from(
        &mut self,
        alerts: &[(alert::Model, alert_item_threshold::Model)],
        world_cache: &WorldCache,
    ) {
        self.by_item.clear();
        for (a, t) in alerts {
            if !a.enabled {
                continue;
            }
            // Deserialize and resolve the world_selector to a flat set of world IDs.
            let world_id_set: HashSet<i32> =
                match serde_json::from_value::<ApiAnySelector>(t.world_selector.clone()) {
                    Ok(api_selector) => {
                        let selector: ultros_db::world_data::world_cache::AnySelector =
                            api_selector.into();
                        match world_cache.lookup_selector(&selector) {
                            Ok(result) => world_cache
                                .get_all_worlds_in(&result)
                                .unwrap_or_default()
                                .into_iter()
                                .collect(),
                            Err(e) => {
                                warn!(
                                    alert_id = a.id,
                                    "could not resolve world_selector for alert: {e}"
                                );
                                HashSet::new()
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            alert_id = a.id,
                            "could not deserialize world_selector for alert: {e}"
                        );
                        HashSet::new()
                    }
                };
            self.by_item.entry(t.item_id).or_default().push(ActiveRule {
                alert_id: a.id,
                item_id: t.item_id,
                price_threshold: t.price_threshold,
                hq_only: t.hq_only,
                cooldown_seconds: a.cooldown_seconds,
                last_fired_at: a.last_fired_at.map(|dt| dt.with_timezone(&Utc)),
                world_id_set,
            });
        }
    }

    /// Pre-compute the list-threshold index. One DB roundtrip per enabled
    /// (alert, list) pair to fetch the list row and its priced items. Cost:
    /// O(active list-alerts) at refresh; O(1) at dispatch.
    async fn refresh_list_rules_from(
        &mut self,
        alerts: &[(alert::Model, alert_list_threshold::Model)],
        db: &UltrosDb,
        world_cache: &WorldCache,
    ) {
        self.by_item_list_rules.clear();
        for (a, t) in alerts {
            if !a.enabled {
                continue;
            }
            let list = match db.get_list_by_id(t.list_id).await {
                Ok(Some(l)) => l,
                Ok(None) => {
                    warn!(alert_id = a.id, list_id = t.list_id, "list not found");
                    continue;
                }
                Err(e) => {
                    warn!(
                        alert_id = a.id,
                        list_id = t.list_id,
                        "list lookup failed: {e}"
                    );
                    continue;
                }
            };
            // Resolve the list's WDR filter into a flat world id set. If the
            // list has no WDR selector at all we treat the rule as "any world"
            // by leaving the set empty — and since `list_rule_matches_listing`
            // requires membership, an empty set means the rule never fires.
            // The legacy `From<&list::Model> for AnySelector` impl errors if
            // none are set; this matches that contract.
            let world_id_set: HashSet<i32> = match DbAnySelector::try_from(&list) {
                Ok(selector) => match world_cache.lookup_selector(&selector) {
                    Ok(result) => world_cache
                        .get_all_worlds_in(&result)
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                    Err(e) => {
                        warn!(
                            alert_id = a.id,
                            "could not resolve list world selector: {e}"
                        );
                        HashSet::new()
                    }
                },
                Err(e) => {
                    warn!(alert_id = a.id, "list has no world filter: {e}");
                    HashSet::new()
                }
            };
            let items = match db.get_list_items_with_target(t.list_id).await {
                Ok(items) => items,
                Err(e) => {
                    warn!(alert_id = a.id, "list items lookup failed: {e}");
                    continue;
                }
            };
            for item in items {
                let Some(target_price) = item.target_price else {
                    continue;
                };
                self.by_item_list_rules
                    .entry(item.item_id)
                    .or_default()
                    .push(ListActiveRule {
                        alert_id: a.id,
                        list_id: t.list_id,
                        item_id: item.item_id,
                        target_price,
                        cooldown_seconds: a.cooldown_seconds,
                        last_fired_at: a.last_fired_at.map(|dt| dt.with_timezone(&Utc)),
                        world_id_set: world_id_set.clone(),
                        list_name: list.name.clone(),
                    });
            }
        }
    }
}

pub(crate) struct PriceAlertListener {
    /// Held to keep the channel sender alive — when `PriceAlertListener` is
    /// dropped, the corresponding `stop_rx.recv()` in the spawned task returns
    /// `None`, ending the loop. Also reserved for future explicit shutdown
    /// (e.g., on `AlertManager`'s cancellation token).
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl PriceAlertListener {
    #[instrument(skip(ultros_db, listings, alert_events, list_events, ctx, world_cache))]
    pub(crate) async fn start(
        ultros_db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        mut alert_events: EventBus<alert::Model>,
        mut list_events: EventBus<ListEventData>,
        ctx: serenity_prelude::Context,
        world_cache: Arc<WorldCache>,
    ) -> Result<Self> {
        let state = Arc::new(Mutex::new(TrackerState::default()));
        let (initial, initial_list) =
            refresh_state_from_db(&state, &ultros_db, &world_cache).await?;
        info!(
            "price-alert tracker started with {} item-rules and {} list-alerts",
            initial.len(),
            initial_list.len()
        );

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

        let state_for_loop = state.clone();
        let db_for_loop = ultros_db.clone();
        let world_cache_for_loop = world_cache.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    msg = alert_events.recv() => {
                        match msg {
                            Ok(_) => {
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop).await {
                                    error!("price-alert tracker refresh failed after alert change: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("price-alert tracker lagged, dropped {n} alert events");
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop).await {
                                    error!("price-alert tracker refresh failed after alert lag: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    msg = list_events.recv() => {
                        match msg {
                            Ok(_) => {
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop).await {
                                    error!("price-alert tracker refresh failed after list change: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("price-alert tracker lagged, dropped {n} list events");
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop).await {
                                    error!("price-alert tracker refresh failed after list lag: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    msg = listings.recv() => {
                        match msg {
                            Ok(event) => {
                                if let EventType::Add(added) = event {
                                    handle_added(&added, &state_for_loop, &db_for_loop, &ctx).await;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("price-alert tracker lagged, dropped {n} listing events");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });

        Ok(Self { stop_tx })
    }
}

async fn refresh_state_from_db(
    state: &Arc<Mutex<TrackerState>>,
    db: &UltrosDb,
    world_cache: &WorldCache,
) -> Result<(
    Vec<(alert::Model, alert_item_threshold::Model)>,
    Vec<(alert::Model, alert_list_threshold::Model)>,
)> {
    let threshold_alerts = db.get_all_active_threshold_alerts().await?;
    let list_threshold_alerts = db.get_all_active_list_threshold_alerts().await?;
    {
        let mut guard = state.lock().await;
        guard.refresh_from(&threshold_alerts, world_cache);
        guard
            .refresh_list_rules_from(&list_threshold_alerts, db, world_cache)
            .await;
    }
    Ok((threshold_alerts, list_threshold_alerts))
}

async fn handle_added(
    added: &ListingEventData,
    state: &Arc<Mutex<TrackerState>>,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) {
    let now = Utc::now();
    let mut to_fire: Vec<(ActiveRule, i32)> = vec![];
    let mut to_fire_list: Vec<(ListActiveRule, i32)> = vec![];

    {
        let mut guard = state.lock().await;
        for (listing, _retainer) in &added.listings {
            if let Some(rules) = guard.by_item.get_mut(&listing.item_id) {
                for rule in rules.iter_mut() {
                    if !rule_matches_listing(rule, listing, now) {
                        continue;
                    }
                    rule.last_fired_at = Some(now);
                    to_fire.push((rule.clone(), listing.price_per_unit));
                }
            }
            if let Some(list_rules) = guard.by_item_list_rules.get_mut(&listing.item_id) {
                // For one listing matching this item, multiple list-alerts may
                // each have their own (alert × list_item) row here. We may
                // also have multiple rows with the same alert_id (one per
                // priced list_item) — bump all of their cooldowns at once
                // so a downstream listing for a different item doesn't
                // double-fire the same alert.
                let mut fired_alert_ids: HashSet<i32> = HashSet::new();
                for rule in list_rules.iter_mut() {
                    if !list_rule_matches_listing(rule, listing, now) {
                        continue;
                    }
                    if !fired_alert_ids.insert(rule.alert_id) {
                        // Already firing this alert for this listing; skip.
                        continue;
                    }
                    to_fire_list.push((rule.clone(), listing.price_per_unit));
                }
                // Apply the cooldown update to every row sharing an alert_id
                // we just fired, regardless of which row triggered.
                for rule in list_rules.iter_mut() {
                    if fired_alert_ids.contains(&rule.alert_id) {
                        rule.last_fired_at = Some(now);
                    }
                }
            }
        }
    }

    for (rule, matched_price) in to_fire {
        let item_name = resolve_item_name(rule.item_id);
        let (title, body) = format_threshold_alert_message(
            &item_name,
            rule.item_id,
            matched_price,
            rule.price_threshold,
        );

        let delivery_result = dispatch_alert(rule.alert_id, &title, &body, db, ctx).await;
        let delivered = delivery_result.is_ok();
        let delivery_error = delivery_result.err().map(|e| e.to_string());

        if let Err(e) = db
            .record_alert_event(
                rule.alert_id,
                rule.item_id,
                None,
                Some(matched_price),
                delivered,
                delivery_error,
            )
            .await
        {
            error!(
                "failed to record alert_event for alert {}: {e}",
                rule.alert_id
            );
        }
        if delivered && let Err(e) = db.update_alert_last_fired(rule.alert_id).await {
            error!(
                "failed to update last_fired_at for alert {}: {e}",
                rule.alert_id
            );
        }
    }

    for (rule, matched_price) in to_fire_list {
        let item_name = resolve_item_name(rule.item_id);
        let (title, body) = format_list_threshold_alert_message(
            &rule.list_name,
            rule.list_id,
            &item_name,
            rule.item_id,
            matched_price,
            rule.target_price,
        );

        let delivery_result = dispatch_alert(rule.alert_id, &title, &body, db, ctx).await;
        let delivered = delivery_result.is_ok();
        let delivery_error = delivery_result.err().map(|e| e.to_string());

        if let Err(e) = db
            .record_alert_event(
                rule.alert_id,
                rule.item_id,
                None,
                Some(matched_price),
                delivered,
                delivery_error,
            )
            .await
        {
            error!(
                "failed to record alert_event for list-alert {}: {e}",
                rule.alert_id
            );
        }
        if delivered && let Err(e) = db.update_alert_last_fired(rule.alert_id).await {
            error!(
                "failed to update last_fired_at for list-alert {}: {e}",
                rule.alert_id
            );
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{Duration, NaiveDateTime, Utc};

    fn rule(threshold: i32, hq_only: bool, worlds: &[i32]) -> ActiveRule {
        ActiveRule {
            alert_id: 1,
            item_id: 42,
            price_threshold: threshold,
            hq_only,
            cooldown_seconds: 3600,
            last_fired_at: None,
            world_id_set: worlds.iter().copied().collect(),
        }
    }

    fn listing(world_id: i32, price: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id: 1,
            world_id,
            item_id: 42,
            retainer_id: 1,
            price_per_unit: price,
            quantity: 1,
            hq,
            timestamp: NaiveDateTime::default(),
        }
    }

    // ---------- is_off_cooldown_at ----------

    #[test]
    fn cooldown_blocks_recent_fire() {
        let now = Utc::now();
        let last = Some(now - Duration::seconds(30));
        assert!(!is_off_cooldown_at(last, 3600, now));
    }

    #[test]
    fn cooldown_allows_old_fire() {
        let now = Utc::now();
        let last = Some(now - Duration::seconds(7200));
        assert!(is_off_cooldown_at(last, 3600, now));
    }

    #[test]
    fn never_fired_is_off_cooldown() {
        assert!(is_off_cooldown_at(None, 3600, Utc::now()));
    }

    #[test]
    fn cooldown_boundary_at_exactly_cooldown_seconds_is_off() {
        // Spec: `>= cooldown_seconds` is off cooldown.
        let now = Utc::now();
        let last = Some(now - Duration::seconds(3600));
        assert!(is_off_cooldown_at(last, 3600, now));
    }

    #[test]
    fn cooldown_one_second_before_boundary_is_blocked() {
        let now = Utc::now();
        let last = Some(now - Duration::seconds(3599));
        assert!(!is_off_cooldown_at(last, 3600, now));
    }

    #[test]
    fn cooldown_zero_is_always_off() {
        let now = Utc::now();
        let last = Some(now);
        assert!(is_off_cooldown_at(last, 0, now));
    }

    // ---------- rule_matches_listing ----------

    #[test]
    fn matches_when_world_price_quality_and_cooldown_all_pass() {
        let r = rule(100, false, &[1]);
        let l = listing(1, 50, false);
        assert!(rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn does_not_match_when_listing_world_not_in_rule_world_set() {
        let r = rule(100, false, &[1, 2]);
        let l = listing(999, 50, false);
        assert!(!rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn does_not_match_when_hq_only_rule_sees_nq_listing() {
        let r = rule(100, true, &[1]);
        let l = listing(1, 50, false);
        assert!(!rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn does_match_when_hq_only_rule_sees_hq_listing() {
        let r = rule(100, true, &[1]);
        let l = listing(1, 50, true);
        assert!(rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn nq_rule_accepts_both_hq_and_nq_listings() {
        let r = rule(100, false, &[1]);
        assert!(rule_matches_listing(&r, &listing(1, 50, true), Utc::now()));
        assert!(rule_matches_listing(&r, &listing(1, 50, false), Utc::now()));
    }

    #[test]
    fn does_not_match_when_listing_price_above_threshold() {
        let r = rule(100, false, &[1]);
        let l = listing(1, 101, false);
        assert!(!rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn matches_at_exact_threshold_price() {
        // "drops to or below" — equal is a match.
        let r = rule(100, false, &[1]);
        let l = listing(1, 100, false);
        assert!(rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn does_not_match_when_within_cooldown_window() {
        let now = Utc::now();
        let mut r = rule(100, false, &[1]);
        r.last_fired_at = Some(now - Duration::seconds(60));
        let l = listing(1, 50, false);
        assert!(!rule_matches_listing(&r, &l, now));
    }

    #[test]
    fn matches_after_cooldown_window_passed() {
        let now = Utc::now();
        let mut r = rule(100, false, &[1]);
        r.last_fired_at = Some(now - Duration::seconds(7200));
        let l = listing(1, 50, false);
        assert!(rule_matches_listing(&r, &l, now));
    }

    // ---------- list_rule_matches_listing ----------

    fn list_rule(target_price: i64, worlds: &[i32]) -> ListActiveRule {
        ListActiveRule {
            alert_id: 1,
            list_id: 1,
            item_id: 42,
            target_price,
            cooldown_seconds: 3600,
            last_fired_at: None,
            world_id_set: worlds.iter().copied().collect(),
            list_name: "My List".to_string(),
        }
    }

    #[test]
    fn list_rule_matches_when_world_price_and_cooldown_all_pass() {
        let r = list_rule(100, &[1]);
        let l = listing(1, 50, false);
        assert!(list_rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn list_rule_does_not_match_when_listing_world_not_in_rule_world_set() {
        let r = list_rule(100, &[1, 2]);
        let l = listing(999, 50, false);
        assert!(!list_rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn list_rule_ignores_hq_status_and_matches_hq_listing() {
        let r = list_rule(100, &[1]);
        let l = listing(1, 50, true);
        assert!(list_rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn list_rule_ignores_hq_status_and_matches_nq_listing() {
        let r = list_rule(100, &[1]);
        let l = listing(1, 50, false);
        assert!(list_rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn list_rule_does_not_match_when_listing_price_above_target_price() {
        let r = list_rule(100, &[1]);
        let l = listing(1, 101, false);
        assert!(!list_rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn list_rule_matches_at_exact_target_price() {
        let r = list_rule(100, &[1]);
        let l = listing(1, 100, false);
        assert!(list_rule_matches_listing(&r, &l, Utc::now()));
    }

    #[test]
    fn list_rule_does_not_match_when_within_cooldown_window() {
        let now = Utc::now();
        let mut r = list_rule(100, &[1]);
        r.last_fired_at = Some(now - Duration::seconds(60));
        let l = listing(1, 50, false);
        assert!(!list_rule_matches_listing(&r, &l, now));
    }

    #[test]
    fn list_rule_matches_after_cooldown_window_passed() {
        let now = Utc::now();
        let mut r = list_rule(100, &[1]);
        r.last_fired_at = Some(now - Duration::seconds(7200));
        let l = listing(1, 50, false);
        assert!(list_rule_matches_listing(&r, &l, now));
    }

    // ---------- format_threshold_alert_message ----------

    #[test]
    fn format_message_includes_item_name_and_matched_price_in_title() {
        let (title, _) = format_threshold_alert_message("Eternity Ring", 36687, 99000, 100000);
        assert!(title.contains("Eternity Ring"));
        assert!(title.contains("99000"));
    }

    #[test]
    fn format_message_body_includes_threshold_and_universalis_link() {
        let (_, body) = format_threshold_alert_message("Cordial", 6141, 500, 1000);
        assert!(body.contains("1000"));
        assert!(body.contains("ultros.app/item/6141"));
    }

    #[test]
    fn format_message_handles_unicode_item_names() {
        let (title, body) = format_threshold_alert_message("水晶", 100, 50, 60);
        assert!(title.contains("水晶"));
        assert!(body.contains("ultros.app/item/100"));
    }

    // ---------- format_list_threshold_alert_message ----------

    #[test]
    fn format_list_message_includes_list_and_item_name_and_matched_price_in_title() {
        let (title, _) = format_list_threshold_alert_message("Shopping", 10, "Eternity Ring", 36687, 99000, 100000);
        assert!(title.contains("Shopping"));
        assert!(title.contains("Eternity Ring"));
        assert!(title.contains("99000"));
    }

    #[test]
    fn format_list_message_body_includes_target_price_and_links() {
        let (_, body) = format_list_threshold_alert_message("Shopping", 10, "Cordial", 6141, 500, 1000);
        assert!(body.contains("1000"));
        assert!(body.contains("ultros.app/item/6141"));
        assert!(body.contains("ultros.app/list/10"));
    }

    #[test]
    fn format_list_message_handles_unicode_names() {
        let (title, body) = format_list_threshold_alert_message("リスト", 20, "水晶", 100, 50, 60);
        assert!(title.contains("リスト"));
        assert!(title.contains("水晶"));
        assert!(body.contains("ultros.app/item/100"));
        assert!(body.contains("ultros.app/list/20"));
    }
}
