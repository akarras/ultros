# In-Browser Notification System Plan

This document outlines the plan to implement a unified notification system for Ultros, adding support for Web Push and In-App Toast notifications, while integrating with existing Discord and Webhook notifications.

## High Level Goals

1.  **Unified Notification System**: Create a backend abstraction to handle multiple notification channels (Discord, Webhook, Web Push, In-App).
2.  **Web Push Support**: Enable users to subscribe to push notifications via the browser (Rust/WASM Service Worker + VAPID).
3.  **In-App Toast Support**: Show real-time alerts to users while they are using the app via WebSockets.
4.  **Reusable Destinations**: Implement a schema where notification destinations (e.g., a specific Discord channel, a specific Browser device) are defined once and can be reused by multiple alerts (undercut, price threshold, etc.).
5.  **Extensibility**: Ensure the system handles not just "undercut" alerts but also generic "price" threshold alerts in the future.

## Architecture

### Backend (`ultros` & `ultros-db`)

*   **Dependencies**:
    *   Add `web-push` crate for handling Web Push protocol.
    *   Add `openssl` (likely needed by `web-push` or for VAPID key generation).
*   **Database**:
    *   New tables to separate "Destinations" from "Alerts".
    *   `notification_endpoint`: Stores the details of *where* to send a notification (e.g., a specific browser's push subscription, a Discord channel ID, a Webhook URL).
    *   `alert_notification_rule`: Links a specific `alert` to a `notification_endpoint`.
*   **Logic**:
    *   Refactor `AlertManager` to use a `Notifier` trait.
    *   Implement `DiscordNotifier`, `WebhookNotifier`, `PushNotifier`, `ToastNotifier`.
    *   `PushNotifier` will use `web-push` to send encrypted payloads.
    *   `ToastNotifier` will send events via the existing WebSocket connection (`ultros-frontend`).
    *   Logic flow: Alert Triggered -> Lookup linked `notification_endpoint`s -> Dispatch to appropriate `Notifier`s.

### Frontend (`ultros-frontend`)

*   **Service Worker (Rust/WASM)**:
    *   Implement the Service Worker in Rust, compiled to WASM.
    *   Advantages: Reuse existing business logic and data structures (e.g., item data, formatting) shared with the main app.
    *   Responsibilities:
        *   Handle `push` event.
        *   Decrypt payload (if not handled by browser/lib).
        *   Format notification title/body using shared Rust logic.
        *   Handle `notificationclick` (open app deep link).
*   **UI**:
    *   "Enable Notifications" button in settings or relevant page.
    *   Request permission (`Notification.requestPermission()`).
    *   Get `PushSubscription` from `navigator.serviceWorker.ready.pushManager`.
    *   Send subscription JSON to backend API to create a `notification_endpoint`.
*   **Toast**:
    *   Listen to a new WebSocket event type (e.g., `ServerClient::Notification`).
    *   Display a toast/snackbar component when an event is received.

## Database Schema Changes

We are moving towards a "many-to-many" style relationship between Alerts and Destinations.

*   `notification_endpoint`:
    *   `id`: PK
    *   `user_id`: Owner of this endpoint.
    *   `name`: User-friendly name (e.g., "My Laptop", "Gaming Discord").
    *   `method`: Enum (DiscordChannel, WebPush, Webhook).
    *   `config`: JSONB (Stores specific config like channel ID, or Push Subscription keys/endpoint).
*   `alert_notification`:
    *   `alert_id`: FK to `alert`
    *   `endpoint_id`: FK to `notification_endpoint`

This allows a user to register their browser *once* and then check a box to say "Send Undercut Alert X to this Browser" and "Send Price Alert Y to this Browser".

## Tasks

See `TASKS.md` for the breakdown, organized by functional track.
