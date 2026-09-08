# Retainer Sold Alert Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Notify a user through their existing alert endpoints when one of their claimed retainers' listings sells, inferred by pairing a `listings/remove` with a matching `sales/add`, tuned to miss rather than over-report.

**Architecture:** A pure `SoldMatcher` (no I/O) pairs removals with sales under the rules in the spec. One `RetainerSaleListener` task, mirroring `ListUpdateAlertListener`, feeds it from the listings and history buses and dispatches through `dispatch_alert`. A new `alert_retainer_sale` table plus an `AlertTrigger::RetainerSold` variant plug into the existing alert API, drawer, rules panel, and Discord commands the same way `RetainerUndercut` does.

**Tech Stack:** Rust (axum, sea-orm, tokio broadcast buses, poise), Leptos 0.8 frontend with `leptos-i18n`, Postgres migration via sea-orm-migration.

Spec: `docs/superpowers/specs/2026-09-07-retainer-sold-alert-design.md`.

## Global Constraints

- Run `./check_ci.sh` before every commit (fmt + clippy `-D warnings` + tests). Read its exit code with `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"`.
- On Windows, Git Bash needs `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"` for the vendored OpenSSL build, and `ultros` crate tests need `CARGO_PROFILE_DEV_DEBUG=0` to link.
- Every user-facing string in `ultros-frontend/ultros-app/` goes through `leptos-i18n`; every new key is added to all seven locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with a real translation.
- Match window 300 s, clock-skew slack 60 s, max sale age 24 h (spec values).
- No per-alert cooldown on sold alerts; `cooldown_seconds` is stored but ignored.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## File Map

| File | Responsibility |
|---|---|
| `migration/src/m20260907_000001_alert_retainer_sale.rs` (create) | `alert_retainer_sale` table |
| `migration/src/lib.rs` (modify) | register migration |
| `ultros-db/src/entity/alert_retainer_sale.rs` (create) | sea-orm entity |
| `ultros-db/src/entity/{mod,prelude,alert}.rs` (modify) | module/relation wiring |
| `ultros-db/src/alerts.rs` (modify) | create/list/delete helpers for sale alerts |
| `ultros-db/src/retainers.rs` (modify) | `get_owned_retainer_ids` |
| `ultros-api-types/src/alert.rs` (modify) | `AlertTrigger::RetainerSold {}` |
| `ultros/src/alerts/sold_matcher.rs` (create) | pure matcher + unit tests |
| `ultros/src/alerts/sold_alert.rs` (create) | listener, message formatting |
| `ultros/src/alerts/{mod,alert_manager}.rs`, `ultros/src/discord/mod.rs` (modify) | wiring |
| `ultros/src/web/api/alerts.rs` (modify) | create/list handlers |
| `ultros/src/discord/ffxiv/retainer.rs` (modify) | `add_sale_alert`, `remove_sale_alert` |
| `ultros/test_data/sold_replay.jsonl` (create) | replay fixture |
| `ultros-frontend/ultros-app/src/components/alert_drawer.rs` (modify) | `AlertKind::Sold` |
| `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs` (modify) | row rendering |
| `ultros-frontend/ultros-app/src/routes/{retainers,alerts,bot}.rs` (modify) | bell button, hints |
| `ultros-frontend/ultros-app/locales/*.json` (modify) | strings |
| `ultros-changelog/changes/2026-09-07-retainer-sale-alerts.json` (create) | changelog |
| `docs/price-alerts.md` (modify) | docs |

---

### Task 1: Table, entity, and DB helpers

**Files:**
- Create: `migration/src/m20260907_000001_alert_retainer_sale.rs`
- Modify: `migration/src/lib.rs`
- Create: `ultros-db/src/entity/alert_retainer_sale.rs`
- Modify: `ultros-db/src/entity/mod.rs`, `ultros-db/src/entity/prelude.rs`, `ultros-db/src/entity/alert.rs`
- Modify: `ultros-db/src/alerts.rs`, `ultros-db/src/retainers.rs`

**Interfaces produced:**
- `UltrosDb::create_retainer_sale_alert(&self, owner: i64, cooldown_seconds: i32, endpoint_ids: &[i32]) -> Result<(alert::Model, alert_retainer_sale::Model)>`
- `UltrosDb::get_user_retainer_sale_alerts(&self, owner: i64) -> Result<Vec<(alert::Model, alert_retainer_sale::Model)>>`
- `UltrosDb::get_all_active_retainer_sale_alerts(&self) -> Result<Vec<(alert::Model, alert_retainer_sale::Model)>>`
- `UltrosDb::add_discord_retainer_sale_alert(&self, channel_id: i64, discord_user: i64) -> Result<alert::Model>`
- `UltrosDb::delete_discord_sale_alert(&self, channel_id: i64, discord_user: i64) -> Result<alert::Model>`
- `UltrosDb::get_owned_retainer_ids(&self, discord_user: i64) -> Result<Vec<i32>>`

- [ ] **Step 1: Write the migration**

`migration/src/m20260907_000001_alert_retainer_sale.rs`:

```rust
use sea_orm_migration::prelude::*;

use crate::m20240424_000001_create_notification_endpoints::Alert;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AlertRetainerSale::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertRetainerSale::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AlertRetainerSale::AlertId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_retainer_sale_alert_id")
                            .from(AlertRetainerSale::Table, AlertRetainerSale::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertRetainerSale::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertRetainerSale {
    Table,
    Id,
    AlertId,
}
```

Register in `migration/src/lib.rs`: add `mod m20260907_000001_alert_retainer_sale;` after the `m20260905_...` line and `Box::new(m20260907_000001_alert_retainer_sale::Migration),` at the end of the vec.

- [ ] **Step 2: Write the entity**

`ultros-db/src/entity/alert_retainer_sale.rs`:

```rust
//! `SeaORM` Entity. Hand-authored to mirror `alert_retainer_undercut` without
//! the margin column: a sold alert has no tunables.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "alert_retainer_sale")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub alert_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::alert::Entity",
        from = "Column::AlertId",
        to = "super::alert::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Alert,
}

impl Related<super::alert::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Alert.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

In `ultros-db/src/entity/mod.rs` add `pub mod alert_retainer_sale;` after `pub mod alert_price;`. In `prelude.rs` add `pub use super::alert_retainer_sale::Entity as AlertRetainerSale;` after the `AlertPrice` line. In `alert.rs` add to `Relation`:

```rust
    #[sea_orm(has_many = "super::alert_retainer_sale::Entity")]
    AlertRetainerSale,
```

and the impl:

```rust
impl Related<super::alert_retainer_sale::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AlertRetainerSale.def()
    }
}
```

- [ ] **Step 3: Add DB helpers**

Append to the `impl UltrosDb` block in `ultros-db/src/alerts.rs` (next to `create_retainer_undercut_alert`):

```rust
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
```

Also tighten the existing `delete_discord_alert` so it never picks a sale alert: change its `.one(&self.db)` lookup into `.all(&self.db)` and choose the first destination whose alert has an `alert_retainer_undercut` row:

```rust
        let destinations = alert_discord_destination::Entity::find()
            .find_also_related(alert::Entity)
            .filter(
                alert_discord_destination::Column::ChannelId
                    .eq(channel_id)
                    .and(alert::Column::Owner.eq(discord_user)),
            )
            .all(&self.db)
            .await?;
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
```

In `ultros-db/src/retainers.rs` add:

```rust
    /// Retainer ids this Discord user has claimed. Unlike
    /// [`Self::get_owned_retainers`] this never creates a user row.
    pub async fn get_owned_retainer_ids(&self, discord_user: i64) -> Result<Vec<i32>> {
        Ok(owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|o| o.retainer_id)
            .collect())
    }
```

- [ ] **Step 4: Check it compiles**

Run: `cargo check -p migration -p ultros-db`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add migration ultros-db
git commit -m "feat(db): alert_retainer_sale table and helpers"
```

---

### Task 2: API trigger variant

**Files:**
- Modify: `ultros-api-types/src/alert.rs`

**Interfaces produced:** `AlertTrigger::RetainerSold {}` serialized as `{"type":"retainer_sold"}`.

- [ ] **Step 1: Write the failing test**

Inside the existing `#[cfg(test)]` module of `ultros-api-types/src/alert.rs` (the one at ~line 200 that uses `serde_json::json!`), add:

```rust
    #[test]
    fn retainer_sold_trigger_round_trips_with_type_tag_only() {
        let t = AlertTrigger::RetainerSold {};
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v, json!({"type": "retainer_sold"}));
        let back: AlertTrigger = serde_json::from_value(v).unwrap();
        assert_eq!(back, t);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-api-types retainer_sold_trigger`
Expected: compile error, no variant `RetainerSold`.

- [ ] **Step 3: Add the variant**

In `pub enum AlertTrigger`, after `RetainerUndercut { margin_percent: i32 },`:

```rust
    /// Fire when one of the user's retainers' listings is inferred to have
    /// sold (a removal paired with a matching sale). No parameters.
    RetainerSold {},
```

The empty braces keep it a struct variant so the `tag = "type"` serialization stays uniform.

- [ ] **Step 4: Run the test**

Run: `cargo test -p ultros-api-types retainer_sold_trigger`
Expected: PASS. Then `cargo check --workspace` will now fail on non-exhaustive matches in `ultros/src/web/api/alerts.rs`, `alert_drawer.rs`, `alert_rules_panel.rs`; those are fixed in Tasks 5 and 7. Do not add wildcard arms.

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/alert.rs
git commit -m "feat(api-types): AlertTrigger::RetainerSold"
```

---

### Task 3: Pure `SoldMatcher`

**Files:**
- Create: `ultros/src/alerts/sold_matcher.rs`
- Modify: `ultros/src/alerts/mod.rs`

**Interfaces produced:**

```rust
pub(crate) struct SaleKey { pub world_id: i32, pub item_id: i32, pub hq: bool, pub price_per_unit: i32, pub quantity: i32 }
pub(crate) struct RemovedListing { pub key: SaleKey, pub retainer_id: i32, pub retainer_name: String, pub listed_at: NaiveDateTime }
pub(crate) struct AddedListing { pub world_id: i32, pub item_id: i32, pub hq: bool, pub quantity: i32, pub retainer_id: i32 }
pub(crate) struct ObservedSale { pub key: SaleKey, pub sold_at: NaiveDateTime, pub buyer_name: Option<String> }
pub(crate) struct SoldEvent { pub retainer_id: i32, pub retainer_name: String, pub key: SaleKey, pub sold_at: NaiveDateTime, pub buyer_name: Option<String> }
impl SoldMatcher {
    pub(crate) fn new(owned: HashSet<i32>) -> Self;
    pub(crate) fn set_owned(&mut self, owned: HashSet<i32>);
    pub(crate) fn on_removed(&mut self, listing: RemovedListing, now: DateTime<Utc>) -> Vec<SoldEvent>;
    pub(crate) fn on_added(&mut self, added: AddedListing, now: DateTime<Utc>);
    pub(crate) fn on_sale(&mut self, sale: ObservedSale, now: DateTime<Utc>) -> Vec<SoldEvent>;
    pub(crate) fn expire(&mut self, now: DateTime<Utc>);
}
```

- [ ] **Step 1: Write the failing tests**

Create `ultros/src/alerts/sold_matcher.rs` with only the test module first (the implementation follows in Step 3; the file must exist so `mod` resolves):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + secs, 0).unwrap()
    }

    fn key() -> SaleKey {
        SaleKey { world_id: 34, item_id: 5, hq: false, price_per_unit: 100, quantity: 99 }
    }

    fn removed(retainer_id: i32, listed_secs_ago: i64, now: DateTime<Utc>) -> RemovedListing {
        RemovedListing {
            key: key(),
            retainer_id,
            retainer_name: format!("Retainer{retainer_id}"),
            listed_at: (now - TimeDelta::seconds(listed_secs_ago)).naive_utc(),
        }
    }

    fn sale_at(sold: DateTime<Utc>) -> ObservedSale {
        ObservedSale { key: key(), sold_at: sold.naive_utc(), buyer_name: Some("Buyer".into()) }
    }

    fn matcher(owned: &[i32]) -> SoldMatcher {
        SoldMatcher::new(owned.iter().copied().collect())
    }

    #[test]
    fn removal_then_matching_sale_fires_once_for_the_owner() {
        let mut m = matcher(&[1]);
        assert!(m.on_removed(removed(1, 600, t(0)), t(0)).is_empty());
        let fired = m.on_sale(sale_at(t(-5)), t(1));
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].retainer_id, 1);
        assert_eq!(fired[0].retainer_name, "Retainer1");
        assert_eq!(fired[0].key, key());
        // Consumed: a second identical sale finds nothing.
        assert!(m.on_sale(sale_at(t(-4)), t(2)).is_empty());
    }

    #[test]
    fn sale_then_removal_fires_once() {
        let mut m = matcher(&[1]);
        assert!(m.on_sale(sale_at(t(-5)), t(0)).is_empty());
        let fired = m.on_removed(removed(1, 600, t(3)), t(3));
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn unowned_retainer_never_fires_but_still_consumes() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(2, 600, t(0)), t(0));
        assert!(m.on_sale(sale_at(t(-5)), t(1)).is_empty());
        // The sale was consumed by retainer 2, so an owned removal arriving
        // afterwards has nothing to pair with.
        assert!(m.on_removed(removed(1, 600, t(2)), t(2)).is_empty());
    }

    #[test]
    fn reprice_by_same_retainer_suppresses() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_added(
            AddedListing { world_id: 34, item_id: 5, hq: false, quantity: 99, retainer_id: 1 },
            t(0),
        );
        assert!(m.on_sale(sale_at(t(-5)), t(1)).is_empty());
    }

    #[test]
    fn add_from_a_different_retainer_is_not_a_reprice() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_added(
            AddedListing { world_id: 34, item_id: 5, hq: false, quantity: 99, retainer_id: 7 },
            t(0),
        );
        assert_eq!(m.on_sale(sale_at(t(-5)), t(1)).len(), 1);
    }

    #[test]
    fn cross_retainer_collision_drops_the_sale() {
        let mut m = matcher(&[1, 2]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_removed(removed(2, 600, t(0)), t(0));
        assert!(m.on_sale(sale_at(t(-5)), t(1)).is_empty());
        // The removals are still there: a later, unambiguous situation can't
        // arise for this key, but a second sale must not fire either.
        assert!(m.on_sale(sale_at(t(-4)), t(2)).is_empty());
    }

    #[test]
    fn three_listings_one_retainer_two_sales_fire_twice() {
        let mut m = matcher(&[1]);
        for _ in 0..3 {
            m.on_removed(removed(1, 600, t(0)), t(0));
        }
        assert_eq!(m.on_sale(sale_at(t(-5)), t(1)).len(), 1);
        assert_eq!(m.on_sale(sale_at(t(-4)), t(1)).len(), 1);
        // Third listing remains pending, a third sale would fire, a fourth not.
        assert_eq!(m.on_sale(sale_at(t(-3)), t(1)).len(), 1);
        assert!(m.on_sale(sale_at(t(-2)), t(1)).is_empty());
    }

    #[test]
    fn stale_sale_is_ignored() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 200_000, t(0)), t(0));
        // 25 hours old at receipt.
        let stale = sale_at(t(-25 * 3600));
        assert!(m.on_sale(stale, t(0)).is_empty());
        // And it was not stored either: a later removal doesn't pair with it.
        assert!(m.on_removed(removed(1, 200_000, t(1)), t(1)).is_empty());
    }

    #[test]
    fn sale_before_listing_was_touched_is_not_a_match() {
        let mut m = matcher(&[1]);
        // Listing last reviewed 10 s ago; sale happened 5 minutes ago.
        m.on_removed(removed(1, 10, t(0)), t(0));
        assert!(m.on_sale(sale_at(t(-300)), t(0)).is_empty());
        // Within the 60 s skew it still matches.
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 10, t(0)), t(0));
        assert_eq!(m.on_sale(sale_at(t(-40)), t(0)).len(), 1);
    }

    #[test]
    fn removal_outside_window_does_not_pair() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        assert!(m.on_sale(sale_at(t(300)), t(301)).is_empty());
    }

    #[test]
    fn expire_drops_both_sides() {
        let mut m = matcher(&[1]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.on_sale(
            ObservedSale {
                key: SaleKey { price_per_unit: 200, ..key() },
                sold_at: t(-5).naive_utc(),
                buyer_name: None,
            },
            t(0),
        );
        m.expire(t(301));
        assert!(m.on_sale(sale_at(t(-5)), t(302)).is_empty());
        assert!(
            m.on_removed(
                RemovedListing { key: SaleKey { price_per_unit: 200, ..key() }, ..removed(1, 600, t(302)) },
                t(302)
            )
            .is_empty()
        );
    }

    #[test]
    fn set_owned_takes_effect_for_later_matches() {
        let mut m = matcher(&[]);
        m.on_removed(removed(1, 600, t(0)), t(0));
        m.set_owned([1].into_iter().collect());
        assert_eq!(m.on_sale(sale_at(t(-5)), t(1)).len(), 1);
    }
}
```

Add `pub(crate) mod sold_matcher;` to `ultros/src/alerts/mod.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros sold_matcher`
Expected: compile errors (types undefined).

- [ ] **Step 3: Implement the matcher**

Put this above the test module in `ultros/src/alerts/sold_matcher.rs`:

```rust
//! Pairs `listings/remove` events with `sales/add` events to infer that a
//! specific retainer's listing sold. Universalis never says which listing a
//! sale came from, so this is inference, tuned to miss a sale rather than
//! report one that did not happen. See
//! `docs/superpowers/specs/2026-09-07-retainer-sold-alert-design.md`.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};

/// How long a removal or sale waits for its partner (both arrival orders
/// happen; the worst observed gap was 59 s).
pub(crate) const MATCH_WINDOW_SECONDS: i64 = 300;
/// A sale may carry a game timestamp slightly before the listing's
/// `lastReviewTime` because two different clients stamped them.
pub(crate) const CLOCK_SKEW_SECONDS: i64 = 60;
/// `sales/add` is mostly history backfill; anything older than this at
/// receipt is not something we can attribute to a live removal.
pub(crate) const MAX_SALE_AGE_SECONDS: i64 = 24 * 3600;

/// Everything a sale and a listing have in common.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SaleKey {
    pub world_id: i32,
    pub item_id: i32,
    pub hq: bool,
    pub price_per_unit: i32,
    pub quantity: i32,
}

/// Reprice signature: the same retainer removing and re-adding the same
/// stack. Price is deliberately absent — that is what a reprice changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RepriceKey {
    world_id: i32,
    item_id: i32,
    hq: bool,
    quantity: i32,
    retainer_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovedListing {
    pub key: SaleKey,
    pub retainer_id: i32,
    pub retainer_name: String,
    /// Universalis `lastReviewTime`: the retainer's last touch. A sale of
    /// this listing cannot predate it.
    pub listed_at: NaiveDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddedListing {
    pub world_id: i32,
    pub item_id: i32,
    pub hq: bool,
    pub quantity: i32,
    pub retainer_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedSale {
    pub key: SaleKey,
    pub sold_at: NaiveDateTime,
    pub buyer_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoldEvent {
    pub retainer_id: i32,
    pub retainer_name: String,
    pub key: SaleKey,
    pub sold_at: NaiveDateTime,
    pub buyer_name: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingRemoval {
    listing: RemovedListing,
    received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct PendingSale {
    sale: ObservedSale,
    received_at: DateTime<Utc>,
}

#[derive(Debug)]
pub(crate) struct SoldMatcher {
    owned: HashSet<i32>,
    window: TimeDelta,
    skew: TimeDelta,
    max_sale_age: TimeDelta,
    /// Removals from *every* retainer: an unowned removal with the same key
    /// is exactly what makes a sale ambiguous.
    pending_removals: HashMap<SaleKey, Vec<PendingRemoval>>,
    pending_sales: HashMap<SaleKey, Vec<PendingSale>>,
}

impl SoldMatcher {
    pub(crate) fn new(owned: HashSet<i32>) -> Self {
        Self {
            owned,
            window: TimeDelta::seconds(MATCH_WINDOW_SECONDS),
            skew: TimeDelta::seconds(CLOCK_SKEW_SECONDS),
            max_sale_age: TimeDelta::seconds(MAX_SALE_AGE_SECONDS),
            pending_removals: HashMap::new(),
            pending_sales: HashMap::new(),
        }
    }

    pub(crate) fn set_owned(&mut self, owned: HashSet<i32>) {
        self.owned = owned;
    }

    pub(crate) fn on_removed(
        &mut self,
        listing: RemovedListing,
        now: DateTime<Utc>,
    ) -> Vec<SoldEvent> {
        let key = listing.key;
        self.pending_removals
            .entry(key)
            .or_default()
            .push(PendingRemoval {
                listing,
                received_at: now,
            });
        self.match_key(key)
    }

    pub(crate) fn on_added(&mut self, added: AddedListing, _now: DateTime<Utc>) {
        let reprice = RepriceKey {
            world_id: added.world_id,
            item_id: added.item_id,
            hq: added.hq,
            quantity: added.quantity,
            retainer_id: added.retainer_id,
        };
        self.pending_removals.retain(|key, removals| {
            if key.world_id != reprice.world_id
                || key.item_id != reprice.item_id
                || key.hq != reprice.hq
                || key.quantity != reprice.quantity
            {
                return true;
            }
            removals.retain(|r| r.listing.retainer_id != reprice.retainer_id);
            !removals.is_empty()
        });
    }

    pub(crate) fn on_sale(&mut self, sale: ObservedSale, now: DateTime<Utc>) -> Vec<SoldEvent> {
        let sold_at = DateTime::<Utc>::from_naive_utc_and_offset(sale.sold_at, Utc);
        if now - sold_at > self.max_sale_age {
            return Vec::new();
        }
        let key = sale.key;
        self.pending_sales.entry(key).or_default().push(PendingSale {
            sale,
            received_at: now,
        });
        self.match_key(key)
    }

    pub(crate) fn expire(&mut self, now: DateTime<Utc>) {
        let window = self.window;
        self.pending_removals.retain(|_, v| {
            v.retain(|r| now - r.received_at <= window);
            !v.is_empty()
        });
        self.pending_sales.retain(|_, v| {
            v.retain(|s| now - s.received_at <= window);
            !v.is_empty()
        });
    }

    /// Resolve every pending sale under `key` that can be resolved now.
    fn match_key(&mut self, key: SaleKey) -> Vec<SoldEvent> {
        let mut fired = Vec::new();
        loop {
            let Some(sales) = self.pending_sales.get_mut(&key) else {
                break;
            };
            let Some(sale) = sales.first().cloned() else {
                self.pending_sales.remove(&key);
                break;
            };
            let outcome = self.resolve(&key, &sale);
            match outcome {
                Resolution::Wait => break,
                Resolution::Ambiguous => {
                    self.pending_sales.get_mut(&key).map(|s| s.remove(0));
                }
                Resolution::Consume(index) => {
                    self.pending_sales.get_mut(&key).map(|s| s.remove(0));
                    let removal = self
                        .pending_removals
                        .get_mut(&key)
                        .map(|r| r.remove(index))
                        .expect("candidate index came from this vec");
                    if self.owned.contains(&removal.listing.retainer_id) {
                        fired.push(SoldEvent {
                            retainer_id: removal.listing.retainer_id,
                            retainer_name: removal.listing.retainer_name,
                            key,
                            sold_at: sale.sale.sold_at,
                            buyer_name: sale.sale.buyer_name,
                        });
                    }
                }
            }
        }
        if self.pending_sales.get(&key).is_some_and(|s| s.is_empty()) {
            self.pending_sales.remove(&key);
        }
        if self.pending_removals.get(&key).is_some_and(|r| r.is_empty()) {
            self.pending_removals.remove(&key);
        }
        fired
    }

    fn resolve(&self, key: &SaleKey, sale: &PendingSale) -> Resolution {
        let Some(removals) = self.pending_removals.get(key) else {
            return Resolution::Wait;
        };
        let sold_at = DateTime::<Utc>::from_naive_utc_and_offset(sale.sale.sold_at, Utc);
        let mut candidates = removals.iter().enumerate().filter(|(_, r)| {
            let gap = (sale.received_at - r.received_at).abs();
            let listed_at = DateTime::<Utc>::from_naive_utc_and_offset(r.listing.listed_at, Utc);
            gap <= self.window && listed_at <= sold_at + self.skew
        });
        let Some((first_index, first)) = candidates.next() else {
            return Resolution::Wait;
        };
        let retainer = first.listing.retainer_id;
        if candidates.any(|(_, r)| r.listing.retainer_id != retainer) {
            return Resolution::Ambiguous;
        }
        Resolution::Consume(first_index)
    }
}

enum Resolution {
    /// No candidate removal yet; the sale stays pending for its partner.
    Wait,
    /// Candidates from different retainers: nobody can say whose sold.
    Ambiguous,
    /// Exactly one retainer among the candidates; consume this index.
    Consume(usize),
}
```

Note `Vec::first()` is the earliest received because pushes are in arrival order and `expire` preserves order.

- [ ] **Step 4: Run the tests**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros sold_matcher`
Expected: 12 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/alerts/sold_matcher.rs ultros/src/alerts/mod.rs
git commit -m "feat(alerts): SoldMatcher pairs listing removals with sales"
```

---

### Task 4: Replay fixture test

**Files:**
- Create: `ultros/test_data/sold_replay.jsonl`
- Modify: `ultros/src/alerts/sold_matcher.rs` (test module)

The 15-minute capture from the investigation lives in the session scratchpad as `full.jsonl` (format: one JSON object per websocket event with `t` receipt epoch seconds, `event`, `item`, `world`, and `listings`/`sales` arrays; see the spec). Build a small fixture whose expected fire count is hand-verifiable.

- [ ] **Step 1: Build the fixture**

Run this from the scratchpad directory (adjust `SRC`):

```python
# trim_fixture.py — pick five boards: two clean owned pairs, one reprice,
# one cross-retainer collision, one stale sale. Prints the chosen boards and
# the retainer ids to treat as owned.
import json, sys
from collections import defaultdict
SRC = "full.jsonl"; OUT = "sold_replay.jsonl"
rows = [json.loads(l) for l in open(SRC, encoding="utf-8")]
boards = defaultdict(list)
for r in rows:
    boards[(r["world"], r["item"])].append(r)
def key(w, i, x): return (w, i, x["hq"], x["ppu"], x["qty"])
picked = {}
for b, evs in boards.items():
    rem = [(r["t"], l) for r in evs if r["event"] == "listings/remove" for l in r["listings"]]
    add = [(r["t"], l) for r in evs if r["event"] == "listings/add" for l in r["listings"]]
    sal = [(r["t"], s) for r in evs if r["event"] == "sales/add" for s in r["sales"]]
    fresh = [(t, s) for t, s in sal if t - s["ts"] <= 3600]
    for t, s in fresh:
        cands = [(tr, l) for tr, l in rem if key(*b, l) == key(*b, s) and abs(tr - t) <= 300 and (l["last_review"] or 0) <= s["ts"] + 60]
        rets = {l["retainer_id"] for _, l in cands}
        repriced = any(a["retainer_id"] == l["retainer_id"] and a["qty"] == l["qty"] and a["hq"] == l["hq"] for _, l in cands for _, a in add)
        if len(cands) == 1 and not repriced and "clean" not in picked: picked["clean"] = (b, cands[0][1]["retainer_id"])
        elif len(cands) == 1 and not repriced and "clean2" not in picked and picked.get("clean", (None,))[0] != b: picked["clean2"] = (b, cands[0][1]["retainer_id"])
        elif len(rets) > 1 and "ambiguous" not in picked: picked["ambiguous"] = (b, next(iter(rets)))
        elif len(cands) == 1 and repriced and "reprice" not in picked: picked["reprice"] = (b, cands[0][1]["retainer_id"])
    stale = [(t, s) for t, s in sal if t - s["ts"] > 90000]
    if stale and "stale" not in picked and len(evs) < 15: picked["stale"] = (b, None)
print(picked)
chosen = {v[0] for v in picked.values()}
with open(OUT, "w", encoding="utf-8") as f:
    for r in rows:
        if (r["world"], r["item"]) in chosen: f.write(json.dumps(r) + "\n")
print("owned:", sorted({v[1] for v in picked.values() if v[1]}))
```

Copy `sold_replay.jsonl` to `ultros/test_data/sold_replay.jsonl`. Inspect the file by hand and confirm the expected number of fires for the printed owned set (expected: 2, the two clean boards; the reprice board's owned retainer must not fire, the ambiguous board's must not, the stale board has no owned retainer). If the capture yields a different verified count, use that number in the test and say so in the commit message.

- [ ] **Step 2: Write the replay test**

Add to the test module in `sold_matcher.rs`:

```rust
    /// Replays a trimmed slice of the 2026-09-07 websocket capture. Boards
    /// were chosen by hand: two clean owned pairs (fire), one reprice (no
    /// fire), one cross-retainer collision (no fire), one stale-history
    /// board (no fire). Receipt times come from the capture's `t` field.
    #[test]
    fn replay_capture_fires_only_for_clean_owned_pairs() {
        const OWNED: &[i32] = &[/* ids printed by trim_fixture.py */];
        const EXPECTED_FIRES: usize = 2;
        let raw = include_str!("../../test_data/sold_replay.jsonl");
        let mut m = SoldMatcher::new(OWNED.iter().copied().collect());
        let mut fired = 0;
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            let now = Utc.timestamp_opt(v["t"].as_f64().unwrap() as i64, 0).unwrap();
            let world_id = v["world"].as_i64().unwrap() as i32;
            let item_id = v["item"].as_i64().unwrap() as i32;
            match v["event"].as_str().unwrap() {
                "listings/remove" => {
                    for l in v["listings"].as_array().unwrap() {
                        fired += m
                            .on_removed(
                                RemovedListing {
                                    key: SaleKey {
                                        world_id,
                                        item_id,
                                        hq: l["hq"].as_bool().unwrap(),
                                        price_per_unit: l["ppu"].as_i64().unwrap() as i32,
                                        quantity: l["qty"].as_i64().unwrap() as i32,
                                    },
                                    retainer_id: l["retainer_id"].as_str().unwrap().parse::<i64>().unwrap() as i32,
                                    retainer_name: l["retainer"].as_str().unwrap_or("").to_string(),
                                    listed_at: Utc
                                        .timestamp_opt(l["last_review"].as_i64().unwrap_or(0), 0)
                                        .unwrap()
                                        .naive_utc(),
                                },
                                now,
                            )
                            .len();
                    }
                }
                "listings/add" => {
                    for l in v["listings"].as_array().unwrap() {
                        m.on_added(
                            AddedListing {
                                world_id,
                                item_id,
                                hq: l["hq"].as_bool().unwrap(),
                                quantity: l["qty"].as_i64().unwrap() as i32,
                                retainer_id: l["retainer_id"].as_str().unwrap().parse::<i64>().unwrap() as i32,
                            },
                            now,
                        );
                    }
                }
                "sales/add" => {
                    for s in v["sales"].as_array().unwrap() {
                        fired += m
                            .on_sale(
                                ObservedSale {
                                    key: SaleKey {
                                        world_id,
                                        item_id,
                                        hq: s["hq"].as_bool().unwrap(),
                                        price_per_unit: s["ppu"].as_i64().unwrap() as i32,
                                        quantity: s["qty"].as_i64().unwrap() as i32,
                                    },
                                    sold_at: Utc.timestamp_opt(s["ts"].as_i64().unwrap(), 0).unwrap().naive_utc(),
                                    buyer_name: s["buyer"].as_str().map(str::to_string),
                                },
                                now,
                            )
                            .len();
                    }
                }
                other => panic!("unexpected event {other}"),
            }
        }
        assert_eq!(fired, EXPECTED_FIRES);
    }
```

Universalis retainer ids are decimal strings wider than `i32`; the fixture test truncates them with `as i32` purely to get a stable per-retainer integer, which is fine because the matcher only compares ids for equality. Replace the `OWNED` array with the truncated values of the ids the script printed (`id as i32` in Python: `((id + 2**31) % 2**32) - 2**31`).

- [ ] **Step 3: Run**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros replay_capture`
Expected: PASS with the hand-verified count.

- [ ] **Step 4: Commit**

```bash
git add ultros/test_data/sold_replay.jsonl ultros/src/alerts/sold_matcher.rs
git commit -m "test(alerts): replay a captured websocket slice through SoldMatcher"
```

---

### Task 5: Listener, manager wiring, web API

**Files:**
- Create: `ultros/src/alerts/sold_alert.rs`
- Modify: `ultros/src/alerts/mod.rs`, `ultros/src/alerts/alert_manager.rs`, `ultros/src/discord/mod.rs`
- Modify: `ultros/src/web/api/alerts.rs`

**Interfaces consumed:** Task 1 DB helpers, Task 2 trigger variant, Task 3 matcher.
**Interfaces produced:** `RetainerSaleListener::start(db, listings, history, retainers, alert_events, ctx) -> Result<Self>`; `format_sold_alert_message(&SoldEvent, item_name: &str) -> (String, String, String)` returning `(title, body, click_url)`.

- [ ] **Step 1: Write the failing message-format test**

`ultros/src/alerts/sold_alert.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn event(hq: bool) -> SoldEvent {
        SoldEvent {
            retainer_id: 9,
            retainer_name: "Moogle".into(),
            key: SaleKey { world_id: 34, item_id: 5, hq, price_per_unit: 1200, quantity: 3 },
            sold_at: NaiveDate::from_ymd_opt(2026, 9, 7).unwrap().and_hms_opt(1, 2, 3).unwrap(),
            buyer_name: Some("Buyer Name".into()),
        }
    }

    #[test]
    fn message_names_retainer_item_quantity_and_unit_price() {
        let (title, body, click_url) = format_sold_alert_message(&event(false), "Fire Shard");
        assert_eq!(title, "Retainer sale: Fire Shard");
        assert!(body.starts_with("Your retainer Moogle sold 3× Fire Shard for 1,200 gil each (3,600 gil total)."), "{body}");
        assert!(body.contains("https://ultros.app/retainers/listings"));
        assert_eq!(click_url, "/retainers/listings");
    }

    #[test]
    fn message_marks_hq() {
        let (_, body, _) = format_sold_alert_message(&event(true), "Fire Shard");
        assert!(body.contains("3× Fire Shard (HQ) for"), "{body}");
    }

    #[test]
    fn rules_index_maps_each_owned_retainer_to_its_alerts() {
        let rules = build_rules_index(vec![(10, 100, vec![1, 2]), (11, 101, vec![2])]);
        assert_eq!(rules.get(&1).map(|v| v.len()), Some(1));
        assert_eq!(rules.get(&2).map(|v| v.len()), Some(2));
        assert_eq!(owned_union(&rules), [1, 2].into_iter().collect());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros sold_alert`
Expected: compile error.

- [ ] **Step 3: Implement the listener**

`ultros/src/alerts/sold_alert.rs` (above the tests):

```rust
//! Single listener for every retainer-sold alert: feeds the shared
//! [`SoldMatcher`] from the listings and history buses and dispatches through
//! the common alert delivery pipeline.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use chrono::Utc;
use poise::serenity_prelude;
use tracing::{error, info, warn};
use ultros_api_types::{
    user::OwnedRetainer,
    websocket::{ListingEventData, SaleEventData},
};
use ultros_db::{UltrosDb, entity::alert};

use crate::{
    alerts::{
        delivery::dispatch_alert,
        price_alert_tracker::resolve_item_name,
        sold_matcher::{AddedListing, ObservedSale, RemovedListing, SaleKey, SoldEvent, SoldMatcher},
    },
    event::{BusRecv, EventBus, EventType, handle_bus_recv},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SaleRule {
    alert_id: i32,
    owner: i64,
}

/// `retainer_id -> alerts that own it`.
type RulesIndex = HashMap<i32, Vec<SaleRule>>;

/// `(alert_id, owner, owned retainer ids)` triples into the index.
fn build_rules_index(rows: Vec<(i32, i64, Vec<i32>)>) -> RulesIndex {
    let mut index: RulesIndex = HashMap::new();
    for (alert_id, owner, retainers) in rows {
        for retainer_id in retainers {
            index
                .entry(retainer_id)
                .or_default()
                .push(SaleRule { alert_id, owner });
        }
    }
    index
}

fn owned_union(rules: &RulesIndex) -> HashSet<i32> {
    rules.keys().copied().collect()
}

async fn load_rules(db: &UltrosDb) -> Result<RulesIndex> {
    let alerts = db.get_all_active_retainer_sale_alerts().await?;
    let mut rows = Vec::with_capacity(alerts.len());
    for (alert, _) in alerts {
        let retainers = db.get_owned_retainer_ids(alert.owner).await?;
        rows.push((alert.id, alert.owner, retainers));
    }
    Ok(build_rules_index(rows))
}

pub(crate) fn format_sold_alert_message(
    event: &SoldEvent,
    item_name: &str,
) -> (String, String, String) {
    let hq = if event.key.hq { " (HQ)" } else { "" };
    let unit = format_gil(event.key.price_per_unit as i64);
    let total = format_gil(event.key.price_per_unit as i64 * event.key.quantity as i64);
    let title = format!("Retainer sale: {item_name}");
    let body = format!(
        "Your retainer {} sold {}× {item_name}{hq} for {unit} gil each ({total} gil total).\nhttps://ultros.app/retainers/listings",
        event.retainer_name, event.key.quantity,
    );
    (title, body, "/retainers/listings".to_string())
}

fn format_gil(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}

pub(crate) struct RetainerSaleListener {
    #[allow(dead_code)]
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl RetainerSaleListener {
    pub(crate) async fn start(
        db: UltrosDb,
        mut listings: EventBus<ListingEventData>,
        mut history: EventBus<SaleEventData>,
        mut retainers: EventBus<OwnedRetainer>,
        mut alert_events: EventBus<alert::Model>,
        ctx: serenity_prelude::Context,
    ) -> Result<Self> {
        let mut rules = load_rules(&db).await?;
        let mut matcher = SoldMatcher::new(owned_union(&rules));
        info!(
            "retainer sale listener started with {} alerts over {} retainers",
            rules.values().flatten().map(|r| r.alert_id).collect::<HashSet<_>>().len(),
            rules.len()
        );
        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    _ = tick.tick() => matcher.expire(Utc::now()),
                    msg = alert_events.recv() => match handle_bus_recv("sale_alert.alerts", msg) {
                        BusRecv::Msg(_) | BusRecv::Lagged => refresh(&db, &mut rules, &mut matcher).await,
                        BusRecv::Closed => break,
                    },
                    msg = retainers.recv() => match handle_bus_recv("sale_alert.retainers", msg) {
                        BusRecv::Msg(_) | BusRecv::Lagged => refresh(&db, &mut rules, &mut matcher).await,
                        BusRecv::Closed => break,
                    },
                    msg = listings.recv() => match handle_bus_recv("sale_alert.listings", msg) {
                        BusRecv::Msg(event) => {
                            let fired = apply_listing_event(&mut matcher, &event);
                            fire_all(&db, &ctx, &rules, fired).await;
                        }
                        BusRecv::Lagged => {}
                        BusRecv::Closed => break,
                    },
                    msg = history.recv() => match handle_bus_recv("sale_alert.history", msg) {
                        BusRecv::Msg(event) => {
                            let fired = apply_sale_event(&mut matcher, &event);
                            fire_all(&db, &ctx, &rules, fired).await;
                        }
                        BusRecv::Lagged => {}
                        BusRecv::Closed => break,
                    },
                }
            }
        });
        Ok(Self { stop_tx })
    }
}

async fn refresh(db: &UltrosDb, rules: &mut RulesIndex, matcher: &mut SoldMatcher) {
    match load_rules(db).await {
        Ok(new_rules) => {
            *rules = new_rules;
            matcher.set_owned(owned_union(rules));
        }
        Err(e) => error!("retainer sale listener failed to reload rules: {e}"),
    }
}

fn apply_listing_event(
    matcher: &mut SoldMatcher,
    event: &EventType<Arc<ListingEventData>>,
) -> Vec<SoldEvent> {
    let now = Utc::now();
    match event {
        EventType::Remove(data) => data
            .listings
            .iter()
            .flat_map(|(listing, retainer)| {
                matcher.on_removed(
                    RemovedListing {
                        key: SaleKey {
                            world_id: listing.world_id,
                            item_id: listing.item_id,
                            hq: listing.hq,
                            price_per_unit: listing.price_per_unit,
                            quantity: listing.quantity,
                        },
                        retainer_id: listing.retainer_id,
                        retainer_name: retainer.name.clone(),
                        listed_at: listing.timestamp,
                    },
                    now,
                )
            })
            .collect(),
        EventType::Add(data) => {
            for (listing, _) in data.listings.iter() {
                matcher.on_added(
                    AddedListing {
                        world_id: listing.world_id,
                        item_id: listing.item_id,
                        hq: listing.hq,
                        quantity: listing.quantity,
                        retainer_id: listing.retainer_id,
                    },
                    now,
                );
            }
            Vec::new()
        }
        EventType::Update(_) => Vec::new(),
    }
}

fn apply_sale_event(
    matcher: &mut SoldMatcher,
    event: &EventType<Arc<SaleEventData>>,
) -> Vec<SoldEvent> {
    let EventType::Add(data) = event else {
        return Vec::new();
    };
    let now = Utc::now();
    data.sales
        .iter()
        .flat_map(|(sale, buyer)| {
            matcher.on_sale(
                ObservedSale {
                    key: SaleKey {
                        world_id: sale.world_id,
                        item_id: sale.sold_item_id,
                        hq: sale.hq,
                        price_per_unit: sale.price_per_item,
                        quantity: sale.quantity,
                    },
                    sold_at: sale.sold_date,
                    buyer_name: sale.buyer_name.clone().or_else(|| Some(buyer.name.clone())),
                },
                now,
            )
        })
        .collect()
}

async fn fire_all(
    db: &UltrosDb,
    ctx: &serenity_prelude::Context,
    rules: &RulesIndex,
    events: Vec<SoldEvent>,
) {
    for event in events {
        let Some(alerts) = rules.get(&event.retainer_id) else {
            continue;
        };
        let item_name = resolve_item_name(event.key.item_id);
        let (title, body, click_url) = format_sold_alert_message(&event, &item_name);
        for rule in alerts {
            let result = dispatch_alert(rule.alert_id, &title, &body, &click_url, db, ctx).await;
            let delivered = result.is_ok();
            let delivery_error = result.err().map(|e| e.to_string());
            if let Some(error) = &delivery_error {
                warn!(alert_id = rule.alert_id, "retainer sale alert not delivered: {error}");
            }
            if let Err(e) = db
                .record_alert_event(
                    rule.alert_id,
                    event.key.item_id,
                    None,
                    Some(event.key.price_per_unit),
                    delivered,
                    delivery_error,
                )
                .await
            {
                error!("failed to record alert_event for sale alert {}: {e}", rule.alert_id);
            }
            if delivered && let Err(e) = db.update_alert_last_fired(rule.alert_id).await {
                error!("failed to update last_fired_at for sale alert {}: {e}", rule.alert_id);
            }
        }
    }
}
```

Check that `resolve_item_name` in `price_alert_tracker.rs` is `pub(crate)` (it is, line 114) and that `SaleHistory` in the bus payload exposes `buyer_name`, `sold_date`, `sold_item_id`, `price_per_item` (it does, `ultros-api-types/src/sale_history.rs`).

Add `pub(crate) mod sold_alert;` to `ultros/src/alerts/mod.rs`.

- [ ] **Step 4: Wire the manager**

`ultros/src/alerts/alert_manager.rs`:
- Import `SaleEventData` alongside `ListingEventData` from `ultros_api_types::websocket`, and `use super::sold_alert::RetainerSaleListener;`.
- Add field `sale_alerts: Option<RetainerSaleListener>,` and initialise `sale_alerts: None,`.
- Add parameter `history: EventBus<SaleEventData>,` to `start_manager` right after `lists: EventBus<ListEventData>,`.
- After the `ListUpdateAlertListener::start` block, add:

```rust
        match RetainerSaleListener::start(
            ultros_db.clone(),
            listings.resubscribe(),
            history,
            retainers.resubscribe(),
            alerts.resubscribe(),
            ctx.clone(),
        )
        .await
        {
            Ok(listener) => manager.sale_alerts = Some(listener),
            Err(e) => error!("failed to start retainer sale alert listener: {e}"),
        }
```

`ultros/src/discord/mod.rs`: in the `AlertManager::start_manager(` call, add `event_receivers.history.resubscribe(),` immediately after `event_receivers.lists.resubscribe(),`.

- [ ] **Step 5: Web API handlers**

In `ultros/src/web/api/alerts.rs`:

In `create_alert`'s match, after the `RetainerUndercut` arm:

```rust
        AlertTrigger::RetainerSold {} => {
            return create_retainer_sold_alert_handler(&db, &senders, owner, cooldown, &req).await;
        }
```

After `create_retainer_undercut_alert_handler`:

```rust
async fn create_retainer_sold_alert_handler(
    db: &UltrosDb,
    senders: &EventSenders,
    owner: i64,
    cooldown: i32,
    req: &CreateAlertRequest,
) -> Result<Json<Alert>, ApiError> {
    if req.endpoint_ids.is_empty() {
        return Err(ApiError::from(anyhow::anyhow!(
            "retainer sale alerts require endpoint_ids"
        )));
    }
    let (alert, _sale) = db
        .create_retainer_sale_alert(owner, cooldown, &req.endpoint_ids)
        .await
        .map_err(ApiError::from)?;
    let _ = senders.alerts.send(EventType::added(alert.clone()));
    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::RetainerSold {},
        delivery: AlertDelivery::DiscordDm,
        endpoint_ids: req.endpoint_ids.clone(),
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}
```

In `list_alerts`, after the `retainer_rows` loop:

```rust
    let sale_rows = db
        .get_user_retainer_sale_alerts(user.id as i64)
        .await
        .map_err(ApiError::from)?;
    for (a, _) in sale_rows {
        let endpoint_ids = db
            .list_endpoint_ids_for_alert(a.id)
            .await
            .map_err(ApiError::from)?;
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::RetainerSold {},
            delivery: AlertDelivery::DiscordDm,
            endpoint_ids,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }
```

`delete_alert` needs no change: the row cascades and the `alerts` bus `removed` event makes the listener reload.

- [ ] **Step 6: Build and test**

Run: `cargo check -p ultros` then `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros sold_`
Expected: clean check; matcher + listener tests pass.

- [ ] **Step 7: Commit**

```bash
git add ultros/src/alerts ultros/src/discord/mod.rs ultros/src/web/api/alerts.rs
git commit -m "feat(alerts): retainer sale listener and API trigger"
```

---

### Task 6: Discord commands and bot docs

**Files:**
- Modify: `ultros/src/discord/ffxiv/retainer.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/bot.rs`, locales (bot command descriptions)

- [ ] **Step 1: Add the commands**

In `retainer.rs`, add `"add_sale_alert"` and `"remove_sale_alert"` to the `subcommands(...)` list of `retainer`, and extend the help embed's **See also** line with `` `add_sale_alert` ``. Then add after `remove_undercut_alert`:

```rust
/// Notify this channel when one of your claimed retainers' listings sells
#[poise::command(slash_command)]
async fn add_sale_alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let alert = ctx
        .data()
        .db
        .add_discord_retainer_sale_alert(ctx.channel_id().get() as i64, ctx.author().id.get() as i64)
        .await?;
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::added(alert))?;
    ctx.say(
        "Now sending alerts to this channel when one of your retainers' listings sells. \
         Sales are inferred from market data: a listing that disappears alongside a matching \
         sale. Same-price listings from other sellers are skipped rather than guessed, so \
         some sales may go unreported.",
    )
    .await?;
    Ok(())
}

/// Stop sale notifications in this channel
#[poise::command(slash_command)]
async fn remove_sale_alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let alert = ctx
        .data()
        .db
        .delete_discord_sale_alert(ctx.channel_id().get() as i64, ctx.author().id.get() as i64)
        .await?;
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::removed(alert))?;
    ctx.say("Sale alerts for this channel removed.").await?;
    Ok(())
}
```

- [ ] **Step 2: Bot page rows**

In `ultros-frontend/ultros-app/src/routes/bot.rs` after the `remove_undercut_alert` row (line ~66) add:

```rust
                        ("/ffxiv retainer add_sale_alert", t_string!(i18n, bot_cmd_retainer_add_sale_alert_desc).to_string()),
                        ("/ffxiv retainer remove_sale_alert", t_string!(i18n, bot_cmd_retainer_remove_sale_alert_desc).to_string()),
```

Locale keys (all seven files, next to `bot_cmd_retainer_remove_undercut_alert_desc`):

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `bot_cmd_retainer_add_sale_alert_desc` | Notify this channel when a retainer's listing sells. | Prévenir ce salon quand une vente d'un servant est conclue. | Diesen Kanal benachrichtigen, wenn ein Gehilfen-Angebot verkauft wird. | リテイナーの出品が売れたらこのチャンネルに通知します。 | 雇员的商品售出时通知此频道。 | 집사의 물품이 판매되면 이 채널에 알립니다. | 雇員的商品售出時通知此頻道。 |
| `bot_cmd_retainer_remove_sale_alert_desc` | Stop sale notifications in this channel. | Arrêter les notifications de vente dans ce salon. | Verkaufsbenachrichtigungen in diesem Kanal beenden. | このチャンネルの売却通知を停止します。 | 停止此频道的售出通知。 | 이 채널의 판매 알림을 중지합니다. | 停止此頻道的售出通知。 |

- [ ] **Step 3: Check and commit**

Run: `cargo check -p ultros` (the frontend check happens in Task 7).

```bash
git add ultros/src/discord/ffxiv/retainer.rs ultros-frontend/ultros-app/src/routes/bot.rs ultros-frontend/ultros-app/locales
git commit -m "feat(discord): add_sale_alert / remove_sale_alert commands"
```

---

### Task 7: Frontend — drawer kind, rules row, retainers button, alerts hint

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/alert_drawer.rs`
- Modify: `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/retainers.rs`, `ultros-frontend/ultros-app/src/routes/alerts.rs`
- Modify: all seven locale files

- [ ] **Step 1: Write the failing drawer tests**

In `alert_drawer.rs` tests, add and extend:

```rust
    #[test]
    fn sold_kind_matches_only_retainer_sold() {
        let sold = AlertTrigger::RetainerSold {};
        assert!(trigger_matches_kind(&sold, AlertKind::Sold));
        assert!(!trigger_matches_kind(&sold, AlertKind::Undercut));
        assert!(!trigger_matches_kind(&sold, AlertKind::ItemPrice));
        let undercut = AlertTrigger::RetainerUndercut { margin_percent: 5 };
        assert!(!trigger_matches_kind(&undercut, AlertKind::Sold));
    }
```

and in `list_scoped_alerts_never_show_in_drawer` add `assert!(!trigger_matches_kind(&trigger, AlertKind::Sold));`.

- [ ] **Step 2: Implement the drawer changes**

- `AlertKind` gains `Sold`.
- `trigger_matches_kind` gains `| (AlertTrigger::RetainerSold {}, AlertKind::Sold)`.
- In `submit`'s `match kind.get()`, add `AlertKind::Sold => AlertTrigger::RetainerSold {},`.
- In the toast match, add `AlertKind::Sold => t_string!(i18n, sold_alert_created_toast).to_string(),`.
- Kind toggle: change `grid-cols-2` to `grid-cols-3` and add `{kind_btn(AlertKind::Sold, t_string!(i18n, alert_kind_sold).to_string())}`.
- After the undercut description `<Show>`, add:

```rust
                <Show when=move || !locked_to_preset_item && kind.get() == AlertKind::Sold>
                    <p class="text-sm opacity-80">{t!(i18n, sold_alert_description)}</p>
                </Show>
```

- In the existing-alerts description match, add before the `_ => String::new()` arm:

```rust
                                                    AlertTrigger::RetainerSold {} => {
                                                        t_string!(i18n, alerts_retainer_sold_rule).to_string()
                                                    }
```

- Submit label match: add `AlertKind::Sold => t_string!(i18n, sold_alert_submit).to_string(),`.

- [ ] **Step 3: Rules panel row**

In `alert_rules_panel.rs`, in the `match a.trigger.clone()` after the `RetainerUndercut` arm:

```rust
                                        AlertTrigger::RetainerSold {} => (
                                            t_string!(i18n, alerts_retainer_sold_rule).to_string(),
                                            "—".to_string(),
                                            "—".to_string(),
                                            "—".to_string(),
                                        ),
```

- [ ] **Step 4: Bell button on the listings tab**

In `routes/retainers.rs` `RetainerListings`: add `let (drawer_visible, set_drawer_visible) = signal(false);` after the `retainers` resource, replace the opening `<span class="content-title">…</span>` with a flex row carrying the button, and mount the drawer inside the `Some(Ok(_))` branch:

```rust
        <div class="flex flex-wrap items-center justify-between gap-3">
            <span class="content-title">{t!(i18n, retainers_all_listings_title)}</span>
            <button class="btn" on:click=move |_| set_drawer_visible.set(true)>
                <Icon icon=i::BsBell />
                <span class="ml-1">{t!(i18n, add_alert_button)}</span>
            </button>
        </div>
```

and, first thing inside the `Some(Ok(_)) => { view! { … } }` block:

```rust
                            <Show when=move || drawer_visible.get()>
                                <AlertDrawer
                                    initial_kind=AlertKind::Sold
                                    set_visible=set_drawer_visible.into()
                                />
                            </Show>
```

- [ ] **Step 5: Alerts page hint**

In `routes/alerts.rs`, after the `add_undercut_alert` `<code>` element and its trailing text, add `" or "` and a second code element:

```rust
                                    <code class="rounded bg-black/40 px-1.5 py-0.5">"/ffxiv retainer add_sale_alert"</code>
```

so the sentence reads: Run `/ffxiv retainer add_undercut_alert` or `/ffxiv retainer add_sale_alert` in any channel where the bot is installed.

- [ ] **Step 6: Locale keys**

Add to all seven files next to the `undercut_alert_*` keys:

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `alert_kind_sold` | Retainer sale | Vente de servant | Gehilfen-Verkauf | リテイナー売却 | 雇员售出 | 집사 판매 | 雇員售出 |
| `sold_alert_description` | Get notified when one of your claimed retainers' listings sells. Same-price listings from other sellers are skipped rather than guessed, so some sales may go unreported. | Soyez prévenu quand une annonce d'un de vos servants se vend. Les annonces au même prix d'autres vendeurs sont ignorées plutôt que devinées, certaines ventes peuvent donc ne pas être signalées. | Werde benachrichtigt, wenn ein Angebot eines deiner Gehilfen verkauft wird. Gleichpreisige Angebote anderer Verkäufer werden übersprungen statt geraten, daher können einzelne Verkäufe unerwähnt bleiben. | 登録したリテイナーの出品が売れたときに通知します。他の出品者の同額の出品は推測せずにスキップするため、通知されない売却もあります。 | 当你认领的雇员的商品售出时收到通知。其他卖家同价的商品会被跳过而不是猜测，因此部分售出可能不会通知。 | 등록한 집사의 물품이 판매되면 알림을 받습니다. 다른 판매자의 같은 가격 물품은 추측하지 않고 건너뛰므로 일부 판매는 알림이 없을 수 있습니다. | 當你認領的雇員的商品售出時收到通知。其他賣家同價的商品會被跳過而不是猜測，因此部分售出可能不會通知。 |
| `sold_alert_created_toast` | Sale alert created | Alerte de vente créée | Verkaufsalarm erstellt | 売却アラートを作成しました | 已创建售出提醒 | 판매 알림을 만들었습니다 | 已建立售出提醒 |
| `sold_alert_submit` | Create sale alert | Créer une alerte de vente | Verkaufsalarm erstellen | 売却アラートを作成 | 创建售出提醒 | 판매 알림 만들기 | 建立售出提醒 |
| `alerts_retainer_sold_rule` | Retainer sales | Ventes des servants | Gehilfen-Verkäufe | リテイナーの売却 | 雇员售出 | 집사 판매 | 雇員售出 |

- [ ] **Step 7: Build and test the frontend**

Run: `cargo test -p ultros-app alert_drawer` and `cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown` (or the project's usual `cargo leptos build` if faster on this machine).
Expected: tests pass; no missing-locale-key warnings.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend
git commit -m "feat(ui): retainer sale alert kind, rules row, listings-tab bell"
```

---

### Task 8: Changelog, docs, CI, PR

**Files:**
- Create: `ultros-changelog/changes/2026-09-07-retainer-sale-alerts.json`
- Modify: `docs/price-alerts.md`

- [ ] **Step 1: Changelog entry**

```json
{
  "category": "features",
  "importance": "high",
  "title": "Get told when your retainer sells something",
  "blurb": "Add a retainer sale alert from the Retainers page, the Alerts page, or /ffxiv retainer add_sale_alert in Discord. Ultros pairs a vanished listing with a matching sale and only notifies you when nobody else was listed at that price.",
  "link": "/retainers/listings"
}
```

- [ ] **Step 2: Docs**

Append to `docs/price-alerts.md`:

```markdown
## Retainer sale alerts

Fires when one of your claimed retainers' listings disappears and a sale with
the same world, item, quality, price and quantity is recorded within five
minutes. Sales are inferred (Universalis never says whose listing sold), so
the matcher prefers to miss a sale over reporting a wrong one: if another
seller had a listing at the same price and quantity removed in the same
window, nothing fires. Create one from the bell on `/retainers/listings`, the
Alerts page drawer, or `/ffxiv retainer add_sale_alert` in Discord.
Design: `docs/superpowers/specs/2026-09-07-retainer-sold-alert-design.md`.
```

- [ ] **Step 3: Full CI check**

Run: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`
Expected: `REAL_EXIT=0`. Fix any fmt/clippy findings (no `#[allow]`).

- [ ] **Step 4: Commit and open the PR**

```bash
git add ultros-changelog docs/price-alerts.md
git commit -m "docs: retainer sale alerts changelog and docs"
git push -u origin claude/detect-sold-listings-f16ac0
gh pr create --base main --title "Retainer sale alerts: infer a sold listing from a removal paired with a matching sale" --body-file /tmp/pr.md
```

PR body: summary of the detection rule, the measured numbers from the spec, what is deliberately skipped (cross-retainer collisions, same-retainer relists, stale history), UI surfaces, migration note (`alert_retainer_sale`, additive), and the test list. End with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

---

## Self-review

- **Spec coverage:** detection rules 1–6 → Task 3; data/table → Task 1; no new bus, shared listener, manager wiring, message, alert_event → Task 5; API → Tasks 2 and 5; Discord → Task 6; drawer/rules panel/listings button/alerts hint → Task 7; tests 1–9 → Task 3 (1,2,3,4,5,6,7,8,9 map to `removal_then_matching_sale…`, `sale_then_removal…`, `reprice_by_same_retainer…`, `cross_retainer_collision…`, `three_listings…`, `stale_sale…`, `sale_before_listing…`, `removal_outside_window…`, `expire_drops…`); replay → Task 4; changelog/docs → Task 8. E2E screenshot from the spec is not planned: the integration harness needs a live login, and the drawer markup is identical to the undercut tab's, which is already covered.
- **Type consistency:** `SaleKey`, `RemovedListing`, `AddedListing`, `ObservedSale`, `SoldEvent`, `SoldMatcher` names and signatures match between Tasks 3, 4 and 5. DB helper names match between Tasks 1, 5 and 6. `AlertKind::Sold` and `AlertTrigger::RetainerSold {}` match between Tasks 2 and 7.
