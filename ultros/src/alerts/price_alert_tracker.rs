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
    ActiveListing, websocket::ListingEventData, world_helper::AnySelector as ApiAnySelector,
};
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_item_threshold},
    world_data::world_cache::WorldCache,
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

/// Look up an item's name in the embedded xiv-gen data, falling back to `"Item {id}"` if missing.
fn resolve_item_name(item_id: i32) -> String {
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

#[derive(Debug, Default)]
struct TrackerState {
    by_item: HashMap<i32, Vec<ActiveRule>>,
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
    #[instrument(skip(ultros_db, listings, ctx, world_cache))]
    pub(crate) async fn start(
        ultros_db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        ctx: serenity_prelude::Context,
        world_cache: Arc<WorldCache>,
    ) -> Result<Self> {
        let initial = ultros_db.get_all_active_threshold_alerts().await?;
        let state = Arc::new(Mutex::new(TrackerState::default()));
        state.lock().await.refresh_from(&initial, &world_cache);
        info!("price-alert tracker started with {} rules", initial.len());

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

        let state_for_loop = state.clone();
        let db_for_loop = ultros_db.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
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

async fn handle_added(
    added: &ListingEventData,
    state: &Arc<Mutex<TrackerState>>,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) {
    let now = Utc::now();
    let mut to_fire: Vec<(ActiveRule, i32)> = vec![];

    {
        let mut guard = state.lock().await;
        for (listing, _retainer) in &added.listings {
            let Some(rules) = guard.by_item.get_mut(&listing.item_id) else {
                continue;
            };
            for rule in rules.iter_mut() {
                if !rule_matches_listing(rule, listing, now) {
                    continue;
                }
                rule.last_fired_at = Some(now);
                to_fire.push((rule.clone(), listing.price_per_unit));
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
}
