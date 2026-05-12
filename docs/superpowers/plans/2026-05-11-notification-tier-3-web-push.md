# Notification Tier 3 — Web Push — Implementation Plan

**Goal:** Add Web Push as a delivery method. Users subscribe their browser to receive notifications when their alerts fire; the same endpoint framework as Discord/Webhook handles delivery.

**Scope:** Tier 3 from the spec only. Excludes shared list notifications (Tier 4).

**Architecture:** Add a `WebPush` variant to `EndpointConfig`. New `push_subscription` table holds the per-browser crypto material. The web-push crate handles aes128gcm content encoding + VAPID signing. Frontend: service worker + subscribe button in `EndpointsPanel`.

**Tech Stack:** `web-push` Rust crate (https://crates.io/crates/web-push), VAPID, browser Push API + Service Workers.

---

## File map

| File | Action |
|---|---|
| `Cargo.toml` (workspace) | Modify — add `web-push` dep |
| `ultros/Cargo.toml` | Modify — depend on `web-push` |
| `migration/src/m20260512_000001_push_subscription.rs` | Create — new table |
| `migration/src/lib.rs` | Modify — register the migration |
| `ultros-db/src/entity/push_subscription.rs` | Create — Sea-ORM entity |
| `ultros-db/src/entity/mod.rs` | Modify — `pub mod push_subscription;` |
| `ultros-db/src/entity/prelude.rs` | Modify — export entity |
| `ultros-db/src/alerts.rs` (or a new `push.rs`) | Add `create_push_subscription`, `get_push_subscription_by_id`, `delete_push_subscription_by_endpoint`, `delete_push_subscriptions_for_user` |
| `ultros-api-types/src/alert.rs` | Modify — add `EndpointMethod::WebPush { subscription_id: i32 }` AND a new wire type `CreatePushSubscriptionRequest { endpoint, p256dh, auth, user_agent }` |
| `ultros/src/alerts/delivery.rs` | Modify — `EndpointConfig::WebPush { subscription_id }`, `send_webpush` impl |
| `ultros/src/web/api/push.rs` | Create — `POST /api/v1/push/vapid-public-key`, `POST /api/v1/push/subscribe` |
| `ultros/src/web/api/mod.rs` | Modify — `pub mod push;` |
| `ultros/src/web/api/endpoints.rs` | Modify — extend `method_to_db`, `db_to_method`, `validate_endpoint_method` to handle WebPush |
| `ultros/src/web.rs` | Modify — register the two push routes |
| `ultros/static/service-worker.js` | Create — push + notificationclick handler |
| `ultros/src/web/static_files.rs` | Modify — route `/service-worker.js` to serve the file with the right content-type and scope header |
| `ultros-frontend/ultros-app/src/api.rs` | Modify — add `get_vapid_public_key()`, `create_push_subscription()` |
| `ultros-frontend/ultros-app/src/components/endpoints_panel.rs` | Modify — add "Enable browser notifications" button + subscribe flow |

---

## Environment variables

Add to the env loader (look at how `DISCORD_TOKEN` etc are read in `ultros/src/lib.rs` or `main.rs`):

- `VAPID_PUBLIC_KEY` — base64url-encoded uncompressed P-256 public key (88 chars).
- `VAPID_PRIVATE_KEY` — PEM or DER private key for signing. The `web-push` crate's `VapidSignatureBuilder::from_pem(...)` or `from_der(...)` is the entry point.
- `VAPID_CONTACT_EMAIL` — `mailto:` value put in the JWT's `sub` claim.

Document a one-time setup step in CLAUDE.md or AGENTS.md: generating VAPID keys (use a helper crate or `openssl ecparam -name prime256v1 -genkey` for the private key, derive the public from it).

If any of the three env vars are missing at startup, log a clear warning and disable the push code path (handlers return "push not configured"). Do NOT panic — the bot/web should still start without push configured.

---

## Task 1: Migration — `push_subscription` table

**File:** `migration/src/m20260512_000001_push_subscription.rs`

Schema:

```rust
push_subscription {
    id: integer pk auto-increment,
    user_id: bigint not null,           // FK → discord_user.id, cascade on delete
    endpoint: text not null,            // The Push Service URL
    p256dh: text not null,              // Base64url public key
    auth: text not null,                // Base64url auth secret
    user_agent: text null,
    created_at: timestamp default now,
    last_seen_at: timestamp default now,
    unique (user_id, endpoint)
}
```

- [ ] Pattern off `m20240424_000001_create_notification_endpoints.rs` for the migration shape.
- [ ] Register in `migration/src/lib.rs`.
- [ ] `cargo check -p migration` passes.
- [ ] Commit: `feat(migration): push_subscription table`

---

## Task 2: Sea-ORM entity

**File:** `ultros-db/src/entity/push_subscription.rs`

Model the table 1:1. Use `DateTimeUtc` for the timestamps to match existing entity style.

- [ ] Wire into `entity/mod.rs` and `entity/prelude.rs`.
- [ ] Commit: `feat(ultros-db): push_subscription entity`

---

## Task 3: DB helpers

**File:** add methods to `ultros-db/src/alerts.rs` (or split into `push.rs` if you prefer; keep one source of truth).

Required methods:

```rust
pub async fn create_push_subscription(
    &self,
    owner: i64,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    user_agent: Option<&str>,
) -> Result<i32> // returns id; upserts on (user_id, endpoint)

pub async fn get_push_subscription_by_id(&self, id: i32) -> Result<push_subscription::Model>

pub async fn delete_push_subscription_by_id(&self, owner: i64, id: i32) -> Result<()>

pub async fn touch_push_subscription_last_seen(&self, id: i32) -> Result<()>
```

The create method should upsert so that a re-subscribe (same endpoint URL, different p256dh/auth — happens when the browser rotates keys) updates the row rather than inserting a duplicate.

- [ ] Commit: `feat(ultros-db): push_subscription CRUD`

---

## Task 4: Add web-push crate

- [ ] Workspace `Cargo.toml`: add `web-push = "0.10"` (verify the latest version on crates.io). Use the `isahc-client` feature only if the existing reqwest stack isn't usable for sending; otherwise default features are fine.
- [ ] `ultros/Cargo.toml`: depend on `web-push` from the workspace.
- [ ] `cargo check -p ultros`.
- [ ] Commit: `chore: add web-push crate`

---

## Task 5: VAPID config + public-key endpoint

**File:** wherever env vars are loaded today (`ultros/src/lib.rs` or `main.rs` — grep for `DISCORD_TOKEN`).

- [ ] Define a `WebPushConfig` struct with the three env values. Make it optional in WebState.
- [ ] At startup, log "Web Push enabled" if all three vars present; else log "Web Push disabled (set VAPID_*)" and proceed without it.
- [ ] Add `GET /api/v1/push/vapid-public-key` handler in `ultros/src/web/api/push.rs` returning `{ "key": "<public_key_base64url>" }`. Returns 503 if push is disabled.
- [ ] Commit: `feat(web): VAPID config + public key endpoint`

---

## Task 6: Subscribe endpoint

**File:** `ultros/src/web/api/push.rs`

`POST /api/v1/push/subscribe` body: `{endpoint: String, p256dh: String, auth: String, user_agent: Option<String>}` (defined as `CreatePushSubscriptionRequest` in api-types).

Flow:
1. AuthDiscordUser extractor (must be logged in).
2. Call `db.create_push_subscription(owner, endpoint, p256dh, auth, user_agent)` → subscription_id.
3. Call `db.create_endpoint(owner, name, "WebPush", json!({"subscription_id": id}))` to create the notification endpoint row pointing at the subscription.
4. Return the new `Endpoint` JSON.

Name format: `"Browser ({user_agent_short})"` — e.g. "Browser (Chrome on macOS)" from a parsed user-agent, or fallback to "Browser".

- [ ] Wire the route in `ultros/src/web.rs`.
- [ ] Commit: `feat(web): /api/v1/push/subscribe`

---

## Task 7: EndpointConfig + send_webpush

**File:** `ultros/src/alerts/delivery.rs`

- [ ] Add `EndpointConfig::WebPush { subscription_id: i32 }` to the parsed enum.
- [ ] Extend `deliver_to_endpoint`'s match to handle WebPush by calling `send_webpush(subscription_id, title, body, db, config)`.
- [ ] Implement `send_webpush`:
  ```rust
  async fn send_webpush(
      subscription_id: i32,
      title: &str,
      body: &str,
      db: &UltrosDb,
      config: &WebPushConfig,
  ) -> Result<()>
  ```
  Pseudocode:
  ```rust
  let sub = db.get_push_subscription_by_id(subscription_id).await?;
  let info = SubscriptionInfo::new(&sub.endpoint, &sub.p256dh, &sub.auth);
  let mut builder = WebPushMessageBuilder::new(&info);
  let payload = serde_json::to_vec(&serde_json::json!({
      "title": title,
      "body": body,
      "url": "/alerts",
  }))?;
  builder.set_payload(ContentEncoding::Aes128Gcm, &payload);
  let sig = VapidSignatureBuilder::from_pem(config.private_key_pem.as_bytes(), &info)?
      .add_claim("sub", config.contact_email.as_str())
      .build()?;
  builder.set_vapid_signature(sig);
  let message = builder.build()?;
  let client = IsahcWebPushClient::new()?;
  match client.send(message).await {
      Ok(()) => { db.touch_push_subscription_last_seen(subscription_id).await.ok(); Ok(()) }
      Err(WebPushError::EndpointNotFound | WebPushError::EndpointNotValid) => {
          // Soft-delete the subscription; the browser unsubscribed.
          let _ = db.delete_push_subscription_by_id(sub.user_id, subscription_id).await;
          Err(anyhow!("push subscription expired"))
      }
      Err(e) => Err(anyhow!("push send failed: {e}")),
  }
  ```
  Verify against the actual `web-push` crate API — names may differ (it's a moving target). Use the crate's docs.
- [ ] Pass `WebPushConfig` to `deliver_to_endpoint` via the `db: &UltrosDb` arg (stuff it into `UltrosDb` itself? Probably no — better: pass it through the call chain. Easiest: a process-wide `OnceLock<WebPushConfig>` mirroring how serenity Context is bridged in `delivery.rs`).
- [ ] Update `dispatch_alert` and `resend_alert_event` callers to thread the config.
- [ ] Commit: `feat(alerts): web push delivery`

---

## Task 8: Endpoint validation + method_to_db round-trip

**File:** `ultros/src/web/api/endpoints.rs`

- [ ] Extend `method_to_db` and `db_to_method` to handle WebPush.
- [ ] `validate_endpoint_method` for WebPush: `subscription_id > 0`.
- [ ] **Disallow** direct creation of a WebPush endpoint via `POST /api/v1/endpoints`. Only the subscribe handler can create them (because they require a real push subscription). Reject with a 400.
- [ ] Commit: `feat(api): WebPush endpoint method`

---

## Task 9: Service worker

**File:** `ultros/static/service-worker.js`

```js
self.addEventListener('push', (event) => {
  if (!event.data) return;
  let data;
  try { data = event.data.json(); } catch { data = { title: 'Ultros', body: event.data.text() }; }
  const title = data.title || 'Ultros';
  const options = {
    body: data.body || '',
    icon: '/static/android-chrome-192x192.png',
    badge: '/static/favicon-32x32.png',
    data: { url: data.url || '/alerts' },
  };
  event.waitUntil(self.registration.showNotification(title, options));
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const url = (event.notification.data && event.notification.data.url) || '/alerts';
  event.waitUntil(
    clients.matchAll({ type: 'window' }).then((wins) => {
      for (const w of wins) {
        if (w.url.endsWith(url) && 'focus' in w) return w.focus();
      }
      return clients.openWindow(url);
    })
  );
});
```

**File:** `ultros/src/web/static_files.rs`

- [ ] Add a handler that returns this JS from the root path `/service-worker.js` (not `/static/...`) — the SW scope is restricted by URL prefix, and a SW at `/service-worker.js` has site-wide scope. Set `Content-Type: application/javascript` and `Service-Worker-Allowed: /`.
- [ ] Register the route in `ultros/src/web.rs` at the root.
- [ ] Commit: `feat(web): /service-worker.js handler`

---

## Task 10: Frontend api stubs

**File:** `ultros-frontend/ultros-app/src/api.rs`

- [ ] `pub(crate) async fn get_vapid_public_key() -> AppResult<VapidPublicKey>` (where `VapidPublicKey` is a small wrapper around `{key: String}` — define in api-types alongside `CreatePushSubscriptionRequest`).
- [ ] `pub(crate) async fn create_push_subscription(req: CreatePushSubscriptionRequest) -> AppResult<Endpoint>`.
- [ ] Commit: `feat(frontend): vapid + push subscribe api stubs`

---

## Task 11: Subscribe flow in EndpointsPanel

**File:** `ultros-frontend/ultros-app/src/components/endpoints_panel.rs`

Add a "Enable browser notifications" button to the form area. On click:
1. Check `'serviceWorker' in navigator && 'PushManager' in window`. If not, toast "Browser doesn't support push" and bail.
2. Request notification permission. If denied, toast and bail.
3. Register `/service-worker.js`.
4. Fetch `/api/v1/push/vapid-public-key`.
5. Convert the base64url key to `Uint8Array` (urlBase64ToUint8Array helper inlined).
6. `registration.pushManager.subscribe({userVisibleOnly: true, applicationServerKey: <Uint8Array>})`.
7. Extract `subscription.endpoint`, `subscription.getKey('p256dh')`, `subscription.getKey('auth')` (both as base64url).
8. POST to `/api/v1/push/subscribe`.
9. On success, refresh the endpoints list.

You'll need to call browser APIs from wasm-bindgen. Look at how the existing codebase calls browser APIs (probably via `web_sys`). Add `web-sys` features as needed: `ServiceWorkerContainer`, `ServiceWorkerRegistration`, `PushManager`, `PushSubscription`, `PushSubscriptionOptionsInit`, `Notification`, `NotificationPermission`.

- [ ] Don't break existing EndpointsPanel functionality.
- [ ] Commit: `feat(frontend): enable browser notifications button`

---

## Task 12: Final CI + smoke

- [ ] `./check_ci.sh` passes.
- [ ] Document the VAPID setup in `AGENTS.md` or a new `docs/push.md`.

---

## Out of scope

- Email delivery (not on the roadmap).
- Mobile push via FCM/APNs (Web Push covers PWA-installed mobile Safari & Chrome).
- Backend rotation of VAPID keys (one set per deployment; rotating invalidates all subs).
- Per-endpoint "test" button calling the WebPush variant — Task 7's `deliver_to_endpoint` change handles this automatically since the test handler calls `deliver_to_endpoint`.

## Risks & mitigations

- **`web-push` crate API drift.** The crate has had churn — pin the version in workspace deps and double-check signatures against current docs before implementing. If a method name in this plan diverges from the real one, follow the crate, not the plan.
- **Service Worker registration race with Leptos hydration.** Mitigation: register the SW lazily, only when the user clicks "Enable browser notifications" — not on every page load.
- **Subscription expiry.** Browsers periodically refresh push subscriptions. The `EndpointNotFound`/`EndpointNotValid` errors are how the push service signals "this sub is dead." The send path soft-deletes and surfaces the failure in `alert_event`.
- **VAPID key in env var.** Operators must keep the private key secret. Document this clearly. Future: load from a secrets manager.
