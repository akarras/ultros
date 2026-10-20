//! Durable acceptance before external side effects. Account -> device -> alert -> endpoint.
use crate::plugin::{PluginError, Result, exec, lock_account, one, query};
use crate::{
    NewAlertEvent, UltrosDb,
    entity::{alert_event, notification_endpoint},
};
use sea_orm::{DatabaseTransaction, EntityTrait, FromQueryResult, TransactionTrait};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use ultros_api_types::plugin::NotificationAction;

pub struct OutboxJob {
    pub id: i64,
    pub claim: String,
    pub event_id: i64,
    pub owner: i64,
    pub payload: Json,
}

/// Legacy destinations retain their owner ping without exposing mention controls
/// through user-defined endpoint configuration.
pub enum AuthorizedDestination {
    Endpoint(notification_endpoint::Model),
    LegacyDiscordChannel { channel_id: i64, owner: i64 },
}

fn hash_config(method: &str, config: &Json) -> Vec<u8> {
    Sha256::digest(format!("{method}:{config}").as_bytes()).to_vec()
}
fn action(fire: &NewAlertEvent) -> NotificationAction {
    if let Some(id) = fire
        .click_url
        .strip_prefix("/list/")
        .and_then(|id| id.parse::<i32>().ok())
        .filter(|id| *id > 0)
    {
        NotificationAction::OpenList { list_id: id }
    } else if fire.item_id > 0 {
        NotificationAction::OpenItem {
            item_id: fire.item_id,
        }
    } else {
        NotificationAction::None
    }
}

pub(crate) struct GameEnqueue<'a> {
    pub device: &'a str,
    pub endpoint: i32,
    pub event: Option<i64>,
    pub operation: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub action: &'a NotificationAction,
    pub ttl: i32,
}
pub(crate) async fn enqueue_game(txn: &DatabaseTransaction, input: GameEnqueue<'_>) -> Result<()> {
    let GameEnqueue {
        device,
        endpoint,
        event,
        operation,
        kind,
        title,
        body,
        action,
        ttl,
    } = input;
    exec(txn,"INSERT INTO plugin_delivery(device_id,endpoint_id,event_id,operation_id,kind,title,body,action,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,now()+make_interval(secs=>$9::integer)) ON CONFLICT(device_id,operation_id) DO NOTHING",vec![device.into(),endpoint.into(),event.into(),operation.into(),kind.into(),title.into(),body.into(),json!(action).into(),ttl.into()]).await?;
    exec(txn,"UPDATE plugin_delivery SET cancelled_at=now() WHERE id IN (SELECT id FROM plugin_delivery WHERE device_id=$1 AND acknowledged_at IS NULL AND cancelled_at IS NULL ORDER BY id DESC OFFSET 1000)",vec![device.into()]).await?;
    Ok(())
}

impl UltrosDb {
    /// Expire queued work as well as retaining completed receipts. Runs without
    /// requiring game connections to be enabled on this deployment.
    pub async fn cleanup_notification_outbox(&self) -> Result<()> {
        let owners=query(&self.db,"SELECT DISTINCT owner FROM (SELECT a.owner FROM notification_outbox o JOIN alert_event e ON e.id=o.event_id JOIN alert a ON a.id=e.alert_id WHERE (o.expires_at<=now() AND o.completed_at IS NULL AND o.cancelled_at IS NULL) OR o.created_at<now()-interval '7 days' UNION ALL SELECT g.owner FROM plugin_delivery d JOIN plugin_grant g ON g.id=d.device_id WHERE (d.expires_at<=now() AND d.acknowledged_at IS NULL AND d.cancelled_at IS NULL) OR d.created_at<now()-interval '7 days') affected ORDER BY owner LIMIT 100",vec![]).await?;
        for row in owners {
            let owner = row.try_get::<i64>("", "owner")?;
            let txn = self.db.begin().await?;
            lock_account(&txn, owner).await?;
            exec(&txn,"UPDATE notification_outbox SET cancelled_at=now(),last_error='Delivery expired' WHERE event_id IN (SELECT e.id FROM alert_event e JOIN alert a ON a.id=e.alert_id WHERE a.owner=$1) AND expires_at<=now() AND completed_at IS NULL AND cancelled_at IS NULL",vec![owner.into()]).await?;
            exec(&txn,"UPDATE plugin_delivery SET cancelled_at=now() WHERE device_id IN (SELECT id FROM plugin_grant WHERE owner=$1) AND expires_at<=now() AND acknowledged_at IS NULL AND cancelled_at IS NULL",vec![owner.into()]).await?;
            exec(&txn,"DELETE FROM notification_outbox WHERE event_id IN (SELECT e.id FROM alert_event e JOIN alert a ON a.id=e.alert_id WHERE a.owner=$1) AND created_at<now()-interval '7 days'",vec![owner.into()]).await?;
            exec(&txn,"DELETE FROM plugin_delivery WHERE device_id IN (SELECT id FROM plugin_grant WHERE owner=$1) AND created_at<now()-interval '7 days'",vec![owner.into()]).await?;
            txn.commit().await?;
        }
        Ok(())
    }
    pub async fn accept_alert_fire(
        &self,
        owner: i64,
        fire: NewAlertEvent,
        policy: Json,
    ) -> Result<Option<alert_event::Model>> {
        let txn = self.db.begin().await?;
        lock_account(&txn, owner).await?;
        query(
            &txn,
            "SELECT id FROM plugin_grant WHERE owner=$1 ORDER BY id FOR UPDATE",
            vec![owner.into()],
        )
        .await?;
        let row=one(&txn,"SELECT *,last_fired_at IS NOT NULL AND last_fired_at+make_interval(secs=>cooldown_seconds)>now() AS cooling FROM alert WHERE id=$1 AND owner=$2 AND enabled FOR UPDATE",vec![fire.alert_id.into(),owner.into()]).await?;
        if row.try_get::<bool>("", "cooling")? {
            return Ok(None);
        }
        let endpoints=query(&txn,"SELECT e.* FROM notification_endpoint e JOIN alert_notification_rule r ON r.endpoint_id=e.id WHERE r.alert_id=$1 AND e.user_id=$2 AND e.disabled_at IS NULL ORDER BY e.id FOR SHARE OF e,r",vec![fire.alert_id.into(),owner.into()]).await?;
        let legacy = query(
            &txn,
            "SELECT * FROM alert_discord_destination WHERE alert_id=$1 ORDER BY id FOR SHARE",
            vec![fire.alert_id.into()],
        )
        .await?;
        let immediate = endpoints.iter().any(|e| {
            e.try_get::<String>("", "method")
                .is_ok_and(|m| m == "InApp")
        });
        let inserted=one(&txn,"INSERT INTO alert_event(alert_id,item_id,matched_listing_id,matched_price,delivered,title,body,click_url,accepted_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,now()) RETURNING id",vec![fire.alert_id.into(),fire.item_id.into(),fire.matched_listing_id.into(),fire.matched_price.into(),immediate.into(),fire.title.clone().into(),fire.body.clone().into(),fire.click_url.clone().into()]).await?.try_get::<i64>("","id")?;
        let payload = json!({"title":fire.title,"body":fire.body,"policy":policy});
        let ttl = if fire.click_url.starts_with("/list/") {
            3600
        } else {
            300
        };
        let mut channels = std::collections::HashSet::new();
        for endpoint in &endpoints {
            let id = endpoint.try_get::<i32>("", "id")?;
            let method = endpoint.try_get::<String>("", "method")?;
            let config = endpoint.try_get::<Json>("", "config")?;
            if method == "InApp" {
                continue;
            }
            if method == "InGame" {
                let Some(device) = config.get("device_id").and_then(Json::as_str) else {
                    continue;
                };
                let active=query(&txn,"SELECT id FROM plugin_grant WHERE id=$1 AND owner=$2 AND endpoint_id=$3 AND revoked_at IS NULL AND expires_at>now() AND refresh_expires_at>now() AND (used_at IS NOT NULL OR created_at>now()-interval '10 minutes') AND scopes @> '[\"notifications:read\"]'::jsonb",vec![device.into(),owner.into(),id.into()]).await?;
                if !active.is_empty() {
                    enqueue_game(
                        &txn,
                        GameEnqueue {
                            device,
                            endpoint: id,
                            event: Some(inserted),
                            operation: &format!("fire/{inserted}"),
                            kind: "fire",
                            title: &fire.title,
                            body: &fire.body,
                            action: &action(&fire),
                            ttl,
                        },
                    )
                    .await?;
                }
            } else if matches!(
                method.as_str(),
                "DiscordDm" | "DiscordChannel" | "Webhook" | "WebPush"
            ) {
                if method == "DiscordChannel"
                    && let Some(channel) = config.get("channel_id").and_then(Json::as_i64)
                {
                    channels.insert(channel);
                }
                exec(&txn,"INSERT INTO notification_outbox(event_id,endpoint_id,config_hash,payload,expires_at) VALUES($1,$2,$3,$4,now()+interval '4 hours')",vec![inserted.into(),id.into(),hash_config(&method,&config).into(),payload.clone().into()]).await?;
            }
        }
        for row in legacy {
            if channels.contains(&row.try_get::<i64>("", "channel_id")?) {
                continue;
            }
            exec(&txn,"INSERT INTO notification_outbox(event_id,legacy_destination_id,payload,expires_at) VALUES($1,$2,$3,now()+interval '4 hours')",vec![inserted.into(),row.try_get::<i32>("","id")?.into(),payload.clone().into()]).await?;
        }
        exec(
            &txn,
            "UPDATE alert SET last_fired_at=now() WHERE id=$1",
            vec![fire.alert_id.into()],
        )
        .await?;
        let event = alert_event::Entity::find_by_id(inserted)
            .one(&txn)
            .await?
            .ok_or(PluginError::Storage)?;
        txn.commit().await?;
        Ok(Some(event))
    }

    pub async fn claim_notification_outbox(&self, discord_ready: bool) -> Result<Vec<OutboxJob>> {
        let claim = super::plugin::crypto::random(16)?;
        let rows=query(&self.db,"WITH picked AS (SELECT id FROM notification_outbox WHERE completed_at IS NULL AND cancelled_at IS NULL AND expires_at>now() AND attempts<8 AND next_attempt_at<=now() AND (lease_until IS NULL OR lease_until<now()) AND ($2 OR (legacy_destination_id IS NULL AND EXISTS(SELECT 1 FROM notification_endpoint n WHERE n.id=notification_outbox.endpoint_id AND n.method NOT IN ('DiscordDm','DiscordChannel')))) ORDER BY id FOR UPDATE SKIP LOCKED LIMIT 4) UPDATE notification_outbox o SET claim=$1,lease_until=now()+interval '60 seconds',attempts=attempts+1 FROM picked p,alert_event e,alert a WHERE o.id=p.id AND e.id=o.event_id AND a.id=e.alert_id RETURNING o.id,o.event_id,o.payload,a.owner",vec![claim.clone().into(),discord_ready.into()]).await?;
        rows.into_iter()
            .map(|r| {
                Ok(OutboxJob {
                    id: r.try_get("", "id")?,
                    claim: claim.clone(),
                    event_id: r.try_get("", "event_id")?,
                    owner: r.try_get("", "owner")?,
                    payload: r.try_get("", "payload")?,
                })
            })
            .collect()
    }

    /// Commit authorization before network work. A later revoke may race an authorized send.
    pub async fn authorize_notification_job(
        &self,
        job: &OutboxJob,
    ) -> Result<Option<AuthorizedDestination>> {
        let txn = self.db.begin().await?;
        lock_account(&txn, job.owner).await?;
        let row=one(&txn,"SELECT o.* FROM notification_outbox o JOIN alert_event e ON e.id=o.event_id JOIN alert a ON a.id=e.alert_id WHERE o.id=$1 AND o.claim=$2 AND o.lease_until>now() AND o.expires_at>now() AND o.completed_at IS NULL AND o.cancelled_at IS NULL AND a.owner=$3 FOR SHARE OF a,o",vec![job.id.into(),job.claim.clone().into(),job.owner.into()]).await?;
        let endpoint = if let Some(id) = row.try_get::<Option<i32>>("", "endpoint_id")? {
            let rows=query(&txn,"SELECT * FROM notification_endpoint WHERE id=$1 AND user_id=$2 AND disabled_at IS NULL FOR SHARE",vec![id.into(),job.owner.into()]).await?;
            let Some(endpoint) = rows.first() else {
                return Ok(None);
            };
            let endpoint = notification_endpoint::Model::from_query_result(endpoint, "")?;
            if row
                .try_get::<Option<Vec<u8>>>("", "config_hash")?
                .as_deref()
                != Some(hash_config(&endpoint.method, &endpoint.config).as_slice())
            {
                return Ok(None);
            }
            AuthorizedDestination::Endpoint(endpoint)
        } else {
            let id = row
                .try_get::<Option<i32>>("", "legacy_destination_id")?
                .ok_or(PluginError::Storage)?;
            let rows=query(&txn,"SELECT d.channel_id FROM alert_discord_destination d JOIN alert_event e ON e.alert_id=d.alert_id WHERE d.id=$1 AND e.id=$2 FOR SHARE OF d",vec![id.into(),job.event_id.into()]).await?;
            let Some(legacy) = rows.first() else {
                return Ok(None);
            };
            AuthorizedDestination::LegacyDiscordChannel {
                channel_id: legacy.try_get("", "channel_id")?,
                owner: job.owner,
            }
        };
        txn.commit().await?;
        Ok(Some(endpoint))
    }

    pub async fn finish_notification_job(
        &self,
        job: &OutboxJob,
        delivered: bool,
        permanent: bool,
    ) -> Result<Option<alert_event::Model>> {
        let txn = self.db.begin().await?;
        lock_account(&txn, job.owner).await?;
        let changed=exec(&txn,"UPDATE notification_outbox SET completed_at=CASE WHEN $1 THEN now() ELSE NULL END,cancelled_at=CASE WHEN NOT $1 AND ($2 OR attempts>=8) THEN now() ELSE NULL END,next_attempt_at=now()+make_interval(secs=>least(300,5*(1<<least(attempts,6)))::integer),last_error=CASE WHEN $1 THEN NULL ELSE 'Delivery unavailable' END,lease_until=NULL WHERE id=$3 AND claim=$4 AND lease_until>now() AND completed_at IS NULL AND cancelled_at IS NULL",vec![delivered.into(),permanent.into(),job.id.into(),job.claim.clone().into()]).await?;
        if changed == 0 {
            return Ok(None);
        }
        if delivered {
            exec(
                &txn,
                "UPDATE alert_event SET delivered=true,delivery_error=NULL WHERE id=$1",
                vec![job.event_id.into()],
            )
            .await?;
        }
        let event = alert_event::Entity::find_by_id(job.event_id)
            .one(&txn)
            .await?;
        txn.commit().await?;
        Ok(event)
    }
}
