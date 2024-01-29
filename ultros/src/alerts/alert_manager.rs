use std::collections::HashMap;

use futures::future::{self, Either};
use poise::serenity_prelude;
use tracing::error;
use ultros_api_types::{user::OwnedRetainer, websocket::ListingEventData, Retainer};
use ultros_db::{
    entity::{alert, alert_retainer_undercut},
    UltrosDb,
};

use crate::event::{EventBus, EventType};

use super::undercut_alert::{RetainerAlertListener, RetainerAlertTx};

pub(crate) struct AlertManager {
    /// Hashmap of the current retainer alerts where the id of the alert is the key
    current_retainer_alerts: HashMap<i32, RetainerAlertListener>,
}

impl AlertManager {
    pub(crate) async fn start_manager(
        ultros_db: UltrosDb,
        (retainers, listings): (EventBus<OwnedRetainer>, EventBus<ListingEventData>),
        (mut alerts, mut undercuts): (
            EventBus<alert::Model>,
            EventBus<alert_retainer_undercut::Model>,
        ),
        ctx: serenity_prelude::Context,
    ) {
        // start all alerts we know about from the db, then use the alert busses to monitor for new alerts being spawned
        let mut manager = AlertManager {
            current_retainer_alerts: HashMap::new(),
        };
        match ultros_db.get_all_alerts().await {
            Ok(all_alerts) => {
                for alert in all_alerts {
                    if let Ok(alert) = ultros_db
                        .get_retainer_alerts_for_related_alert_id(alert.id)
                        .await
                    {
                        for alert in alert {
                            manager
                                .create_retainer_alert_listener(
                                    &alert,
                                    &ultros_db,
                                    &ctx,
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
        loop {
            let alerts = Box::pin(alerts.recv());
            let retainer_alert_events = Box::pin(undercuts.recv());
            match future::select(alerts, retainer_alert_events).await {
                Either::Left(_alert) => {
                    /*if let Ok(alert) = alert {
                        manager.remove_retainer_alert(alert);
                    }*/
                }
                Either::Right((retainer_alert_create, _)) => {
                    if let Ok(retainer) = &retainer_alert_create {
                        match retainer {
                            EventType::Remove(removed) => {
                                manager.remove_retainer_alert(removed).await;
                            }
                            EventType::Add(retainer_alert) => {
                                manager
                                    .create_retainer_alert_listener(
                                        retainer_alert,
                                        &ultros_db,
                                        &ctx,
                                        listings.resubscribe(),
                                        retainers.resubscribe(),
                                    )
                                    .await;
                            }
                            EventType::Update(m) => {
                                manager.update_alert(m, m.margin_percent).await;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn create_retainer_alert_listener(
        &mut self,
        alert: &alert_retainer_undercut::Model,
        ultros_db: &UltrosDb,
        ctx: &serenity_prelude::Context,
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
            ctx.clone(),
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

    async fn update_alert(&self, alert: &alert_retainer_undercut::Model, margin: i32) {
        if let Some(listener) = self.current_retainer_alerts.get(&alert.id) {
            let _ = listener
                .cancellation_sender
                .send(RetainerAlertTx::UpdateMargin(margin))
                .await;
        }
    }
}
