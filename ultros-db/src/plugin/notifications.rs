//! Durable per-device notification deliveries.
use super::auth::{PluginPrincipal, authorize};
use super::{PluginError, Result, exec, lock_account, one, operation_id, query};
use crate::{
    UltrosDb,
    notification_outbox::{GameEnqueue, enqueue_game},
};
use sea_orm::TransactionTrait;
use serde_json::Value as Json;
use ultros_api_types::plugin::*;

fn delivery_ids(value: &DeliveryIds) -> Result<Vec<i64>> {
    if value.ids.len() > 100 {
        return Err(PluginError::Invalid("too_many_deliveries"));
    }
    value
        .ids
        .iter()
        .map(|id| {
            id.parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or(PluginError::Invalid("invalid_delivery"))
        })
        .collect()
}
impl UltrosDb {
    pub async fn pending_plugin_deliveries(&self, user: &PluginPrincipal) -> Result<DeliveryBatch> {
        let txn = self.db.begin().await?;
        let session = authorize(&txn, user, Some("notifications:read")).await?;
        let rows=query(&txn,"SELECT d.* FROM plugin_delivery d JOIN notification_endpoint e ON e.id=d.endpoint_id WHERE d.device_id=$1 AND e.user_id=$2 AND e.disabled_at IS NULL AND e.method='InGame' AND e.config->>'device_id'=$1 AND d.acknowledged_at IS NULL AND d.cancelled_at IS NULL AND d.expires_at>now() ORDER BY d.id LIMIT 100",vec![session.id.into(),user.owner.into()]).await?;
        let deliveries = rows
            .iter()
            .map(|r| {
                Ok(GameDelivery {
                    id: r.try_get::<i64>("", "id")?.to_string(),
                    event_id: r.try_get("", "event_id")?,
                    kind: r.try_get("", "kind")?,
                    title: r.try_get("", "title")?,
                    body: r.try_get("", "body")?,
                    action: serde_json::from_value(r.try_get::<Json>("", "action")?)
                        .map_err(|_| PluginError::Storage)?,
                    expires_at: r.try_get("", "expires_at")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        txn.commit().await?;
        Ok(DeliveryBatch {
            poll_after_ms: if deliveries.len() == 100 {
                1000
            } else {
                60_000
            },
            deliveries,
        })
    }
    pub async fn acknowledge_plugin_deliveries(
        &self,
        user: &PluginPrincipal,
        ids: &DeliveryIds,
        mark_read: bool,
    ) -> Result<()> {
        let ids = delivery_ids(ids)?;
        let txn = self.db.begin().await?;
        let session = authorize(
            &txn,
            user,
            Some(if mark_read {
                "notifications:mark-read"
            } else {
                "notifications:ack"
            }),
        )
        .await?;
        for id in ids {
            if mark_read {
                exec(&txn,"UPDATE alert_event SET read_at=coalesce(read_at,now()) WHERE id IN (SELECT event_id FROM plugin_delivery WHERE id=$1 AND device_id=$2 AND cancelled_at IS NULL) AND alert_id IN (SELECT id FROM alert WHERE owner=$3)",vec![id.into(),session.id.clone().into(),user.owner.into()]).await?;
            } else {
                exec(&txn,"UPDATE plugin_delivery SET acknowledged_at=coalesce(acknowledged_at,now()) WHERE id=$1 AND device_id=$2 AND cancelled_at IS NULL AND expires_at>now()",vec![id.into(),session.id.clone().into()]).await?;
                exec(&txn,"UPDATE alert_event SET delivered=true WHERE id IN (SELECT event_id FROM plugin_delivery WHERE id=$1 AND device_id=$2 AND acknowledged_at IS NOT NULL AND cancelled_at IS NULL)",vec![id.into(),session.id.clone().into()]).await?;
            }
        }
        txn.commit().await?;
        Ok(())
    }
    pub async fn test_plugin_delivery(
        &self,
        user: &PluginPrincipal,
        operation: &str,
    ) -> Result<()> {
        if !operation_id(operation) {
            return Err(PluginError::Invalid("invalid_operation_id"));
        }
        let txn = self.db.begin().await?;
        let session = authorize(&txn, user, Some("notifications:read")).await?;
        let endpoint = session.endpoint_id.ok_or(PluginError::Forbidden)?;
        one(&txn,"SELECT id FROM notification_endpoint WHERE id=$1 AND user_id=$2 AND method='InGame' AND config->>'device_id'=$3 AND disabled_at IS NULL",vec![endpoint.into(),user.owner.into(),session.id.clone().into()]).await?;
        enqueue_game(
            &txn,
            GameEnqueue {
                device: &session.id,
                endpoint,
                event: None,
                operation: &format!("test/{operation}"),
                kind: "test",
                title: "Ultros is connected",
                body: "This notification travelled through your Ultros game endpoint.",
                action: &NotificationAction::None,
                ttl: 300,
            },
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }
    pub async fn enqueue_browser_game_delivery(
        &self,
        owner: i64,
        endpoint: i32,
        event: Option<i64>,
        operation: &str,
        title: &str,
        body: &str,
    ) -> Result<()> {
        let txn = self.db.begin().await?;
        lock_account(&txn, owner).await?;
        let row=one(&txn,"SELECT g.id FROM plugin_grant g JOIN notification_endpoint e ON e.id=g.endpoint_id WHERE e.id=$1 AND e.user_id=$2 AND e.method='InGame' AND e.config->>'device_id'=g.id AND e.disabled_at IS NULL AND g.owner=$2 AND g.revoked_at IS NULL AND g.expires_at>now() AND g.refresh_expires_at>now() AND (g.used_at IS NOT NULL OR g.created_at>now()-interval '10 minutes') AND g.scopes @> '[\"notifications:read\"]'::jsonb FOR UPDATE OF g",vec![endpoint.into(),owner.into()]).await?;
        if let Some(event) = event {
            one(&txn,"SELECT e.id FROM alert_event e JOIN alert a ON a.id=e.alert_id WHERE e.id=$1 AND a.owner=$2",vec![event.into(),owner.into()]).await?;
        }
        let device = row.try_get::<String>("", "id")?;
        enqueue_game(
            &txn,
            GameEnqueue {
                device: &device,
                endpoint,
                event,
                operation,
                kind: if event.is_some() { "resend" } else { "test" },
                title,
                body,
                action: &NotificationAction::None,
                ttl: 300,
            },
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }
}
