use std::{collections::HashMap, sync::Arc};

use futures::future::{self, Either};
use tokio_util::sync::CancellationToken;
use tracing::error;
use ultros_api_types::{
    user::OwnedRetainer,
    websocket::{ListEventData, ListingEventData, SaleEventData},
    world_helper::WorldHelper,
};
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_retainer_undercut},
    world_data::world_cache::WorldCache,
};

use crate::event::{EventBus, EventProducer, EventType, NotificationEvent};

use super::list_update_alert_tracker::ListUpdateAlertListener;
use super::market_trigger_tracker::{MarketTriggerListener, MarketTriggerServices};
use super::median_cache::{ClickHouseBaselineSource, MedianCache};
use super::price_alert_tracker::{PriceAlertListener, PriceAlertServices};
use super::sold_alert::RetainerSaleListener;
use super::undercut_alert::{RetainerAlertListener, RetainerAlertServices, RetainerAlertTx};

/// Long-lived handles `start_manager` and its sub-listeners share, bundled so
/// adding one (like `notifications`/`world_helper` for the notification
/// inbox) doesn't push `start_manager`'s argument count past clippy's
/// `too_many_arguments` threshold.
pub struct AlertManagerServices {
    pub token: CancellationToken,
    pub world_cache: Arc<WorldCache>,
    pub world_helper: Arc<WorldHelper>,
    pub notifications: EventProducer<NotificationEvent>,
    /// Source of the 30-day medians below-median alerts compare against.
    pub ch_client: ClickHouseClient,
    /// Fires when the Postgres `LISTEN/NOTIFY` bridge reconnects so the manager
    /// reconciles its retainer undercut listeners against Postgres.
    pub wakeup_resync: tokio::sync::broadcast::Receiver<()>,
}

/// The alerts whose undercut rules get a listener when the manager starts.
///
/// Only enabled ones. The live path already honours the flag — an
/// `EventType::Update` with `enabled: false` stops the rule's listener — but
/// the startup sweep used to spawn one for every alert in the table, so a
/// disabled undercut rule came back to life on each restart and fired
/// alongside the user's enabled one: two identical inbox rows (and Discord /
/// Web Push messages) for every undercut, while the alerts page showed the
/// rule as off. The price and sale trackers skip disabled alerts at load;
/// this brings the undercut startup in line.
fn alerts_to_start(alerts: Vec<alert::Model>) -> impl Iterator<Item = alert::Model> {
    alerts.into_iter().filter(|alert| alert.enabled)
}

pub struct AlertManager {
    /// Hashmap of the current retainer alerts where the id of the alert is the key
    current_retainer_alerts: HashMap<i32, RetainerAlertListener>,
    price_alerts: Option<PriceAlertListener>,
    list_update_alerts: Option<ListUpdateAlertListener>,
    sale_alerts: Option<RetainerSaleListener>,
    market_alerts: Option<MarketTriggerListener>,
    notifications: EventProducer<NotificationEvent>,
}

impl AlertManager {
    pub async fn start_manager(
        ultros_db: UltrosDb,
        (retainers, listings): (EventBus<OwnedRetainer>, EventBus<ListingEventData>),
        (mut alerts, mut undercuts): (
            EventBus<alert::Model>,
            EventBus<alert_retainer_undercut::Model>,
        ),
        (lists, history): (EventBus<ListEventData>, EventBus<SaleEventData>),
        services: AlertManagerServices,
    ) {
        let AlertManagerServices {
            token,
            world_cache,
            world_helper,
            notifications,
            ch_client,
            mut wakeup_resync,
        } = services;
        // start all alerts we know about from the db, then use the alert busses to monitor for new alerts being spawned
        let mut manager = AlertManager {
            current_retainer_alerts: HashMap::new(),
            price_alerts: None,
            list_update_alerts: None,
            sale_alerts: None,
            market_alerts: None,
            notifications: notifications.clone(),
        };
        match ultros_db.get_all_alerts().await {
            Ok(all_alerts) => {
                for alert in alerts_to_start(all_alerts) {
                    if let Ok(alert) = ultros_db
                        .get_retainer_alerts_for_related_alert_id(alert.id)
                        .await
                    {
                        for alert in alert {
                            manager
                                .create_retainer_alert_listener(
                                    &alert,
                                    &ultros_db,
                                    listings.resubscribe(),
                                    retainers.resubscribe(),
                                )
                                .await;
                        }
                    }
                }
            }
            Err(e) => error!("Error creating all alerts {e:?}"),
        }
        let medians = Arc::new(MedianCache::new(Arc::new(ClickHouseBaselineSource::new(
            ch_client,
            world_helper.clone(),
        ))));
        match MarketTriggerListener::start(
            ultros_db.clone(),
            listings.resubscribe(),
            alerts.resubscribe(),
            MarketTriggerServices {
                world_helper: world_helper.clone(),
                notifications: notifications.clone(),
                medians,
            },
        )
        .await
        {
            Ok(listener) => manager.market_alerts = Some(listener),
            Err(e) => error!("failed to start market-trigger alert listener: {e}"),
        }
        match PriceAlertListener::start(
            ultros_db.clone(),
            listings.resubscribe(),
            alerts.resubscribe(),
            lists.resubscribe(),
            PriceAlertServices {
                world_cache,
                world_helper,
                notifications: notifications.clone(),
            },
        )
        .await
        {
            Ok(listener) => manager.price_alerts = Some(listener),
            Err(e) => error!("failed to start price alert listener: {e}"),
        }
        match ListUpdateAlertListener::start(
            ultros_db.clone(),
            lists.resubscribe(),
            alerts.resubscribe(),
            notifications.clone(),
        )
        .await
        {
            Ok(listener) => manager.list_update_alerts = Some(listener),
            Err(e) => error!("failed to start list update alert listener: {e}"),
        }
        match RetainerSaleListener::start(
            ultros_db.clone(),
            listings.resubscribe(),
            history,
            retainers.resubscribe(),
            alerts.resubscribe(),
            notifications.clone(),
        )
        .await
        {
            Ok(listener) => manager.sale_alerts = Some(listener),
            Err(e) => error!("failed to start retainer sale alert listener: {e}"),
        }
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    break;
                }
                resync = wakeup_resync.recv() => {
                    match resync {
                        Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            manager
                                .reconcile_retainer_alerts(&ultros_db, &listings, &retainers)
                                .await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                    }
                }
                either = future::select(Box::pin(alerts.recv()), Box::pin(undercuts.recv())) => {
                    match either {
                        Either::Left((alert_event, _)) => match alert_event {
                            Ok(EventType::Update(alert)) => {
                                match ultros_db
                                    .get_retainer_alerts_for_related_alert_id(alert.id)
                                    .await
                                {
                                    Ok(retainer_alerts) => {
                                        for retainer_alert in retainer_alerts {
                                            if alert.enabled {
                                                if manager.current_retainer_alerts.contains_key(&retainer_alert.id) {
                                                    manager
                                                        .update_cooldown(&retainer_alert, alert.cooldown_seconds)
                                                        .await;
                                                } else {
                                                    manager
                                                        .create_retainer_alert_listener(
                                                            &retainer_alert,
                                                            &ultros_db,
                                                            listings.resubscribe(),
                                                            retainers.resubscribe(),
                                                        )
                                                        .await;
                                                }
                                            } else {
                                                manager.remove_retainer_alert(&retainer_alert).await;
                                            }
                                        }
                                    }
                                    Err(e) => error!("Error refreshing retainer alert state {e:?}"),
                                }
                            }
                            Ok(_) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                manager
                                    .reconcile_retainer_alerts(&ultros_db, &listings, &retainers)
                                    .await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        },
                        Either::Right((retainer_alert_create, _)) => match retainer_alert_create {
                            Ok(EventType::Remove(removed)) => {
                                manager.remove_retainer_alert(&removed).await;
                            }
                            Ok(EventType::Add(retainer_alert)) => {
                                manager
                                    .create_retainer_alert_listener(
                                        &retainer_alert,
                                        &ultros_db,
                                        listings.resubscribe(),
                                        retainers.resubscribe(),
                                    )
                                    .await;
                            }
                            Ok(EventType::Update(m)) => {
                                manager.update_alert(&m, m.margin_percent).await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                manager
                                    .reconcile_retainer_alerts(&ultros_db, &listings, &retainers)
                                    .await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        },
                    }
                }
            }
        }
    }

    async fn reconcile_retainer_alerts(
        &mut self,
        ultros_db: &UltrosDb,
        listings: &EventBus<ListingEventData>,
        retainers: &EventBus<OwnedRetainer>,
    ) {
        let all_alerts = match ultros_db.get_all_alerts().await {
            Ok(alerts) => alerts,
            Err(e) => {
                error!("Error reconciling retainer alerts from Postgres: {e:?}");
                return;
            }
        };
        let mut desired: HashMap<i32, (alert_retainer_undercut::Model, i32)> = HashMap::new();
        for alert in alerts_to_start(all_alerts) {
            match ultros_db
                .get_retainer_alerts_for_related_alert_id(alert.id)
                .await
            {
                Ok(retainer_alerts) => {
                    for retainer_alert in retainer_alerts {
                        desired.insert(retainer_alert.id, (retainer_alert, alert.cooldown_seconds));
                    }
                }
                Err(e) => error!(
                    alert_id = alert.id,
                    "Error loading retainer alerts during reconcile: {e:?}"
                ),
            }
        }
        let stale_ids: Vec<i32> = self
            .current_retainer_alerts
            .keys()
            .copied()
            .filter(|id| !desired.contains_key(id))
            .collect();
        for id in stale_ids {
            if let Some(listener) = self.current_retainer_alerts.remove(&id) {
                let _ = listener
                    .cancellation_sender
                    .send(RetainerAlertTx::Stop)
                    .await;
            }
        }
        for (id, (retainer_alert, cooldown_seconds)) in desired {
            if let Some(listener) = self.current_retainer_alerts.get(&id) {
                let _ = listener
                    .cancellation_sender
                    .send(RetainerAlertTx::UpdateMargin(retainer_alert.margin_percent))
                    .await;
                let _ = listener
                    .cancellation_sender
                    .send(RetainerAlertTx::UpdateCooldown(cooldown_seconds))
                    .await;
                let _ = listener
                    .cancellation_sender
                    .send(RetainerAlertTx::RefreshRetainers)
                    .await;
            } else {
                self.create_retainer_alert_listener(
                    &retainer_alert,
                    ultros_db,
                    listings.resubscribe(),
                    retainers.resubscribe(),
                )
                .await;
            }
        }
    }

    async fn create_retainer_alert_listener(
        &mut self,
        alert: &alert_retainer_undercut::Model,
        ultros_db: &UltrosDb,
        listings: EventBus<ListingEventData>,
        active_retainers: EventBus<OwnedRetainer>,
    ) {
        let alert_retainer_undercut::Model {
            id,
            alert_id,
            margin_percent,
        } = alert;
        let listener = match RetainerAlertListener::create_listener(
            *id,
            *alert_id,
            *margin_percent,
            ultros_db.clone(),
            listings,
            active_retainers,
            RetainerAlertServices {
                notifications: self.notifications.clone(),
            },
        )
        .await
        {
            Ok(l) => l,
            Err(e) => {
                error!("Error creating retainer alert listener {e}");
                return;
            }
        };
        self.current_retainer_alerts.insert(*id, listener);
    }

    async fn remove_retainer_alert(&mut self, alert: &alert_retainer_undercut::Model) {
        if let Some(listener) = self.current_retainer_alerts.remove(&alert.id) {
            let _ = listener
                .cancellation_sender
                .send(RetainerAlertTx::Stop)
                .await;
        }
    }

    /// A running listener keeps its own copy of the alert's cooldown; this is
    /// how an edit reaches it.
    async fn update_cooldown(&self, alert: &alert_retainer_undercut::Model, cooldown_seconds: i32) {
        if let Some(listener) = self.current_retainer_alerts.get(&alert.id) {
            let _ = listener
                .cancellation_sender
                .send(RetainerAlertTx::UpdateCooldown(cooldown_seconds))
                .await;
        }
    }

    async fn update_alert(&self, alert: &alert_retainer_undercut::Model, margin: i32) {
        if let Some(listener) = self.current_retainer_alerts.get(&alert.id) {
            let _ = listener
                .cancellation_sender
                .send(RetainerAlertTx::UpdateMargin(margin))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(id: i32, enabled: bool) -> alert::Model {
        alert::Model {
            id,
            owner: 1,
            enabled,
            last_fired_at: None,
            cooldown_seconds: 3600,
        }
    }

    /// A disabled alert must not get an undercut listener at startup. The
    /// live `Update` path already stops a listener when its alert is
    /// disabled, but a restart used to bring every alert back: the user's
    /// disabled rule fired alongside their enabled one, so each undercut
    /// arrived twice.
    #[test]
    fn startup_skips_disabled_alerts() {
        let ids: Vec<i32> = alerts_to_start(vec![alert(1, false), alert(27, true)])
            .map(|a| a.id)
            .collect();
        assert_eq!(ids, vec![27]);
    }
}
