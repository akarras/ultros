# Price Alerts — Phase 2 & 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship webhook delivery (Phase 2 backend) + the full frontend surface (Phase 3): per-item bell icon → alert config drawer, top-nav `/alerts` page listing active rules and recent fires, and toast-based success/error feedback. Combined into one PR.

**Architecture:** Phase 2 extends the existing `EndpointConfig` enum in `delivery.rs` with a `Webhook { url }` variant + a Discord-webhook-shaped HTTP POST sender. Phase 3 builds Leptos components consuming the existing `/api/v1/alerts` API surface via free `async fn` calls in `api.rs`, following the same `Action::new` + `Resource::new` pattern as Lists.

**Tech Stack:** Rust (backend: Axum + reqwest for webhook POST); Leptos 0.8 (frontend: `<Modal>`, `<WorldPicker>`, `<Select>`, `Action` + `Resource`).

**Out of scope:**
- AI suggestion endpoints (Phase 4 — separate plan)
- Per-list "alert defaults" UI (defer — composes cleanly with per-item, no schema change)
- Top-bar badge for unread fires (defer — polish)
- "% drop from median" trigger (defer — requires median tracker, separate plan)
- Live state refresh in `PriceAlertTracker` (Phase 1 known gap, separate effort)

**Spec:** [2026-05-10-price-alerts-design.md](../specs/2026-05-10-price-alerts-design.md)
**Prior plan:** [2026-05-10-price-alerts-phase-1.md](2026-05-10-price-alerts-phase-1.md) (merged via PR #586)

---

## File Structure

**Phase 2 (backend) — modifies existing:**
- `ultros-api-types/src/alert.rs` — add `Webhook { url }` variant to `AlertDelivery`
- `ultros/src/alerts/delivery.rs` — add `Webhook { url }` to `EndpointConfig`, new `send_webhook` helper
- `ultros/src/web/api/alerts.rs` — handle `AlertDelivery::Webhook` in `create_alert`, validate URL
- `ultros/Cargo.toml` — confirm `reqwest` already present (it should be; check)

**Phase 3 (frontend) — modifies existing + creates new:**
- `ultros-frontend/ultros-app/src/api.rs` — add `get_alerts`, `create_alert`, `patch_alert`, `delete_alert`, `get_alert_events`, plus a `patch_api` helper
- `ultros-frontend/ultros-app/src/lib.rs` — register `/alerts` route + import `Alerts` component
- `ultros-frontend/ultros-app/src/components/apps_menu.rs` — add Alerts link in `UserMenu` (auth-gated block)
- `ultros-frontend/ultros-app/src/routes/alerts.rs` — replace stub with the Alerts page (rule list + events)
- `ultros-frontend/ultros-app/src/components/alert_config_drawer.rs` — NEW: drawer to create an alert from a list item
- `ultros-frontend/ultros-app/src/components/list/list_item_row.rs` — add bell icon button that opens the drawer
- `ultros-frontend/ultros-app/src/components/mod.rs` — declare new module

---

## Task 1: Add Webhook variant to AlertDelivery API type

**File:** `ultros-api-types/src/alert.rs`

- [ ] **Step 1: Add the variant**

Modify `AlertDelivery`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AlertDelivery {
    /// Send a Discord DM to the user. The user_id is derived from the auth session, not the request body.
    DiscordDm,
    /// POST a Discord-shaped embed to a user-provided webhook URL (typically a Discord channel webhook).
    Webhook { url: String },
}
```

- [ ] **Step 2: Verify build**

```bash
cargo check -p ultros-api-types
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add ultros-api-types/src/alert.rs
git commit -m "Add Webhook variant to AlertDelivery API type"
```

---

## Task 2: Add webhook delivery to dispatcher

**File:** `ultros/src/alerts/delivery.rs`

- [ ] **Step 1: Add the variant to `EndpointConfig`**

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "method")]
pub(crate) enum EndpointConfig {
    #[serde(rename = "DiscordChannel")]
    DiscordChannel { channel_id: i64 },
    #[serde(rename = "DiscordDm")]
    DiscordDm { user_id: i64 },
    #[serde(rename = "Webhook")]
    Webhook { url: String },
}
```

- [ ] **Step 2: Add `send_webhook` helper**

Add the function at the bottom of `delivery.rs`:

```rust
async fn send_webhook(url: &str, title: &str, body: &str) -> Result<()> {
    // Discord webhook expects a JSON body with `embeds`.
    let payload = serde_json::json!({
        "embeds": [{
            "title": title,
            "description": body,
            "color": 0x00c850, // green, matches the Discord channel/DM color
        }],
        "allowed_mentions": { "parse": [] },
    });
    let resp = reqwest::Client::new()
        .post(url)
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("webhook returned {status}: {body}"));
    }
    Ok(())
}
```

- [ ] **Step 3: Wire the variant into `dispatch_alert`**

In the `match parsed` block inside `dispatch_alert`:

```rust
let result = match parsed {
    EndpointConfig::DiscordChannel { channel_id } => {
        send_to_channel(channel_id, title, body, ctx).await
    }
    EndpointConfig::DiscordDm { user_id } => send_dm(user_id, title, body, ctx).await,
    EndpointConfig::Webhook { url } => send_webhook(&url, title, body).await,
};
```

- [ ] **Step 4: Verify `reqwest` is a dep of `ultros`**

```bash
grep "^reqwest" ultros/Cargo.toml
```

If not, add it: `reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }` — note `rustls-tls` not `native-tls`, to avoid pulling openssl back in.

- [ ] **Step 5: Verify build**

```bash
cargo check -p ultros
```

- [ ] **Step 6: Commit**

```bash
git add ultros/src/alerts/delivery.rs ultros/Cargo.toml
git commit -m "Add Discord-shaped webhook delivery to alert dispatcher"
```

---

## Task 3: Update create_alert handler for webhook delivery

**File:** `ultros/src/web/api/alerts.rs`

- [ ] **Step 1: Replace the delivery-method match with webhook support**

In `create_alert`, find the existing `match req.delivery` block:

```rust
let (notification_method, notification_config, notification_name) = match req.delivery {
    AlertDelivery::DiscordDm => (
        "DiscordDm",
        serde_json::json!({ "user_id": owner }),
        format!("DM to {}", user.name),
    ),
};
```

Replace with:

```rust
let (notification_method, notification_config, notification_name) = match &req.delivery {
    AlertDelivery::DiscordDm => (
        "DiscordDm".to_string(),
        serde_json::json!({ "user_id": owner }),
        format!("DM to {}", user.name),
    ),
    AlertDelivery::Webhook { url } => {
        validate_discord_webhook_url(url)?;
        (
            "Webhook".to_string(),
            serde_json::json!({ "url": url }),
            format!("Webhook ({})", host_of_url(url)),
        )
    }
};
```

Update the `db.create_threshold_alert(...)` call site so the `notification_method` arg is `&notification_method` (since it's now `String`, not `&'static str`).

Update the response-building section to echo back the same `delivery`:

```rust
delivery: req.delivery,
```

(previously hardcoded `AlertDelivery::DiscordDm` — now uses what the user requested)

- [ ] **Step 2: Add validation helpers**

Append to the same file:

```rust
fn validate_discord_webhook_url(url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url).map_err(|e| {
        ApiError::from(anyhow::anyhow!("invalid webhook URL: {e}"))
    })?;
    if parsed.scheme() != "https" {
        return Err(ApiError::from(anyhow::anyhow!(
            "webhook URL must use https"
        )));
    }
    let host = parsed.host_str().unwrap_or("");
    // Discord webhook hosts. Accept the canonical + ptb/canary subdomains.
    let allowed = ["discord.com", "discordapp.com", "ptb.discord.com", "canary.discord.com"];
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

fn host_of_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}
```

The `url` crate should already be a transitive dep. If not, add `url = "2.5"` to `ultros/Cargo.toml`.

- [ ] **Step 3: Update `list_alerts` to return the actual delivery shape**

In `list_alerts`, the current code hardcodes `delivery: AlertDelivery::DiscordDm`. To return the actual delivery, the handler needs to look up the `notification_endpoint` joined via `alert_notification_rule`. Add a helper in `ultros-db/src/alerts.rs` (small extension):

```rust
// In ultros-db/src/alerts.rs:
pub async fn get_first_endpoint_for_alert(
    &self,
    alert_id: i32,
) -> Result<Option<notification_endpoint::Model>> {
    let rules = alert_notification_rule::Entity::find()
        .filter(alert_notification_rule::Column::AlertId.eq(alert_id))
        .all(&self.db)
        .await?;
    if let Some(rule) = rules.first() {
        Ok(notification_endpoint::Entity::find_by_id(rule.endpoint_id)
            .one(&self.db)
            .await?)
    } else {
        Ok(None)
    }
}
```

In `list_alerts`, change the loop to:

```rust
for (a, t) in rows {
    let world_selector = serde_json::from_value(t.world_selector.clone())
        .map_err(|e| ApiError::from(anyhow::anyhow!("bad world_selector in db: {}", e)))?;
    let delivery = match db.get_first_endpoint_for_alert(a.id).await
        .map_err(ApiError::from)? {
        Some(e) if e.method == "Webhook" => {
            let url = e.config.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            AlertDelivery::Webhook { url }
        }
        _ => AlertDelivery::DiscordDm,
    };
    out.push(Alert { /* …fields…, delivery */ });
}
```

- [ ] **Step 4: Verify build**

```bash
cargo check -p ultros -p ultros-db
```

- [ ] **Step 5: Commit**

```bash
git add ultros/src/web/api/alerts.rs ultros-db/src/alerts.rs
git commit -m "Wire webhook delivery method through HTTP API"
```

---

## Task 4: Frontend API functions

**File:** `ultros-frontend/ultros-app/src/api.rs`

- [ ] **Step 1: Add a `patch_api` helper**

Look at the existing `post_api` and `delete_api` functions. Add a `patch_api` mirroring `post_api` but using `gloo_net::http::Request::patch(...)` (or equivalent — check what the SSR side uses).

For SSR (`#[cfg(feature = "ssr")]` path): use `reqwest::Client::patch(...)`.
For WASM: use `gloo_net::http::Request::patch(...)`.

- [ ] **Step 2: Add alert API functions**

Add these alongside the existing list functions:

```rust
use ultros_api_types::alert::{Alert, AlertEvent, CreateAlertRequest, UpdateAlertRequest};

pub async fn get_alerts() -> AppResult<Vec<Alert>> {
    fetch_api("/api/v1/alerts").await
}

pub async fn create_alert(req: CreateAlertRequest) -> AppResult<Alert> {
    post_api("/api/v1/alerts", &req).await
}

pub async fn patch_alert(id: i32, req: UpdateAlertRequest) -> AppResult<()> {
    patch_api(&format!("/api/v1/alerts/{id}"), &req).await
}

pub async fn delete_alert(id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/alerts/{id}")).await
}

pub async fn get_alert_events() -> AppResult<Vec<AlertEvent>> {
    fetch_api("/api/v1/alerts/events").await
}
```

- [ ] **Step 3: Verify**

```bash
cargo check -p ultros-app
```

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/api.rs
git commit -m "Add alert API client functions to ultros-app"
```

---

## Task 5: Register Alerts route + nav link

**Files:** `ultros-frontend/ultros-app/src/lib.rs`, `ultros-frontend/ultros-app/src/components/apps_menu.rs`

- [ ] **Step 1: Add import in `lib.rs`**

Find the `use crate::routes::{...}` block (around lines 29–51) and add `alerts::Alerts` to the list (alphabetical).

- [ ] **Step 2: Register the route in `<Routes>` block**

In `AppInner`'s `<Routes>` block (around line 317), add alongside the list route:

```rust
<Route path=path!("alerts") view=Alerts />
```

- [ ] **Step 3: Add nav entry in `UserMenu`**

In `components/apps_menu.rs`, find the `UserMenu` auth-gated block (around line 282 where the Lists link is). Add directly after:

```rust
<A href="/alerts" attr:class="nav-link w-full justify-start" on:click=close_menu>
    <Icon height="1.1em" width="1.1em" icon=i::BsBell />
    <span class="ml-2">"Alerts"</span>
</A>
```

(No i18n yet — Phase 1/2/3 stays English-only for new strings; i18n sweep is a separate effort.)

- [ ] **Step 4: Verify build**

```bash
cargo check -p ultros-app
```

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/lib.rs ultros-frontend/ultros-app/src/components/apps_menu.rs
git commit -m "Register /alerts route and UserMenu nav link"
```

---

## Task 6: AlertConfigDrawer component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/alert_config_drawer.rs`
- Modify: `ultros-frontend/ultros-app/src/components/mod.rs`

The drawer opens when the user clicks the bell on a list item row. Pre-fills `item_id` from the row. User picks world/DC, sets price threshold, optionally checks HQ-only, picks delivery (DM | Webhook), and (if webhook) pastes URL.

- [ ] **Step 1: Create the component**

```rust
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use icondata as i;
use ultros_api_types::{
    alert::{AlertDelivery, AlertTrigger, CreateAlertRequest},
    world_helper::AnySelector,
};

use crate::api::create_alert;
use crate::components::{modal::Modal, world_picker::WorldPicker, icon::Icon};
use crate::global_state::toasts::use_toast;

#[component]
pub fn AlertConfigDrawer(
    item_id: i32,
    item_name: String,
    /// Default world selector for the form (e.g., from the user's home world). If None, user must pick.
    #[prop(into)] default_world: Signal<Option<AnySelector>>,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let (world, set_world) = signal::<Option<AnySelector>>(default_world.get_untracked());
    let (price_threshold, set_price_threshold) = signal::<String>("".to_string());
    let (hq_only, set_hq_only) = signal(false);
    let (delivery_kind, set_delivery_kind) = signal::<&'static str>("discord_dm");
    let (webhook_url, set_webhook_url) = signal::<String>("".to_string());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let submit = move |_| {
        set_error.set(None);
        let Some(world_selector) = world.get() else {
            set_error.set(Some("Pick a world or DC".into()));
            return;
        };
        let Ok(price_threshold) = price_threshold.get().parse::<i32>() else {
            set_error.set(Some("Price threshold must be a positive integer".into()));
            return;
        };
        if price_threshold <= 0 {
            set_error.set(Some("Price threshold must be positive".into()));
            return;
        }
        let delivery = match delivery_kind.get() {
            "webhook" => {
                let url = webhook_url.get();
                if url.trim().is_empty() {
                    set_error.set(Some("Webhook URL required".into()));
                    return;
                }
                AlertDelivery::Webhook { url }
            }
            _ => AlertDelivery::DiscordDm,
        };
        let req = CreateAlertRequest {
            trigger: AlertTrigger::BelowThreshold {
                item_id,
                world_selector,
                price_threshold,
                hq_only: hq_only.get(),
            },
            delivery,
            cooldown_seconds: None,
        };
        let toasts = toasts.clone();
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => {
                    if let Some(t) = toasts { t.success("Alert created"); }
                    set_visible.set(false);
                }
                Err(e) => {
                    set_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem]">
                <h2 class="text-xl font-bold">"Create price alert: "{item_name.clone()}</h2>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"World / DC / Region"</label>
                    <WorldPicker
                        current_world=world.into()
                        set_current_world=set_world.into()
                    />
                </div>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Price threshold (gil)"</label>
                    <input
                        class="input w-full"
                        type="number"
                        min="1"
                        placeholder="e.g. 150000"
                        prop:value=price_threshold
                        on:input=move |e| set_price_threshold.set(event_target_value(&e))
                    />
                </div>

                <label class="flex items-center gap-2">
                    <input
                        type="checkbox"
                        prop:checked=hq_only
                        on:change=move |e| set_hq_only.set(event_target_checked(&e))
                    />
                    <span class="text-sm">"HQ only"</span>
                </label>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">"Delivery"</label>
                    <div class="flex gap-3">
                        <label class="flex items-center gap-1">
                            <input
                                type="radio"
                                name="delivery"
                                prop:checked=move || delivery_kind.get() == "discord_dm"
                                on:change=move |_| set_delivery_kind.set("discord_dm")
                            />
                            "Discord DM"
                        </label>
                        <label class="flex items-center gap-1">
                            <input
                                type="radio"
                                name="delivery"
                                prop:checked=move || delivery_kind.get() == "webhook"
                                on:change=move |_| set_delivery_kind.set("webhook")
                            />
                            "Webhook"
                        </label>
                    </div>
                </div>

                <Show when=move || delivery_kind.get() == "webhook">
                    <div class="space-y-1">
                        <label class="text-sm font-semibold">"Discord webhook URL"</label>
                        <input
                            class="input w-full"
                            type="url"
                            placeholder="https://discord.com/api/webhooks/..."
                            prop:value=webhook_url
                            on:input=move |e| set_webhook_url.set(event_target_value(&e))
                        />
                        <p class="text-xs opacity-70">
                            "Get a webhook URL from a channel's Integrations settings in Discord."
                        </p>
                    </div>
                </Show>

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

- [ ] **Step 2: Declare the module**

In `ultros-frontend/ultros-app/src/components/mod.rs`, add (alphabetical):

```rust
pub mod alert_config_drawer;
```

- [ ] **Step 3: Verify build**

```bash
cargo check -p ultros-app
```

If the path for `Modal`, `WorldPicker`, `Icon`, or `use_toast` differs, adjust imports.

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/alert_config_drawer.rs ultros-frontend/ultros-app/src/components/mod.rs
git commit -m "Add AlertConfigDrawer component for creating price alerts"
```

---

## Task 7: Add bell icon to ListItemRow

**File:** `ultros-frontend/ultros-app/src/components/list/list_item_row.rs`

- [ ] **Step 1: Add imports + state**

Near the top of the function body of `ListItemRow`, after the existing signals:

```rust
let (alert_drawer_open, set_alert_drawer_open) = signal(false);
```

- [ ] **Step 2: Add the bell button to the actions cell**

In the `<td>` that contains the existing delete/edit/check buttons (around line 130 — view mode block), add a new `Tooltip + button + Icon` before the trash button:

```rust
<Tooltip tooltip_text="Create price alert">
    <button
        class="btn"
        aria-label="Create price alert"
        on:click=move |_| set_alert_drawer_open.set(true)
    >
        <Icon icon=i::BsBell />
    </button>
</Tooltip>
```

Repeat for the edit-mode block (around line 261) so the bell is available in both modes.

- [ ] **Step 3: Render the drawer conditionally at the row level**

At the bottom of the row's view (or just inside the outermost wrapper), add:

```rust
<Show when=alert_drawer_open>
    <AlertConfigDrawer
        item_id=item.with(|i| i.item_id)
        item_name=xiv_gen_db::data()
            .items
            .get(&xiv_gen::ItemId(item.with(|i| i.item_id)))
            .map(|i| i.name.as_str().to_string())
            .unwrap_or_default()
        default_world=Signal::derive(|| None)
        set_visible=set_alert_drawer_open.into()
    />
</Show>
```

(If the row receives a world signal as a prop, use that for `default_world` instead of `Signal::derive(|| None)`.)

- [ ] **Step 4: Import the drawer**

At the top of the file:

```rust
use crate::components::alert_config_drawer::AlertConfigDrawer;
```

- [ ] **Step 5: Verify build**

```bash
cargo check -p ultros-app
```

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/list/list_item_row.rs
git commit -m "Add bell icon + AlertConfigDrawer to ListItemRow"
```

---

## Task 8: Alerts page (rule list + recent events)

**File:** `ultros-frontend/ultros-app/src/routes/alerts.rs`

The existing stub needs to be replaced with the full page. Two sections in a single component:
1. **Active rules** — table of user's alerts with disable/enable + delete buttons
2. **Recent events** — table of the last 50 fires (item name, time, price, delivered status)

- [ ] **Step 1: Replace the stub**

```rust
use leptos::{prelude::*, task::spawn_local};
use icondata as i;
use ultros_api_types::alert::{Alert, AlertDelivery, AlertEvent, AlertTrigger, UpdateAlertRequest};

use crate::api::{delete_alert, get_alert_events, get_alerts, patch_alert};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;

#[component]
pub fn Alerts() -> impl IntoView {
    let action_version = RwSignal::new(0u64);
    let alerts = Resource::new(
        move || action_version.get(),
        move |_| get_alerts(),
    );
    let events = Resource::new(
        move || action_version.get(),
        move |_| get_alert_events(),
    );
    let toasts = use_toast();

    let toggle = move |alert: Alert| {
        let new_enabled = !alert.enabled;
        let toasts = toasts.clone();
        spawn_local(async move {
            match patch_alert(alert.id, UpdateAlertRequest {
                enabled: Some(new_enabled),
                price_threshold: None,
            }).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(if new_enabled { "Alert enabled" } else { "Alert disabled" });
                    }
                    action_version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts { t.error(format!("{e}")); }
                }
            }
        });
    };

    let remove = move |id: i32| {
        let toasts = toasts.clone();
        spawn_local(async move {
            match delete_alert(id).await {
                Ok(()) => {
                    if let Some(t) = toasts { t.success("Alert deleted"); }
                    action_version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts { t.error(format!("{e}")); }
                }
            }
        });
    };

    view! {
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-bold">"Price alerts"</h1>

            <section>
                <h2 class="text-lg font-semibold mb-2">"Active rules"</h2>
                <Suspense fallback=move || view! { <div>"Loading..."</div> }>
                    {move || alerts.get().map(|r| match r {
                        Ok(rows) if rows.is_empty() => view! {
                            <p class="opacity-70">"No alerts yet. Add one from any item on a list."</p>
                        }.into_any(),
                        Ok(rows) => view! {
                            <table class="w-full text-sm">
                                <thead><tr>
                                    <th class="text-left p-1">"Item"</th>
                                    <th class="text-left p-1">"Threshold"</th>
                                    <th class="text-left p-1">"World"</th>
                                    <th class="text-left p-1">"HQ"</th>
                                    <th class="text-left p-1">"Delivery"</th>
                                    <th class="text-left p-1">"Status"</th>
                                    <th class="text-left p-1">"Actions"</th>
                                </tr></thead>
                                <tbody>
                                    <For
                                        each=move || rows.clone()
                                        key=|a| a.id
                                        children=move |a: Alert| {
                                            let AlertTrigger::BelowThreshold {
                                                item_id, price_threshold, hq_only, ..
                                            } = &a.trigger;
                                            let item_name = xiv_gen_db::data()
                                                .items
                                                .get(&xiv_gen::ItemId(*item_id))
                                                .map(|it| it.name.as_str().to_string())
                                                .unwrap_or_else(|| format!("Item {item_id}"));
                                            let delivery_label = match &a.delivery {
                                                AlertDelivery::DiscordDm => "Discord DM".to_string(),
                                                AlertDelivery::Webhook { .. } => "Webhook".to_string(),
                                            };
                                            let enabled = a.enabled;
                                            let a_clone = a.clone();
                                            let id = a.id;
                                            view! {
                                                <tr class="border-t">
                                                    <td class="p-1">{item_name}</td>
                                                    <td class="p-1">{format!("≤ {price_threshold} gil")}</td>
                                                    <td class="p-1">{format!("{:?}", a.trigger)}</td>
                                                    <td class="p-1">{if *hq_only { "HQ" } else { "any" }}</td>
                                                    <td class="p-1">{delivery_label}</td>
                                                    <td class="p-1">
                                                        {if enabled { "enabled" } else { "disabled" }}
                                                    </td>
                                                    <td class="p-1 flex gap-1">
                                                        <button
                                                            class="btn-ghost"
                                                            aria-label="Toggle enabled"
                                                            on:click=move |_| toggle(a_clone.clone())
                                                        >
                                                            <Icon icon=if enabled { i::BsPauseFill } else { i::BsPlayFill } />
                                                        </button>
                                                        <button
                                                            class="btn-ghost"
                                                            aria-label="Delete alert"
                                                            on:click=move |_| remove(id)
                                                        >
                                                            <Icon icon=i::BiTrashSolid />
                                                        </button>
                                                    </td>
                                                </tr>
                                            }
                                        }
                                    />
                                </tbody>
                            </table>
                        }.into_any(),
                        Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
                    })}
                </Suspense>
            </section>

            <section>
                <h2 class="text-lg font-semibold mb-2">"Recent fires"</h2>
                <Suspense fallback=move || view! { <div>"Loading..."</div> }>
                    {move || events.get().map(|r| match r {
                        Ok(rows) if rows.is_empty() => view! {
                            <p class="opacity-70">"No fires yet."</p>
                        }.into_any(),
                        Ok(rows) => view! {
                            <table class="w-full text-sm">
                                <thead><tr>
                                    <th class="text-left p-1">"Time"</th>
                                    <th class="text-left p-1">"Item"</th>
                                    <th class="text-left p-1">"Matched price"</th>
                                    <th class="text-left p-1">"Delivered"</th>
                                </tr></thead>
                                <tbody>
                                    <For
                                        each=move || rows.clone()
                                        key=|e| e.id
                                        children=move |e: AlertEvent| {
                                            let item_name = xiv_gen_db::data()
                                                .items
                                                .get(&xiv_gen::ItemId(e.item_id))
                                                .map(|it| it.name.as_str().to_string())
                                                .unwrap_or_else(|| format!("Item {}", e.item_id));
                                            view! {
                                                <tr class="border-t">
                                                    <td class="p-1">{e.fired_at.to_rfc3339()}</td>
                                                    <td class="p-1">{item_name}</td>
                                                    <td class="p-1">{e.matched_price.map(|p| p.to_string()).unwrap_or_else(|| "—".into())}</td>
                                                    <td class="p-1">{if e.delivered { "✓" } else {
                                                        e.delivery_error.as_deref().unwrap_or("✗")
                                                    }}</td>
                                                </tr>
                                            }
                                        }
                                    />
                                </tbody>
                            </table>
                        }.into_any(),
                        Err(e) => view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any(),
                    })}
                </Suspense>
            </section>
        </div>
    }
}
```

- [ ] **Step 2: Verify build**

```bash
cargo check -p ultros-app
```

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/alerts.rs
git commit -m "Build Alerts page: active rules + recent fires"
```

---

## Task 9: README + design doc reconciliation

**Files:** `README.md`, `docs/superpowers/specs/2026-05-10-price-alerts-design.md`

- [ ] **Step 1: Update README**

Replace the Phase 1 README block (added in Phase 1 Task 10) with:

```markdown
## Price Alerts

Logged-in users can create per-item price-threshold alerts via the UI:
1. Add an item to a List
2. Click the bell icon on the item row
3. Pick a world/DC, set a threshold, choose Discord DM or webhook delivery
4. Manage rules + view recent fires at `/alerts`

API: `GET/POST /api/v1/alerts`, `PATCH/DELETE /api/v1/alerts/{id}`, `GET /api/v1/alerts/events`.

Delivery methods:
- Discord DM (default — uses your Discord OAuth identity)
- Discord channel webhook (paste a webhook URL from a channel's Integrations settings)

See `docs/superpowers/plans/2026-05-11-price-alerts-phase-2-3.md` for the Phase 2+3 implementation plan.

(Phase 4 — AI-suggested alert thresholds — is tracked separately.)
```

- [ ] **Step 2: Add a "Status as of 2026-05-11" addendum to the design spec**

Append to `docs/superpowers/specs/2026-05-10-price-alerts-design.md`:

```markdown
---

## Implementation status (2026-05-11)

- **Phase 1** (backend foundation) — shipped via PR #586
- **Phase 2** (webhook delivery) — combined into Phase 3 PR
- **Phase 3** (frontend UI: bell icon + drawer + /alerts page) — this PR

**Still deferred:**
- **Phase 4**: AI-suggested watchlist items + threshold recommendations
- **% drop from median** trigger (requires a median tracker reading from sale_history)
- **Live state refresh** in `PriceAlertTracker` (alerts created via API don't fire until app restart)
- **Per-list "alert defaults"** UI sugar
- **Top-bar badge** for unread fires
- **Email delivery**, **web push (PWA)** — out of original scope
```

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-05-10-price-alerts-design.md
git commit -m "Update Price Alerts docs for Phase 2+3"
```

---

## Definition of done

- [ ] Webhook URL validation rejects non-Discord-host URLs at create time (400)
- [ ] Creating an alert with `delivery: { method: "webhook", url: "https://discord.com/api/webhooks/..." }` succeeds
- [ ] `GET /api/v1/alerts` returns the actual delivery (DM or Webhook with URL) rather than always DiscordDm
- [ ] Logged-in user can navigate to `/alerts` from the user menu
- [ ] Bell icon on any list item row opens a drawer; submitting creates an alert via the API and shows a success toast
- [ ] Alerts page shows the user's rules with enable/disable + delete buttons that round-trip to the API
- [ ] Recent fires section lists events from `/api/v1/alerts/events`
- [ ] `cargo check -p ultros -p ultros-db -p ultros-api-types -p ultros-app -p migration` passes
- [ ] `cargo clippy -p ultros -p ultros-db -p ultros-api-types -p ultros-app -- -D warnings` passes
- [ ] `cargo fmt --check` passes

---

## Known follow-ups (not in this PR)

- **Per-list alert defaults** UI: a small "🔔 defaults" config near the list header that applies to all newly added items. Composes with existing schema, no migration.
- **Top-bar badge**: show an unread count next to the user menu when there are recent fires the user hasn't seen.
- **AlertConfigDrawer reuse**: currently per-item. Could be extended to an "ad-hoc alert" entry point (not tied to a List) by adding an item-picker on the drawer.
- **`get_first_endpoint_for_alert` → JOIN**: Task 3 introduces an N+1 in `list_alerts`. Acceptable for v1 (most users have <20 alerts). Future: single JOIN query.
- **i18n for new strings**: 14 new English strings introduced. Should be folded into the existing i18n sweep.
