use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use ultros_api_types::websocket::ListEventData;
use ultros_db::{UltrosDb, entity::alert};

use crate::{
    alerts::{
        delivery::dispatch_alert,
        price_alert_tracker::{is_off_cooldown_at, resolve_item_name},
    },
    event::{EventBus, EventType},
};

#[derive(Debug, Clone)]
struct ListUpdateRule {
    alert_id: i32,
    list_id: i32,
    cooldown_seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
    list_name: String,
}

#[derive(Debug, Default)]
struct TrackerState {
    by_list: HashMap<i32, Vec<ListUpdateRule>>,
}

impl TrackerState {
    async fn refresh_from_db(&mut self, db: &UltrosDb) -> Result<usize> {
        let rows = db.get_all_active_list_update_alerts().await?;
        self.by_list.clear();
        for (alert, list_update) in &rows {
            let list = match db.get_list_by_id(list_update.list_id).await {
                Ok(Some(list)) => list,
                Ok(None) => {
                    warn!(
                        alert_id = alert.id,
                        list_id = list_update.list_id,
                        "list update alert points at a missing list"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(
                        alert_id = alert.id,
                        list_id = list_update.list_id,
                        "failed to load list for list update alert: {e}"
                    );
                    continue;
                }
            };
            self.by_list
                .entry(list_update.list_id)
                .or_default()
                .push(ListUpdateRule {
                    alert_id: alert.id,
                    list_id: list_update.list_id,
                    cooldown_seconds: alert.cooldown_seconds,
                    last_fired_at: alert.last_fired_at.map(|dt| dt.with_timezone(&Utc)),
                    list_name: list.name,
                });
        }
        Ok(rows.len())
    }
}

pub(crate) struct ListUpdateAlertListener {
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl ListUpdateAlertListener {
    pub(crate) async fn start(
        db: UltrosDb,
        mut list_events: EventBus<ListEventData>,
        mut alert_events: EventBus<alert::Model>,
        ctx: serenity_prelude::Context,
    ) -> Result<Self> {
        let state = Arc::new(Mutex::new(TrackerState::default()));
        let initial = {
            let mut guard = state.lock().await;
            guard.refresh_from_db(&db).await?
        };
        info!("list-update alert tracker started with {initial} alerts");

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
        let state_for_loop = state.clone();
        let db_for_loop = db.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    msg = alert_events.recv() => {
                        match msg {
                            Ok(_) => {
                                if let Err(e) = refresh_state(&state_for_loop, &db_for_loop).await {
                                    error!("list-update tracker refresh failed after alert change: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("list-update tracker lagged, dropped {n} alert events");
                                if let Err(e) = refresh_state(&state_for_loop, &db_for_loop).await {
                                    error!("list-update tracker refresh failed after alert lag: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    msg = list_events.recv() => {
                        match msg {
                            Ok(event) => {
                                handle_list_event(&event, &state_for_loop, &db_for_loop, &ctx).await;
                                if let Err(e) = refresh_state(&state_for_loop, &db_for_loop).await {
                                    error!("list-update tracker refresh failed after list change: {e}");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                error!("list-update tracker lagged, dropped {n} list events");
                                if let Err(e) = refresh_state(&state_for_loop, &db_for_loop).await {
                                    error!("list-update tracker refresh failed after list lag: {e}");
                                }
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

async fn refresh_state(state: &Arc<Mutex<TrackerState>>, db: &UltrosDb) -> Result<()> {
    let mut guard = state.lock().await;
    guard.refresh_from_db(db).await?;
    Ok(())
}

async fn handle_list_event(
    event: &EventType<Arc<ListEventData>>,
    state: &Arc<Mutex<TrackerState>>,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) {
    let Some((list_id, item_id, title_hint, body_hint)) = describe_list_event(event) else {
        return;
    };
    let now = Utc::now();
    let mut to_fire = Vec::new();
    {
        let mut guard = state.lock().await;
        if let Some(rules) = guard.by_list.get_mut(&list_id) {
            for rule in rules.iter_mut() {
                if !is_off_cooldown_at(rule.last_fired_at, rule.cooldown_seconds, now) {
                    continue;
                }
                rule.last_fired_at = Some(now);
                to_fire.push(rule.clone());
            }
        }
    }

    for rule in to_fire {
        let title = format!("List updated: {}", rule.list_name);
        let body = format!(
            "{title_hint}{body_hint}\nhttps://ultros.app/list/{}",
            rule.list_id
        );
        let delivery_result = dispatch_alert(rule.alert_id, &title, &body, db, ctx).await;
        let delivered = delivery_result.is_ok();
        let delivery_error = delivery_result.err().map(|e| e.to_string());

        if let Err(e) = db
            .record_alert_event(
                rule.alert_id,
                item_id.unwrap_or_default(),
                None,
                None,
                delivered,
                delivery_error,
            )
            .await
        {
            error!(
                "failed to record alert_event for list-update alert {}: {e}",
                rule.alert_id
            );
        }
        if delivered && let Err(e) = db.update_alert_last_fired(rule.alert_id).await {
            error!(
                "failed to update last_fired_at for list-update alert {}: {e}",
                rule.alert_id
            );
        }
    }
}

fn describe_list_event(
    event: &EventType<Arc<ListEventData>>,
) -> Option<(i32, Option<i32>, String, String)> {
    match event {
        EventType::Add(data) => match data.as_ref() {
            ListEventData::List(list) => Some((
                list.id,
                None,
                "A list was created: ".to_string(),
                list.name.clone(),
            )),
            ListEventData::ListItem(item) => Some((
                item.list_id,
                Some(item.item_id),
                resolve_item_name(item.item_id),
                " was added.".to_string(),
            )),
        },
        EventType::Update(data) => match data.as_ref() {
            ListEventData::List(list) => Some((
                list.id,
                None,
                "List details changed: ".to_string(),
                list.name.clone(),
            )),
            ListEventData::ListItem(item) => Some((
                item.list_id,
                Some(item.item_id),
                resolve_item_name(item.item_id),
                " was updated.".to_string(),
            )),
        },
        EventType::Remove(data) => match data.as_ref() {
            ListEventData::List(list) => Some((
                list.id,
                None,
                "List was removed: ".to_string(),
                list.name.clone(),
            )),
            ListEventData::ListItem(item) => Some((
                item.list_id,
                Some(item.item_id),
                resolve_item_name(item.item_id),
                " was removed.".to_string(),
            )),
        },
    }
}
