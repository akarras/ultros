//! Dedicated Postgres `LISTEN/NOTIFY` cross-replica wake-up bridge.
//!
//! Follows the four rules in `docs/architecture/service-decomposition.md` §3.2:
//! 1. Subscribe (`LISTEN`) on a dedicated non-transactional connection **before**
//!    initial reads begin; reconcile on every reconnect and on a periodic timer.
//! 2. Carry compact identifiers only (never Loro CRDT updates or raw tokens);
//!    load authoritative state from Postgres on wake-up.
//! 3. Use `PgListener::try_recv()` (never `recv()`) so reconnects (`Ok(None)`)
//!    are observed explicitly rather than silently swallowed.
//! 4. On reconnect, flush the entire local `AuthUserCache` and signal active
//!    WebSocket/SSE/alert subscribers to reconcile against Postgres.

use std::sync::Arc;
use std::time::Duration;

use sea_orm::sqlx::postgres::PgListener;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use ultros_api_types::{
    list::{List, ListActivity},
    user::OwnedRetainer,
    websocket::ListEventData,
};
use ultros_db::{
    UltrosDb,
    auth_revocation::parse_auth_token_hash_hex,
    entity::{alert, alert_retainer_undercut},
    wakeup::{
        ALL_WAKEUP_CHANNELS, AlertWakeup, AlertWakeupKind, AuthRevokeWakeup, CHANNEL_ALERT,
        CHANNEL_AUTH_REVOKE, CHANNEL_LIST_DOC, CHANNEL_NOTIFICATION, CHANNEL_RETAINER,
        ListDocWakeup, NotificationWakeup, NotificationWakeupKind, RetainerWakeup,
    },
};

use crate::alerts::inbox::alert_event_to_api;
use crate::event::{EventSenders, EventType, ListDocEvent, NotificationEvent};
use crate::web::oauth::AuthUserCache;

/// Periodic fallback housekeeping interval (cleans expired token revocations).
const FALLBACK_HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(5 * 60);
const INITIAL_RECONNECT_BACKOFF: Duration = Duration::from_secs(1);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);

/// Connected `PgListener` ready to be spawned as a background task.
///
/// Constructing this via [`PgWakeupBridge::connect`] guarantees `LISTEN` is
/// registered on Postgres before any caller reads initial state.
pub struct PgWakeupBridge {
    database_url: String,
    listener: Option<PgListener>,
    db: UltrosDb,
    user_cache: AuthUserCache,
    senders: EventSenders,
    token: CancellationToken,
}

impl PgWakeupBridge {
    /// Open a dedicated non-transactional Postgres connection and subscribe to
    /// all wake-up channels. If the initial connect fails transiently, returns
    /// a bridge that retries in the background and flushes/reconciles once
    /// connected.
    pub async fn connect(
        database_url: String,
        db: UltrosDb,
        user_cache: AuthUserCache,
        senders: EventSenders,
        token: CancellationToken,
    ) -> Self {
        let listener = match open_and_listen(&database_url).await {
            Ok(listener) => {
                info!(
                    channels = ?ALL_WAKEUP_CHANNELS,
                    origin = db.notify_origin(),
                    "Postgres wakeup listener subscribed"
                );
                Some(listener)
            }
            Err(error) => {
                warn!(
                    ?error,
                    "initial Postgres wakeup listener connect failed; retrying in background"
                );
                None
            }
        };
        Self {
            database_url,
            listener,
            db,
            user_cache,
            senders,
            token,
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        let mut backoff = INITIAL_RECONNECT_BACKOFF;
        let mut housekeeping = tokio::time::interval(FALLBACK_HOUSEKEEPING_INTERVAL);
        housekeeping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate startup tick so we don't run cleanup on cold boot.
        housekeeping.tick().await;

        loop {
            if self.token.is_cancelled() {
                break;
            }
            let Some(listener) = self.listener.as_mut() else {
                tokio::select! {
                    _ = self.token.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {}
                }
                match open_and_listen(&self.database_url).await {
                    Ok(listener) => {
                        info!("Postgres wakeup listener re-established");
                        self.listener = Some(listener);
                        backoff = INITIAL_RECONNECT_BACKOFF;
                        handle_reconnect(&self.user_cache, &self.senders).await;
                    }
                    Err(error) => {
                        warn!(
                            ?error,
                            ?backoff,
                            "Postgres wakeup listener reconnect failed"
                        );
                        backoff = (backoff * 2).min(MAX_RECONNECT_BACKOFF);
                    }
                }
                continue;
            };

            tokio::select! {
                _ = self.token.cancelled() => break,
                _ = housekeeping.tick() => {
                    if let Err(error) = self.db.cleanup_expired_auth_token_revocations().await {
                        warn!(?error, "failed to clean up expired auth token revocations");
                    }
                }
                result = listener.try_recv() => match result {
                    Ok(Some(notification)) => {
                        backoff = INITIAL_RECONNECT_BACKOFF;
                        dispatch_notification(
                            notification.channel(),
                            notification.payload(),
                            &self.db,
                            &self.user_cache,
                            &self.senders,
                        )
                        .await;
                    }
                    Ok(None) => {
                        // sqlx's PgListener transparently reconnected after a
                        // connection drop; notifications during the gap were lost.
                        warn!("Postgres wakeup listener reconnected; flushing auth cache and reconciling");
                        backoff = INITIAL_RECONNECT_BACKOFF;
                        handle_reconnect(&self.user_cache, &self.senders).await;
                    }
                    Err(error) => {
                        warn!(?error, "Postgres wakeup listener receive error; reconnecting");
                        self.listener = None;
                    }
                }
            }
        }
        debug!("Postgres wakeup listener stopped");
    }
}

async fn open_and_listen(database_url: &str) -> Result<PgListener, sea_orm::sqlx::Error> {
    let mut listener = PgListener::connect(database_url).await?;
    listener.listen_all(ALL_WAKEUP_CHANNELS).await?;
    Ok(listener)
}

async fn handle_reconnect(user_cache: &AuthUserCache, senders: &EventSenders) {
    metrics::counter!("ultros_pg_wakeup_reconnects_total").increment(1);
    // Rule 4: flush local auth cache completely so any logout/deletion missed
    // during the disconnect forces authoritative Postgres re-check.
    user_cache.clear().await;
    crate::web::plugin::wake_all_notifications_local();
    let _ = senders.wakeup_resync.send(());
    // Prompt alert trackers (PriceAlertListener, MarketTriggerListener,
    // ListUpdateAlertListener, RetainerSaleListener) to reload from Postgres.
    // AlertManager ignores `EventType::Add` on `alerts` and reconciles undercut
    // listeners via `wakeup_resync`.
    let _ = senders
        .alerts
        .send(EventType::added(reconcile_alert_tombstone(0)));
}

fn reconcile_alert_tombstone(alert_id: i32) -> alert::Model {
    alert::Model {
        id: alert_id,
        owner: 0,
        enabled: false,
        last_fired_at: None,
        cooldown_seconds: 0,
    }
}

fn retainer_wake_event(owner: i64) -> OwnedRetainer {
    OwnedRetainer {
        id: 0,
        retainer_id: 0,
        discord_id: owner,
        character_id: None,
        weight: None,
    }
}

pub(crate) async fn dispatch_notification(
    channel: &str,
    payload: &str,
    db: &UltrosDb,
    user_cache: &AuthUserCache,
    senders: &EventSenders,
) {
    let local_origin = db.notify_origin();
    match channel {
        CHANNEL_AUTH_REVOKE => {
            let Ok(msg) = serde_json::from_str::<AuthRevokeWakeup>(payload) else {
                warn!(channel, "ignoring malformed auth revoke wakeup");
                return;
            };
            if msg.o == local_origin {
                return;
            }
            if let Some(hex) = msg.t.as_deref() {
                if let Some(hash) = parse_auth_token_hash_hex(hex) {
                    user_cache.remove_token_hash(&hash).await;
                } else {
                    warn!("ignoring malformed token hash in auth revoke wakeup");
                }
            }
            if let Some(user_id) = msg.u
                && let Ok(uid) = u64::try_from(user_id)
            {
                user_cache.remove_user(uid).await;
            }
        }
        CHANNEL_LIST_DOC => {
            let Ok(msg) = serde_json::from_str::<ListDocWakeup>(payload) else {
                warn!(channel, "ignoring malformed list wakeup");
                return;
            };
            if msg.o == local_origin {
                return;
            }
            let _ = senders
                .list_docs
                .send(EventType::Update(Arc::new(ListDocEvent::remote_wake(
                    msg.l,
                ))));
            if let Ok(Some(list)) = db.get_list_by_id(msg.l).await
                && let Ok(api_list) = List::try_from(list)
            {
                let _ = senders
                    .lists
                    .send(EventType::updated(ListEventData::List(api_list)));
            }
            for activity_id in msg.a {
                if let Ok(Some(activity)) = db.get_list_activity_by_id(activity_id).await {
                    let _ = senders.lists.send(EventType::added(ListEventData::Activity(
                        ListActivity::from(activity),
                    )));
                }
            }
        }
        CHANNEL_NOTIFICATION => {
            let Ok(msg) = serde_json::from_str::<NotificationWakeup>(payload) else {
                warn!(channel, "ignoring malformed notification wakeup");
                return;
            };
            if msg.o == local_origin {
                return;
            }
            match msg.k {
                NotificationWakeupKind::PluginWake => {
                    crate::web::plugin::wake_notifications_local(msg.u);
                }
                NotificationWakeupKind::Add | NotificationWakeupKind::Update => {
                    let Some(event_id) = msg.e else {
                        return;
                    };
                    match db.get_alert_event_by_id_owned_by(msg.u, event_id).await {
                        Ok(model) => {
                            let notif = NotificationEvent {
                                owner: msg.u,
                                event: alert_event_to_api(model),
                            };
                            let ev = match msg.k {
                                NotificationWakeupKind::Add => EventType::added(notif),
                                NotificationWakeupKind::Update => EventType::updated(notif),
                                NotificationWakeupKind::PluginWake => unreachable!(),
                            };
                            let _ = senders.notifications.send(ev);
                        }
                        Err(error) => {
                            warn!(
                                ?error,
                                owner = msg.u,
                                event_id,
                                "failed to load alert_event on remote wakeup"
                            );
                            // Row may already be deleted or transiently unreachable;
                            // still wake local plugin SSE so durable HTTP poll reconciles.
                            crate::web::plugin::wake_notifications_local(msg.u);
                        }
                    }
                }
            }
        }
        CHANNEL_ALERT => {
            let Ok(msg) = serde_json::from_str::<AlertWakeup>(payload) else {
                warn!(channel, "ignoring malformed alert wakeup");
                return;
            };
            if msg.o == local_origin {
                return;
            }
            match msg.k {
                AlertWakeupKind::Add | AlertWakeupKind::Update => {
                    match db.get_alert(msg.a).await {
                        Ok(Some(alert_model)) => {
                            let ev = match msg.k {
                                AlertWakeupKind::Add => EventType::added(alert_model),
                                AlertWakeupKind::Update => EventType::updated(alert_model),
                                AlertWakeupKind::Remove => unreachable!(),
                            };
                            let _ = senders.alerts.send(ev);
                            if msg.k == AlertWakeupKind::Add
                                && let Ok(undercuts) =
                                    db.get_retainer_alerts_for_related_alert_id(msg.a).await
                            {
                                for undercut in undercuts {
                                    let _ =
                                        senders.retainer_undercut.send(EventType::added(undercut));
                                }
                            }
                        }
                        Ok(None) => {
                            // Alert was deleted before we loaded it; treat as removal.
                            let _ = senders
                                .alerts
                                .send(EventType::removed(reconcile_alert_tombstone(msg.a)));
                        }
                        Err(error) => {
                            warn!(
                                ?error,
                                alert_id = msg.a,
                                "failed to load alert on remote wakeup"
                            );
                        }
                    }
                }
                AlertWakeupKind::Remove => {
                    let _ = senders
                        .alerts
                        .send(EventType::removed(reconcile_alert_tombstone(msg.a)));
                    for ru_id in msg.ru {
                        let _ = senders.retainer_undercut.send(EventType::removed(
                            alert_retainer_undercut::Model {
                                id: ru_id,
                                alert_id: msg.a,
                                margin_percent: 0,
                            },
                        ));
                    }
                }
            }
        }
        CHANNEL_RETAINER => {
            let Ok(msg) = serde_json::from_str::<RetainerWakeup>(payload) else {
                warn!(channel, "ignoring malformed retainer wakeup");
                return;
            };
            if msg.o == local_origin {
                return;
            }
            let _ = senders
                .retainers
                .send(EventType::updated(retainer_wake_event(msg.u)));
        }
        other => {
            debug!(channel = other, "ignoring unknown wakeup channel");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::create_event_busses;
    use crate::web::oauth::AuthDiscordUser;
    use ultros_db::auth_revocation::{auth_token_hash_hex, hash_auth_token};

    async fn sqlite_db() -> UltrosDb {
        let conn = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        UltrosDb::from_connection(conn)
    }

    fn user(id: u64) -> AuthDiscordUser {
        AuthDiscordUser {
            id,
            name: format!("user-{id}"),
            avatar_url: String::new(),
        }
    }

    #[tokio::test]
    async fn dispatch_ignores_self_echo_and_evicts_remote_auth_revocations() {
        let db = sqlite_db().await;
        let cache = AuthUserCache::new();
        let (senders, mut receivers) = create_event_busses();

        cache.store_user("token-a", user(10)).await;
        cache.store_user("token-b", user(20)).await;

        // Self-echo must be ignored.
        let self_payload = serde_json::to_string(&AuthRevokeWakeup {
            o: db.notify_origin().to_string(),
            t: Some(auth_token_hash_hex(&hash_auth_token("token-a"))),
            u: Some(20),
        })
        .unwrap();
        dispatch_notification(CHANNEL_AUTH_REVOKE, &self_payload, &db, &cache, &senders).await;
        assert!(cache.get_user("token-a").await.is_some());
        assert!(cache.get_user("token-b").await.is_some());

        // Remote token + user revocation evicts both immediately.
        let remote_payload = serde_json::to_string(&AuthRevokeWakeup {
            o: "remote0000000001".into(),
            t: Some(auth_token_hash_hex(&hash_auth_token("token-a"))),
            u: Some(20),
        })
        .unwrap();
        dispatch_notification(CHANNEL_AUTH_REVOKE, &remote_payload, &db, &cache, &senders).await;
        assert!(cache.get_user("token-a").await.is_none());
        assert!(cache.get_user("token-b").await.is_none());

        // Remote list wake emits a compact remote_wake ListDocEvent.
        let list_payload = serde_json::to_string(&ListDocWakeup {
            o: "remote0000000001".into(),
            l: 42,
            a: vec![],
        })
        .unwrap();
        dispatch_notification(CHANNEL_LIST_DOC, &list_payload, &db, &cache, &senders).await;
        let doc_event = receivers.list_docs.try_recv().unwrap();
        assert_eq!(doc_event.as_ref().list_id, 42);
        assert!(doc_event.as_ref().remote_wake);
        assert!(doc_event.as_ref().update.is_empty());

        // Remote alert remove emits tombstones for both alerts and retainer_undercut.
        let alert_payload = serde_json::to_string(&AlertWakeup {
            o: "remote0000000001".into(),
            a: 99,
            k: AlertWakeupKind::Remove,
            ru: vec![7],
        })
        .unwrap();
        dispatch_notification(CHANNEL_ALERT, &alert_payload, &db, &cache, &senders).await;
        assert!(matches!(
            receivers.alerts.try_recv().unwrap(),
            EventType::Remove(a) if a.id == 99
        ));
        assert!(matches!(
            receivers.retainer_undercut.try_recv().unwrap(),
            EventType::Remove(ru) if ru.id == 7 && ru.alert_id == 99
        ));

        // Remote retainer wake publishes OwnedRetainer for the owner.
        let retainer_payload = serde_json::to_string(&RetainerWakeup {
            o: "remote0000000001".into(),
            u: 555,
        })
        .unwrap();
        dispatch_notification(CHANNEL_RETAINER, &retainer_payload, &db, &cache, &senders).await;
        assert_eq!(
            receivers.retainers.try_recv().unwrap().as_ref().discord_id,
            555
        );
    }

    #[tokio::test]
    async fn reconnect_flushes_entire_auth_cache_and_signals_resync() {
        let cache = AuthUserCache::new();
        let (senders, mut receivers) = create_event_busses();
        cache.store_user("token-1", user(1)).await;
        cache.store_user("token-2", user(2)).await;

        handle_reconnect(&cache, &senders).await;

        assert!(cache.get_user("token-1").await.is_none());
        assert!(cache.get_user("token-2").await.is_none());
        assert!(receivers.wakeup_resync.try_recv().is_ok());
        assert!(receivers.alerts.try_recv().is_ok());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn live_postgres_bridge_delivers_cross_replica_wakeups() {
        use migration::{Migrator, MigratorTrait};
        use sea_orm::{ConnectOptions, ConnectionTrait, Database};

        let url = std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let schema = format!("wakeup_bridge_{}_{nonce}", std::process::id());

        // Keep `search_path` pinned to one session per test pool.
        let mut opt = ConnectOptions::new(url.clone());
        opt.max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let conn_a = Database::connect(opt.clone()).await.unwrap();
        conn_a
            .execute_unprepared(&format!(
                "CREATE SCHEMA {schema}; SET search_path TO {schema}"
            ))
            .await
            .unwrap();
        Migrator::up(&conn_a, None).await.unwrap();

        let conn_b = Database::connect(opt).await.unwrap();
        conn_b
            .execute_unprepared(&format!("SET search_path TO {schema}"))
            .await
            .unwrap();

        let db_a = UltrosDb::from_connection(conn_a.clone());
        let db_b = UltrosDb::from_connection(conn_b);
        assert_ne!(db_a.notify_origin(), db_b.notify_origin());

        let cache_b = AuthUserCache::new();
        cache_b.store_user("shared-session-token", user(42)).await;
        let (senders_b, mut receivers_b) = create_event_busses();
        let token = CancellationToken::new();

        let bridge =
            PgWakeupBridge::connect(url, db_b, cache_b.clone(), senders_b, token.clone()).await;
        let handle = bridge.spawn();

        // Replica A revokes the token durably in Postgres + emits NOTIFY.
        let hash = hash_auth_token("shared-session-token");
        db_a.revoke_auth_token(&hash, 3600).await.unwrap();

        // Replica B's local cache must evict promptly.
        tokio::time::timeout(Duration::from_secs(5), async {
            while cache_b.get_user("shared-session-token").await.is_some() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("remote replica B must evict revoked token");

        // Replica A signals a list change; Replica B emits remote_wake.
        db_a.notify_list_doc_changed(123, &[]).await;
        let ev = tokio::time::timeout(Duration::from_secs(5), receivers_b.list_docs.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.as_ref().list_id, 123);
        assert!(ev.as_ref().remote_wake);

        token.cancel();
        let _ = handle.await;
        conn_a
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }
}
