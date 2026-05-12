# Notification Tier 4 — Shared list notifications — Implementation Plan

**Goal:** Let users subscribe to price alerts on a shopping list. When any item in the list (including shared lists they have view permission on) drops to or below its per-item target price, fire the alert through the subscriber's chosen endpoints.

**Scope:** Tier 4 from the spec, **list-scoped price alerts only**. Excludes list-change events (item added/removed/member added) — defer to a follow-up plan if requested. The spec called those "lower priority" and they need a separate `list_event` table plus mutation-side wiring.

**Architecture:** Reuse the alert framework — new `AlertTrigger::ListItemThreshold { list_id }` variant. The trigger fans out to the subscriber's chosen endpoints just like a `BelowThreshold` alert. A new `alert_list_threshold` junction joins the alert row to the list. `list_item` gains a `target_price` column so the threshold per item can be set inline on the list itself. Permission gate at alert-creation time: must satisfy `can_view_list(user_id, list_id) >= View`.

**Tech Stack:** Sea-ORM, Axum, Leptos. No new external deps.

---

## File map

| File | Action |
|---|---|
| `migration/src/m20260513_000001_list_item_target_price.rs` | Create — add `list_item.target_price BIGINT NULL` |
| `migration/src/m20260513_000002_alert_list_threshold.rs` | Create — new table |
| `migration/src/lib.rs` | Modify — register both |
| `ultros-db/src/entity/list_item.rs` | Modify — add `target_price: Option<i64>` |
| `ultros-db/src/entity/alert_list_threshold.rs` | Create — entity |
| `ultros-db/src/entity/{mod,prelude}.rs` | Modify — wire new entity |
| `ultros-db/src/alerts.rs` | Modify — `create_list_threshold_alert`, `get_user_list_threshold_alerts`, `get_all_active_list_threshold_alerts` |
| `ultros-db/src/lists.rs` | Modify — extend `list_item` update DTO + `set_list_item_target_price(owner, list_item_id, target_price)` with permission check |
| `ultros-api-types/src/alert.rs` | Modify — add `AlertTrigger::ListItemThreshold { list_id: i32 }` variant; serde tag stays `type=list_item_threshold` |
| `ultros-api-types/src/list.rs` | Modify — add `target_price: Option<i64>` to `ListItem` |
| `ultros/src/web/api/alerts.rs` | Modify — `create_alert` accepts new trigger; `list_alerts` returns new trigger |
| `ultros/src/web/api/lists.rs` (or wherever list mutations live) | Modify — accept `target_price` in update payloads |
| `ultros/src/alerts/price_alert_tracker.rs` | Modify — track list-threshold alerts in parallel to item alerts; on each listing, look up matching `list_item`s and fire |
| `ultros/src/discord/ffxiv/alert.rs` | Optional — add `/ffxiv alert list <list_name>` to subscribe to a list. **Skip for v1** unless trivial. |
| `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs` | Modify — render the new trigger type's row |
| `ultros-frontend/ultros-app/src/routes/list.rs` (or wherever shopping lists render) | Modify — per-row "target price" input, "Subscribe to alerts on this list" button |

---

## Task 1: Migrations

**Files:**
- `migration/src/m20260513_000001_list_item_target_price.rs`
- `migration/src/m20260513_000002_alert_list_threshold.rs`

### m20260513_000001

```rust
ALTER TABLE list_item ADD COLUMN target_price BIGINT NULL;
```
Down: drop the column.

### m20260513_000002

```rust
CREATE TABLE alert_list_threshold (
  id SERIAL PRIMARY KEY,
  alert_id INTEGER NOT NULL REFERENCES alert(id) ON DELETE CASCADE,
  list_id  INTEGER NOT NULL REFERENCES "list"(id) ON DELETE CASCADE,
  UNIQUE (alert_id)  -- one list-threshold per alert; alerts subscribe to exactly one list
);
CREATE INDEX idx_alert_list_threshold_list_id ON alert_list_threshold(list_id);
```
Down: drop the table.

Pattern off the existing alert_item_threshold migration shape (look at `migration/src/m20221227_164853_price_alert.rs` and the notification endpoints migration for cascade syntax).

- [ ] Register both in `migration/src/lib.rs` in date order.
- [ ] `cargo check -p migration` passes.
- [ ] Commit each migration separately: `feat(migration): list_item.target_price` and `feat(migration): alert_list_threshold table`.

---

## Task 2: Sea-ORM entities

**Files:** `ultros-db/src/entity/list_item.rs` (modify), `ultros-db/src/entity/alert_list_threshold.rs` (create).

- [ ] `list_item.rs`: add `pub target_price: Option<i64>` to the Model.
- [ ] `alert_list_threshold.rs`: new entity with `id, alert_id, list_id`. Pattern off `alert_item_threshold.rs`. Set up `belongs_to` relations for both Alert and List.
- [ ] Wire `mod.rs` and `prelude.rs`.
- [ ] `cargo check -p ultros-db` passes.
- [ ] Commit: `feat(ultros-db): list_item.target_price + alert_list_threshold entity`

---

## Task 3: DB methods

**File:** `ultros-db/src/alerts.rs`

Add:

```rust
/// Create an alert + alert_list_threshold + alert_notification_rules linking to `endpoint_ids`.
/// Caller MUST check `can_view_list(owner, list_id)` before calling.
pub async fn create_list_threshold_alert(
    &self,
    owner: i64,
    list_id: i32,
    cooldown_seconds: i32,
    endpoint_ids: &[i32],
) -> Result<alert::Model> { ... }

/// Return (alert, alert_list_threshold) rows owned by `owner`.
pub async fn get_user_list_threshold_alerts(
    &self,
    owner: i64,
) -> Result<Vec<(alert::Model, alert_list_threshold::Model)>> { ... }

/// Return all active (enabled=true) list-threshold alerts for the price tracker.
pub async fn get_all_active_list_threshold_alerts(
    &self,
) -> Result<Vec<(alert::Model, alert_list_threshold::Model)>> { ... }
```

**File:** `ultros-db/src/lists.rs`

Add:

```rust
/// Set the target_price on a list_item. Requires Edit permission on the list.
pub async fn set_list_item_target_price(
    &self,
    owner: i64,
    list_item_id: i32,
    target_price: Option<i64>,
) -> Result<()> {
    let item = list_item::Entity::find_by_id(list_item_id).one(&self.db).await?
        .ok_or_else(|| anyhow::Error::msg("list_item not found"))?;
    self.require_list_permission(item.list_id, owner, ListPermission::Edit).await?;
    let mut active: list_item::ActiveModel = item.into();
    active.target_price = Set(target_price);
    active.update(&self.db).await?;
    Ok(())
}

/// Return all list_items in `list_id` that have a non-null target_price AND whose item_id matches
/// the supplied id. Used by the price tracker to check whether a listing meets a list's target.
pub async fn get_list_items_with_target(
    &self,
    list_id: i32,
    item_id: i32,
) -> Result<Vec<list_item::Model>> { ... }
```

The exact permission helper name (`require_list_permission`) — look at the existing patterns in `ultros-db/src/lists.rs` and use whatever's there. The codebase already has a permission model from the shared-lists PR.

- [ ] Commit: `feat(ultros-db): list-threshold alert + list_item target_price methods`

---

## Task 4: API types

**File:** `ultros-api-types/src/alert.rs`

Extend `AlertTrigger`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertTrigger {
    BelowThreshold { ... },           // existing
    ListItemThreshold { list_id: i32 }, // new
}
```

**Important**: every `match` site on `AlertTrigger` in the codebase becomes non-exhaustive after this change. Fix them all:
- `ultros/src/web/api/alerts.rs` — `create_alert` destructures; needs a branch for the new variant
- `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs` — renders the trigger; needs a branch
- `ultros-frontend/ultros-app/src/components/alert_config_drawer.rs` — only creates BelowThreshold; safe but verify it doesn't pattern-match exhaustively
- Anywhere else `AlertTrigger` is matched (search `match.*trigger\|AlertTrigger::`)

**File:** `ultros-api-types/src/list.rs` (or wherever `ListItem` is defined)

Add `pub target_price: Option<i64>` to `ListItem`. Updates that don't include `target_price` should preserve the existing value — use `#[serde(default, skip_serializing_if = "Option::is_none")]` if there's an existing pattern for this on the struct.

- [ ] Update tests/snapshots if any.
- [ ] `cargo check -p ultros-api-types`.
- [ ] Commit: `feat(api-types): AlertTrigger::ListItemThreshold + ListItem.target_price`

---

## Task 5: API handlers

**File:** `ultros/src/web/api/alerts.rs`

Extend `create_alert`'s trigger destructure to handle `ListItemThreshold`:

```rust
match req.trigger {
    AlertTrigger::BelowThreshold { ... } => {
        // existing path
    }
    AlertTrigger::ListItemThreshold { list_id } => {
        // 1. Permission check: db.require_list_permission(list_id, owner, View) (or whatever the helper is)
        // 2. If req.endpoint_ids non-empty, verify each is owned by the caller.
        // 3. Call db.create_list_threshold_alert(owner, list_id, cooldown, &endpoint_ids).
        // 4. Return Alert { trigger: ListItemThreshold { list_id }, endpoint_ids, ..., delivery: DiscordDm /* deprecated */ }.
        // If endpoint_ids is empty, error out — the legacy `delivery` path doesn't apply to list-scoped alerts.
    }
}
```

`list_alerts` needs to fetch both `get_user_threshold_alerts` and `get_user_list_threshold_alerts` and concatenate them in the response. `endpoint_ids` are populated via `list_endpoint_ids_for_alert` as before.

**File:** `ultros/src/web/api/lists.rs` (or wherever list_item updates live)

- [ ] Accept `target_price` in the list_item update payload.
- [ ] Wire to `db.set_list_item_target_price` (which enforces Edit permission).
- [ ] `./check_ci.sh` passes.
- [ ] Commit: `feat(api): list-threshold alerts + list_item.target_price`

---

## Task 6: Price tracker — list-threshold dispatch

**File:** `ultros/src/alerts/price_alert_tracker.rs`

This is the runtime piece. Today the tracker indexes `by_item: HashMap<i32, Vec<ActiveRule>>`. Add a parallel `list_rules: Vec<ListActiveRule>`:

```rust
struct ListActiveRule {
    alert_id: i32,
    list_id: i32,
    cooldown_seconds: i32,
    last_fired_at: Option<DateTime<Utc>>,
}
```

On `refresh_from`, load list-threshold alerts via `get_all_active_list_threshold_alerts` and populate `list_rules`.

On each incoming listing:
1. For each list_rule, call `db.get_list_items_with_target(list_rule.list_id, listing.item_id)`. (This is a DB query per (rule × incoming listing matching that item). Bound the cost: cache per-tick, or build a (list_id, item_id) → target_price map on `refresh_from` instead of querying live.) **Pre-compute on refresh** is the cleaner approach — build a `by_item_list_target: HashMap<i32, Vec<(alert_id, list_id, target_price, hq?)>>` keyed by item_id during `refresh_from`. Then incoming listing lookup is O(1).
2. If `listing.price <= target_price` (and optionally HQ matches), fire the alert with title "List `{list_name}`: `{item_name}` at {price} gil".

For v1, treat `hq` as "any" — list_items already have an optional hq flag, but we can ignore it for the trigger to keep behavior simple. Document this.

The cooldown logic mirrors the existing BelowThreshold path.

Title formatting: look up the list name + item name. Item name from `xiv_gen_db::data().items`. List name: add a `list::Model` lookup, or include the list name in the pre-computed map.

- [ ] Verify the tracker's `refresh_from` runs at the right cadence — same as today.
- [ ] `./check_ci.sh` passes.
- [ ] Commit: `feat(alerts): dispatch list-threshold alerts`

---

## Task 7: Frontend — list page integration

**File:** `ultros-frontend/ultros-app/src/routes/list.rs` (or wherever the shopping list view is)

- [ ] Add a numeric input column "Target price" to the list_item rows. On change, PATCH the list_item with the new target_price (debounced or onBlur).
- [ ] Add a "Notify me on this list" button at the top of the list view. On click, opens a modal that fetches `list_endpoints()`, lets the user pick endpoints, and POSTs `create_alert` with `AlertTrigger::ListItemThreshold { list_id }` + `endpoint_ids`. On success, toast "Subscribed".

- [ ] Commit: `feat(frontend): list page target_price input + subscribe button`

---

## Task 8: Frontend — alert rules panel

**File:** `ultros-frontend/ultros-app/src/components/alert_rules_panel.rs`

The existing panel matches on `AlertTrigger::BelowThreshold { .. }`. Add a branch for `ListItemThreshold { list_id }`:

- Render a row like: "List `#{list_id}`" (or fetch the list name; if list-name lookup is expensive, just show the id with a link to `/list/{id}`).
- Same toggle/delete actions.

- [ ] `cargo check -p ultros-app` passes.
- [ ] Commit: `feat(frontend): render list-threshold alerts in rules panel`

---

## Task 9: Final CI + smoke

- [ ] `./check_ci.sh` passes.
- [ ] Migrations apply cleanly on a fresh DB.

---

## Out of scope (deferred)

- **List-change events** (item added/removed/member added). The spec called these lower priority. They need a new `list_event` table and mutation-side `INSERT` calls, plus `AlertTrigger::ListChange { list_id, events }`. Separate PR.
- **Bot command for subscribing to a list.** `/ffxiv alert list <name>` could be useful but adds autocomplete complexity. Web-only for v1.
- **Per-item HQ filter** on list-threshold alerts. Today list_item.hq exists but is ignored by the trigger; treating it as "any" is the simpler v1 behavior.

## Risks

- **Permission drift.** A user with a now-revoked share could have an active list-threshold alert. At dispatch time, the tracker doesn't re-check permission — it just sees an enabled alert pointing at a list. Mitigation: when a share is revoked, also disable any list-threshold alerts owned by users who no longer have View. This is a cleanup task for the revoke path — punt if not trivial; the alert will deliver a few times before someone notices, which is annoying but not a leak (the list owner sees nothing they don't already own).
- **Tracker memory.** Pre-computing `by_item_list_target` means holding `(item_id, list_id, target_price)` tuples in memory. With N users × M lists × K items per list this could grow. v1: don't worry; revisit if it becomes a problem.
- **HashMap rebuild cost on every refresh.** Same as today's item-threshold logic; acceptable.
