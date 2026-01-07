# In-Browser Notification System Plan

This document outlines the plan to implement a unified notification system for Ultros, adding support for Web Push and In-App Toast notifications, while integrating with existing Discord and Webhook notifications.

## High Level Goals

1.  **Unified Notification System**: Create a backend abstraction to handle multiple notification channels (Discord, Webhook, Web Push, In-App).
2.  **Web Push Support**: Enable users to subscribe to push notifications via the browser (Service Worker + VAPID).
3.  **In-App Toast Support**: Show real-time alerts to users while they are using the app via WebSockets.
4.  **Manage Subscriptions**: specific database updates to track user subscriptions to different notification methods.

## Architecture

### Backend (`ultros` & `ultros-db`)

*   **Dependencies**:
    *   Add `web-push` crate for handling Web Push protocol.
    *   Add `openssl` (likely needed by `web-push` or for VAPID key generation).
*   **Database**:
    *   New tables/entities to store notification destinations.
    *   `alert_destination`: A generic table or a set of tables to store config for each type.
        *   `type`: Enum (Discord, Webhook, Push, Toast)
        *   `config`: JSON (stores webhook URL, or push subscription info)
        *   `alert_id`: FK to `alert`
*   **Logic**:
    *   Refactor `AlertManager` to use a `Notifier` trait.
    *   Implement `DiscordNotifier`, `WebhookNotifier`, `PushNotifier`, `ToastNotifier`.
    *   `PushNotifier` will use `web-push` to send encrypted payloads.
    *   `ToastNotifier` will send events via the existing WebSocket connection (`ultros-frontend`).

### Frontend (`ultros-frontend`)

*   **Service Worker**:
    *   Create `sw.js` (or use `gloo-worker` / `wasm-bindgen` if possible, but raw JS `sw.js` is often easier for push).
    *   Handle `push` event to display system notification.
    *   Handle `notificationclick` event.
*   **UI**:
    *   "Enable Notifications" button in settings or relevant page.
    *   Request permission (`Notification.requestPermission()`).
    *   Get `PushSubscription` from `navigator.serviceWorker.ready.pushManager`.
    *   Send subscription JSON to backend API.
*   **Toast**:
    *   Listen to a new WebSocket event type (e.g., `ServerClient::Notification`).
    *   Display a toast/snackbar component when an event is received.

## Database Schema Changes

We will likely replace or augment `alert_discord_destination`.

**Option A: Separate Tables (Cleanest for SQL)**
*   `alert_discord_destination` (Existing)
*   `alert_web_push_destination`
    *   `id`
    *   `alert_id`
    *   `endpoint` (TEXT)
    *   `p256dh` (TEXT)
    *   `auth` (TEXT)
*   `alert_webhook_destination`
    *   `id`
    *   `alert_id`
    *   `url` (TEXT)

**Option B: Single Table (Flexible)**
*   `alert_destination`
    *   `id`
    *   `alert_id`
    *   `type` (Enum: Discord, WebPush, Webhook)
    *   `config` (JSONB)

*Recommendation*: Option A is better for type safety with SeaORM, but Option B reduces the number of joins if we want "all destinations for an alert". Given SeaORM handles relations well, Option A might be safer. However, we already have `alert_discord_destination`. Let's stick to separate tables for now to avoid migrating existing data excessively, or migrate `alert_discord_destination` to a generic one later. For this plan, we will add new tables.

## Tasks

See `TASKS.md` for the breakdown.
