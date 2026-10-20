//! At-least-once external dispatch after durable alert acceptance.
use crate::{
    delivery::{
        PushOptions, deliver_legacy_discord_channel, deliver_non_discord_endpoint,
        deliver_to_endpoint, get_serenity_ctx, permanent_failure_reason,
    },
    event::{EventProducer, EventType, NotificationEvent},
    inbox::alert_event_to_api,
};
use std::time::Duration;
use ultros_db::{
    UltrosDb,
    notification_outbox::{AuthorizedDestination, OutboxJob},
};

pub fn start(
    db: UltrosDb,
    notifications: EventProducer<NotificationEvent>,
    token: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut cleanup_at = std::time::Instant::now();
        loop {
            tokio::select! { _=token.cancelled()=>break,_=interval.tick()=>{} }
            if std::time::Instant::now() >= cleanup_at {
                if db.cleanup_notification_outbox().await.is_err() {
                    tracing::warn!("notification retention cleanup unavailable");
                }
                cleanup_at = std::time::Instant::now() + Duration::from_secs(60);
            }
            let jobs = match db
                .claim_notification_outbox(get_serenity_ctx().is_some())
                .await
            {
                Ok(jobs) => jobs,
                Err(_) => {
                    tracing::warn!("notification outbox unavailable");
                    continue;
                }
            };
            futures::future::join_all(jobs.into_iter().map(|job| run(&db, &notifications, job)))
                .await;
        }
    });
}

async fn run(db: &UltrosDb, notifications: &EventProducer<NotificationEvent>, job: OutboxJob) {
    let endpoint = match db.authorize_notification_job(&job).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => {
            let _ = db.finish_notification_job(&job, false, true).await;
            return;
        }
        Err(_) => {
            let _ = db.finish_notification_job(&job, false, false).await;
            return;
        }
    };
    let title = job
        .payload
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Ultros alert");
    let body = job
        .payload
        .get("body")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let policy = &job.payload["policy"];
    let push = PushOptions {
        click_url: policy["click_url"]
            .as_str()
            .unwrap_or("/alerts")
            .to_string(),
        ttl_secs: policy["ttl_secs"].as_u64().unwrap_or(300).min(14_400) as u32,
        topic: policy["topic"].as_str().map(str::to_string),
    };
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        match &endpoint {
            AuthorizedDestination::Endpoint(endpoint) => {
                if let Some(ctx) = get_serenity_ctx() {
                    deliver_to_endpoint(endpoint, title, body, &push, db, &ctx).await
                } else {
                    deliver_non_discord_endpoint(endpoint, title, body, &push, db).await
                }
            }
            AuthorizedDestination::LegacyDiscordChannel { channel_id, owner } => {
                let ctx = get_serenity_ctx().ok_or_else(|| {
                    anyhow::anyhow!("Discord destinations require the bot to be connected")
                })?;
                deliver_legacy_discord_channel(*channel_id, *owner, title, body, &ctx).await
            }
        }
    })
    .await;
    let delivered = matches!(&result, Ok(Ok(())));
    let permanent = matches!(&result,Ok(Err(e)) if permanent_failure_reason(e).is_some());
    if permanent && let AuthorizedDestination::Endpoint(endpoint) = &endpoint {
        let _ = db
            .disable_endpoint_for_delivery_failure(endpoint.id, "Destination unavailable")
            .await;
    }
    // Transport errors may contain webhook URLs. Persist and log classifications only.
    if let Ok(Some(event)) = db.finish_notification_job(&job, delivered, permanent).await {
        let _ = notifications.send(EventType::updated(NotificationEvent {
            owner: job.owner,
            event: alert_event_to_api(event),
        }));
    }
}
