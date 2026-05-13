use crate::UltrosDb;
use crate::entity::*;
use anyhow::Result;
use futures::future::try_join_all;
use sea_orm::sea_query::Expr;
use sea_orm::*;

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
    #[allow(clippy::too_many_arguments)]
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
                "config::jsonb = ?::jsonb",
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

    pub async fn set_alert_enabled(&self, owner: i64, alert_id: i32, enabled: bool) -> Result<()> {
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

    pub async fn get_first_endpoint_for_alert(
        &self,
        alert_id: i32,
    ) -> Result<Option<notification_endpoint::Model>> {
        let rule = alert_notification_rule::Entity::find()
            .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
            .limit(1)
            .one(&self.db)
            .await?;
        if let Some(rule) = rule {
            Ok(notification_endpoint::Entity::find_by_id(rule.endpoint_id)
                .one(&self.db)
                .await?)
        } else {
            Ok(None)
        }
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

    /// Fetch an alert event by id, but only if the underlying alert is owned by `owner`.
    /// Returns `Err` for both "no such event" and "event belongs to someone else" so the
    /// caller doesn't leak existence to non-owners.
    pub async fn get_alert_event_by_id_owned_by(
        &self,
        owner: i64,
        event_id: i64,
    ) -> Result<alert_event::Model> {
        let event = alert_event::Entity::find_by_id(event_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert event not found"))?;
        alert::Entity::find_by_id(event.alert_id)
            .filter(alert::Column::Owner.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert event not found"))?;
        Ok(event)
    }

    pub async fn get_recent_alert_events_for_user(
        &self,
        owner: i64,
        limit: u64,
    ) -> Result<Vec<alert_event::Model>> {
        Ok(alert_event::Entity::find()
            .inner_join(alert::Entity)
            .filter(alert::Column::Owner.eq(owner))
            .order_by_desc(alert_event::Column::FiredAt)
            .limit(limit)
            .all(&self.db)
            .await?)
    }

    pub async fn list_endpoints(&self, owner: i64) -> Result<Vec<notification_endpoint::Model>> {
        Ok(notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .order_by_asc(notification_endpoint::Column::Id)
            .all(&self.db)
            .await?)
    }

    pub async fn create_endpoint(
        &self,
        owner: i64,
        name: &str,
        method: &str,
        config: JsonValue,
    ) -> Result<i32> {
        let model = notification_endpoint::Entity::insert(notification_endpoint::ActiveModel {
            id: ActiveValue::default(),
            user_id: Set(owner),
            name: Set(name.to_string()),
            method: Set(method.to_string()),
            config: Set(config),
            created_at: Set(chrono::Utc::now()),
        })
        .exec_with_returning(&self.db)
        .await?;
        Ok(model.id)
    }

    pub async fn update_endpoint(
        &self,
        owner: i64,
        endpoint_id: i32,
        name: Option<String>,
        method_and_config: Option<(String, JsonValue)>,
    ) -> Result<()> {
        let existing = notification_endpoint::Entity::find_by_id(endpoint_id)
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("endpoint not found"))?;
        let mut active: notification_endpoint::ActiveModel = existing.into();
        if let Some(n) = name {
            active.name = Set(n);
        }
        if let Some((m, c)) = method_and_config {
            active.method = Set(m);
            active.config = Set(c);
        }
        active.update(&self.db).await?;
        Ok(())
    }

    pub async fn delete_endpoint(&self, owner: i64, endpoint_id: i32) -> Result<()> {
        let existing = notification_endpoint::Entity::find_by_id(endpoint_id)
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("endpoint not found"))?;
        existing.delete(&self.db).await?;
        Ok(())
    }

    pub async fn get_endpoint_owned_by(
        &self,
        owner: i64,
        endpoint_id: i32,
    ) -> Result<notification_endpoint::Model> {
        notification_endpoint::Entity::find_by_id(endpoint_id)
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("endpoint not found"))
    }

    /// Replace the set of endpoint rules for an alert with the provided list.
    /// Verifies all endpoints belong to `owner` and the alert belongs to `owner`.
    pub async fn set_alert_rules(
        &self,
        owner: i64,
        alert_id: i32,
        endpoint_ids: &[i32],
    ) -> Result<()> {
        use sea_orm::TransactionTrait;
        // Ownership check on the alert
        alert::Entity::find_by_id(alert_id)
            .filter(alert::Column::Owner.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert not found"))?;
        // Ownership check on every endpoint id (no orphans, no cross-user)
        for &eid in endpoint_ids {
            notification_endpoint::Entity::find_by_id(eid)
                .filter(notification_endpoint::Column::UserId.eq(owner))
                .one(&self.db)
                .await?
                .ok_or_else(|| anyhow::Error::msg(format!("endpoint {eid} not owned by user")))?;
        }
        let txn = self.db.begin().await?;
        alert_notification_rule::Entity::delete_many()
            .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
            .exec(&txn)
            .await?;
        for &eid in endpoint_ids {
            alert_notification_rule::Entity::insert(alert_notification_rule::ActiveModel {
                alert_id: Set(alert_id),
                endpoint_id: Set(eid),
            })
            .exec(&txn)
            .await?;
        }
        txn.commit().await?;
        Ok(())
    }

    /// Create an alert + alert_item_threshold in a single transaction, without
    /// creating any notification endpoint or rules. The caller is expected to
    /// bind endpoint rules via `set_alert_rules` afterward.
    pub async fn create_threshold_alert_without_endpoint(
        &self,
        owner: i64,
        item_id: i32,
        world_selector_json: JsonValue,
        price_threshold: i32,
        hq_only: bool,
        cooldown_seconds: i32,
    ) -> Result<alert::Model> {
        use sea_orm::TransactionTrait;
        let txn = self.db.begin().await?;
        let alert = alert::Entity::insert(alert::ActiveModel {
            id: ActiveValue::default(),
            owner: Set(owner),
            enabled: Set(true),
            last_fired_at: Set(None),
            cooldown_seconds: Set(cooldown_seconds),
        })
        .exec_with_returning(&txn)
        .await?;
        alert_item_threshold::Entity::insert(alert_item_threshold::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            item_id: Set(item_id),
            world_selector: Set(world_selector_json),
            price_threshold: Set(price_threshold),
            hq_only: Set(hq_only),
        })
        .exec(&txn)
        .await?;
        txn.commit().await?;
        Ok(alert)
    }

    /// Create an alert + alert_list_threshold in a single transaction and bind
    /// the supplied notification endpoints. Caller MUST have already checked
    /// that `owner` has at least `Read` permission on `list_id`.
    pub async fn create_list_threshold_alert(
        &self,
        owner: i64,
        list_id: i32,
        cooldown_seconds: i32,
        endpoint_ids: &[i32],
    ) -> Result<alert::Model> {
        use sea_orm::TransactionTrait;
        // Ownership check on every endpoint id before we open a transaction.
        for &eid in endpoint_ids {
            notification_endpoint::Entity::find_by_id(eid)
                .filter(notification_endpoint::Column::UserId.eq(owner))
                .one(&self.db)
                .await?
                .ok_or_else(|| anyhow::Error::msg(format!("endpoint {eid} not owned by user")))?;
        }
        let txn = self.db.begin().await?;
        let alert = alert::Entity::insert(alert::ActiveModel {
            id: ActiveValue::default(),
            owner: Set(owner),
            enabled: Set(true),
            last_fired_at: Set(None),
            cooldown_seconds: Set(cooldown_seconds),
        })
        .exec_with_returning(&txn)
        .await?;
        alert_list_threshold::Entity::insert(alert_list_threshold::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            list_id: Set(list_id),
        })
        .exec(&txn)
        .await?;
        for &eid in endpoint_ids {
            alert_notification_rule::Entity::insert(alert_notification_rule::ActiveModel {
                alert_id: Set(alert.id),
                endpoint_id: Set(eid),
            })
            .exec(&txn)
            .await?;
        }
        txn.commit().await?;
        Ok(alert)
    }

    /// Return the user's list-threshold alerts (alert row + junction row).
    pub async fn get_user_list_threshold_alerts(
        &self,
        owner: i64,
    ) -> Result<Vec<(alert::Model, alert_list_threshold::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .find_with_related(alert_list_threshold::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    /// Return all enabled list-threshold alerts. Used by the price tracker on
    /// each `refresh_from` to rebuild its in-memory index.
    pub async fn get_all_active_list_threshold_alerts(
        &self,
    ) -> Result<Vec<(alert::Model, alert_list_threshold::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Enabled.eq(true))
            .find_with_related(alert_list_threshold::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    /// Return the endpoint ids attached to an alert, in order of attachment.
    pub async fn list_endpoint_ids_for_alert(&self, alert_id: i32) -> Result<Vec<i32>> {
        let rules = alert_notification_rule::Entity::find()
            .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
            .all(&self.db)
            .await?;
        Ok(rules.into_iter().map(|r| r.endpoint_id).collect())
    }

    /// Find an existing endpoint owned by `owner` whose method+config matches; otherwise
    /// create a new one. Returns the endpoint id. Used by bot commands to bind alerts to
    /// the caller's default DM endpoint without dup-ing rows on repeat use.
    pub async fn get_or_create_dm_endpoint(&self, owner: i64, name: &str) -> Result<i32> {
        let cfg = serde_json::json!({ "user_id": owner });
        if let Some(existing) = notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .filter(notification_endpoint::Column::Method.eq("DiscordDm"))
            .filter(Expr::cust_with_values(
                "config::jsonb = ?::jsonb",
                vec![cfg.clone()],
            ))
            .one(&self.db)
            .await?
        {
            return Ok(existing.id);
        }
        self.create_endpoint(owner, name, "DiscordDm", cfg).await
    }

    /// Same as `get_or_create_dm_endpoint` but for a DiscordChannel pointed at `channel_id`.
    /// Optional `channel_name`/`guild_id`/`guild_name` are persisted alongside the id so
    /// the web UI can render a friendly label later instead of raw "Channel <id>".
    ///
    /// Dedupe is keyed strictly on `channel_id` (extracted from JSONB) so a second
    /// invocation that supplies *more* metadata than the first still hits the existing
    /// row instead of creating a duplicate.
    pub async fn get_or_create_channel_endpoint(
        &self,
        owner: i64,
        channel_id: i64,
        name: &str,
        channel_name: Option<&str>,
        guild_id: Option<i64>,
        guild_name: Option<&str>,
    ) -> Result<i32> {
        if let Some(existing) = notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .filter(notification_endpoint::Column::Method.eq("DiscordChannel"))
            .filter(Expr::cust_with_values(
                "(config->>'channel_id')::bigint = ?",
                vec![channel_id],
            ))
            .one(&self.db)
            .await?
        {
            return Ok(existing.id);
        }
        let mut cfg = serde_json::Map::new();
        cfg.insert("channel_id".into(), serde_json::json!(channel_id));
        if let Some(cn) = channel_name {
            cfg.insert("channel_name".into(), serde_json::json!(cn));
        }
        if let Some(gid) = guild_id {
            cfg.insert("guild_id".into(), serde_json::json!(gid));
        }
        if let Some(gn) = guild_name {
            cfg.insert("guild_name".into(), serde_json::json!(gn));
        }
        self.create_endpoint(
            owner,
            name,
            "DiscordChannel",
            serde_json::Value::Object(cfg),
        )
        .await
    }

    /// Same as `get_or_create_dm_endpoint` but for a WebPush endpoint pointing
    /// at a persisted browser subscription.
    pub async fn get_or_create_webpush_endpoint(
        &self,
        owner: i64,
        subscription_id: i32,
        name: &str,
    ) -> Result<i32> {
        let cfg = serde_json::json!({ "subscription_id": subscription_id });
        if let Some(existing) = notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .filter(notification_endpoint::Column::Method.eq("WebPush"))
            .filter(Expr::cust_with_values(
                "config::jsonb = ?::jsonb",
                vec![cfg.clone()],
            ))
            .one(&self.db)
            .await?
        {
            return Ok(existing.id);
        }
        self.create_endpoint(owner, name, "WebPush", cfg).await
    }

    /// Insert (or upsert) a per-browser Web Push subscription. The unique key is
    /// `(user_id, endpoint)` — browsers may rotate `p256dh`/`auth` on the same
    /// endpoint URL, so we update those + `last_seen_at` on conflict rather than
    /// erroring. Returns the row id (new or existing).
    pub async fn create_push_subscription(
        &self,
        owner: i64,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        user_agent: Option<&str>,
    ) -> Result<i32> {
        use migration::OnConflict;
        let now = chrono::Utc::now();
        let model = push_subscription::Entity::insert(push_subscription::ActiveModel {
            id: ActiveValue::default(),
            user_id: Set(owner),
            endpoint: Set(endpoint.to_string()),
            p256dh: Set(p256dh.to_string()),
            auth: Set(auth.to_string()),
            user_agent: Set(user_agent.map(|s| s.to_string())),
            created_at: Set(now),
            last_seen_at: Set(now),
        })
        .on_conflict(
            OnConflict::columns([
                push_subscription::Column::UserId,
                push_subscription::Column::Endpoint,
            ])
            .update_columns([
                push_subscription::Column::P256dh,
                push_subscription::Column::Auth,
                push_subscription::Column::UserAgent,
                push_subscription::Column::LastSeenAt,
            ])
            .to_owned(),
        )
        .exec_with_returning(&self.db)
        .await?;
        Ok(model.id)
    }

    /// Look up a single push subscription by id (no ownership check — callers
    /// that need one should filter by `user_id` themselves, or use this from
    /// internal delivery code that has already authorized the operation).
    pub async fn get_push_subscription_by_id(&self, id: i32) -> Result<push_subscription::Model> {
        push_subscription::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("push subscription not found"))
    }

    /// Delete a push subscription owned by `owner`. Used both when a user
    /// explicitly removes their browser endpoint and when delivery discovers
    /// the subscription has been revoked by the push service.
    pub async fn delete_push_subscription_by_id(&self, owner: i64, id: i32) -> Result<()> {
        let existing = push_subscription::Entity::find_by_id(id)
            .filter(push_subscription::Column::UserId.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("push subscription not found"))?;
        existing.delete(&self.db).await?;
        Ok(())
    }

    /// Bump `last_seen_at` after a successful push delivery. Best-effort —
    /// callers ignore the result.
    pub async fn touch_push_subscription_last_seen(&self, id: i32) -> Result<()> {
        push_subscription::Entity::update_many()
            .filter(push_subscription::Column::Id.eq(id))
            .col_expr(
                push_subscription::Column::LastSeenAt,
                Expr::value(chrono::Utc::now()),
            )
            .exec(&self.db)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;

    /// Connect to a live test database. No `test_helpers::test_db` convention
    /// currently exists in this crate (see CLAUDE.md / ultros-db tests); these
    /// tests are therefore `#[ignore]`d and only exercised when a developer
    /// runs them explicitly with `DATABASE_URL` pointed at a disposable DB:
    ///
    /// ```bash
    /// cargo test -p ultros-db endpoint_tests -- --ignored --test-threads=1
    /// ```
    async fn test_db() -> UltrosDb {
        UltrosDb::connect().await.expect("connect to test DB")
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn create_endpoint_and_list_returns_it() {
        let db = test_db().await;
        let id = db
            .create_endpoint(42, "My DM", "DiscordDm", serde_json::json!({"user_id": 42}))
            .await
            .unwrap();
        let list = db.list_endpoints(42).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].name, "My DM");
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn list_endpoints_scopes_by_user() {
        let db = test_db().await;
        db.create_endpoint(1, "A", "DiscordDm", serde_json::json!({"user_id": 1}))
            .await
            .unwrap();
        db.create_endpoint(2, "B", "DiscordDm", serde_json::json!({"user_id": 2}))
            .await
            .unwrap();
        let only_user_1 = db.list_endpoints(1).await.unwrap();
        assert_eq!(only_user_1.len(), 1);
        assert_eq!(only_user_1[0].user_id, 1);
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn delete_endpoint_refuses_other_users_endpoint() {
        let db = test_db().await;
        let id = db
            .create_endpoint(1, "A", "DiscordDm", serde_json::json!({"user_id": 1}))
            .await
            .unwrap();
        let err = db.delete_endpoint(2, id).await;
        assert!(err.is_err(), "expected delete by non-owner to fail");
        // and the row should still be there
        assert_eq!(db.list_endpoints(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn set_alert_rules_replaces_the_set() {
        let db = test_db().await;
        let e1 = db
            .create_endpoint(1, "A", "DiscordDm", serde_json::json!({"user_id": 1}))
            .await
            .unwrap();
        let e2 = db
            .create_endpoint(
                1,
                "B",
                "Webhook",
                serde_json::json!({"url": "https://discord.com/api/webhooks/1/x"}),
            )
            .await
            .unwrap();
        let alert = db
            .create_threshold_alert(
                1,
                5057,
                serde_json::json!({"World": 22}),
                1000,
                false,
                3600,
                "DiscordDm",
                serde_json::json!({"user_id": 1}),
                "tmp",
            )
            .await
            .unwrap();
        db.set_alert_rules(1, alert.id, &[e1, e2]).await.unwrap();
        let endpoints = db
            .get_notification_endpoints_for_alert(alert.id)
            .await
            .unwrap();
        assert_eq!(endpoints.len(), 2);
        db.set_alert_rules(1, alert.id, &[e1]).await.unwrap();
        let endpoints = db
            .get_notification_endpoints_for_alert(alert.id)
            .await
            .unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].id, e1);
    }
}
