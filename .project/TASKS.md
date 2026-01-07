# Notification System Implementation Checklist

This checklist is organized by "Track" to allow parallel development.

## Track A: Backend Core & Database (Dependencies & Schema)

- [ ] **Dependencies**: Add `web-push` to `ultros/Cargo.toml`.
- [ ] **Environment**: Add `VAPID_PRIVATE_KEY` and `VAPID_PUBLIC_KEY` to `.env`.
- [ ] **Migration**: Create `migration/src/m2024..._create_notification_endpoints.rs`.
    - Define `notification_endpoint` table.
    - Define `alert_notification_rule` table.
- [ ] **Entities**: Run `generate_entities.sh` to update Rust structs.
- [ ] **Data Migration**: Write a script or migration step to convert existing `alert_discord_destination` rows into the new schema (preserving existing user alerts).

## Track B: Backend Logic (The Notifier System)

- [ ] **Notifier Trait**: Define `Notifier` trait in `ultros/src/alerts/notification.rs`.
    - `async fn notify(&self, endpoint: &NotificationEndpoint, message: &MessagePayload) -> Result<()>`
- [ ] **Implementations**:
    - `DiscordNotifier`: Adapts existing Discord logic.
    - `WebPushNotifier`: Uses `web-push` crate.
    - `WebhookNotifier`: Uses `reqwest`.
- [ ] **Alert Manager**:
    - Update `AlertManager` to load destinations from `alert_notification_rule` -> `notification_endpoint`.
    - Fan-out logic: For a triggered alert, iterate linked endpoints and call `notify`.

## Track C: Frontend (Service Worker & UI)

- [ ] **WASM Service Worker**:
    - Create a new crate or module (e.g., `ultros-frontend/ultros-sw`) for the Service Worker.
    - Use `wasm-bindgen` to bind to Service Worker APIs (`Self::registration`, `PushManager`, etc.).
    - Implement `push` event handler.
    - Implement `notificationclick` handler.
    - Build process: Ensure this compiles to a `.wasm` file and a JS shim that can be served by `ultros`.
- [ ] **UI - Subscriptions**:
    - Create a reusable "Notification Destinations" settings page.
    - "Add Browser Device" button:
        - calls `navigator.serviceWorker.register`.
        - calls `pushManager.subscribe`.
        - POSTs subscription to `/api/v1/user/endpoints`.
- [ ] **UI - Alert Config**:
    - Update Alert creation/edit modal.
    - Instead of just "Discord Channel ID", show a list of "Available Destinations" (Checkboxes).

## Track D: Discord & Integration

- [ ] **Discord Bot Commands**:
    - Update `/alert` commands to support the new schema.
    - When a user types `/alert subscribe`, it creates a `notification_endpoint` (if not exists for that channel) and links it.
- [ ] **Refinement**:
    - Ensure `DiscordNotifier` properly handles rate limits and retries (if moved to a generic system).

## Track E: In-App Toast (WebSocket)

- [ ] **Protocol**: Update `ultros-api-types` with `ServerClient::Notification`.
- [ ] **Backend**:
    - In `AlertManager`, detect if an endpoint is "Toast/ActiveSession" (or just broadcast to all active sessions for that `user_id`).
    - *Decision*: Should "Toast" be a persistent `notification_endpoint`? Probably not. It's ephemeral. The `AlertManager` should probably *always* try to send a Toast if the user is connected.
- [ ] **Frontend**:
    - Handle `ServerClient::Notification` in `live_data.rs`.
    - Render a Toast component.
