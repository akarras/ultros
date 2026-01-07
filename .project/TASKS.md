# Notification System Implementation Checklist

## Phase 1: Preparation & Dependencies

- [ ] **Dependencies**: Add `web-push` to `ultros/Cargo.toml`.
- [ ] **Environment**: Add `VAPID_PRIVATE_KEY` and `VAPID_PUBLIC_KEY` to `.env` (and generate them if missing).
- [ ] **API Types**: Add `SubscriptionInfo` struct to `ultros-api-types` (compatible with JS `PushSubscription`).

## Phase 2: Database Schema

- [ ] **Create Migration**: Create a new SeaORM migration in `migration/`.
- [ ] **Table `alert_web_push_destination`**:
    - `id`: Serial/Integer PK
    - `alert_id`: Integer FK to `alert.id`
    - `endpoint`: String/Text (The push service URL)
    - `p256dh`: String/Text (Public key)
    - `auth`: String/Text (Auth secret)
    - `user_agent`: String (Optional, for debugging/info)
- [ ] **Table `alert_webhook_destination`**:
    - `id`: Serial/Integer PK
    - `alert_id`: Integer FK to `alert.id`
    - `url`: String/Text
- [ ] **Generate Entities**: Run `generate_entities.sh` to update `ultros-db/src/entity`.

## Phase 3: Backend Core & Abstraction

- [ ] **Notifier Trait**: Define `Notifier` trait in `ultros/src/alerts/notification.rs`.
    - `async fn notify(&self, message: &str, ...)`
- [ ] **Discord Notifier**: Refactor `send_discord_alerts` into `DiscordNotifier` implementing `Notifier`.
- [ ] **Push Notifier**: Implement `WebPushNotifier`.
    - Construct `WebPushMessage`.
    - Sign with VAPID.
    - Send using `web_push` client.
- [ ] **Webhook Notifier**: Implement `WebhookNotifier` using `reqwest`.
- [ ] **Alert Manager Update**:
    - Fetch all destination types for an alert.
    - Instantiate appropriate Notifiers.
    - Fan-out notifications.

## Phase 4: Backend API

- [ ] **API Endpoint**: Create `POST /api/v1/alerts/{alert_id}/subscribe/push`
    - Accepts `SubscriptionInfo`.
    - Saves to `alert_web_push_destination`.
- [ ] **API Endpoint**: Create `POST /api/v1/alerts/{alert_id}/subscribe/webhook`
    - Accepts URL.
    - Saves to `alert_webhook_destination`.

## Phase 5: Frontend - Service Worker & Push

- [ ] **Service Worker File**: Create `sw.js` (or `service-worker.js`) in `ultros/static` or appropriate public folder.
    - Implement `push` event listener.
    - Implement `notificationclick` event listener (open URL).
- [ ] **Registration**: In `ultros-app`, add logic to register the Service Worker.
- [ ] **VAPID Key**: Expose VAPID Public Key via API or config to frontend.
- [ ] **Subscription Logic**:
    - Check `serviceWorker.ready`.
    - Call `pushManager.subscribe()`.
    - Send result to Backend API.

## Phase 6: Frontend - In-App Toast

- [ ] **WebSocket Protocol**: Update `ultros-api-types/src/websocket.rs`
    - Add `ServerClient::Notification(String)` variant.
- [ ] **Backend WS Handler**: In `AlertManager` or `RetainerAlertListener`, when an alert fires, find active websocket connections for the user and send the message.
    - *Note*: This requires mapping `alert.owner` (Discord ID) to active WebSocket sessions. Currently, WS seems anonymous or based on filters. We might need to ensure the WS session is authenticated or linked to the user.
- [ ] **Frontend Toast Component**:
    - Create a `Toast` or `Snackbar` component.
    - In `ws/live_data.rs` (or similar), handle `ServerClient::Notification`.
    - Push notification to global signal to display Toast.

## Phase 7: Verification

- [ ] **Unit Tests**: Test `Notifier` implementations (mocking external calls).
- [ ] **Integration Test**: Verify database persistence of subscriptions.
- [ ] **Manual Test**:
    - Subscribe via Browser.
    - Trigger Alert (mock or real).
    - Verify Push Notification appears (even with tab closed).
    - Verify In-App Toast appears (when tab open).
    - Verify Discord message still works.
