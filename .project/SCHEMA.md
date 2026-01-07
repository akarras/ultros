# Database Schema Changes

## Concepts

We are introducing a reusable "Notification Endpoint" concept. This decouples the *destination* of a notification from the *reason* (Alert) for the notification.

## New Tables

### `notification_endpoint`

Stores a valid destination for notifications.

| Column | Type | Constraints | Description |
| :--- | :--- | :--- | :--- |
| `id` | `INTEGER` | `PRIMARY KEY AUTOINCREMENT` | Unique ID |
| `user_id` | `INTEGER` | `FOREIGN KEY REFERENCES discord_user(id)` | The user who owns this endpoint |
| `name` | `TEXT` | `NOT NULL` | User-friendly name (e.g., "Chrome Desktop", "Market Discord") |
| `method` | `TEXT` | `NOT NULL` | Enum: `DiscordChannel`, `WebPush`, `Webhook` |
| `config` | `JSON` | `NOT NULL` | Driver-specific configuration |
| `created_at` | `TIMESTAMP` | `DEFAULT CURRENT_TIMESTAMP` | |

#### Config JSON Schema Examples

**WebPush:**
```json
{
  "endpoint": "https://fcm.googleapis.com/fcm/send/...",
  "p256dh": "BNc...",
  "auth": "Zn..."
}
```

**DiscordChannel:**
```json
{
  "channel_id": 1234567890
}
```

**Webhook:**
```json
{
  "url": "https://hooks.slack.com/...",
  "secret": "optional-signing-secret"
}
```

### `alert_notification_rule`

Links an specific alert to a notification endpoint. This allows multiple alerts to trigger the same endpoint, and one alert to trigger multiple endpoints.

| Column | Type | Constraints | Description |
| :--- | :--- | :--- | :--- |
| `alert_id` | `INTEGER` | `FOREIGN KEY REFERENCES alert(id) ON DELETE CASCADE` | The alert source |
| `endpoint_id` | `INTEGER` | `FOREIGN KEY REFERENCES notification_endpoint(id) ON DELETE CASCADE` | The destination |
| `PRIMARY KEY` | | `(alert_id, endpoint_id)` | Composite PK |

## Migration Plan

1.  **Create Migration**: `sea-orm-cli migrate generate create_notification_endpoints`.
2.  **Schema Definition**: Define the tables above in the migration file.
3.  **Data Migration (Optional)**:
    *   We currently have `alert_discord_destination`.
    *   We can migrate these existing rows into `notification_endpoint` + `alert_notification_rule`.
    *   *Step*: Iterate `alert_discord_destination`. For each row, create a `notification_endpoint` (method=DiscordChannel, config={channel_id: ...}), then link it in `alert_notification_rule`.
    *   *Note*: Since `alert_discord_destination` didn't have a name, we can generate one like "Discord Channel {id}".
4.  **Cleanup**: Eventually drop `alert_discord_destination` after verifying the new system works.
