# Notification Tier 1 — Endpoint-first UI & History — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the `/alerts` page from a flat list of alerts-with-inline-delivery into a three-tab settings hub (Endpoints, Rules, History) where notification endpoints are first-class, reusable, and testable.

**Scope:** This plan covers **step 1 of the spec's build sequence** — Endpoint-first UI + history surface, no schema changes. It explicitly does **not** cover:
- The in-app SSE inbox (spec build-step 3) — separate plan.
- Bot commands (spec Tier 2) — separate plan, can run in parallel with this one.
- Web push (spec Tier 3) — separate plan, depends on this one.
- Shared list notifications (spec Tier 4) — separate plan, depends on this one.

**Architecture:** Promote `notification_endpoint` from a backing-table-implementation-detail to a top-level user resource. Existing DB tables (`notification_endpoint`, `alert_notification_rule`) need no schema changes; the migration cost is entirely in API surface and UI. Alerts gain an `endpoint_ids: Vec<i32>` field replacing the per-alert `AlertDelivery` inline blob; existing rows are already shaped correctly so no data migration is needed. A new `POST /endpoints/{id}/test` endpoint and a `POST /alerts/events/{id}/resend` endpoint round out the surface.

**Tech Stack:** Rust (workspace: `ultros`, `ultros-db`, `ultros-api-types`), Sea-ORM, Axum, Leptos (CSR+SSR), Tailwind, serde.

**Parallelization plan:** After Task 1 lands, Tasks 2 (backend DB), 6 (frontend client stubs), and 7 (frontend components) can run in three parallel worktrees because they share only the api-types shape. Tasks 3-5 (handlers) follow Task 2; Tasks 10-11 (route wiring) follow both subtrees. See "Parallel waves" at the bottom.

---

## File structure

| File | Responsibility | Status |
|---|---|---|
| `ultros-api-types/src/alert.rs` | Wire types for alerts + endpoints | Modify — add `Endpoint`, `EndpointMethod`, `CreateEndpointRequest`, `UpdateEndpointRequest`, `ResendResult`. Add `endpoint_ids: Vec<i32>` to `Alert` and `CreateAlertRequest`. |
| `ultros-db/src/alerts.rs` | Sea-ORM data access for alerts and endpoints | Modify — add `list_endpoints`, `create_endpoint`, `update_endpoint`, `delete_endpoint`, `set_alert_rules`, `get_endpoint_owned_by`, replace `get_first_endpoint_for_alert` callers with multi-endpoint flow. |
| `ultros/src/web/api/alerts.rs` | Axum handlers for alerts | Modify — change `create_alert` to consume `endpoint_ids`; change `list_alerts` to return endpoint ids; add `resend_alert_event`. |
| `ultros/src/web/api/endpoints.rs` | Axum handlers for endpoints (new file) | Create — `list_endpoints`, `create_endpoint`, `update_endpoint`, `delete_endpoint`, `test_endpoint`. |
| `ultros/src/web/api/mod.rs` (or wherever the api module lives) | Module exports | Modify — `pub mod endpoints;` |
| `ultros/src/web.rs` | Route registration | Modify — register the five endpoint routes + the resend route. |
| `ultros/src/alerts/delivery.rs` | Notification dispatch | Modify — extract a `deliver_to_endpoint(endpoint, title, body)` helper used by both `dispatch_alert` and the test/resend handlers. |
| `ultros-frontend/ultros-app/src/api.rs` | HTTP client | Modify — add `list_endpoints`, `create_endpoint`, `update_endpoint`, `delete_endpoint`, `test_endpoint`, `resend_alert_event`. |
| `ultros-frontend/ultros-app/src/routes/alerts.rs` | `/alerts` page | Rewrite — three tabs, one shared resource for endpoints. |
| `ultros-frontend/ultros-app/src/components/endpoints_panel.rs` | "Endpoints" tab UI (new file) | Create — list/create/edit/delete/test, per-method form sections. |
| `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs` | "Rules" tab UI (new file) | Create — moved out of `alerts.rs`; endpoint multi-select replaces delivery dropdown. |
| `ultros-frontend/ultros-app/src/components/history_panel.rs` | "History" tab UI (new file) | Create — moved out of `alerts.rs`; adds Resend button. |
| `ultros-frontend/ultros-app/src/components/alert_config_drawer.rs` | Per-item alert creation modal | Modify — replace delivery radio + webhook URL field with an "endpoints" multi-select + "manage endpoints" link. |
| `ultros-frontend/ultros-app/src/components/mod.rs` | Component module exports | Modify — `pub mod endpoints_panel;` etc. |

---

## Task 1: Define API types for endpoints, and add `endpoint_ids` to alerts

**Files:**
- Modify: `ultros-api-types/src/alert.rs`

- [ ] **Step 1.1: Add the endpoint API types**

Append to `ultros-api-types/src/alert.rs`:

```rust
/// Delivery channel for a notification endpoint. Mirrors the `method` discriminator
/// stored in the `notification_endpoint.method` DB column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "PascalCase")]
pub enum EndpointMethod {
    DiscordDm { user_id: i64 },
    DiscordChannel { channel_id: i64 },
    Webhook { url: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: i32,
    pub name: String,
    #[serde(flatten)]
    pub method: EndpointMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEndpointRequest {
    pub name: String,
    #[serde(flatten)]
    pub method: EndpointMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateEndpointRequest {
    pub name: Option<String>,
    #[serde(flatten)]
    pub method: Option<EndpointMethod>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResendResult {
    pub delivered: bool,
    pub error: Option<String>,
}
```

- [ ] **Step 1.2: Extend `Alert` and `CreateAlertRequest` with `endpoint_ids`**

Edit the existing `Alert` and `CreateAlertRequest` structs:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAlertRequest {
    pub trigger: AlertTrigger,
    /// Deprecated for new clients; if `endpoint_ids` is empty an endpoint is created from this.
    /// Kept for backward compatibility with existing client-side code until the drawer is migrated.
    #[serde(default)]
    pub delivery: Option<AlertDelivery>,
    /// Endpoints to attach to this alert. Required if `delivery` is None.
    #[serde(default)]
    pub endpoint_ids: Vec<i32>,
    /// Defaults to 3600 (1 hour) if omitted.
    pub cooldown_seconds: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub id: i32,
    pub trigger: AlertTrigger,
    /// Deprecated; reflects the first endpoint's method for old clients. New clients should use `endpoint_ids`.
    pub delivery: AlertDelivery,
    pub endpoint_ids: Vec<i32>,
    pub enabled: bool,
    pub cooldown_seconds: i32,
    pub last_fired_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

The `delivery` field on both is **kept for backward compatibility** with the existing frontend code until Task 11 migrates the drawer. Once Task 11 lands and we verify no callers send `delivery`, a follow-up commit can remove it.

- [ ] **Step 1.3: Write a unit test for the EndpointMethod serde shape**

Add to the bottom of `ultros-api-types/src/alert.rs`:

```rust
#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_method_serializes_with_method_tag() {
        let m = EndpointMethod::DiscordDm { user_id: 42 };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({"method": "DiscordDm", "user_id": 42}));
    }

    #[test]
    fn endpoint_serializes_with_flattened_method() {
        let e = Endpoint {
            id: 7,
            name: "Test".to_string(),
            method: EndpointMethod::Webhook { url: "https://example.invalid".into() },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            json!({"id": 7, "name": "Test", "method": "Webhook", "url": "https://example.invalid"})
        );
    }

    #[test]
    fn create_endpoint_request_round_trips() {
        let req = CreateEndpointRequest {
            name: "My channel".into(),
            method: EndpointMethod::DiscordChannel { channel_id: 9 },
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: CreateEndpointRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }
}
```

- [ ] **Step 1.4: Run the test**

```bash
cargo test -p ultros-api-types alert::endpoint_tests
```
Expected: 3 passed.

- [ ] **Step 1.5: Run check_ci**

```bash
./check_ci.sh
```
Expected: pass. If fmt fails, run `cargo fmt --all`.

- [ ] **Step 1.6: Commit**

```bash
git add ultros-api-types/src/alert.rs
git commit -m "feat(api-types): add Endpoint, CreateEndpointRequest, ResendResult; endpoint_ids on Alert"
```

---

## Task 2: DB layer — endpoint CRUD + `set_alert_rules`

> Can run **in parallel** with Tasks 6 and 7 after Task 1 is merged.

**Files:**
- Modify: `ultros-db/src/alerts.rs`

- [ ] **Step 2.1: Write the failing tests first**

Add at the bottom of `ultros-db/src/alerts.rs`. These are pure logic-level tests; they require a live DB and are therefore gated on `#[cfg(all(test, feature = "db_tests"))]` like other DB-touching tests in this crate. If `db_tests` doesn't yet exist as a feature, check `ultros-db/Cargo.toml` for the equivalent gate used elsewhere and copy it.

```rust
#[cfg(all(test, feature = "db_tests"))]
mod endpoint_tests {
    use super::*;
    use crate::test_helpers::test_db;

    #[tokio::test]
    async fn create_endpoint_and_list_returns_it() {
        let db = test_db().await;
        let id = db.create_endpoint(
            42, "My DM", "DiscordDm",
            serde_json::json!({"user_id": 42}),
        ).await.unwrap();
        let list = db.list_endpoints(42).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].name, "My DM");
    }

    #[tokio::test]
    async fn list_endpoints_scopes_by_user() {
        let db = test_db().await;
        db.create_endpoint(1, "A", "DiscordDm", serde_json::json!({"user_id": 1})).await.unwrap();
        db.create_endpoint(2, "B", "DiscordDm", serde_json::json!({"user_id": 2})).await.unwrap();
        let only_user_1 = db.list_endpoints(1).await.unwrap();
        assert_eq!(only_user_1.len(), 1);
        assert_eq!(only_user_1[0].user_id, 1);
    }

    #[tokio::test]
    async fn delete_endpoint_refuses_other_users_endpoint() {
        let db = test_db().await;
        let id = db.create_endpoint(1, "A", "DiscordDm", serde_json::json!({"user_id": 1})).await.unwrap();
        let err = db.delete_endpoint(2, id).await;
        assert!(err.is_err(), "expected delete by non-owner to fail");
        // and the row should still be there
        assert_eq!(db.list_endpoints(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn set_alert_rules_replaces_the_set() {
        let db = test_db().await;
        let e1 = db.create_endpoint(1, "A", "DiscordDm", serde_json::json!({"user_id": 1})).await.unwrap();
        let e2 = db.create_endpoint(1, "B", "Webhook", serde_json::json!({"url": "https://discord.com/api/webhooks/1/x"})).await.unwrap();
        let alert = db.create_threshold_alert(
            1, 5057, serde_json::json!({"World": 22}), 1000, false, 3600,
            "DiscordDm", serde_json::json!({"user_id": 1}), "tmp",
        ).await.unwrap();
        db.set_alert_rules(1, alert.id, &[e1, e2]).await.unwrap();
        let endpoints = db.get_notification_endpoints_for_alert(alert.id).await.unwrap();
        assert_eq!(endpoints.len(), 2);
        db.set_alert_rules(1, alert.id, &[e1]).await.unwrap();
        let endpoints = db.get_notification_endpoints_for_alert(alert.id).await.unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].id, e1);
    }
}
```

If `crate::test_helpers::test_db` does not exist, look for the existing convention in `ultros-db/src/` (e.g. a `tests/` directory or an existing `#[cfg(test)]` module that spawns a test DB) and adapt. If there is no convention, mark these tests `#[ignore]` and write a follow-up note in the commit message — do NOT invent a new test infrastructure as part of this task.

- [ ] **Step 2.2: Run the tests to verify they fail**

```bash
cargo test -p ultros-db endpoint_tests --features db_tests
```
Expected: compile errors / `create_endpoint not found` etc. If `db_tests` feature doesn't exist or no DB is available, this step is "tests don't compile" which is the analog of "tests fail" — proceed to implementation.

- [ ] **Step 2.3: Implement `list_endpoints`, `create_endpoint`, `update_endpoint`, `delete_endpoint`**

Add to `impl UltrosDb` in `ultros-db/src/alerts.rs`:

```rust
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
```

- [ ] **Step 2.4: Implement `set_alert_rules` and a list helper**

Add to the same `impl UltrosDb` block:

```rust
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

/// Return the endpoint ids attached to an alert, in order of attachment.
pub async fn list_endpoint_ids_for_alert(&self, alert_id: i32) -> Result<Vec<i32>> {
    let rules = alert_notification_rule::Entity::find()
        .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
        .all(&self.db)
        .await?;
    Ok(rules.into_iter().map(|r| r.endpoint_id).collect())
}
```

- [ ] **Step 2.5: Run the tests to verify they pass**

```bash
cargo test -p ultros-db endpoint_tests --features db_tests
```
Expected: 4 passed (or all `#[ignore]`d if the test DB scaffolding wasn't present).

- [ ] **Step 2.6: Run check_ci**

```bash
./check_ci.sh
```

- [ ] **Step 2.7: Commit**

```bash
git add ultros-db/src/alerts.rs
git commit -m "feat(ultros-db): endpoint CRUD + set_alert_rules + list_endpoint_ids_for_alert"
```

---

## Task 3: Webhook URL validator and Discord channel validator extracted

**Files:**
- Modify: `ultros/src/web/api/alerts.rs` (extract) → `ultros/src/web/api/endpoints.rs` (new file)

- [ ] **Step 3.1: Move `validate_discord_webhook_url` into a shared module**

Create `ultros/src/web/api/endpoint_validation.rs` (or place at top of `endpoints.rs` and re-import from `alerts.rs`):

```rust
use crate::web::error::ApiError;

#[allow(clippy::result_large_err)]
pub(crate) fn validate_discord_webhook_url(url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ApiError::from(anyhow::anyhow!("invalid webhook URL: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(ApiError::from(anyhow::anyhow!("webhook URL must use https")));
    }
    let host = parsed.host_str().unwrap_or("");
    let allowed = [
        "discord.com",
        "discordapp.com",
        "ptb.discord.com",
        "canary.discord.com",
    ];
    if !allowed.contains(&host) {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL host must be a Discord webhook host"
        )));
    }
    if !parsed.path().starts_with("/api/webhooks/") {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL path must start with /api/webhooks/"
        )));
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
pub(crate) fn validate_discord_channel_id(channel_id: i64) -> Result<(), ApiError> {
    if channel_id <= 0 {
        return Err(ApiError::from(anyhow::anyhow!("channel_id must be positive")));
    }
    Ok(())
}
```

Update `alerts.rs` to `use crate::web::api::endpoint_validation::validate_discord_webhook_url;` and delete the local copy. **Do not delete or rename the existing tests** — they should keep passing against the moved function.

- [ ] **Step 3.2: Run `cargo test`**

```bash
cargo test -p ultros web::api::alerts::tests
```
Expected: all 12 existing tests still pass.

- [ ] **Step 3.3: Commit**

```bash
git add ultros/src/web/api/endpoint_validation.rs ultros/src/web/api/alerts.rs
git commit -m "refactor: extract endpoint validation helpers"
```

---

## Task 4: Endpoint API handlers + routes

> Depends on Tasks 1–3. Can run **in parallel** with Task 5.

**Files:**
- Create: `ultros/src/web/api/endpoints.rs`
- Modify: `ultros/src/web/api/mod.rs` (or whichever file declares `pub mod alerts;`)
- Modify: `ultros/src/web.rs`

- [ ] **Step 4.1: Write the handler tests first**

At the bottom of the new `ultros/src/web/api/endpoints.rs`, add unit tests for the conversion logic (no HTTP needed):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ultros_api_types::alert::EndpointMethod;

    #[test]
    fn method_to_db_round_trip_discord_dm() {
        let m = EndpointMethod::DiscordDm { user_id: 99 };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "DiscordDm");
        assert_eq!(config, json!({"user_id": 99}));
    }

    #[test]
    fn method_to_db_round_trip_discord_channel() {
        let m = EndpointMethod::DiscordChannel { channel_id: 12345 };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "DiscordChannel");
        assert_eq!(config, json!({"channel_id": 12345}));
    }

    #[test]
    fn method_to_db_round_trip_webhook() {
        let url = "https://discord.com/api/webhooks/1/abc";
        let m = EndpointMethod::Webhook { url: url.into() };
        let (method, config) = method_to_db(&m);
        assert_eq!(method, "Webhook");
        assert_eq!(config, json!({"url": url}));
    }

    #[test]
    fn db_to_method_round_trip_all_three() {
        for m in [
            EndpointMethod::DiscordDm { user_id: 1 },
            EndpointMethod::DiscordChannel { channel_id: 2 },
            EndpointMethod::Webhook { url: "https://discord.com/api/webhooks/1/abc".into() },
        ] {
            let (method, config) = method_to_db(&m);
            let back = db_to_method(&method, &config).unwrap();
            assert_eq!(m, back);
        }
    }

    #[test]
    fn validate_method_rejects_bad_webhook_url() {
        let m = EndpointMethod::Webhook { url: "http://evil.example/api/webhooks/1/x".into() };
        assert!(validate_endpoint_method(&m).is_err());
    }

    #[test]
    fn validate_method_rejects_zero_channel_id() {
        let m = EndpointMethod::DiscordChannel { channel_id: 0 };
        assert!(validate_endpoint_method(&m).is_err());
    }
}
```

- [ ] **Step 4.2: Implement the handlers**

Write `ultros/src/web/api/endpoints.rs`:

```rust
use axum::{Json, extract::{Path, State}};
use serde_json::Value as JsonValue;
use ultros_api_types::alert::{
    CreateEndpointRequest, Endpoint, EndpointMethod, ResendResult, UpdateEndpointRequest,
};
use ultros_db::UltrosDb;

use crate::web::api::endpoint_validation::{
    validate_discord_channel_id, validate_discord_webhook_url,
};
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

pub(crate) fn method_to_db(m: &EndpointMethod) -> (&'static str, JsonValue) {
    match m {
        EndpointMethod::DiscordDm { user_id } => ("DiscordDm", serde_json::json!({"user_id": user_id})),
        EndpointMethod::DiscordChannel { channel_id } => {
            ("DiscordChannel", serde_json::json!({"channel_id": channel_id}))
        }
        EndpointMethod::Webhook { url } => ("Webhook", serde_json::json!({"url": url})),
    }
}

pub(crate) fn db_to_method(method: &str, config: &JsonValue) -> anyhow::Result<EndpointMethod> {
    match method {
        "DiscordDm" => Ok(EndpointMethod::DiscordDm {
            user_id: config.get("user_id").and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("DiscordDm missing user_id"))?,
        }),
        "DiscordChannel" => Ok(EndpointMethod::DiscordChannel {
            channel_id: config.get("channel_id").and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("DiscordChannel missing channel_id"))?,
        }),
        "Webhook" => Ok(EndpointMethod::Webhook {
            url: config.get("url").and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Webhook missing url"))?
                .to_string(),
        }),
        other => Err(anyhow::anyhow!("unknown method {other}")),
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn validate_endpoint_method(m: &EndpointMethod) -> Result<(), ApiError> {
    match m {
        EndpointMethod::Webhook { url } => validate_discord_webhook_url(url),
        EndpointMethod::DiscordChannel { channel_id } => validate_discord_channel_id(*channel_id),
        EndpointMethod::DiscordDm { .. } => Ok(()),
    }
}

pub(crate) async fn list_endpoints(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<Endpoint>>, ApiError> {
    let rows = db.list_endpoints(user.id as i64).await.map_err(ApiError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let method = db_to_method(&r.method, &r.config).map_err(ApiError::from)?;
        out.push(Endpoint { id: r.id, name: r.name, method });
    }
    Ok(Json(out))
}

pub(crate) async fn create_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreateEndpointRequest>,
) -> Result<Json<Endpoint>, ApiError> {
    validate_endpoint_method(&req.method)?;
    let (method, config) = method_to_db(&req.method);
    let id = db
        .create_endpoint(user.id as i64, &req.name, method, config)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(Endpoint { id, name: req.name, method: req.method }))
}

pub(crate) async fn update_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Json(req): Json<UpdateEndpointRequest>,
) -> Result<Json<()>, ApiError> {
    let method_and_config = match &req.method {
        Some(m) => {
            validate_endpoint_method(m)?;
            let (method, config) = method_to_db(m);
            Some((method.to_string(), config))
        }
        None => None,
    };
    db.update_endpoint(user.id as i64, id, req.name, method_and_config)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

pub(crate) async fn delete_endpoint(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<()>, ApiError> {
    db.delete_endpoint(user.id as i64, id).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

pub(crate) async fn test_endpoint(
    State(db): State<UltrosDb>,
    State(serenity_ctx): State<std::sync::Arc<poise::serenity_prelude::Context>>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<ResendResult>, ApiError> {
    let endpoint = db
        .get_endpoint_owned_by(user.id as i64, id)
        .await
        .map_err(ApiError::from)?;
    match crate::alerts::delivery::deliver_to_endpoint(
        &endpoint, "Ultros test notification",
        "If you can read this, your endpoint is wired up correctly.",
        &db, &serenity_ctx,
    ).await {
        Ok(()) => Ok(Json(ResendResult { delivered: true, error: None })),
        Err(e) => Ok(Json(ResendResult { delivered: false, error: Some(format!("{e}")) })),
    }
}
```

The `serenity_ctx` injection assumes the app's `State` already carries a serenity `Context`. Verify by searching for `Arc<serenity_prelude::Context>` in `ultros/src/web.rs` — if the state shape differs, adapt the extractor to match (the exact shape was not loaded in plan-writing context).

- [ ] **Step 4.3: Register the routes**

Edit `ultros/src/web.rs`:

```rust
use crate::web::api::endpoints::{
    create_endpoint, delete_endpoint, list_endpoints, test_endpoint, update_endpoint,
};
```

In the router-builder where the existing alert routes are registered, add:

```rust
.route("/api/v1/endpoints", get(list_endpoints).post(create_endpoint))
.route(
    "/api/v1/endpoints/{id}",
    axum::routing::patch(update_endpoint).delete(delete_endpoint),
)
.route("/api/v1/endpoints/{id}/test", post(test_endpoint))
```

Add `pub mod endpoints;` and `pub mod endpoint_validation;` to whichever module declares `pub mod alerts;` (likely `ultros/src/web/api/mod.rs` or `ultros/src/web/api.rs`).

- [ ] **Step 4.4: Run tests + check_ci**

```bash
cargo test -p ultros web::api::endpoints::tests
./check_ci.sh
```

Note: `test_endpoint` calls `deliver_to_endpoint` which doesn't exist yet — Task 5 introduces it. Until Task 5 lands, you may need to comment out the body of `test_endpoint` and return `Ok(Json(ResendResult { delivered: false, error: Some("deliver_to_endpoint not yet wired".into()) }))` — or merge Tasks 4 and 5 sequentially in one agent. The plan assumes sequential; if running in parallel make Task 5 a prerequisite of Task 4's final compile.

- [ ] **Step 4.5: Commit**

```bash
git add ultros/src/web/api/endpoints.rs ultros/src/web/api/endpoint_validation.rs ultros/src/web.rs ultros/src/web/api/mod.rs
git commit -m "feat(api): /api/v1/endpoints CRUD + test"
```

---

## Task 5: Extract `deliver_to_endpoint` and add `resend_alert_event`

**Files:**
- Modify: `ultros/src/alerts/delivery.rs`
- Modify: `ultros/src/web/api/alerts.rs`
- Modify: `ultros/src/web.rs`

- [ ] **Step 5.1: Extract a single-endpoint delivery helper**

In `ultros/src/alerts/delivery.rs`, add a new public-in-crate function alongside `dispatch_alert`:

```rust
/// Deliver a single message to one endpoint. Returns Ok(()) on success.
/// Used by `dispatch_alert` (fan-out), the endpoint test handler, and the resend handler.
pub(crate) async fn deliver_to_endpoint(
    endpoint: &ultros_db::entity::notification_endpoint::Model,
    title: &str,
    body: &str,
    _db: &UltrosDb,
    ctx: &serenity_prelude::Context,
) -> Result<()> {
    let parsed = parse_endpoint_config(&endpoint.method, &endpoint.config)?;
    match parsed {
        EndpointConfig::DiscordChannel { channel_id } => send_to_channel(channel_id, title, body, ctx).await,
        EndpointConfig::DiscordDm { user_id } => send_dm(user_id, title, body, ctx).await,
        EndpointConfig::Webhook { url } => send_webhook(&url, title, body).await,
    }
}
```

Then refactor the body of `dispatch_alert` to call `deliver_to_endpoint(&endpoint, title, body, db, ctx)` inside its loop instead of the inline match. The existing fan-out, error aggregation, and "any_ok" logic stays.

- [ ] **Step 5.2: Add `resend_alert_event` handler**

In `ultros/src/web/api/alerts.rs`, add:

```rust
pub(crate) async fn resend_alert_event(
    State(db): State<UltrosDb>,
    State(serenity_ctx): State<std::sync::Arc<poise::serenity_prelude::Context>>,
    user: AuthDiscordUser,
    Path(event_id): Path<i64>,
) -> Result<Json<ResendResult>, ApiError> {
    let event = db
        .get_alert_event_by_id_owned_by(user.id as i64, event_id)
        .await
        .map_err(ApiError::from)?;
    let endpoints = db.get_notification_endpoints_for_alert(event.alert_id).await.map_err(ApiError::from)?;
    if endpoints.is_empty() {
        return Ok(Json(ResendResult {
            delivered: false,
            error: Some("alert has no endpoints".into()),
        }));
    }
    let title = "Ultros alert (resend)";
    let body = format!(
        "Resending alert for item {} (matched price: {:?})",
        event.item_id, event.matched_price
    );
    let mut last_err: Option<String> = None;
    let mut any_ok = false;
    for endpoint in endpoints {
        match crate::alerts::delivery::deliver_to_endpoint(&endpoint, title, &body, &db, &serenity_ctx).await {
            Ok(()) => any_ok = true,
            Err(e) => last_err = Some(format!("{e}")),
        }
    }
    Ok(Json(ResendResult { delivered: any_ok, error: last_err }))
}
```

- [ ] **Step 5.3: Add the corresponding DB helper**

In `ultros-db/src/alerts.rs`:

```rust
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
```

- [ ] **Step 5.4: Register the route**

In `ultros/src/web.rs`, after the existing alerts events route:

```rust
.route("/api/v1/alerts/events/{id}/resend", post(resend_alert_event))
```

Import `use crate::web::api::alerts::resend_alert_event;`.

- [ ] **Step 5.5: Verify with check_ci**

```bash
./check_ci.sh
```

- [ ] **Step 5.6: Commit**

```bash
git add ultros/src/alerts/delivery.rs ultros/src/web/api/alerts.rs ultros/src/web.rs ultros-db/src/alerts.rs
git commit -m "feat(api): deliver_to_endpoint helper + alert event resend"
```

---

## Task 6: Frontend HTTP client functions

> Can run **in parallel** with Tasks 2-5 once Task 1 lands.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/api.rs`

- [ ] **Step 6.1: Add the imports**

Near the top of `ultros-frontend/ultros-app/src/api.rs`, extend the alert-related imports:

```rust
use ultros_api_types::alert::{
    Alert, AlertEvent, CreateAlertRequest, CreateEndpointRequest, Endpoint, ResendResult,
    UpdateAlertRequest, UpdateEndpointRequest,
};
```

- [ ] **Step 6.2: Add the six client functions**

Append after the existing `get_alert_events`:

```rust
pub(crate) async fn list_endpoints() -> AppResult<Vec<Endpoint>> {
    fetch_api("/api/v1/endpoints").await
}

pub(crate) async fn create_endpoint(req: CreateEndpointRequest) -> AppResult<Endpoint> {
    post_api("/api/v1/endpoints", req).await
}

pub(crate) async fn update_endpoint(id: i32, req: UpdateEndpointRequest) -> AppResult<()> {
    patch_api(&format!("/api/v1/endpoints/{id}"), req).await
}

pub(crate) async fn delete_endpoint(id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/endpoints/{id}")).await
}

pub(crate) async fn test_endpoint(id: i32) -> AppResult<ResendResult> {
    post_api(&format!("/api/v1/endpoints/{id}/test"), ()).await
}

pub(crate) async fn resend_alert_event(event_id: i64) -> AppResult<ResendResult> {
    post_api(&format!("/api/v1/alerts/events/{event_id}/resend"), ()).await
}
```

If `post_api` does not accept `()` as a body (i.e. it requires a Serialize body), pass `&serde_json::json!({})` instead. Verify by looking at the signature of `post_api` in this file before committing.

- [ ] **Step 6.3: Run `cargo check`**

```bash
cargo check -p ultros-app --target wasm32-unknown-unknown
```

If wasm target isn't installed, fall back to `cargo leptos build` from the workspace root.

- [ ] **Step 6.4: Commit**

```bash
git add ultros-frontend/ultros-app/src/api.rs
git commit -m "feat(frontend): client stubs for endpoints + resend"
```

---

## Task 7: `EndpointsPanel` component

> Can run **in parallel** with Tasks 8 and 9.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/endpoints_panel.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

- [ ] **Step 7.1: Write the component**

```rust
use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{CreateEndpointRequest, Endpoint, EndpointMethod};

use crate::api::{create_endpoint, delete_endpoint, list_endpoints, test_endpoint};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;

#[component]
pub fn EndpointsPanel() -> impl IntoView {
    let version = RwSignal::new(0u64);
    let endpoints = Resource::new(move || version.get(), move |_| list_endpoints());
    let toasts = use_toast();
    let (show_form, set_show_form) = signal(false);

    let on_delete = move |id: i32| {
        spawn_local(async move {
            match delete_endpoint(id).await {
                Ok(()) => {
                    if let Some(t) = toasts { t.success("Endpoint deleted"); }
                    version.update(|v| *v += 1);
                }
                Err(e) => { if let Some(t) = toasts { t.error(format!("{e}")); } }
            }
        });
    };

    let on_test = move |id: i32| {
        spawn_local(async move {
            match test_endpoint(id).await {
                Ok(r) if r.delivered => { if let Some(t) = toasts { t.success("Test delivered"); } }
                Ok(r) => { if let Some(t) = toasts { t.error(r.error.unwrap_or_else(|| "Delivery failed".into())); } }
                Err(e) => { if let Some(t) = toasts { t.error(format!("{e}")); } }
            }
        });
    };

    view! {
        <div class="space-y-4">
            <div class="flex justify-between items-center">
                <h2 class="text-lg font-semibold">"Endpoints"</h2>
                <button class="btn" on:click=move |_| set_show_form.update(|v| *v = !*v)>
                    {move || if show_form.get() { "Cancel" } else { "Add endpoint" }}
                </button>
            </div>

            <Show when=move || show_form.get()>
                <EndpointCreateForm
                    on_created=Callback::new(move |_| { set_show_form.set(false); version.update(|v| *v += 1); })
                />
            </Show>

            <Suspense fallback=move || view! { <div>"Loading..."</div> }>
                {move || endpoints.get().map(|r| match r {
                    Ok(rows) if rows.is_empty() => view! {
                        <p class="opacity-70">"No endpoints yet. Add one to receive alerts."</p>
                    }.into_any(),
                    Ok(rows) => view! {
                        <ul class="divide-y">
                            <For
                                each=move || rows.clone()
                                key=|e| e.id
                                children=move |e: Endpoint| {
                                    let label = match &e.method {
                                        EndpointMethod::DiscordDm { .. } => "Discord DM",
                                        EndpointMethod::DiscordChannel { .. } => "Discord Channel",
                                        EndpointMethod::Webhook { .. } => "Webhook",
                                    };
                                    let id = e.id;
                                    view! {
                                        <li class="flex items-center justify-between py-2">
                                            <div>
                                                <div class="font-medium">{e.name.clone()}</div>
                                                <div class="text-xs opacity-70">{label}</div>
                                            </div>
                                            <div class="flex gap-1">
                                                <button class="btn-ghost" on:click=move |_| on_test(id)>
                                                    <Icon icon=i::BsSend />
                                                    <span class="ml-1">"Test"</span>
                                                </button>
                                                <button
                                                    class="btn-ghost text-red-400"
                                                    on:click=move |_| on_delete(id)
                                                >
                                                    <Icon icon=i::BiTrashSolid />
                                                </button>
                                            </div>
                                        </li>
                                    }
                                }
                            />
                        </ul>
                    }.into_any(),
                    Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn EndpointCreateForm(#[prop(into)] on_created: Callback<()>) -> impl IntoView {
    let (name, set_name) = signal::<String>("".into());
    let (method_kind, set_method_kind) = signal::<&'static str>("discord_dm");
    let (channel_id, set_channel_id) = signal::<String>("".into());
    let (webhook_url, set_webhook_url) = signal::<String>("".into());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let submit = move |_| {
        set_error.set(None);
        let n = name.get();
        if n.trim().is_empty() {
            set_error.set(Some("Name is required".into()));
            return;
        }
        let method = match method_kind.get() {
            "discord_channel" => {
                let Ok(cid) = channel_id.get().parse::<i64>() else {
                    set_error.set(Some("Channel ID must be a number".into()));
                    return;
                };
                EndpointMethod::DiscordChannel { channel_id: cid }
            }
            "webhook" => {
                let url = webhook_url.get();
                if url.trim().is_empty() {
                    set_error.set(Some("Webhook URL required".into()));
                    return;
                }
                EndpointMethod::Webhook { url }
            }
            // DiscordDm uses the *current user's* discord id. We pass 0 here and let the
            // server fill it in if it sees method=DiscordDm with user_id=0; OR we read
            // the current user via a context. Simpler: pass 0; backend rewrites to user.id.
            _ => EndpointMethod::DiscordDm { user_id: 0 },
        };
        let req = CreateEndpointRequest { name: n, method };
        spawn_local(async move {
            match create_endpoint(req).await {
                Ok(_) => {
                    if let Some(t) = toasts { t.success("Endpoint created"); }
                    on_created.run(());
                }
                Err(e) => set_error.set(Some(format!("{e}"))),
            }
        });
    };

    view! {
        <div class="p-3 border rounded space-y-3">
            <div class="space-y-1">
                <label class="text-sm font-semibold">"Name"</label>
                <input class="input w-full" prop:value=name
                    on:input=move |e| set_name.set(event_target_value(&e)) />
            </div>
            <div class="space-y-1">
                <label class="text-sm font-semibold">"Method"</label>
                <select class="input w-full" prop:value=method_kind
                    on:change=move |e| {
                        let v = event_target_value(&e);
                        set_method_kind.set(match v.as_str() {
                            "discord_channel" => "discord_channel",
                            "webhook" => "webhook",
                            _ => "discord_dm",
                        });
                    }>
                    <option value="discord_dm">"Discord DM (me)"</option>
                    <option value="discord_channel">"Discord channel"</option>
                    <option value="webhook">"Webhook URL"</option>
                </select>
            </div>
            <Show when=move || method_kind.get() == "discord_channel">
                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Channel ID"</label>
                    <input class="input w-full" prop:value=channel_id
                        on:input=move |e| set_channel_id.set(event_target_value(&e)) />
                </div>
            </Show>
            <Show when=move || method_kind.get() == "webhook">
                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Webhook URL"</label>
                    <input class="input w-full" prop:value=webhook_url
                        on:input=move |e| set_webhook_url.set(event_target_value(&e)) />
                </div>
            </Show>
            <Show when=move || error.get().is_some()>
                <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
            </Show>
            <div class="flex justify-end">
                <button class="btn" on:click=submit>"Create"</button>
            </div>
        </div>
    }
}
```

**Server-side note for the DiscordDm hack:** the form passes `user_id: 0` for DiscordDm because the frontend doesn't always have the caller's discord id handy. The backend's `create_endpoint` handler (Task 4) must therefore rewrite `EndpointMethod::DiscordDm { user_id: 0 }` to the authenticated user's id before persisting. **Add this rewrite to Task 4's `create_endpoint` before the `method_to_db` call.**

To avoid forgetting: add to Task 4's `create_endpoint` body, between `validate_endpoint_method` and `method_to_db`:

```rust
let method = match req.method {
    EndpointMethod::DiscordDm { user_id: 0 } => EndpointMethod::DiscordDm { user_id: user.id as i64 },
    other => other,
};
```

Then use `method` instead of `req.method` for the rest.

- [ ] **Step 7.2: Wire the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add:

```rust
pub mod endpoints_panel;
```

- [ ] **Step 7.3: cargo check**

```bash
cargo check -p ultros-app --target wasm32-unknown-unknown
```

- [ ] **Step 7.4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/endpoints_panel.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(frontend): EndpointsPanel component"
```

---

## Task 8: `AlertRulesPanel` component

> Can run **in parallel** with Tasks 7 and 9.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

- [ ] **Step 8.1: Move the existing "Active rules" table into a new component**

Copy the `<section>` block currently in `routes/alerts.rs` that renders the "Active rules" table. Replace the inline delivery column with an endpoint-name list resolved from the new `endpoint_ids: Vec<i32>` on `Alert` against the endpoints Resource.

```rust
use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{Alert, AlertTrigger, Endpoint, UpdateAlertRequest};
use xiv_gen::ItemId;

use crate::api::{delete_alert, get_alerts, list_endpoints, patch_alert};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;

#[component]
pub fn AlertRulesPanel() -> impl IntoView {
    let version = RwSignal::new(0u64);
    let alerts = Resource::new(move || version.get(), move |_| get_alerts());
    let endpoints = Resource::new(move || version.get(), move |_| list_endpoints());
    let toasts = use_toast();

    let toggle = move |alert: Alert| {
        let new_enabled = !alert.enabled;
        spawn_local(async move {
            match patch_alert(alert.id, UpdateAlertRequest { enabled: Some(new_enabled), price_threshold: None }).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(if new_enabled { "Alert enabled" } else { "Alert disabled" });
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => { if let Some(t) = toasts { t.error(format!("{e}")); } }
            }
        });
    };

    let remove = move |id: i32| {
        spawn_local(async move {
            match delete_alert(id).await {
                Ok(()) => { if let Some(t) = toasts { t.success("Alert deleted"); } version.update(|v| *v += 1); }
                Err(e) => { if let Some(t) = toasts { t.error(format!("{e}")); } }
            }
        });
    };

    view! {
        <Suspense fallback=move || view! { <div>"Loading..."</div> }>
            {move || {
                let endpoint_list = endpoints.get().and_then(|r| r.ok()).unwrap_or_default();
                let ep_name = move |id: i32| {
                    endpoint_list.iter().find(|e| e.id == id).map(|e| e.name.clone()).unwrap_or_else(|| format!("#{id}"))
                };
                alerts.get().map(|r| match r {
                    Ok(rows) if rows.is_empty() => view! {
                        <p class="opacity-70">"No alerts yet. Add one from any item on a list."</p>
                    }.into_any(),
                    Ok(rows) => view! {
                        <div class="overflow-x-auto">
                            <table class="w-full text-sm">
                                <thead>
                                    <tr>
                                        <th class="text-left p-1">"Item"</th>
                                        <th class="text-left p-1">"Threshold"</th>
                                        <th class="text-left p-1">"World"</th>
                                        <th class="text-left p-1">"HQ"</th>
                                        <th class="text-left p-1">"Endpoints"</th>
                                        <th class="text-left p-1">"Status"</th>
                                        <th class="text-left p-1">"Actions"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    <For each=move || rows.clone() key=|a| a.id
                                        children=move |a: Alert| {
                                            let AlertTrigger::BelowThreshold {
                                                item_id, price_threshold, hq_only, world_selector,
                                            } = a.trigger.clone();
                                            let item_name = tracked_data().items.get(&ItemId(item_id))
                                                .map(|it| it.name.as_str().to_string())
                                                .unwrap_or_else(|| format!("Item {item_id}"));
                                            let threshold_str = format!("≤ {price_threshold} gil");
                                            let world_str = match world_selector {
                                                ultros_api_types::world_helper::AnySelector::World(id) => format!("World({id})"),
                                                ultros_api_types::world_helper::AnySelector::Datacenter(id) => format!("DC({id})"),
                                                ultros_api_types::world_helper::AnySelector::Region(id) => format!("Region({id})"),
                                            };
                                            let hq_str = if hq_only { "HQ" } else { "any" };
                                            let endpoints_str = a.endpoint_ids.iter()
                                                .map(|id| ep_name(*id))
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            let enabled = a.enabled;
                                            let a_clone = a.clone();
                                            let id = a.id;
                                            view! {
                                                <tr class="border-t">
                                                    <td class="p-1">{item_name}</td>
                                                    <td class="p-1">{threshold_str}</td>
                                                    <td class="p-1">{world_str}</td>
                                                    <td class="p-1">{hq_str}</td>
                                                    <td class="p-1">{endpoints_str}</td>
                                                    <td class="p-1">{if enabled { "enabled" } else { "disabled" }}</td>
                                                    <td class="p-1 flex gap-1">
                                                        <button class="btn-ghost" aria-label="Toggle enabled"
                                                            on:click=move |_| toggle(a_clone.clone())>
                                                            <Icon icon=if enabled { i::BsPauseFill } else { i::BsPlayFill } />
                                                        </button>
                                                        <button class="btn-ghost text-red-400" aria-label="Delete alert"
                                                            on:click=move |_| remove(id)>
                                                            <Icon icon=i::BiTrashSolid />
                                                        </button>
                                                    </td>
                                                </tr>
                                            }
                                        }
                                    />
                                </tbody>
                            </table>
                        </div>
                    }.into_any(),
                    Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
                })
            }}
        </Suspense>
    }
}
```

- [ ] **Step 8.2: Register module**

Add `pub mod alert_rules_panel;` to `components/mod.rs`.

- [ ] **Step 8.3: cargo check + commit**

```bash
cargo check -p ultros-app --target wasm32-unknown-unknown
git add ultros-frontend/ultros-app/src/components/alert_rules_panel.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(frontend): AlertRulesPanel component with endpoint name resolution"
```

---

## Task 9: `HistoryPanel` component with Resend

> Can run **in parallel** with Tasks 7 and 8.

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/history_panel.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

- [ ] **Step 9.1: Write the component**

```rust
use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::AlertEvent;
use xiv_gen::ItemId;

use crate::api::{get_alert_events, resend_alert_event};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;

#[component]
pub fn HistoryPanel() -> impl IntoView {
    let version = RwSignal::new(0u64);
    let events = Resource::new(move || version.get(), move |_| get_alert_events());
    let toasts = use_toast();

    let resend = move |event_id: i64| {
        spawn_local(async move {
            match resend_alert_event(event_id).await {
                Ok(r) if r.delivered => { if let Some(t) = toasts { t.success("Resent"); } version.update(|v| *v += 1); }
                Ok(r) => { if let Some(t) = toasts { t.error(r.error.unwrap_or_else(|| "Resend failed".into())); } }
                Err(e) => { if let Some(t) = toasts { t.error(format!("{e}")); } }
            }
        });
    };

    view! {
        <Suspense fallback=move || view! { <div>"Loading..."</div> }>
            {move || events.get().map(|r| match r {
                Ok(rows) if rows.is_empty() => view! {
                    <p class="opacity-70">"No fires yet."</p>
                }.into_any(),
                Ok(rows) => view! {
                    <div class="overflow-x-auto">
                        <table class="w-full text-sm">
                            <thead>
                                <tr>
                                    <th class="text-left p-1">"Time"</th>
                                    <th class="text-left p-1">"Item"</th>
                                    <th class="text-left p-1">"Matched price"</th>
                                    <th class="text-left p-1">"Delivered"</th>
                                    <th class="text-left p-1">"Actions"</th>
                                </tr>
                            </thead>
                            <tbody>
                                <For each=move || rows.clone() key=|e| e.id
                                    children=move |e: AlertEvent| {
                                        let item_name = tracked_data().items.get(&ItemId(e.item_id))
                                            .map(|it| it.name.as_str().to_string())
                                            .unwrap_or_else(|| format!("Item {}", e.item_id));
                                        let fired_str = e.fired_at.to_rfc3339();
                                        let price_str = e.matched_price.map(|p| p.to_string()).unwrap_or_else(|| "\u{2014}".into());
                                        let delivered_str = if e.delivered {
                                            "\u{2713}".to_string()
                                        } else {
                                            e.delivery_error.as_deref().unwrap_or("\u{2717}").to_string()
                                        };
                                        let event_id = e.id;
                                        let delivered = e.delivered;
                                        view! {
                                            <tr class="border-t">
                                                <td class="p-1">{fired_str}</td>
                                                <td class="p-1">{item_name}</td>
                                                <td class="p-1">{price_str}</td>
                                                <td class="p-1">{delivered_str}</td>
                                                <td class="p-1">
                                                    <Show when=move || !delivered>
                                                        <button class="btn-ghost" on:click=move |_| resend(event_id)>
                                                            <Icon icon=i::BsArrowRepeat />
                                                            <span class="ml-1">"Resend"</span>
                                                        </button>
                                                    </Show>
                                                </td>
                                            </tr>
                                        }
                                    }
                                />
                            </tbody>
                        </table>
                    </div>
                }.into_any(),
                Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
            })}
        </Suspense>
    }
}
```

- [ ] **Step 9.2: Register module + commit**

```bash
# add `pub mod history_panel;` to components/mod.rs
cargo check -p ultros-app --target wasm32-unknown-unknown
git add ultros-frontend/ultros-app/src/components/history_panel.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "feat(frontend): HistoryPanel with Resend"
```

---

## Task 10: Update `create_alert` and `list_alerts` handlers for endpoint_ids

> Depends on Tasks 1–5.

**Files:**
- Modify: `ultros/src/web/api/alerts.rs`

- [ ] **Step 10.1: Update `create_alert` to honor `endpoint_ids`**

Replace the existing `create_alert` body:

```rust
pub(crate) async fn create_alert(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(req): Json<CreateAlertRequest>,
) -> Result<Json<Alert>, ApiError> {
    let cooldown = resolve_cooldown_seconds(req.cooldown_seconds);
    let owner = user.id as i64;

    let AlertTrigger::BelowThreshold { item_id, world_selector, price_threshold, hq_only } = req.trigger;
    validate_price_threshold(price_threshold)?;

    let world_selector_json = serde_json::to_value(world_selector)
        .map_err(|e| ApiError::from(anyhow::anyhow!("invalid world_selector: {e}")))?;

    // Three paths:
    //  1. endpoint_ids non-empty → create alert + bind to those endpoints (preferred)
    //  2. endpoint_ids empty AND delivery provided → legacy path: create endpoint inline, bind one rule
    //  3. neither → error
    if !req.endpoint_ids.is_empty() {
        // Verify all endpoints belong to this user before creating the alert.
        for &eid in &req.endpoint_ids {
            db.get_endpoint_owned_by(owner, eid).await.map_err(ApiError::from)?;
        }
        let alert = db.create_threshold_alert_without_endpoint(
            owner, item_id, world_selector_json, price_threshold, hq_only, cooldown,
        ).await.map_err(ApiError::from)?;
        db.set_alert_rules(owner, alert.id, &req.endpoint_ids).await.map_err(ApiError::from)?;

        return Ok(Json(Alert {
            id: alert.id,
            trigger: AlertTrigger::BelowThreshold { item_id, world_selector, price_threshold, hq_only },
            delivery: AlertDelivery::DiscordDm, // deprecated fallback
            endpoint_ids: req.endpoint_ids,
            enabled: alert.enabled,
            cooldown_seconds: alert.cooldown_seconds,
            last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        }));
    }

    // Legacy path
    let delivery = req.delivery.ok_or_else(|| {
        ApiError::from(anyhow::anyhow!("either endpoint_ids or delivery must be supplied"))
    })?;

    let (notification_method, notification_config, notification_name): (&str, _, String) = match &delivery {
        AlertDelivery::DiscordDm => (
            "DiscordDm",
            serde_json::json!({ "user_id": owner }),
            format!("DM to {}", user.name),
        ),
        AlertDelivery::Webhook { url } => {
            crate::web::api::endpoint_validation::validate_discord_webhook_url(url)?;
            ("Webhook", serde_json::json!({ "url": url }), format!("Webhook to {url}"))
        }
    };

    let alert = db.create_threshold_alert(
        owner, item_id, world_selector_json, price_threshold, hq_only, cooldown,
        notification_method, notification_config, &notification_name,
    ).await.map_err(ApiError::from)?;

    let endpoint_ids = db.list_endpoint_ids_for_alert(alert.id).await.map_err(ApiError::from)?;
    Ok(Json(Alert {
        id: alert.id,
        trigger: AlertTrigger::BelowThreshold { item_id, world_selector, price_threshold, hq_only },
        delivery,
        endpoint_ids,
        enabled: alert.enabled,
        cooldown_seconds: alert.cooldown_seconds,
        last_fired_at: alert.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
    }))
}
```

- [ ] **Step 10.2: Add `create_threshold_alert_without_endpoint` to ultros-db**

In `ultros-db/src/alerts.rs`:

```rust
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
```

- [ ] **Step 10.3: Update `list_alerts` to populate `endpoint_ids`**

Replace the body of `list_alerts` so the returned `Alert` includes `endpoint_ids`:

```rust
pub(crate) async fn list_alerts(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<Alert>>, ApiError> {
    let rows = db.get_user_threshold_alerts(user.id as i64).await.map_err(ApiError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for (a, t) in rows {
        let world_selector = serde_json::from_value(t.world_selector.clone())
            .map_err(|e| ApiError::from(anyhow::anyhow!("bad world_selector in db: {}", e)))?;
        let endpoint_ids = db.list_endpoint_ids_for_alert(a.id).await.map_err(ApiError::from)?;
        let delivery = match db.get_first_endpoint_for_alert(a.id).await.map_err(ApiError::from)? {
            Some(e) if e.method == "Webhook" => {
                let url = e.config.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                AlertDelivery::Webhook { url }
            }
            _ => AlertDelivery::DiscordDm,
        };
        out.push(Alert {
            id: a.id,
            trigger: AlertTrigger::BelowThreshold {
                item_id: t.item_id, world_selector, price_threshold: t.price_threshold, hq_only: t.hq_only,
            },
            delivery,
            endpoint_ids,
            enabled: a.enabled,
            cooldown_seconds: a.cooldown_seconds,
            last_fired_at: a.last_fired_at.map(|t| t.with_timezone(&chrono::Utc)),
        });
    }
    Ok(Json(out))
}
```

- [ ] **Step 10.4: check_ci + commit**

```bash
./check_ci.sh
git add ultros/src/web/api/alerts.rs ultros-db/src/alerts.rs
git commit -m "feat(api): create_alert honors endpoint_ids; list_alerts returns them"
```

---

## Task 11: Refactor `/alerts` route into three tabs

> Depends on Tasks 7, 8, 9, 10.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/alerts.rs`

- [ ] **Step 11.1: Replace the entire route body**

```rust
use leptos::prelude::*;

use crate::components::alert_rules_panel::AlertRulesPanel;
use crate::components::endpoints_panel::EndpointsPanel;
use crate::components::history_panel::HistoryPanel;

#[component]
pub fn Alerts() -> impl IntoView {
    let (tab, set_tab) = signal::<&'static str>("endpoints");

    let tab_btn = move |id: &'static str, label: &'static str| {
        view! {
            <button
                class=move || if tab.get() == id { "btn" } else { "btn-ghost" }
                on:click=move |_| set_tab.set(id)
            >
                {label}
            </button>
        }
    };

    view! {
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-bold">"Notifications"</h1>
            <div class="flex gap-2">
                {tab_btn("endpoints", "Endpoints")}
                {tab_btn("rules", "Alert rules")}
                {tab_btn("history", "History")}
            </div>
            <div>
                <Show when=move || tab.get() == "endpoints">
                    <EndpointsPanel />
                </Show>
                <Show when=move || tab.get() == "rules">
                    <AlertRulesPanel />
                </Show>
                <Show when=move || tab.get() == "history">
                    <HistoryPanel />
                </Show>
            </div>
        </div>
    }
}
```

The old imports (`get_alerts`, `patch_alert`, etc.) used directly in this file go away — they now live inside the panel components.

- [ ] **Step 11.2: Verify the build**

```bash
cargo leptos build
```

- [ ] **Step 11.3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/alerts.rs
git commit -m "feat(frontend): tabbed /alerts page (Endpoints | Rules | History)"
```

---

## Task 12: Migrate `alert_config_drawer` to endpoint picker

> Depends on Task 11.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/alert_config_drawer.rs`

- [ ] **Step 12.1: Replace the delivery radio + webhook URL field with an endpoint multi-select**

Rewrite the drawer to fetch endpoints via `list_endpoints()` and present checkboxes. If the user has no endpoints, show a link to `/alerts` (Endpoints tab) and disable the Create button.

```rust
use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use std::collections::HashSet;
use ultros_api_types::{
    alert::{AlertTrigger, CreateAlertRequest},
    world_helper::AnySelector,
};

use crate::api::{create_alert, list_endpoints};
use crate::components::{icon::Icon, modal::Modal, world_picker::WorldPicker};
use crate::global_state::toasts::use_toast;

#[component]
pub fn AlertConfigDrawer(
    item_id: i32,
    item_name: String,
    #[prop(into)] default_world: Signal<Option<AnySelector>>,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let (world, set_world) = signal::<Option<AnySelector>>(default_world.get_untracked());
    let (price_threshold, set_price_threshold) = signal::<String>("".to_string());
    let (hq_only, set_hq_only) = signal(false);
    let endpoints = Resource::new(|| (), |_| list_endpoints());
    let selected = RwSignal::new(HashSet::<i32>::new());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let toggle = move |id: i32| {
        selected.update(|s| { if !s.insert(id) { s.remove(&id); } });
    };

    let submit = move |_| {
        set_error.set(None);
        let Some(world_selector) = world.get() else {
            set_error.set(Some("Pick a world or DC".into())); return;
        };
        let Ok(threshold) = price_threshold.get().parse::<i32>() else {
            set_error.set(Some("Price threshold must be a positive integer".into())); return;
        };
        if threshold <= 0 {
            set_error.set(Some("Price threshold must be positive".into())); return;
        }
        let endpoint_ids: Vec<i32> = selected.get().into_iter().collect();
        if endpoint_ids.is_empty() {
            set_error.set(Some("Pick at least one endpoint".into())); return;
        }
        let req = CreateAlertRequest {
            trigger: AlertTrigger::BelowThreshold {
                item_id, world_selector, price_threshold: threshold, hq_only: hq_only.get(),
            },
            delivery: None,
            endpoint_ids,
            cooldown_seconds: None,
        };
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => { if let Some(t) = toasts { t.success("Alert created"); } set_visible.set(false); }
                Err(e) => set_error.set(Some(format!("{e}"))),
            }
        });
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem]">
                <h2 class="text-xl font-bold">"Create price alert: " {item_name.clone()}</h2>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"World / DC / Region"</label>
                    <WorldPicker current_world=world.into() set_current_world=set_world.into() />
                </div>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Price threshold (gil)"</label>
                    <input class="input w-full" type="number" min="1" placeholder="e.g. 150000"
                        prop:value=price_threshold
                        on:input=move |e| set_price_threshold.set(event_target_value(&e)) />
                </div>

                <label class="flex items-center gap-2">
                    <input type="checkbox" prop:checked=hq_only
                        on:change=move |e| set_hq_only.set(event_target_checked(&e)) />
                    <span class="text-sm">"HQ only"</span>
                </label>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Deliver to"</label>
                    <Suspense fallback=move || view! { <div class="text-sm opacity-70">"Loading endpoints..."</div> }>
                        {move || endpoints.get().map(|r| match r {
                            Ok(list) if list.is_empty() => view! {
                                <p class="text-sm opacity-70">
                                    "No endpoints yet. "
                                    <a href="/alerts" class="underline">"Add one"</a>
                                    " before creating alerts."
                                </p>
                            }.into_any(),
                            Ok(list) => view! {
                                <ul class="space-y-1">
                                    {list.into_iter().map(|e| {
                                        let id = e.id;
                                        let is_sel = move || selected.get().contains(&id);
                                        view! {
                                            <li>
                                                <label class="flex items-center gap-2">
                                                    <input type="checkbox" prop:checked=is_sel
                                                        on:change=move |_| toggle(id) />
                                                    <span>{e.name}</span>
                                                </label>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                            }.into_any(),
                            Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
                        })}
                    </Suspense>
                </div>

                <Show when=move || error.get().is_some()>
                    <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
                </Show>

                <div class="flex justify-end gap-2 pt-2">
                    <button class="btn-ghost" on:click=move |_| set_visible.set(false)>"Cancel"</button>
                    <button class="btn" on:click=submit>
                        <Icon icon=i::BsBell width="1em" height="1em" />
                        <span class="ml-1">"Create alert"</span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}
```

- [ ] **Step 12.2: Verify nothing else still uses `AlertDelivery` from the frontend**

```bash
rg "AlertDelivery" ultros-frontend/
```
Expected: no hits in `ultros-app/src`. If there are leftover references, replace them with endpoint-based equivalents.

- [ ] **Step 12.3: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/alert_config_drawer.rs
git commit -m "feat(frontend): alert drawer picks endpoints instead of inline delivery"
```

---

## Task 13: E2E smoke + final verification

**Files:**
- No code changes; verification only.

- [ ] **Step 13.1: Run check_ci**

```bash
./check_ci.sh
```

- [ ] **Step 13.2: Run the E2E harness**

```bash
LEPTOS_FEATURES=test-auth ./scripts/run_e2e.sh
```
Expected: pass. Note `/alerts` is in the curated route list; the harness will screenshot the tabbed UI.

- [ ] **Step 13.3: Manual smoke (if you have a local dev env)**

Boot the app, log in via test-auth, then walk through:
1. Visit `/alerts` → Endpoints tab shows empty state.
2. Click "Add endpoint" → create a "Discord DM (me)" endpoint named "Test DM" → list refreshes.
3. Click "Test" on the new endpoint → toast "Test delivered" or specific error.
4. Visit an item page → "Create price alert" drawer → endpoints checklist now shows "Test DM" → select + create.
5. Switch to "Alert rules" tab → see the alert with "Test DM" in the Endpoints column.
6. Switch to "History" tab → empty (no fires yet).

If any of those steps fail, the failure is the regression; fix-then-recommit.

- [ ] **Step 13.4: No commit required for this task.** The plan is complete.

---

## Parallel waves (suggested coordination)

| Wave | Tasks | Rationale |
|---|---|---|
| 1 | Task 1 | API types must land first; everyone depends on them. Single worktree. |
| 2 (parallel) | Task 2 ‖ Task 3 ‖ Task 6 ‖ Task 7 ‖ Task 9 | DB layer, validation refactor, frontend client stubs, EndpointsPanel, HistoryPanel — none of them touch each other. Five worktrees. |
| 3 (parallel) | Task 4 ‖ Task 5 ‖ Task 8 | Endpoint handlers + delivery helper + AlertRulesPanel. Task 4 and Task 5 share `ultros/src/web.rs` so coordinate the route-registration block, but the handler bodies are independent. AlertRulesPanel uses Task 1's `Alert.endpoint_ids` and is otherwise free. |
| 4 | Task 10 | Touches both `web/api/alerts.rs` and `ultros-db/src/alerts.rs` — must merge after Wave 3. |
| 5 (parallel) | Task 11 ‖ Task 12 | Tabbed route and drawer are independent. |
| 6 | Task 13 | E2E smoke. Single agent. |

Conflict-prone files where you'll need to rebase/merge carefully:
- `ultros/src/web.rs` (route table) — Tasks 4, 5
- `ultros/src/web/api/alerts.rs` — Tasks 3, 5, 10
- `ultros-frontend/ultros-app/src/components/mod.rs` — Tasks 7, 8, 9

For those, prefer landing waves serially rather than splitting across worktrees, OR ensure each agent does a `git pull --rebase` before its final commit.
