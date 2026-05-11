use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude;
use tokio::sync::Mutex;
use tracing::{error, info, instrument, warn};
use ultros_api_types::{websocket::ListingEventData, world_helper::AnySelector as ApiAnySelector};
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_item_threshold},
    world_data::world_cache::WorldCache,
};

use crate::event::{EventBus, EventType};
use crate::alerts::delivery::dispatch_alert;

pub(crate) fn is_off_cooldown(last_fired_at: Option<DateTime<Utc>>, cooldown_seconds: i32) -> bool {
    match last_fired_at {
        None => true,
        Some(t) => Utc::now().signed_duration_since(t).num_seconds() >= cooldown_seconds as i64,
    }
}

#[derive(Debug, Clone)]
struct ActiveRule {
    alert_id: i32,
    item_id: i32,
    price_threshold: i32,
    hq_only: bool,
    cooldown_seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
    /// Pre-resolved set of world IDs this rule applies to.
    world_id_set: HashSet<i32>,
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
            let world_id_set: HashSet<i32> = match serde_json::from_value::<ApiAnySelector>(t.world_selector.clone()) {
                Ok(api_selector) => {
                    let selector: ultros_db::world_data::world_cache::AnySelector = api_selector.into();
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
    pub(crate) stop_tx: tokio::sync::mpsc::Sender<()>,
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
    let mut to_fire: Vec<(ActiveRule, i32)> = vec![];

    {
        let mut guard = state.lock().await;
        for (listing, _retainer) in &added.listings {
            let Some(rules) = guard.by_item.get_mut(&listing.item_id) else { continue };
            for rule in rules.iter_mut() {
                if !rule.world_id_set.contains(&listing.world_id) {
                    continue;
                }
                if rule.hq_only && !listing.hq {
                    continue;
                }
                if listing.price_per_unit > rule.price_threshold {
                    continue;
                }
                if !is_off_cooldown(rule.last_fired_at, rule.cooldown_seconds) {
                    continue;
                }
                rule.last_fired_at = Some(Utc::now());
                to_fire.push((rule.clone(), listing.price_per_unit));
            }
        }
    }

    for (rule, matched_price) in to_fire {
        let item_name = xiv_gen_db::data()
            .items
            .get(&xiv_gen::ItemId(rule.item_id))
            .map(|i| i.name.as_str().to_string())
            .unwrap_or_else(|| format!("Item {}", rule.item_id));
        let title = format!("🎯 {item_name} dropped to {matched_price} gil");
        let body = format!(
            "Threshold: {} gil\nhttps://ultros.app/item/{}",
            rule.price_threshold, rule.item_id
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
            error!("failed to record alert_event for alert {}: {e}", rule.alert_id);
        }
        if delivered
            && let Err(e) = db.update_alert_last_fired(rule.alert_id).await
        {
            error!("failed to update last_fired_at for alert {}: {e}", rule.alert_id);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn cooldown_blocks_recent_fire() {
        let last = Some(Utc::now() - Duration::seconds(30));
        let cooldown_s = 3600;
        assert!(!is_off_cooldown(last, cooldown_s));
    }

    #[test]
    fn cooldown_allows_old_fire() {
        let last = Some(Utc::now() - Duration::seconds(7200));
        let cooldown_s = 3600;
        assert!(is_off_cooldown(last, cooldown_s));
    }

    #[test]
    fn never_fired_is_off_cooldown() {
        assert!(is_off_cooldown(None, 3600));
    }
}
