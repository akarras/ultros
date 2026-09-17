use crate::UltrosDb;
use crate::entity::*;
use anyhow::Result;
use futures::future::try_join_all;
use sea_orm::sea_query::Expr;
use sea_orm::*;

/// Parameters for [`UltrosDb::record_alert_event`]. A struct rather than a
/// long argument list (9 positional args would trip clippy's
/// `too_many_arguments`).
pub struct NewAlertEvent {
    pub alert_id: i32,
    pub item_id: i32,
    pub matched_listing_id: Option<i64>,
    pub matched_price: Option<i32>,
    pub delivered: bool,
    pub delivery_error: Option<String>,
    /// Notification title, rendered in the inbox list.
    pub title: String,
    /// Notification body text.
    pub body: String,
    /// Relative or absolute URL the inbox entry links to when clicked.
    pub click_url: String,
}

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
        let endpoint_id = self
            .get_or_create_channel_endpoint(
                discord_user,
                channel_id,
                &format!("Discord channel {channel_id}"),
                None,
                None,
                None,
            )
            .await?;
        self.set_alert_rules(discord_user, alert.id, &[endpoint_id])
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
        let destinations = alert_discord_destination::Entity::find()
            .find_also_related(alert::Entity)
            .filter(
                alert_discord_destination::Column::ChannelId
                    .eq(channel_id)
                    .and(alert::Column::Owner.eq(discord_user)),
            )
            .all(&self.db)
            .await?;
        // Only an alert that actually carries an undercut row qualifies: a
        // sale alert registered in the same channel must be left alone.
        for (discord, alert) in destinations {
            let Some(alert) = alert else { continue };
            let undercut = alert_retainer_undercut::Entity::find()
                .filter(alert_retainer_undercut::Column::AlertId.eq(alert.id))
                .all(&self.db)
                .await?;
            if undercut.is_empty() {
                continue;
            }
            discord.delete(&self.db).await?;
            let _ = try_join_all(undercut.clone().into_iter().map(|u| u.delete(&self.db))).await?;
            alert.clone().delete(&self.db).await?;
            return Ok((alert, undercut));
        }
        Err(anyhow::Error::msg(
            "Alert not found for this discord channel",
        ))
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
                "config::jsonb = $1::jsonb",
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
                    disabled_at: Set(None),
                    last_error: Set(None),
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

    pub async fn set_alert_cooldown(
        &self,
        owner: i64,
        alert_id: i32,
        cooldown_seconds: i32,
    ) -> Result<()> {
        let alert = alert::Entity::find_by_id(alert_id)
            .filter(alert::Column::Owner.eq(owner))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert not found"))?;
        let mut a: alert::ActiveModel = alert.into();
        a.cooldown_seconds = Set(cooldown_seconds);
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

    pub async fn record_alert_event(&self, new: NewAlertEvent) -> Result<alert_event::Model> {
        Ok(alert_event::Entity::insert(alert_event::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(new.alert_id),
            // fired_at is DateTimeWithTimeZone
            fired_at: Set(chrono::Utc::now().into()),
            item_id: Set(new.item_id),
            matched_listing_id: Set(new.matched_listing_id),
            matched_price: Set(new.matched_price),
            delivered: Set(new.delivered),
            delivery_error: Set(new.delivery_error),
            read_at: Set(None),
            title: Set(Some(new.title)),
            body: Set(Some(new.body)),
            click_url: Set(Some(new.click_url)),
        })
        .exec_with_returning(&self.db)
        .await?)
    }

    /// Return the *deliverable* notification endpoints linked to an alert via
    /// alert_notification_rule.
    ///
    /// Endpoints that hit a permanent delivery failure (`disabled_at` set — the
    /// Discord channel was deleted or the bot was removed) are excluded. They
    /// stay linked to the alert so the endpoints UI can show why they stopped
    /// and the user can repair them; they just aren't retried every fire.
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
            .filter(notification_endpoint::Column::DisabledAt.is_null())
            .all(&self.db)
            .await?)
    }

    /// Mark an endpoint as permanently broken so delivery stops retrying it.
    ///
    /// Called from the alert delivery path when Discord reports an
    /// unrecoverable condition (`Unknown Channel`, `Missing Access`). Idempotent
    /// on `disabled_at` — re-disabling an already-disabled endpoint keeps the
    /// original timestamp so "broken since" stays truthful — but always
    /// refreshes `last_error`.
    pub async fn disable_endpoint_for_delivery_failure(
        &self,
        endpoint_id: i32,
        reason: &str,
    ) -> Result<()> {
        let Some(existing) = notification_endpoint::Entity::find_by_id(endpoint_id)
            .one(&self.db)
            .await?
        else {
            return Ok(());
        };
        let already_disabled = existing.disabled_at;
        let mut active: notification_endpoint::ActiveModel = existing.into();
        active.disabled_at = Set(Some(already_disabled.unwrap_or_else(chrono::Utc::now)));
        active.last_error = Set(Some(reason.to_string()));
        active.update(&self.db).await?;
        Ok(())
    }

    /// Clear an endpoint's failure state after a delivery (or an explicit
    /// "test") succeeds. This is how a repaired endpoint comes back: the user
    /// fixes the channel, hits Test, and the endpoint re-enters the rotation.
    ///
    /// Skips the write when the endpoint is already healthy so the common
    /// success path doesn't issue an UPDATE per alert fire.
    pub async fn clear_endpoint_delivery_failure(&self, endpoint_id: i32) -> Result<()> {
        let Some(existing) = notification_endpoint::Entity::find_by_id(endpoint_id)
            .one(&self.db)
            .await?
        else {
            return Ok(());
        };
        if existing.disabled_at.is_none() && existing.last_error.is_none() {
            return Ok(());
        }
        let mut active: notification_endpoint::ActiveModel = existing.into();
        active.disabled_at = Set(None);
        active.last_error = Set(None);
        active.update(&self.db).await?;
        Ok(())
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

    /// Cursor-paginate a user's alert events, newest first. `before_id`, when
    /// given, restricts the page to events strictly older than that id (i.e.
    /// the last id seen on the previous page), so pages never overlap or skip
    /// a row even if new events are inserted between requests.
    pub async fn get_recent_alert_events_for_user(
        &self,
        owner: i64,
        limit: u64,
        before_id: Option<i64>,
    ) -> Result<Vec<alert_event::Model>> {
        let mut query = alert_event::Entity::find()
            .inner_join(alert::Entity)
            .filter(alert::Column::Owner.eq(owner));
        if let Some(before_id) = before_id {
            query = query.filter(alert_event::Column::Id.lt(before_id));
        }
        Ok(query
            .order_by_desc(alert_event::Column::Id)
            .limit(limit)
            .all(&self.db)
            .await?)
    }

    /// Count of a user's alert events that have not yet been marked read.
    pub async fn count_unread_alert_events_for_user(&self, owner: i64) -> Result<u64> {
        Ok(alert_event::Entity::find()
            .inner_join(alert::Entity)
            .filter(alert::Column::Owner.eq(owner))
            .filter(alert_event::Column::ReadAt.is_null())
            .count(&self.db)
            .await?)
    }

    /// Mark alert events read, scoped to events belonging to alerts `owner`
    /// owns. An event is matched if its id is in `ids`, or if `up_to_id` is
    /// given and the event's id is `<= up_to_id`. Ids that don't belong to the
    /// caller (foreign or nonexistent) silently no-op, the same opacity as
    /// [`Self::get_alert_event_by_id_owned_by`]. Returns the number of rows
    /// actually flipped from unread to read.
    ///
    /// Guards against issuing an unbounded UPDATE: with an empty `ids` and no
    /// `up_to_id`, there is nothing to match, so this returns `Ok(0)` without
    /// touching the database — an empty `Condition::any()` would otherwise add
    /// no restriction to the query and mark every unread event for the user.
    pub async fn mark_alert_events_read_for_user(
        &self,
        owner: i64,
        ids: &[i64],
        up_to_id: Option<i64>,
    ) -> Result<u64> {
        if ids.is_empty() && up_to_id.is_none() {
            return Ok(0);
        }
        let alert_ids: Vec<i32> = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .select_only()
            .column(alert::Column::Id)
            .into_tuple()
            .all(&self.db)
            .await?;
        if alert_ids.is_empty() {
            return Ok(0);
        }
        let mut matched = Condition::any();
        if !ids.is_empty() {
            matched = matched.add(alert_event::Column::Id.is_in(ids.to_vec()));
        }
        if let Some(up_to_id) = up_to_id {
            matched = matched.add(alert_event::Column::Id.lte(up_to_id));
        }
        let result = alert_event::Entity::update_many()
            .col_expr(
                alert_event::Column::ReadAt,
                Expr::value(chrono::Utc::now().fixed_offset()),
            )
            .filter(alert_event::Column::AlertId.is_in(alert_ids))
            .filter(alert_event::Column::ReadAt.is_null())
            .filter(matched)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Delete alert events belonging to alerts `owner` owns: every event with
    /// `id <= up_to_id` when given, otherwise all of them. Rows belonging to
    /// other users are never touched, whatever `up_to_id` says. Returns the
    /// number of rows deleted.
    pub async fn delete_alert_events_for_user(
        &self,
        owner: i64,
        up_to_id: Option<i64>,
    ) -> Result<u64> {
        let alert_ids: Vec<i32> = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .select_only()
            .column(alert::Column::Id)
            .into_tuple()
            .all(&self.db)
            .await?;
        if alert_ids.is_empty() {
            return Ok(0);
        }
        let mut query = alert_event::Entity::delete_many()
            .filter(alert_event::Column::AlertId.is_in(alert_ids));
        if let Some(up_to_id) = up_to_id {
            query = query.filter(alert_event::Column::Id.lte(up_to_id));
        }
        let result = query.exec(&self.db).await?;
        Ok(result.rows_affected)
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
            disabled_at: Set(None),
            last_error: Set(None),
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
            // Repointing an endpoint at a different destination invalidates any
            // previous permanent failure — give the new target a fresh chance
            // instead of leaving it silently disabled.
            active.disabled_at = Set(None);
            active.last_error = Set(None);
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

    /// Create an alert + alert_retainer_undercut in a single transaction and bind
    /// the supplied notification endpoints. This is the web/API path; legacy
    /// Discord commands still write `alert_discord_destination` as a fallback,
    /// then bind an endpoint rule for the shared delivery pipeline.
    pub async fn create_retainer_undercut_alert(
        &self,
        owner: i64,
        margin_percent: i32,
        cooldown_seconds: i32,
        endpoint_ids: &[i32],
    ) -> Result<(alert::Model, alert_retainer_undercut::Model)> {
        use sea_orm::TransactionTrait;
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
        let undercut =
            alert_retainer_undercut::Entity::insert(alert_retainer_undercut::ActiveModel {
                id: ActiveValue::default(),
                alert_id: Set(alert.id),
                margin_percent: Set(margin_percent),
            })
            .exec_with_returning(&txn)
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
        Ok((alert, undercut))
    }

    /// Create an alert + alert_retainer_sale in one transaction and bind the
    /// supplied notification endpoints. A sold alert has no parameters.
    pub async fn create_retainer_sale_alert(
        &self,
        owner: i64,
        cooldown_seconds: i32,
        endpoint_ids: &[i32],
    ) -> Result<(alert::Model, alert_retainer_sale::Model)> {
        use sea_orm::TransactionTrait;
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
        let sale = alert_retainer_sale::Entity::insert(alert_retainer_sale::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
        })
        .exec_with_returning(&txn)
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
        Ok((alert, sale))
    }

    pub async fn get_user_retainer_sale_alerts(
        &self,
        owner: i64,
    ) -> Result<Vec<(alert::Model, alert_retainer_sale::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .find_with_related(alert_retainer_sale::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    pub async fn get_all_active_retainer_sale_alerts(
        &self,
    ) -> Result<Vec<(alert::Model, alert_retainer_sale::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Enabled.eq(true))
            .find_with_related(alert_retainer_sale::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    /// Discord-command path: alert + legacy channel destination + sale row,
    /// then a channel endpoint bound through the shared delivery pipeline.
    pub async fn add_discord_retainer_sale_alert(
        &self,
        channel_id: i64,
        discord_user: i64,
    ) -> Result<alert::Model> {
        let alert = alert::Entity::insert(alert::ActiveModel {
            id: ActiveValue::default(),
            owner: Set(discord_user),
            enabled: ActiveValue::default(),
            last_fired_at: ActiveValue::default(),
            cooldown_seconds: ActiveValue::default(),
        })
        .exec_with_returning(&self.db)
        .await?;
        alert_discord_destination::Entity::insert(alert_discord_destination::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            channel_id: Set(channel_id),
        })
        .exec(&self.db)
        .await?;
        alert_retainer_sale::Entity::insert(alert_retainer_sale::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
        })
        .exec(&self.db)
        .await?;
        let endpoint_id = self
            .get_or_create_channel_endpoint(
                discord_user,
                channel_id,
                &format!("Discord channel {channel_id}"),
                None,
                None,
                None,
            )
            .await?;
        self.set_alert_rules(discord_user, alert.id, &[endpoint_id])
            .await?;
        Ok(alert)
    }

    /// Delete the sold alert this user registered in this channel. Only alerts
    /// that carry an `alert_retainer_sale` row qualify, so an undercut alert in
    /// the same channel is left alone.
    pub async fn delete_discord_sale_alert(
        &self,
        channel_id: i64,
        discord_user: i64,
    ) -> Result<alert::Model> {
        let destinations = alert_discord_destination::Entity::find()
            .find_also_related(alert::Entity)
            .filter(
                alert_discord_destination::Column::ChannelId
                    .eq(channel_id)
                    .and(alert::Column::Owner.eq(discord_user)),
            )
            .all(&self.db)
            .await?;
        for (destination, alert) in destinations {
            let Some(alert) = alert else { continue };
            let has_sale_row = alert_retainer_sale::Entity::find()
                .filter(alert_retainer_sale::Column::AlertId.eq(alert.id))
                .one(&self.db)
                .await?
                .is_some();
            if !has_sale_row {
                continue;
            }
            destination.delete(&self.db).await?;
            // alert_retainer_sale and alert_notification_rule cascade.
            alert.clone().delete(&self.db).await?;
            return Ok(alert);
        }
        Err(anyhow::Error::msg(
            "No sale alert found for this discord channel",
        ))
    }

    /// Create an alert that fires whenever the referenced list or one of its
    /// rows changes. Caller MUST have already checked Read permission.
    pub async fn create_list_update_alert(
        &self,
        owner: i64,
        list_id: i32,
        cooldown_seconds: i32,
        endpoint_ids: &[i32],
    ) -> Result<alert::Model> {
        use sea_orm::TransactionTrait;
        for &eid in endpoint_ids {
            notification_endpoint::Entity::find_by_id(eid)
                .filter(notification_endpoint::Column::UserId.eq(owner))
                .one(&self.db)
                .await?
                .ok_or_else(|| anyhow::Error::msg(format!("endpoint {eid} not owned by user")))?;
        }

        if let Some((existing_alert, _)) = self.get_list_update_alert(owner, list_id).await? {
            let mut active: alert::ActiveModel = existing_alert.into();
            active.enabled = Set(true);
            active.cooldown_seconds = Set(cooldown_seconds);
            let alert = active.update(&self.db).await?;
            self.set_alert_rules(owner, alert.id, endpoint_ids).await?;
            return Ok(alert);
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
        alert_list_update::Entity::insert(alert_list_update::ActiveModel {
            id: ActiveValue::default(),
            alert_id: Set(alert.id),
            owner: Set(owner),
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

    pub async fn get_list_update_alert(
        &self,
        owner: i64,
        list_id: i32,
    ) -> Result<Option<(alert::Model, alert_list_update::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .find_with_related(alert_list_update::Entity)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().find_map(|(a, triggers)| {
            triggers
                .into_iter()
                .find(|t| t.list_id == list_id)
                .map(|t| (a, t))
        }))
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

    pub async fn get_user_retainer_undercut_alerts(
        &self,
        owner: i64,
    ) -> Result<Vec<(alert::Model, alert_retainer_undercut::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .find_with_related(alert_retainer_undercut::Entity)
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .flat_map(|(a, ts)| ts.into_iter().map(move |t| (a.clone(), t)))
            .collect())
    }

    pub async fn get_user_list_update_alerts(
        &self,
        owner: i64,
    ) -> Result<Vec<(alert::Model, alert_list_update::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Owner.eq(owner))
            .find_with_related(alert_list_update::Entity)
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

    pub async fn get_all_active_list_update_alerts(
        &self,
    ) -> Result<Vec<(alert::Model, alert_list_update::Model)>> {
        let rows = alert::Entity::find()
            .filter(alert::Column::Enabled.eq(true))
            .find_with_related(alert_list_update::Entity)
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
                "config::jsonb = $1::jsonb",
                vec![cfg.clone()],
            ))
            .one(&self.db)
            .await?
        {
            return Ok(existing.id);
        }
        self.create_endpoint(owner, name, "DiscordDm", cfg).await
    }

    /// Find or create the user's InApp (notification inbox) endpoint. Unlike
    /// `get_or_create_dm_endpoint`/`get_or_create_webpush_endpoint`, dedupe is
    /// on `UserId` + `Method` alone — a user has at most one inbox, so there is
    /// no config payload to distinguish between rows.
    ///
    /// The select-then-insert below is still a race: two concurrent
    /// first-time callers can both pass the select and both attempt the
    /// insert. `notification_endpoint` carries a partial unique index on
    /// `(user_id) WHERE method = 'InApp'` (migration
    /// `m20260915_000001_alert_event_inbox`) to make the loser's insert fail
    /// instead of creating a permanent duplicate (`delete_endpoint` refuses
    /// to delete InApp rows) — on a unique-constraint violation we re-select
    /// and return the winner's id.
    pub async fn get_or_create_inapp_endpoint(&self, owner: i64, name: &str) -> Result<i32> {
        if let Some(existing) = notification_endpoint::Entity::find()
            .filter(notification_endpoint::Column::UserId.eq(owner))
            .filter(notification_endpoint::Column::Method.eq("InApp"))
            .one(&self.db)
            .await?
        {
            return Ok(existing.id);
        }
        let inserted = notification_endpoint::Entity::insert(notification_endpoint::ActiveModel {
            id: ActiveValue::default(),
            user_id: Set(owner),
            name: Set(name.to_string()),
            method: Set("InApp".to_string()),
            config: Set(serde_json::json!({})),
            created_at: Set(chrono::Utc::now()),
            disabled_at: Set(None),
            last_error: Set(None),
        })
        .exec_with_returning(&self.db)
        .await;
        match inserted {
            Ok(model) => Ok(model.id),
            Err(error) if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                notification_endpoint::Entity::find()
                    .filter(notification_endpoint::Column::UserId.eq(owner))
                    .filter(notification_endpoint::Column::Method.eq("InApp"))
                    .one(&self.db)
                    .await?
                    .map(|existing| existing.id)
                    .ok_or_else(|| {
                        anyhow::Error::msg(
                            "InApp endpoint insert hit a unique violation but no row was found on re-select",
                        )
                    })
            }
            Err(error) => Err(error.into()),
        }
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
                "(config->>'channel_id')::bigint = $1",
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
                "config::jsonb = $1::jsonb",
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

#[cfg(test)]
mod inbox_tests {
    use super::*;

    /// Connect to a scratch PostgreSQL database whose migrations are already
    /// applied (same convention as `guest_list_adoption.rs`/`list_doc.rs`).
    /// These tests are `#[ignore]`d and only run when a developer points
    /// `MIGRATION_TEST_DATABASE_URL` at a disposable database:
    ///
    /// ```bash
    /// MIGRATION_TEST_DATABASE_URL=... cargo test -p ultros-db inbox_tests -- --ignored --test-threads=1
    /// ```
    async fn test_db() -> UltrosDb {
        let conn = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        UltrosDb::from_connection(conn)
    }

    /// A discord user id that is unlikely to collide with another test run
    /// (or another test in this module), so repeated/parallel runs against
    /// the same scratch database never see each other's leftover rows.
    fn unique_owner(salt: i64) -> i64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64;
        (nanos % 1_000_000_000_000) + salt
    }

    async fn owned_alert(db: &UltrosDb, owner: i64) -> alert::Model {
        db.get_or_create_discord_user(owner as u64, format!("InboxUser{owner}"))
            .await
            .unwrap();
        db.create_threshold_alert_without_endpoint(
            owner,
            1,
            serde_json::json!({}),
            100,
            false,
            3600,
        )
        .await
        .unwrap()
    }

    fn new_event(alert_id: i32) -> NewAlertEvent {
        NewAlertEvent {
            alert_id,
            item_id: 1,
            matched_listing_id: None,
            matched_price: None,
            delivered: true,
            delivery_error: None,
            title: "Test alert".to_string(),
            body: "Test alert body".to_string(),
            click_url: "/item/1".to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn mark_read_only_touches_owners_events() {
        let db = test_db().await;
        let owner_a = unique_owner(1);
        let owner_b = unique_owner(2);
        let alert_a = owned_alert(&db, owner_a).await;
        let alert_b = owned_alert(&db, owner_b).await;
        let event_a = db.record_alert_event(new_event(alert_a.id)).await.unwrap();
        let event_b = db.record_alert_event(new_event(alert_b.id)).await.unwrap();

        // Ask to mark both events read as owner_a; only the one owner_a
        // actually owns should be touched.
        let affected = db
            .mark_alert_events_read_for_user(owner_a, &[event_a.id, event_b.id], None)
            .await
            .unwrap();
        assert_eq!(affected, 1);

        let a = db
            .get_alert_event_by_id_owned_by(owner_a, event_a.id)
            .await
            .unwrap();
        assert!(a.read_at.is_some());
        let b = db
            .get_alert_event_by_id_owned_by(owner_b, event_b.id)
            .await
            .unwrap();
        assert!(b.read_at.is_none());

        // Same scoping, but via `up_to_id` alone (empty `ids`): owner_a's
        // `up_to_id` is at least owner_b's event id, yet owner_b's row must
        // still come back unread — `up_to_id` is scoped by the caller's own
        // alerts, not a global "every event with id <= N".
        let up_to = std::cmp::max(event_a.id, event_b.id);
        let affected_up_to = db
            .mark_alert_events_read_for_user(owner_a, &[], Some(up_to))
            .await
            .unwrap();
        assert_eq!(affected_up_to, 0, "event_a was already marked read above");
        let b_after_up_to = db
            .get_alert_event_by_id_owned_by(owner_b, event_b.id)
            .await
            .unwrap();
        assert!(b_after_up_to.read_at.is_none());
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn mark_read_up_to_id_leaves_newer_unread() {
        let db = test_db().await;
        let owner = unique_owner(3);
        let alert = owned_alert(&db, owner).await;
        let e1 = db.record_alert_event(new_event(alert.id)).await.unwrap();
        let e2 = db.record_alert_event(new_event(alert.id)).await.unwrap();
        let e3 = db.record_alert_event(new_event(alert.id)).await.unwrap();

        let affected = db
            .mark_alert_events_read_for_user(owner, &[], Some(e2.id))
            .await
            .unwrap();
        assert_eq!(affected, 2);

        assert!(
            db.get_alert_event_by_id_owned_by(owner, e1.id)
                .await
                .unwrap()
                .read_at
                .is_some()
        );
        assert!(
            db.get_alert_event_by_id_owned_by(owner, e2.id)
                .await
                .unwrap()
                .read_at
                .is_some()
        );
        assert!(
            db.get_alert_event_by_id_owned_by(owner, e3.id)
                .await
                .unwrap()
                .read_at
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn clear_only_deletes_owners_events_up_to_id() {
        let db = test_db().await;
        let owner_a = unique_owner(11);
        let owner_b = unique_owner(12);
        let alert_a = owned_alert(&db, owner_a).await;
        let alert_b = owned_alert(&db, owner_b).await;

        let a1 = db.record_alert_event(new_event(alert_a.id)).await.unwrap();
        let a2 = db.record_alert_event(new_event(alert_a.id)).await.unwrap();
        let b1 = db.record_alert_event(new_event(alert_b.id)).await.unwrap();
        let a3 = db.record_alert_event(new_event(alert_a.id)).await.unwrap();

        // `up_to_id` covers b1 too, but only owner_a's rows may go.
        let deleted = db
            .delete_alert_events_for_user(owner_a, Some(a2.id))
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        assert!(
            db.get_alert_event_by_id_owned_by(owner_a, a1.id)
                .await
                .is_err()
        );
        assert!(
            db.get_alert_event_by_id_owned_by(owner_a, a2.id)
                .await
                .is_err()
        );
        assert!(
            db.get_alert_event_by_id_owned_by(owner_a, a3.id)
                .await
                .is_ok()
        );
        assert!(
            db.get_alert_event_by_id_owned_by(owner_b, b1.id)
                .await
                .is_ok()
        );

        // No bound: everything left that owner_a owns goes, owner_b's stays.
        let deleted_all = db
            .delete_alert_events_for_user(owner_a, None)
            .await
            .unwrap();
        assert_eq!(deleted_all, 1);
        assert!(
            db.get_alert_event_by_id_owned_by(owner_a, a3.id)
                .await
                .is_err()
        );
        assert!(
            db.get_alert_event_by_id_owned_by(owner_b, b1.id)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn unread_count_excludes_read_and_foreign_events() {
        let db = test_db().await;
        let owner_a = unique_owner(4);
        let owner_b = unique_owner(5);
        let alert_a = owned_alert(&db, owner_a).await;
        let alert_b = owned_alert(&db, owner_b).await;

        let e1 = db.record_alert_event(new_event(alert_a.id)).await.unwrap();
        db.record_alert_event(new_event(alert_a.id)).await.unwrap();
        db.record_alert_event(new_event(alert_a.id)).await.unwrap();
        db.record_alert_event(new_event(alert_b.id)).await.unwrap();
        db.record_alert_event(new_event(alert_b.id)).await.unwrap();

        db.mark_alert_events_read_for_user(owner_a, &[e1.id], None)
            .await
            .unwrap();

        assert_eq!(
            db.count_unread_alert_events_for_user(owner_a)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            db.count_unread_alert_events_for_user(owner_b)
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn recent_events_cursor_pages_by_id_desc() {
        let db = test_db().await;
        let owner = unique_owner(6);
        let alert = owned_alert(&db, owner).await;
        let mut ids = Vec::new();
        for _ in 0..5 {
            ids.push(db.record_alert_event(new_event(alert.id)).await.unwrap().id);
        }
        ids.sort_unstable();

        let page1 = db
            .get_recent_alert_events_for_user(owner, 2, None)
            .await
            .unwrap();
        assert_eq!(
            page1.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![ids[4], ids[3]]
        );

        let page2 = db
            .get_recent_alert_events_for_user(owner, 2, Some(page1.last().unwrap().id))
            .await
            .unwrap();
        assert_eq!(
            page2.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![ids[2], ids[1]]
        );

        let page3 = db
            .get_recent_alert_events_for_user(owner, 2, Some(page2.last().unwrap().id))
            .await
            .unwrap();
        assert_eq!(page3.iter().map(|e| e.id).collect::<Vec<_>>(), vec![ids[0]]);
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn inapp_endpoint_is_created_once_per_user() {
        let db = test_db().await;
        let owner = unique_owner(7);
        db.get_or_create_discord_user(owner as u64, "InboxEndpointOwner".into())
            .await
            .unwrap();

        let first = db
            .get_or_create_inapp_endpoint(owner, "Inbox")
            .await
            .unwrap();
        let second = db
            .get_or_create_inapp_endpoint(owner, "Inbox")
            .await
            .unwrap();
        assert_eq!(first, second);

        let endpoints = db.list_endpoints(owner).await.unwrap();
        assert_eq!(endpoints.iter().filter(|e| e.method == "InApp").count(), 1);
    }

    /// Proves `uq_notification_endpoint_inapp` (migration
    /// `m20260915_000001_alert_event_inbox`) actually exists and rejects a
    /// second `InApp` row for the same user — this is what makes
    /// `get_or_create_inapp_endpoint`'s race-loser re-select path reachable
    /// rather than dead code. Inserts directly through the entity API
    /// (bypassing `get_or_create_inapp_endpoint`'s own select-first check) to
    /// isolate the database constraint from the application-level guard.
    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn duplicate_inapp_endpoint_insert_is_rejected_by_unique_index() {
        let db = test_db().await;
        let owner = unique_owner(8);
        db.get_or_create_discord_user(owner as u64, "InboxUniqueOwner".into())
            .await
            .unwrap();

        db.create_endpoint(owner, "Inbox", "InApp", serde_json::json!({}))
            .await
            .unwrap();

        let second = notification_endpoint::Entity::insert(notification_endpoint::ActiveModel {
            id: ActiveValue::default(),
            user_id: Set(owner),
            name: Set("Inbox (duplicate)".to_string()),
            method: Set("InApp".to_string()),
            config: Set(serde_json::json!({})),
            created_at: Set(chrono::Utc::now()),
            disabled_at: Set(None),
            last_error: Set(None),
        })
        .exec_with_returning(&db.db)
        .await;

        match second {
            Err(error) => assert!(
                matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))),
                "expected a unique-constraint violation, got {error:?}"
            ),
            Ok(model) => panic!(
                "second InApp row for the same user should have been rejected by \
                 uq_notification_endpoint_inapp, but inserted as id {}",
                model.id
            ),
        }

        let endpoints = db.list_endpoints(owner).await.unwrap();
        assert_eq!(endpoints.iter().filter(|e| e.method == "InApp").count(), 1);
    }
}
