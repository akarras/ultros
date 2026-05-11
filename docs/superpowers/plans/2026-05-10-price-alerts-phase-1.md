# Price Alerts — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a backend-only foundation where a user can create a per-item price-threshold alert, have it evaluated against live listing events, and receive a Discord DM when the threshold is met.

**Architecture:** Extend the existing `alert` + `alert_price` + `notification_endpoint` schema rather than redesigning. Build a `PriceAlertTracker` that mirrors the existing `UndercutTracker` pattern, register it with the running `AlertManager`, and add a new `DiscordDm` delivery method to the `notification_endpoint` system. Expose CRUD over HTTP at `/api/v1/alerts`.

**Tech Stack:** Rust, sea-orm migrations, Axum, Serenity (Discord), Tokio broadcast channels for the event bus.

**Out of scope for Phase 1:** Webhook delivery (Phase 2), frontend UI (Phase 3), AI suggestions (Phase 4). The % drop from median trigger is also deferred — Phase 1 ships **below threshold** only. Existing **undercut** trigger remains untouched.

**Spec:** [2026-05-10-price-alerts-design.md](../specs/2026-05-10-price-alerts-design.md)

---

## File Structure

**Files created:**
- `migration/src/m20260510_000001_price_alert_phase1.rs` — schema migration
- `ultros-db/src/entity/alert_event.rs` — new entity (regenerated from sea-orm)
- `ultros-db/src/entity/alert_item_threshold.rs` — new entity for per-item price alerts (regenerated)
- `ultros-api-types/src/alert.rs` — API types
- `ultros/src/alerts/price_alert_tracker.rs` — the new tracker (mirrors `undercut_alert.rs`)
- `ultros/src/alerts/delivery.rs` — generic delivery dispatcher (DM vs channel)
- `ultros/src/web/api/alerts.rs` — HTTP CRUD endpoints

**Files modified:**
- `migration/src/lib.rs` — register new migration
- `ultros-db/src/entity/mod.rs` — declare new modules
- `ultros-db/src/entity/alert.rs` — add new columns (auto-regenerated, but verify)
- `ultros-db/src/entity/alert_price.rs` — add new column (auto-regenerated, but verify)
- `ultros-db/src/entity/prelude.rs` — re-export new entities
- `ultros-db/src/alerts.rs` — new DB access methods
- `ultros-api-types/src/lib.rs` — export `alert` module
- `ultros/src/alerts/mod.rs` — declare new modules
- `ultros/src/alerts/price_alert.rs` — delete (replaced by tracker) **or** keep as thin re-export
- `ultros/src/alerts/alert_manager.rs` — add price alert listener spawn path
- `ultros/src/event.rs` — add price_alert event bus
- `ultros/src/web.rs` — register new routes

---

## Task 1: Schema migration

**Files:**
- Create: `migration/src/m20260510_000001_price_alert_phase1.rs`
- Modify: `migration/src/lib.rs`

This migration:
1. Adds operational columns to `alert`: `enabled`, `last_fired_at`, `cooldown_seconds`
2. Creates `alert_item_threshold` table (per-item version of alert_price — leaves alert_price alone for backward compat with existing list-scoped alerts)
3. Creates `alert_event` table for fire history
4. Backfills `enabled=true` and `cooldown_seconds=3600` for existing alerts

- [ ] **Step 1: Create the migration file**

Create `migration/src/m20260510_000001_price_alert_phase1.rs`:

```rust
use sea_orm_migration::prelude::*;

use crate::m20240424_000001_create_notification_endpoints::Alert;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Add operational columns to `alert`
        manager
            .alter_table(
                Table::alter()
                    .table(Alert::Table)
                    .add_column(
                        ColumnDef::new(AlertExt::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .add_column(ColumnDef::new(AlertExt::LastFiredAt).timestamp_with_time_zone())
                    .add_column(
                        ColumnDef::new(AlertExt::CooldownSeconds)
                            .integer()
                            .not_null()
                            .default(3600),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. Create alert_item_threshold (per-item version of alert_price)
        manager
            .create_table(
                Table::create()
                    .table(AlertItemThreshold::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertItemThreshold::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::AlertId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::ItemId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::WorldSelector)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::PriceThreshold)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AlertItemThreshold::HqOnly).boolean().not_null().default(false))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_item_threshold_alert_id")
                            .from(AlertItemThreshold::Table, AlertItemThreshold::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_alert_item_threshold_item")
                    .table(AlertItemThreshold::Table)
                    .col(AlertItemThreshold::ItemId)
                    .to_owned(),
            )
            .await?;

        // 3. Create alert_event for fire history
        manager
            .create_table(
                Table::create()
                    .table(AlertEvent::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertEvent::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AlertEvent::AlertId).integer().not_null())
                    .col(
                        ColumnDef::new(AlertEvent::FiredAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(AlertEvent::ItemId).integer().not_null())
                    .col(ColumnDef::new(AlertEvent::MatchedListingId).big_integer())
                    .col(ColumnDef::new(AlertEvent::MatchedPrice).integer())
                    .col(
                        ColumnDef::new(AlertEvent::Delivered)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(AlertEvent::DeliveryError).text())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_event_alert_id")
                            .from(AlertEvent::Table, AlertEvent::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_alert_event_alert_fired")
                    .table(AlertEvent::Table)
                    .col(AlertEvent::AlertId)
                    .col(AlertEvent::FiredAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertEvent::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AlertItemThreshold::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Alert::Table)
                    .drop_column(AlertExt::CooldownSeconds)
                    .drop_column(AlertExt::LastFiredAt)
                    .drop_column(AlertExt::Enabled)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertExt {
    Enabled,
    LastFiredAt,
    CooldownSeconds,
}

#[derive(DeriveIden)]
enum AlertItemThreshold {
    Table,
    Id,
    AlertId,
    ItemId,
    WorldSelector,
    PriceThreshold,
    HqOnly,
}

#[derive(DeriveIden)]
enum AlertEvent {
    Table,
    Id,
    AlertId,
    FiredAt,
    ItemId,
    MatchedListingId,
    MatchedPrice,
    Delivered,
    DeliveryError,
}
```

- [ ] **Step 2: Register the migration in `migration/src/lib.rs`**

Add the module declaration alongside the other `mod m...` lines, and add `Box::new(m20260510_000001_price_alert_phase1::Migration)` to the end of the `migrations()` vec:

```rust
mod m20260510_000001_price_alert_phase1;
// ... in migrations() vec at the end:
Box::new(m20260510_000001_price_alert_phase1::Migration),
```

- [ ] **Step 3: Apply the migration locally**

The app applies migrations on boot. To run them in isolation:

```bash
cargo run -p migration -- up
```

Expected: prints `Migration 'm20260510_000001_price_alert_phase1' applied`.

- [ ] **Step 4: Verify schema**

```bash
psql -U postgres -d postgres -c "\d alert" -c "\d alert_item_threshold" -c "\d alert_event"
```

Expected: `alert` table now has `enabled`, `last_fired_at`, `cooldown_seconds`. The two new tables exist with FKs.

- [ ] **Step 5: Commit**

```bash
git add migration/src/m20260510_000001_price_alert_phase1.rs migration/src/lib.rs
git commit -m "Add Phase 1 migration: alert ops columns, alert_item_threshold, alert_event"
```

---

## Task 2: Regenerate sea-orm entities

**Files:**
- Modify: `ultros-db/src/entity/alert.rs` (regen, then verify diff)
- Create: `ultros-db/src/entity/alert_item_threshold.rs`
- Create: `ultros-db/src/entity/alert_event.rs`
- Modify: `ultros-db/src/entity/mod.rs`, `ultros-db/src/entity/prelude.rs`

- [ ] **Step 1: Regenerate entities from the DB**

```bash
sea-orm-cli generate entity -u postgres://postgres:ultros-dev-password@localhost:5432/postgres -o /tmp/ultros-entities
```

This writes regenerated entities to `/tmp/ultros-entities`. Don't blow away `ultros-db/src/entity/` — copy only the relevant files:

```bash
cp /tmp/ultros-entities/alert.rs ultros-db/src/entity/alert.rs
cp /tmp/ultros-entities/alert_item_threshold.rs ultros-db/src/entity/alert_item_threshold.rs
cp /tmp/ultros-entities/alert_event.rs ultros-db/src/entity/alert_event.rs
```

- [ ] **Step 2: Verify the regenerated `alert.rs` has new columns**

```bash
grep -E "enabled|last_fired_at|cooldown_seconds" ultros-db/src/entity/alert.rs
```

Expected: three fields shown on the Model struct.

- [ ] **Step 3: Register new entities in `mod.rs`**

Edit `ultros-db/src/entity/mod.rs`. Add:

```rust
pub mod alert_event;
pub mod alert_item_threshold;
```

And in `ultros-db/src/entity/prelude.rs`, add re-exports:

```rust
pub use super::alert_event::Entity as AlertEvent;
pub use super::alert_item_threshold::Entity as AlertItemThreshold;
```

- [ ] **Step 4: Verify the crate compiles**

```bash
cargo check -p ultros-db
```

Expected: no errors. If sea-orm-cli emitted the new relations differently than expected, sea-orm may report missing trait impls — add `impl ActiveModelBehavior for ActiveModel {}` to any new entity that lacks it.

- [ ] **Step 5: Commit**

```bash
git add ultros-db/src/entity/
git commit -m "Regenerate sea-orm entities for Phase 1 alert tables"
```

---

## Task 3: API types for alerts

**Files:**
- Create: `ultros-api-types/src/alert.rs`
- Modify: `ultros-api-types/src/lib.rs`

- [ ] **Step 1: Create the API types module**

Create `ultros-api-types/src/alert.rs`:

```rust
use serde::{Deserialize, Serialize};

use crate::world_helper::AnySelector;

/// What kind of condition the alert checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertTrigger {
    /// Fire when any listing for this item drops to or below `price_threshold`.
    BelowThreshold {
        item_id: i32,
        world_selector: AnySelector,
        price_threshold: i32,
        hq_only: bool,
    },
}

/// Where to send a fired alert.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AlertDelivery {
    /// Send a Discord DM to the user. The user_id is derived from the auth session, not the request body.
    DiscordDm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateAlertRequest {
    pub trigger: AlertTrigger,
    pub delivery: AlertDelivery,
    /// Defaults to 3600 (1 hour) if omitted.
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateAlertRequest {
    pub enabled: Option<bool>,
    pub price_threshold: Option<i32>,
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alert {
    pub id: i32,
    pub trigger: AlertTrigger,
    pub delivery: AlertDelivery,
    pub enabled: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertEvent {
    pub id: i64,
    pub alert_id: i32,
    pub fired_at: chrono::DateTime<chrono::Utc>,
    pub item_id: i32,
    pub matched_price: Option<i32>,
    pub delivered: bool,
    pub delivery_error: Option<String>,
}
```

- [ ] **Step 2: Wire into `ultros-api-types/src/lib.rs`**

Add `pub mod alert;` next to the other `pub mod` declarations.

- [ ] **Step 3: Verify build**

```bash
cargo check -p ultros-api-types
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add ultros-api-types/
git commit -m "Add API types for Phase 1 price alerts"
```

---

## Task 4: Database access methods

**Files:**
- Modify: `ultros-db/src/alerts.rs`

Adds DB methods needed for both the listener path and the HTTP API path.

- [ ] **Step 1: Add `create_threshold_alert` method**

Append to `ultros-db/src/alerts.rs`:

```rust
use crate::entity::alert_item_threshold;

impl UltrosDb {
    /// Create an alert + alert_item_threshold + alert_notification_rule + (if needed) notification_endpoint
    /// in a single transaction.
    pub async fn create_threshold_alert(
        &self,
        owner_discord_user_id: i64,
        item_id: i32,
        world_selector_json: serde_json::Value,
        price_threshold: i32,
        hq_only: bool,
        cooldown_seconds: i32,
        notification_method: &str,
        notification_config: serde_json::Value,
        notification_name: &str,
    ) -> Result<alert::Model> {
        use sea_orm::{TransactionTrait};
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
                "config = $1::jsonb",
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
                    created_at: Set(Some(chrono::Utc::now().naive_utc())),
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
}
```

(Note: `notification_endpoint` and `alert_notification_rule` imports are already in `entity::*` glob.)

- [ ] **Step 2: Add `get_user_threshold_alerts`**

```rust
impl UltrosDb {
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
}
```

- [ ] **Step 3: Add `update_alert` and `delete_alert`**

```rust
impl UltrosDb {
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
}
```

- [ ] **Step 4: Add alert_event helpers**

```rust
use crate::entity::alert_event;

impl UltrosDb {
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

    pub async fn update_alert_last_fired(&self, alert_id: i32) -> Result<()> {
        let alert = alert::Entity::find_by_id(alert_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("alert not found"))?;
        let mut a: alert::ActiveModel = alert.into();
        a.last_fired_at = Set(Some(chrono::Utc::now().into()));
        a.update(&self.db).await?;
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
```

- [ ] **Step 5: Verify build**

```bash
cargo check -p ultros-db
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add ultros-db/src/alerts.rs
git commit -m "Add DB access methods for per-item threshold alerts and events"
```

---

## Task 5: Discord DM delivery dispatcher

**Files:**
- Create: `ultros/src/alerts/delivery.rs`
- Modify: `ultros/src/alerts/mod.rs`

A single function that takes an alert_id, a message, and dispatches via the notification_endpoint's `method` (DiscordDm or DiscordChannel). Returns a Result so the caller can record delivery success/failure in alert_event.

- [ ] **Step 1: Create `ultros/src/alerts/delivery.rs`**

```rust
use anyhow::{Result, anyhow};
use poise::serenity_prelude::{
    self, Color, CreateAllowedMentions, CreateEmbed, CreateMessage, UserId,
};
use serde::Deserialize;
use tracing::error;
use ultros_db::{UltrosDb, entity::*};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub(crate) enum EndpointConfig {
    #[serde(rename = "DiscordChannel")]
    DiscordChannel { channel_id: i64 },
    #[serde(rename = "DiscordDm")]
    DiscordDm { user_id: i64 },
}

/// Look up all notification endpoints for an alert and dispatch the message via each.
/// Returns Ok(()) if at least one delivered; Err describing the last failure otherwise.
pub(crate) async fn dispatch_alert(
    alert_id: i32,
    title: &str,
    body: &str,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let rules = alert_notification_rule::Entity::find()
        .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
        .all(&db.db)
        .await?;

    if rules.is_empty() {
        return Err(anyhow!("alert {alert_id} has no notification rules"));
    }

    let mut last_err: Option<anyhow::Error> = None;
    let mut any_ok = false;

    for rule in rules {
        let endpoint = notification_endpoint::Entity::find_by_id(rule.endpoint_id)
            .one(&db.db)
            .await?;
        let Some(endpoint) = endpoint else {
            last_err = Some(anyhow!("endpoint {} missing", rule.endpoint_id));
            continue;
        };

        // The DB column is generic JSON; we re-construct the tagged enum from method + config.
        let mut config_obj = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            endpoint.config.clone(),
        )
        .unwrap_or_default();
        config_obj.insert(
            "method".to_string(),
            serde_json::Value::String(endpoint.method.clone()),
        );
        let parsed: EndpointConfig = match serde_json::from_value(serde_json::Value::Object(config_obj)) {
            Ok(p) => p,
            Err(e) => {
                last_err = Some(anyhow!("bad endpoint config for {}: {e}", endpoint.id));
                continue;
            }
        };

        let result = match parsed {
            EndpointConfig::DiscordChannel { channel_id } => {
                send_to_channel(channel_id, title, body, ctx).await
            }
            EndpointConfig::DiscordDm { user_id } => send_dm(user_id, title, body, ctx).await,
        };
        match result {
            Ok(()) => any_ok = true,
            Err(e) => {
                error!("delivery failed for alert {alert_id}: {e}");
                last_err = Some(e);
            }
        }
    }

    if any_ok {
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| anyhow!("no deliveries succeeded")))
    }
}

async fn send_to_channel(
    channel_id: i64,
    title: &str,
    body: &str,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let channel_id = serenity_prelude::ChannelId::new(channel_id as u64);
    channel_id
        .send_message(
            ctx,
            CreateMessage::new().embed(
                CreateEmbed::new()
                    .color(Color::from_rgb(0, 200, 80))
                    .title(title)
                    .description(body),
            ),
        )
        .await?;
    Ok(())
}

async fn send_dm(
    user_id: i64,
    title: &str,
    body: &str,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let user_id = UserId::new(user_id as u64);
    let dm = user_id.create_dm_channel(ctx).await?;
    dm.send_message(
        ctx,
        CreateMessage::new()
            .embed(
                CreateEmbed::new()
                    .color(Color::from_rgb(0, 200, 80))
                    .title(title)
                    .description(body),
            )
            .allowed_mentions(CreateAllowedMentions::new()),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 2: Register module in `ultros/src/alerts/mod.rs`**

Replace the contents of `ultros/src/alerts/mod.rs` with:

```rust
pub mod alert_manager;
pub(crate) mod delivery;
pub(crate) mod price_alert_tracker;
#[allow(unused)]
pub mod price_alert;       // stub retained until tracker replaces it
#[allow(unused)]
pub mod undercut_alert;
```

(`price_alert.rs` stays in place until Task 6 lands; we'll delete it then.)

- [ ] **Step 3: Verify build**

```bash
cargo check -p ultros
```

Expected: errors only about `price_alert_tracker` not yet existing — fine, we create it in Task 6.

- [ ] **Step 4: Commit**

```bash
git add ultros/src/alerts/delivery.rs ultros/src/alerts/mod.rs
git commit -m "Add Discord delivery dispatcher (DM + channel)"
```

---

## Task 6: Implement PriceAlertTracker

**Files:**
- Create: `ultros/src/alerts/price_alert_tracker.rs`
- Delete: `ultros/src/alerts/price_alert.rs` (the stub)
- Modify: `ultros/src/alerts/mod.rs`

The tracker holds the per-rule state (item_id → threshold + cooldown), receives listing events, and fires when any new listing matches.

- [ ] **Step 1: Write a failing test for cooldown behavior**

Create `ultros/src/alerts/price_alert_tracker.rs` with just the test scaffold first:

```rust
#[cfg(test)]
mod test {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn cooldown_blocks_recent_fire() {
        let last = Some(Utc::now() - Duration::seconds(30));
        let cooldown_s = 3600;
        assert!(!is_off_cooldown(last, cooldown_s));
    }

    #[test]
    fn cooldown_allows_old_fire() {
        let last = Some(Utc::now() - Duration::seconds(7200));
        let cooldown_s = 3600;
        assert!(is_off_cooldown(last, cooldown_s));
    }

    #[test]
    fn never_fired_is_off_cooldown() {
        assert!(is_off_cooldown(None, 3600));
    }
}
```

- [ ] **Step 2: Run the test, verify it fails to compile**

```bash
cargo test -p ultros alerts::price_alert_tracker::test 2>&1 | tail -10
```

Expected: compile error "cannot find function `is_off_cooldown`".

- [ ] **Step 3: Implement `is_off_cooldown` and the rest of the tracker**

Prepend to `ultros/src/alerts/price_alert_tracker.rs` (above the test module):

```rust
use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use chrono::{DateTime, Utc};
use poise::serenity_prelude;
use tokio::sync::Mutex;
use tracing::{error, info, instrument};
use ultros_api_types::websocket::ListingEventData;
use ultros_db::{
    UltrosDb,
    entity::{alert, alert_item_threshold},
};

use crate::event::{EventBus, EventType};
use crate::alerts::delivery::dispatch_alert;

pub(crate) fn is_off_cooldown(last_fired_at: Option<DateTime<Utc>>, cooldown_seconds: i32) -> bool {
    match last_fired_at {
        None => true,
        Some(t) => Utc::now().signed_duration_since(t).num_seconds() >= cooldown_seconds as i64,
    }
}

#[derive(Debug, Clone)]
struct ActiveRule {
    alert_id: i32,
    item_id: i32,
    price_threshold: i32,
    hq_only: bool,
    cooldown_seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct TrackerState {
    /// Map item_id -> active rules. Multiple rules per item are allowed.
    by_item: HashMap<i32, Vec<ActiveRule>>,
}

impl TrackerState {
    fn refresh_from(&mut self, alerts: &[(alert::Model, alert_item_threshold::Model)]) {
        self.by_item.clear();
        for (a, t) in alerts {
            if !a.enabled {
                continue;
            }
            self.by_item.entry(t.item_id).or_default().push(ActiveRule {
                alert_id: a.id,
                item_id: t.item_id,
                price_threshold: t.price_threshold,
                hq_only: t.hq_only,
                cooldown_seconds: a.cooldown_seconds,
                last_fired_at: a.last_fired_at.map(|dt| dt.with_timezone(&Utc)),
            });
        }
    }
}

pub(crate) struct PriceAlertListener {
    /// Send to stop the listener.
    pub(crate) stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl PriceAlertListener {
    #[instrument(skip(ultros_db, listings, ctx))]
    pub(crate) async fn start(
        ultros_db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        ctx: serenity_prelude::Context,
    ) -> Result<Self> {
        let initial = ultros_db.get_all_active_threshold_alerts().await?;
        let state = Arc::new(Mutex::new(TrackerState::default()));
        state.lock().await.refresh_from(&initial);
        info!("price-alert tracker started with {} rules", initial.len());

        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

        let state_for_loop = state.clone();
        let db_for_loop = ultros_db.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    msg = listings.recv() => {
                        let Ok(event) = msg else { continue };
                        if let EventType::Add(added) = event {
                            handle_added(&added, &state_for_loop, &db_for_loop, &ctx).await;
                        }
                    }
                }
            }
        });

        Ok(Self { stop_tx })
    }
}

async fn handle_added(
    added: &ListingEventData,
    state: &Arc<Mutex<TrackerState>>,
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) {
    // Group by item; we assume listings event already groups by single item, but defensive.
    // (rule, matched_price). We don't track listing_id in Phase 1 because the ActiveListing API
    // type doesn't reliably expose the DB primary key in all paths — alert_event.matched_listing_id
    // stays NULL for now and can be wired in later when we add a richer event payload.
    let mut to_fire: Vec<(ActiveRule, i32)> = vec![];

    {
        let mut guard = state.lock().await;
        for (listing, _retainer) in &added.listings {
            let Some(rules) = guard.by_item.get_mut(&listing.item_id) else { continue };
            for rule in rules.iter_mut() {
                if rule.hq_only && !listing.hq {
                    continue;
                }
                if listing.price_per_unit > rule.price_threshold {
                    continue;
                }
                if !is_off_cooldown(rule.last_fired_at, rule.cooldown_seconds) {
                    continue;
                }
                rule.last_fired_at = Some(Utc::now());
                to_fire.push((rule.clone(), listing.price_per_unit));
            }
        }
    }

    for (rule, matched_price) in to_fire {
        let item_name = xiv_gen_db::data()
            .items
            .get(&xiv_gen::ItemId(rule.item_id))
            .map(|i| i.name.as_str().to_string())
            .unwrap_or_else(|| format!("Item {}", rule.item_id));
        let title = format!("🎯 {item_name} dropped to {matched_price} gil");
        let body = format!(
            "Threshold: {} gil\nhttps://ultros.app/item/{}",
            rule.price_threshold, rule.item_id
        );

        let delivery_result = dispatch_alert(rule.alert_id, &title, &body, db, ctx).await;
        let delivered = delivery_result.is_ok();
        let delivery_error = delivery_result.err().map(|e| e.to_string());

        if let Err(e) = db
            .record_alert_event(
                rule.alert_id,
                rule.item_id,
                None, // matched_listing_id — see note above
                Some(matched_price),
                delivered,
                delivery_error,
            )
            .await
        {
            error!("failed to record alert_event for alert {}: {e}", rule.alert_id);
        }
        if delivered
            && let Err(e) = db.update_alert_last_fired(rule.alert_id).await
        {
            error!("failed to update last_fired_at for alert {}: {e}", rule.alert_id);
        }
    }
}
```

- [ ] **Step 4: Run the unit tests, verify pass**

```bash
cargo test -p ultros alerts::price_alert_tracker::test 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 5: Delete the obsolete stub and clean up mod.rs**

```bash
rm ultros/src/alerts/price_alert.rs
```

Edit `ultros/src/alerts/mod.rs` and remove the `price_alert` line.

- [ ] **Step 6: Verify the crate compiles**

```bash
cargo check -p ultros 2>&1 | tail -20
```

Expected: no errors. Pay attention to: `add_recipe_to_current_list.rs` or other unrelated frontend touches (this branch is behind main) — if so, the cargo check may flag unrelated warnings; only the alert/ folder should have changes.

- [ ] **Step 7: Commit**

```bash
git add ultros/src/alerts/
git commit -m "Implement PriceAlertTracker (replaces stub price_alert.rs)"
```

---

## Task 7: Wire price alert listener into AlertManager

**Files:**
- Modify: `ultros/src/alerts/alert_manager.rs`
- Modify: `ultros/src/event.rs` (only if we add a price_alert event bus — see below)

The existing AlertManager owns retainer undercut listeners. We add a single PriceAlertListener (shared across all price-alert rules) since the tracker handles all rules in-process.

- [ ] **Step 1: Add the price alert listener field**

Modify `ultros/src/alerts/alert_manager.rs`. Add to the struct:

```rust
use super::price_alert_tracker::PriceAlertListener;

pub(crate) struct AlertManager {
    current_retainer_alerts: HashMap<i32, RetainerAlertListener>,
    price_alerts: Option<PriceAlertListener>,
}
```

- [ ] **Step 2: Initialize `price_alerts: None` in the struct construction inside `start_manager`**

In `start_manager`, where `manager` is created:

```rust
let mut manager = AlertManager {
    current_retainer_alerts: HashMap::new(),
    price_alerts: None,
};
```

- [ ] **Step 3: Start the price alert listener**

Immediately after the existing `match ultros_db.get_all_alerts().await { ... }` block in `start_manager`, add:

```rust
match PriceAlertListener::start(ultros_db.clone(), listings.resubscribe(), ctx.clone()).await {
    Ok(listener) => manager.price_alerts = Some(listener),
    Err(e) => error!("failed to start price alert listener: {e}"),
}
```

- [ ] **Step 4: Verify build**

```bash
cargo check -p ultros
```

Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/alerts/alert_manager.rs
git commit -m "Wire PriceAlertListener into AlertManager"
```

---

## Task 8: HTTP API endpoints

**Files:**
- Create: `ultros/src/web/api/alerts.rs`
- Modify: `ultros/src/web/api/mod.rs`
- Modify: `ultros/src/web.rs` (route registration)

- [ ] **Step 1: Create the handler module**

Create `ultros/src/web/api/alerts.rs`:

```rust
use axum::{
    Json,
    extract::{Path, State},
};
use ultros_api_types::alert::{
    Alert, AlertDelivery, AlertEvent as ApiAlertEvent, AlertTrigger, CreateAlertRequest,
    UpdateAlertRequest,
};
use ultros_db::UltrosDb;

use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

pub(crate) async fn create_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreateAlertRequest>,
) -> Result<Json<Alert>, ApiError> {
    let cooldown = req.cooldown_seconds.unwrap_or(3600).max(60).min(86400);
    let owner = user.id as i64;

    let AlertTrigger::BelowThreshold {
        item_id,
        world_selector,
        price_threshold,
        hq_only,
    } = req.trigger;

    if price_threshold <= 0 {
        return Err(ApiError::BadRequest("price_threshold must be positive".into()));
    }

    let (notification_method, notification_config, notification_name) = match req.delivery {
        AlertDelivery::DiscordDm => (
            "DiscordDm",
            serde_json::json!({ "user_id": owner }),
            format!("DM to {}", user.name),
        ),
    };

    let world_selector_json = serde_json::to_value(&world_selector)
        .map_err(|e| ApiError::BadRequest(format!("invalid world_selector: {e}")))?;

    let alert = db
        .create_threshold_alert(
            owner,
            item_id,
            world_selector_json,
            price_threshold,
            hq_only,
            cooldown,
            notification_method,
            notification_config,
            &notification_name,
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::BelowThreshold {
            item_id,
            world_selector,
            price_threshold,
            hq_only,
        },
        delivery: AlertDelivery::DiscordDm,
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}

pub(crate) async fn list_alerts(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<Alert>>, ApiError> {
    let rows = db
        .get_user_threshold_alerts(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for (a, t) in rows {
        let world_selector = serde_json::from_value(t.world_selector.clone())
            .map_err(|e| ApiError::Other(format!("bad world_selector in db: {e}")))?;
        // Phase 1: assume one delivery rule per alert; just default to DiscordDm in the response.
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::BelowThreshold {
                item_id: t.item_id,
                world_selector,
                price_threshold: t.price_threshold,
                hq_only: t.hq_only,
            },
            delivery: AlertDelivery::DiscordDm,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }
    Ok(Json(out))
}

pub(crate) async fn update_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(alert_id): Path<i32>,
    Json(req): Json<UpdateAlertRequest>,
) -> Result<Json<()>, ApiError> {
    let owner = user.id as i64;
    if let Some(enabled) = req.enabled {
        db.set_alert_enabled(owner, alert_id, enabled)
            .await
            .map_err(ApiError::from)?;
    }
    if let Some(price) = req.price_threshold {
        if price <= 0 {
            return Err(ApiError::BadRequest("price_threshold must be positive".into()));
        }
        db.update_threshold_alert_price(owner, alert_id, price)
            .await
            .map_err(ApiError::from)?;
    }
    Ok(Json(()))
}

pub(crate) async fn delete_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(alert_id): Path<i32>,
) -> Result<Json<()>, ApiError> {
    db.delete_alert_owned_by(user.id as i64, alert_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

pub(crate) async fn list_alert_events(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<ApiAlertEvent>>, ApiError> {
    let rows = db
        .get_recent_alert_events_for_user(user.id as i64, 50)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ApiAlertEvent {
                id: r.id,
                alert_id: r.alert_id,
                fired_at: r.fired_at.with_timezone(&chrono::Utc),
                item_id: r.item_id,
                matched_price: r.matched_price,
                delivered: r.delivered,
                delivery_error: r.delivery_error,
            })
            .collect(),
    ))
}
```

- [ ] **Step 2: Register the module in `ultros/src/web/api/mod.rs`**

Add: `pub mod alerts;`

- [ ] **Step 3: Verify ApiError supports `BadRequest` / `Other` variants**

```bash
grep -A2 "pub enum ApiError" ultros/src/web/error.rs
```

If `BadRequest(String)` or `Other(String)` variants don't exist, add them (with appropriate IntoResponse mapping to 400/500). Otherwise adapt the handler to use an existing variant — `ApiError::Anyhow(anyhow::anyhow!(...))` is fine for both cases as a fallback.

- [ ] **Step 4: Register routes in `ultros/src/web.rs`**

Add imports near the top:

```rust
use self::web::api::alerts::{
    create_alert, delete_alert, list_alert_events, list_alerts, update_alert,
};
```

Add to the router-build block (near the `.route("/api/v1/list/...")` lines):

```rust
        .route("/api/v1/alerts", get(list_alerts).post(create_alert))
        .route(
            "/api/v1/alerts/{id}",
            axum::routing::patch(update_alert).delete(delete_alert),
        )
        .route("/api/v1/alerts/events", get(list_alert_events))
```

- [ ] **Step 5: Verify build**

```bash
cargo check -p ultros 2>&1 | tail -20
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add ultros/src/web/api/alerts.rs ultros/src/web/api/mod.rs ultros/src/web.rs
git commit -m "Add /api/v1/alerts HTTP endpoints"
```

---

## Task 9: Smoke test end-to-end

No new code in this task — just runtime verification.

- [ ] **Step 1: Start the app**

```bash
cargo leptos serve
```

Wait for `listening on 0.0.0.0:8080`.

- [ ] **Step 2: Log in via Discord and copy the auth cookie**

In a browser, visit `http://localhost:8080`, click "Login with Discord," complete OAuth. Open DevTools → Application → Cookies and copy the session cookie value into an env var for curl:

```bash
export COOKIE='session_token_value_here'
```

- [ ] **Step 3: Create an alert via the API**

Pick an item you know has cheap listings on your home world (or set an artificially high threshold). Use item_id 5057 (Earth Shard) as a test:

```bash
curl -X POST http://localhost:8080/api/v1/alerts \
  -H "Content-Type: application/json" \
  -H "Cookie: $COOKIE" \
  -d '{
    "trigger": {
      "type": "below_threshold",
      "item_id": 5057,
      "world_selector": {"World": 60},
      "price_threshold": 999999,
      "hq_only": false
    },
    "delivery": {"method": "discord_dm"},
    "cooldown_seconds": 60
  }'
```

Expected: 200 OK with JSON body containing the new alert.

- [ ] **Step 4: Verify in DB**

```bash
psql -U postgres -d postgres -c "SELECT a.id, a.enabled, a.cooldown_seconds, t.item_id, t.price_threshold FROM alert a JOIN alert_item_threshold t ON t.alert_id = a.id ORDER BY a.id DESC LIMIT 1;"
```

Expected: one row showing the alert you just created.

- [ ] **Step 5: Wait for a listing event (or trigger one)**

Either wait for a real listing event from Universalis (may take minutes on a quiet item), or shim one via the existing data delta admin tooling. Inspect logs for:

```
price-alert tracker started with N rules
```

When the alert fires, you should see a Discord DM from the bot and a new row in `alert_event`:

```bash
psql -U postgres -d postgres -c "SELECT * FROM alert_event ORDER BY fired_at DESC LIMIT 5;"
```

- [ ] **Step 6: Verify GET /alerts/events returns the event**

```bash
curl -H "Cookie: $COOKIE" http://localhost:8080/api/v1/alerts/events | jq .
```

- [ ] **Step 7: Disable the alert via PATCH**

```bash
curl -X PATCH http://localhost:8080/api/v1/alerts/1 \
  -H "Content-Type: application/json" \
  -H "Cookie: $COOKIE" \
  -d '{"enabled": false}'
```

Restart the app, verify the tracker now reports `N-1` rules.

- [ ] **Step 8: Delete the alert**

```bash
curl -X DELETE http://localhost:8080/api/v1/alerts/1 -H "Cookie: $COOKIE"
```

Verify row is gone in DB.

- [ ] **Step 9: Commit any test artifacts (if applicable)**

If smoke-test surfaced bugs that you fixed inline, commit fixes now. Otherwise no commit.

---

## Task 10: Documentation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a short section to README**

Append after the existing `## Ads` section:

```markdown
## Price Alerts

Logged-in users can create per-item price-threshold alerts via the API. When a listing matches, a Discord DM fires (subject to a per-rule cooldown).

API surface: `GET/POST /api/v1/alerts`, `PATCH/DELETE /api/v1/alerts/{id}`, `GET /api/v1/alerts/events`. See `docs/superpowers/plans/2026-05-10-price-alerts-phase-1.md` for the implementation plan.

(Phases 2-4 — webhook delivery, frontend UI, AI suggestions — are tracked separately.)
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "Document Phase 1 Price Alerts in README"
```

---

## Definition of done

- [ ] Migration applies cleanly on a fresh DB and on the current production schema
- [ ] All four CRUD endpoints return 200/204 for happy-path requests with valid auth
- [ ] An alert created via the API fires a Discord DM within one listings event tick of a matching listing
- [ ] `alert_event` rows are written for every fire (whether delivery succeeded or not)
- [ ] Cooldown prevents duplicate fires within the configured window
- [ ] `cargo check -p ultros -p ultros-db -p ultros-api-types -p migration` passes
- [ ] `cargo test -p ultros alerts::` passes (3 unit tests on `is_off_cooldown`)
- [ ] Manual smoke test (Task 9) completed end-to-end with at least one real or shimmed alert fire

---

## Known gaps deferred to later phases

- **Webhook delivery** (Phase 2): the dispatcher's `EndpointConfig` enum is structured to accept a `Webhook { url }` variant in Phase 2 with minimal change.
- **% drop from median trigger** (Phase 2 or 3): needs a `MedianTracker` analogous to the threshold tracker, reading from `sale_history`.
- **Frontend UI** (Phase 3): the API surface is complete enough for the Leptos frontend to build against.
- **AI suggest-items / suggest-threshold** (Phase 4): both will reuse the GET endpoints to read existing rules.
- **List-scoped alerts**: the existing `alert_price` (list-scoped) table is untouched. If the user wants Phase 1 alerts to also support "alert on any item in this list," we extend the tracker to query alert_price too — but the spec calls for per-item, so this is intentionally deferred.
