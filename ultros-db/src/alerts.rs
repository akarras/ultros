use crate::UltrosDb;
use crate::entity::*;
use anyhow::Result;
use futures::future::try_join_all;
use sea_orm::*;
use sea_orm::sea_query::Expr;

impl UltrosDb {
    pub async fn get_alert(&self, alert_id: i32) -> Result<Option<alert::Model>> {
        Ok(alert::Entity::find_by_id(alert_id).one(&self.db).await?)
    }

    pub async fn get_alert_discord_destinations(
        &self,
        alert_id: i32,
    ) -> Result<Vec<alert_discord_destination::Model>> {
        Ok(alert_discord_destination::Entity::find()
            .filter(alert_discord_destination::Column::AlertId.eq(alert_id))
            .all(&self.db)
            .await?)
    }

    pub async fn get_retainer_alert(
        &self,
        retainer_alert_id: i32,
    ) -> Result<Option<alert_retainer_undercut::Model>> {
        Ok(
            alert_retainer_undercut::Entity::find_by_id(retainer_alert_id)
                .one(&self.db)
                .await?,
        )
    }

    pub async fn get_all_alerts(&self) -> Result<Vec<alert::Model>> {
        Ok(alert::Entity::find().all(&self.db).await?)
    }

    pub async fn get_retainer_alerts_for_related_alert_id(
        &self,
        alert_id: i32,
    ) -> Result<Vec<alert_retainer_undercut::Model>> {
        Ok(alert_retainer_undercut::Entity::find()
            .filter(alert_retainer_undercut::Column::AlertId.eq(alert_id))
            .all(&self.db)
            .await?)
    }

    pub async fn add_discord_retainer_alert(
        &self,
        channel_id: i64,
        discord_user: i64,
        margin_percent: i32,
    ) -> Result<alert_retainer_undercut::Model> {
        let alert = alert::Entity::insert(alert::ActiveModel {
            id: ActiveValue::default(),
            owner: Set(discord_user),
            enabled: ActiveValue::default(),
            last_fired_at: ActiveValue::default(),
            cooldown_seconds: ActiveValue::default(),
        })
        .exec_with_returning(&self.db)
        .await?;
        let _ = alert_discord_destination::Entity::insert(alert_discord_destination::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            channel_id: Set(channel_id),
        })
        .exec(&self.db)
        .await?;
        let retainer_margin =
            alert_retainer_undercut::Entity::insert(alert_retainer_undercut::ActiveModel {
                id: ActiveValue::default(),
                alert_id: Set(alert.id),
                margin_percent: Set(margin_percent),
            })
            .exec_with_returning(&self.db)
            .await?;
        Ok(retainer_margin)
    }

    /// Attempts to delete the retainer alert from the database. Returns an error if the channel_id/discord_user do not exist in the database
    /// or if another database error occured.
    pub async fn delete_discord_alert(
        &self,
        channel_id: i64,
        discord_user: i64,
    ) -> Result<(alert::Model, Vec<alert_retainer_undercut::Model>)> {
        let (discord, alert) = alert_discord_destination::Entity::find()
            .find_also_related(alert::Entity)
            .filter(
                alert_discord_destination::Column::ChannelId
                    .eq(channel_id)
                    .and(alert::Column::Owner.eq(discord_user)),
            )
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg(
                "Alert not found for this discord channel",
            ))?;
        let alert =
            alert.expect("Since we're querying based on FK we shoudln't ever panic here...");
        // now query to ensure this alert has a retainer undercut associated
        let undercut = alert_retainer_undercut::Entity::find()
            .filter(alert_retainer_undercut::Column::AlertId.eq(alert.id))
            .all(&self.db)
            .await?;
        discord.delete(&self.db).await?;
        let _ = try_join_all(undercut.clone().into_iter().map(|u| u.delete(&self.db))).await?;
        alert.clone().delete(&self.db).await?;
        Ok((alert, undercut))
    }

    /// Create an alert + alert_item_threshold + alert_notification_rule + (if needed) notification_endpoint
    /// in a single transaction.
    pub async fn create_threshold_alert(
        &self,
        owner_discord_user_id: i64,
        item_id: i32,
        world_selector_json: JsonValue,
        price_threshold: i32,
        hq_only: bool,
        cooldown_seconds: i32,
        notification_method: &str,
        notification_config: JsonValue,
        notification_name: &str,
    ) -> Result<alert::Model> {
        use sea_orm::TransactionTrait;
        let txn = self.db.begin().await?;
        let alert = alert::Entity::insert(alert::ActiveModel {
            id: ActiveValue::default(),
            owner: Set(owner_discord_user_id),
            enabled: Set(true),
            last_fired_at: Set(None),
            cooldown_seconds: Set(cooldown_seconds),
        })
        .exec_with_returning(&txn)
        .await?;

        let _ = alert_item_threshold::Entity::insert(alert_item_threshold::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            item_id: Set(item_id),
            world_selector: Set(world_selector_json),
            price_threshold: Set(price_threshold),
            hq_only: Set(hq_only),
        })
        .exec(&txn)
        .await?;

        // Find or create a notification_endpoint with matching method+config for this user
        let endpoint = notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::UserId.eq(owner_discord_user_id))
            .filter(notification_endpoint::Column::Method.eq(notification_method))
            .filter(Expr::cust_with_values(
                "config = ?::jsonb",
                vec![notification_config.clone()],
            ))
            .one(&txn)
            .await?;

        let endpoint_id = match endpoint {
            Some(e) => e.id,
            None => {
                notification_endpoint::Entity::insert(notification_endpoint::ActiveModel {
                    id: ActiveValue::default(),
                    user_id: Set(owner_discord_user_id),
                    name: Set(notification_name.to_string()),
                    method: Set(notification_method.to_string()),
                    config: Set(notification_config),
                    // created_at is DateTimeUtc = DateTime<Utc>
                    created_at: Set(chrono::Utc::now()),
                })
                .exec_with_returning(&txn)
                .await?
                .id
            }
        };

        alert_notification_rule::Entity::insert(alert_notification_rule::ActiveModel {
            alert_id: Set(alert.id),
            endpoint_id: Set(endpoint_id),
        })
        .exec(&txn)
        .await?;

        txn.commit().await?;
        Ok(alert)
    }

    pub async fn get_user_threshold_alerts(
        &self,
        owner_discord_user_id: i64,
    ) -> Result<Vec<(alert::Model, alert_item_threshold::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner_discord_user_id))
            .find_with_related(alert_item_threshold::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    pub async fn get_all_active_threshold_alerts(
        &self,
    ) -> Result<Vec<(alert::Model, alert_item_threshold::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Enabled.eq(true))
            .find_with_related(alert_item_threshold::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    pub async fn set_alert_enabled(
        &self,
        owner: i64,
        alert_id: i32,
        enabled: bool,
    ) -> Result<()> {
        let alert = alert::Entity::find_by_id(alert_id)
            .filter(alert::Column::Owner.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert not found"))?;
        let mut a: alert::ActiveModel = alert.into();
        a.enabled = Set(enabled);
        a.update(&self.db).await?;
        Ok(())
    }

    pub async fn update_threshold_alert_price(
        &self,
        owner: i64,
        alert_id: i32,
        new_price: i32,
    ) -> Result<()> {
        // Ownership check first
        alert::Entity::find_by_id(alert_id)
            .filter(alert::Column::Owner.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert not found"))?;
        let threshold = alert_item_threshold::Entity::find()
            .filter(alert_item_threshold::Column::AlertId.eq(alert_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("threshold not found"))?;
        let mut active: alert_item_threshold::ActiveModel = threshold.into();
        active.price_threshold = Set(new_price);
        active.update(&self.db).await?;
        Ok(())
    }

    pub async fn delete_alert_owned_by(&self, owner: i64, alert_id: i32) -> Result<()> {
        let alert = alert::Entity::find_by_id(alert_id)
            .filter(alert::Column::Owner.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert not found"))?;
        alert.delete(&self.db).await?;
        Ok(())
    }

    pub async fn record_alert_event(
        &self,
        alert_id: i32,
        item_id: i32,
        matched_listing_id: Option<i64>,
        matched_price: Option<i32>,
        delivered: bool,
        delivery_error: Option<String>,
    ) -> Result<()> {
        alert_event::Entity::insert(alert_event::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert_id),
            // fired_at is DateTimeWithTimeZone
            fired_at: Set(chrono::Utc::now().into()),
            item_id: Set(item_id),
            matched_listing_id: Set(matched_listing_id),
            matched_price: Set(matched_price),
            delivered: Set(delivered),
            delivery_error: Set(delivery_error),
        })
        .exec(&self.db)
        .await?;
        Ok(())
    }

    /// Return all notification endpoints linked to an alert via alert_notification_rule.
    pub async fn get_notification_endpoints_for_alert(
        &self,
        alert_id: i32,
    ) -> Result<Vec<notification_endpoint::Model>> {
        let rules = alert_notification_rule::Entity::find()
            .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
            .all(&self.db)
            .await?;

        let endpoint_ids: Vec<i32> = rules.into_iter().map(|r| r.endpoint_id).collect();
        if endpoint_ids.is_empty() {
            return Ok(vec![]);
        }

        Ok(notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::Id.is_in(endpoint_ids))
            .all(&self.db)
            .await?)
    }

    pub async fn update_alert_last_fired(&self, alert_id: i32) -> Result<()> {
        alert::Entity::update_many()
            .col_expr(
                alert::Column::LastFiredAt,
                Expr::value(chrono::Utc::now().fixed_offset()),
            )
            .filter(alert::Column::Id.eq(alert_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn get_recent_alert_events_for_user(
        &self,
        owner: i64,
        limit: u64,
    ) -> Result<Vec<alert_event::Model>> {
        let alert_ids: Vec<i32> = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|a| a.id)
            .collect();
        if alert_ids.is_empty() {
            return Ok(vec![]);
        }
        Ok(alert_event::Entity::find()
            .filter(alert_event::Column::AlertId.is_in(alert_ids))
            .order_by_desc(alert_event::Column::FiredAt)
            .limit(limit)
            .all(&self.db)
            .await?)
    }
}
