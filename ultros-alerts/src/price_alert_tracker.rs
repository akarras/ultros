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
    alert::{ThresholdRule, threshold_listing_matches},
    websocket::{ListEventData, ListingEventData},
    world_helper::{AnySelector as ApiAnySelector, WorldHelper},
};
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_item_threshold, alert_list_threshold},
    world_data::world_cache::{AnySelector as DbAnySelector, WorldCache},
};

use crate::alerts::delivery::{AlertKind, PushOptions, dispatch_alert};
use crate::alerts::inbox::{AlertFire, record_fire};
use crate::event::{EventBus, EventProducer, EventType, NotificationEvent};

/// True when an alert with the given `last_fired_at` is free to fire again given `cooldown_seconds`
/// as of the reference timestamp `now`. `None` (never fired) is always off cooldown.
///
/// Re-exported from `ultros_api_types::alert` so this crate's other trackers
/// (e.g. `list_update_alert_tracker.rs`) keep importing it from here, while the
/// actual logic is shared with the browser's guest-alert evaluation.
pub use ultros_api_types::alert::is_off_cooldown_at;

/// Returns true if `listing` satisfies every condition of `rule` and the rule is off
/// cooldown at `now`. Pure: no DB calls, no `Utc::now()`. Thin wrapper over the
/// shared `threshold_listing_matches` predicate so account and guest alerts
/// evaluate identically.
pub fn rule_matches_listing(
    rule: &ActiveRule,
    listing: &ActiveListing,
    worlds: &WorldHelper,
    now: DateTime<Utc>,
) -> bool {
    threshold_listing_matches(&rule.threshold_rule(), listing, worlds, now)
}

/// Build the Discord embed title + body for a threshold-alert firing. Pure.
pub fn format_threshold_alert_message(
    item_name: &str,
    matched_price: i32,
    price_threshold: i32,
    click_url: &str,
) -> (String, String) {
    let title = format!("🎯 {item_name} dropped to {matched_price} gil");
    let body = format!("Threshold: {price_threshold} gil\nhttps://ultros.app{click_url}");
    (title, body)
}

pub(crate) fn item_click_url(item_id: i32, world_name: Option<&str>) -> String {
    world_name.map_or_else(
        || format!("/item/{item_id}"),
        |world_name| format!("/item/{world_name}/{item_id}"),
    )
}

/// Build the Discord embed title + body for a list-threshold alert firing. Pure.
/// Format mirrors the item-threshold one so the delivery shape stays familiar,
/// but the title is prefixed with the list name to disambiguate when a user
/// subscribes to multiple lists.
pub fn format_list_threshold_alert_message(
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
pub fn list_rule_matches_listing(
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
pub fn resolve_item_name(item_id: i32) -> String {
    xiv_gen_db::data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| format!("Item {item_id}"))
}

#[derive(Debug, Clone)]
pub struct ActiveRule {
    pub alert_id: i32,
    pub owner: i64,
    pub item_id: i32,
    pub price_threshold: i32,
    pub hq_only: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
    /// World scope this rule applies to. Resolved against a `WorldHelper` at
    /// match time (via `threshold_rule`/`threshold_listing_matches`) rather
    /// than pre-flattened, matching how the browser evaluates guest alerts.
    pub world_selector: ApiAnySelector,
}

impl ActiveRule {
    /// Project onto the shared `ThresholdRule` shape so matching goes through
    /// `threshold_listing_matches` — the same predicate the browser runs for
    /// guest price alerts.
    pub fn threshold_rule(&self) -> ThresholdRule {
        ThresholdRule {
            item_id: self.item_id,
            world_selector: self.world_selector,
            price_threshold: self.price_threshold,
            hq_only: self.hq_only,
            cooldown_seconds: self.cooldown_seconds,
            last_fired_at: self.last_fired_at,
        }
    }
}

/// A pre-computed (alert, list_item) pair the price-alert tracker fires when a
/// listing meets the per-row `target_price`. One per (list-threshold alert ×
/// list_item-with-target). The list's name and the precomputed world id set
/// are folded in so the dispatch path stays O(1).
#[derive(Debug, Clone)]
pub struct ListActiveRule {
    pub alert_id: i32,
    pub owner: i64,
    pub list_id: i32,
    pub item_id: i32,
    pub target_price: i64,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub world_id_set: HashSet<i32>,
    pub list_name: String,
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
        world_helper: &WorldHelper,
    ) {
        self.by_item.clear();
        for (a, t) in alerts {
            if !a.enabled {
                continue;
            }
            // World containment is resolved lazily at match time (see
            // `ActiveRule::threshold_rule`), so we only need to deserialize the
            // selector here, not flatten it against the world cache. We still
            // do a one-off resolve check now so a stale/nonexistent world,
            // datacenter, or region id gets logged at refresh time rather than
            // failing silently on every future listing (it would otherwise
            // never match, since an unresolvable selector fails closed in
            // `threshold_listing_matches`).
            let world_selector =
                match serde_json::from_value::<ApiAnySelector>(t.world_selector.clone()) {
                    Ok(selector) => selector,
                    Err(e) => {
                        warn!(
                            alert_id = a.id,
                            "could not deserialize world_selector for alert: {e}"
                        );
                        continue;
                    }
                };
            if world_helper.lookup_selector(world_selector).is_none() {
                warn!(
                    alert_id = a.id,
                    ?world_selector,
                    "world_selector for alert does not resolve against known world data; rule will never match"
                );
            }
            self.by_item.entry(t.item_id).or_default().push(ActiveRule {
                alert_id: a.id,
                owner: a.owner,
                item_id: t.item_id,
                price_threshold: t.price_threshold,
                hq_only: t.hq_only,
                cooldown_seconds: a.cooldown_seconds,
                last_fired_at: a.last_fired_at.map(|dt| dt.with_timezone(&Utc)),
                world_selector,
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
                        owner: a.owner,
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

/// Shared handles `PriceAlertListener::start` needs beyond the event buses.
/// Grouped so adding `world_helper`/`notifications` for the notification
/// inbox didn't push the function's argument count past clippy's
/// `too_many_arguments` threshold.
pub struct PriceAlertServices {
    pub ctx: serenity_prelude::Context,
    pub world_cache: Arc<WorldCache>,
    pub world_helper: Arc<WorldHelper>,
    pub notifications: EventProducer<NotificationEvent>,
}

pub struct PriceAlertListener {
    /// Held to keep the channel sender alive — when `PriceAlertListener` is
    /// dropped, the corresponding `stop_rx.recv()` in the spawned task returns
    /// `None`, ending the loop. Also reserved for future explicit shutdown
    /// (e.g., on `AlertManager`'s cancellation token).
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl PriceAlertListener {
    #[instrument(skip(ultros_db, listings, alert_events, list_events, services))]
    pub async fn start(
        ultros_db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        mut alert_events: EventBus<alert::Model>,
        mut list_events: EventBus<ListEventData>,
        services: PriceAlertServices,
    ) -> Result<Self> {
        let PriceAlertServices {
            ctx,
            world_cache,
            world_helper,
            notifications,
        } = services;
        let state = Arc::new(Mutex::new(TrackerState::default()));
        let (initial, initial_list) =
            refresh_state_from_db(&state, &ultros_db, &world_cache, &world_helper).await?;
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
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop, &world_helper).await {
                                    error!("price-alert tracker refresh failed after alert change: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("price-alert tracker lagged, dropped {n} alert events");
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop, &world_helper).await {
                                    error!("price-alert tracker refresh failed after alert lag: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    msg = list_events.recv() => {
                        match msg {
                            Ok(_) => {
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop, &world_helper).await {
                                    error!("price-alert tracker refresh failed after list change: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("price-alert tracker lagged, dropped {n} list events");
                                if let Err(e) = refresh_state_from_db(&state_for_loop, &db_for_loop, &world_cache_for_loop, &world_helper).await {
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
                                    handle_added(
                                        &added,
                                        &state_for_loop,
                                        &db_for_loop,
                                        &ctx,
                                        &world_helper,
                                        &notifications,
                                    )
                                    .await;
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
    world_helper: &WorldHelper,
) -> Result<(
    Vec<(alert::Model, alert_item_threshold::Model)>,
    Vec<(alert::Model, alert_list_threshold::Model)>,
)> {
    let threshold_alerts = db.get_all_active_threshold_alerts().await?;
    let list_threshold_alerts = db.get_all_active_list_threshold_alerts().await?;
    {
        let mut guard = state.lock().await;
        guard.refresh_from(&threshold_alerts, world_helper);
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
    world_helper: &WorldHelper,
    notifications: &EventProducer<NotificationEvent>,
) {
    let now = Utc::now();
    let mut to_fire: Vec<(ActiveRule, i32, i32)> = vec![];
    let mut to_fire_list: Vec<(ListActiveRule, i32)> = vec![];

    {
        let mut guard = state.lock().await;
        for (listing, _retainer) in &added.listings {
            if let Some(rules) = guard.by_item.get_mut(&listing.item_id) {
                for rule in rules.iter_mut() {
                    if !rule_matches_listing(rule, listing, world_helper, now) {
                        continue;
                    }
                    rule.last_fired_at = Some(now);
                    to_fire.push((rule.clone(), listing.price_per_unit, listing.world_id));
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

    for (rule, matched_price, world_id) in to_fire {
        let item_name = resolve_item_name(rule.item_id);
        let world_name = world_helper
            .lookup_selector(ApiAnySelector::World(world_id))
            .map(|world| world.get_name().to_string());
        let click_url = item_click_url(rule.item_id, world_name.as_deref());
        let (title, body) = format_threshold_alert_message(
            &item_name,
            matched_price,
            rule.price_threshold,
            &click_url,
        );

        let push = PushOptions::for_alert(AlertKind::Price, rule.alert_id, &click_url);
        let delivery_result = dispatch_alert(rule.alert_id, &title, &body, &push, db, ctx).await;
        let delivered = delivery_result.is_ok();
        let delivery_error = delivery_result.err().map(|e| e.to_string());

        record_fire(
            db,
            notifications,
            AlertFire {
                alert_id: rule.alert_id,
                owner: rule.owner,
                item_id: rule.item_id,
                matched_listing_id: None,
                matched_price: Some(matched_price),
                title: &title,
                body: &body,
                click_url: &click_url,
                delivered,
                delivery_error,
            },
        )
        .await;
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

        let click_url = format!("/list/{}", rule.list_id);
        let push = PushOptions::for_alert(AlertKind::Price, rule.alert_id, &click_url);
        let delivery_result = dispatch_alert(rule.alert_id, &title, &body, &push, db, ctx).await;
        let delivered = delivery_result.is_ok();
        let delivery_error = delivery_result.err().map(|e| e.to_string());

        record_fire(
            db,
            notifications,
            AlertFire {
                alert_id: rule.alert_id,
                owner: rule.owner,
                item_id: rule.item_id,
                matched_listing_id: None,
                matched_price: Some(matched_price),
                title: &title,
                body: &body,
                click_url: &click_url,
                delivered,
                delivery_error,
            },
        )
        .await;
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
    use chrono::{Duration, Utc};

    // ---------- is_off_cooldown_at ----------
    //
    // `rule_matches_listing` itself is now a thin wrapper over
    // `ultros_api_types::alert::threshold_listing_matches`; the item/world/hq/
    // price/cooldown combinations it used to cover here are tested once,
    // against the shared predicate, in `ultros-api-types/src/alert.rs`
    // (`threshold_tests`). The boundary-condition tests for
    // `is_off_cooldown_at` below aren't duplicated there (that module only
    // exercises cooldown behavior indirectly via the combined predicate), so
    // they stay here against the re-exported function.

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

    // ---------- format_threshold_alert_message ----------

    #[test]
    fn format_message_includes_item_name_and_matched_price_in_title() {
        let (title, _) =
            format_threshold_alert_message("Eternity Ring", 99000, 100000, "/item/Seraph/36687");
        assert!(title.contains("Eternity Ring"));
        assert!(title.contains("99000"));
    }

    #[test]
    fn format_message_body_includes_threshold_and_item_link() {
        let (_, body) = format_threshold_alert_message("Cordial", 500, 1000, "/item/Seraph/6141");
        assert!(body.contains("1000"));
        assert!(body.contains("ultros.app/item/Seraph/6141"));
    }

    #[test]
    fn format_message_handles_unicode_item_names() {
        let (title, body) = format_threshold_alert_message("水晶", 50, 60, "/item/Tiamat/100");
        assert!(title.contains("水晶"));
        assert!(body.contains("ultros.app/item/Tiamat/100"));
    }

    #[test]
    fn item_click_url_includes_triggering_world() {
        assert_eq!(item_click_url(6141, Some("Seraph")), "/item/Seraph/6141");
    }

    #[test]
    fn item_click_url_falls_back_to_default_price_zone() {
        assert_eq!(item_click_url(6141, None), "/item/6141");
    }
}
