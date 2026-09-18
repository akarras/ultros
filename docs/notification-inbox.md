# Notification inbox

Every alert fire is recorded and pushed to the owner's open sockets, regardless
of whether Discord/webhook/push delivery succeeded. The inbox is a read of
that same `alert_event` history, plus a read/unread cursor kept server-side.
Design: `docs/superpowers/specs/2026-09-15-notification-inbox-design.md`.

## Data model

`alert_event` (existing delivery-attempt log) gained four columns in
`migration/src/m20260915_000001_alert_event_inbox.rs`:

| Column | Type | Meaning |
|---|---|---|
| `read_at` | `timestamptz`, nullable | When the owner marked this event read. `NULL` = unread. |
| `title` | `text`, nullable | Inbox display title. |
| `body` | `text`, nullable | Inbox display body. |
| `click_url` | `text`, nullable | Relative/absolute URL the inbox entry links to when clicked. |

Two indexes:
- `idx_alert_owner` on `alert(owner)` — every inbox query joins `alert_event`
  to `alert` to scope by owner (there's no `owner` column on `alert_event`
  itself, only `alert_id`).
- `idx_alert_event_alert_unread` on `alert_event(alert_id) WHERE read_at IS
  NULL` — a partial index sized to the unread set, backing the unread-count
  query.

Read state lives entirely server-side in `read_at`; there is no client-side
"seen" cache to reconcile. `ultros-db/src/alerts.rs` owns every inbox query:
`record_alert_event`, `get_recent_alert_events_for_user` (cursor pagination by
`id DESC`), `count_unread_alert_events_for_user`,
`mark_alert_events_read_for_user`, `get_alert_event_by_id_owned_by`.

Every tracker (`price_alert_tracker`, `list_update_alert_tracker`,
`sold_alert`, `undercut_alert`) funnels through
`ultros/src/alerts/inbox.rs::record_fire` instead of writing `alert_event`
directly, so a fire is recorded *and* broadcast on the `notifications` event
bus (`ultros/src/event.rs`, `NotificationEvent { owner, event }`, ring size
256) in one place — regardless of whether any endpoint actually delivered.
`alert_event_to_api` is the single DB-model → wire-type mapping, reused by
both the websocket push and the REST history endpoint so they never disagree
on shape.

`undercut_alert` batches before it fires: undercuts one alert detects are
held in an `UndercutRollup` (30 s of quiet after the latest, at most 120 s
after the first) and sent as a single `record_fire` — repeats of the same
item merge, several items become one "undercut on N items: …" body. This is
what keeps six copies of one item, or a competitor relisting a whole set,
from producing a row (and a Discord/Web Push message) per listing event.

## REST

All three routes require Discord auth (`AuthDiscordUser`) and are scoped to
the caller's own alerts by joining through `alert.owner`. Routes registered in
`ultros/src/web.rs`.

| Method | Path | Notes |
|---|---|---|
| `GET` | `/api/v1/alerts/events` | `limit` (default 50, clamped to `[1, 200]`), `before_id` (cursor: strictly older than this id, i.e. the last id seen on the previous page). Returns newest-first `Vec<AlertEvent>`. |
| `POST` | `/api/v1/alerts/events/read` | Body `MarkAlertEventsReadRequest { ids: Vec<i64>, up_to_id: Option<i64> }`. Marks events matching `id` in `ids` OR `id <= up_to_id`. Both empty → no-op, returns `updated: 0` (guards against an accidental unbounded UPDATE). Returns `MarkAlertEventsReadResponse { updated, unread_count }` — the new unread count comes back in the same response so the frontend doesn't need a second round trip for its badge. |
| `GET` | `/api/v1/alerts/events/unread_count` | Returns `UnreadAlertEventCount { unread }`. |

`ids` is capped at 500 per `POST .../read` request (`ApiError::BadRequest` on
`len() > 500`).

Ownership is opaque: `get_alert_event_by_id_owned_by` and
`mark_alert_events_read_for_user` return the same "not found" outcome whether
an id doesn't exist at all or belongs to another user's alert — a caller can't
use these endpoints to probe whether a given event id exists for someone else.

## Websocket protocol

Subscribe on the existing realtime socket (`GET /api/v1/realtime/events`):

```json
{"SubscribeNotifications":{"subscription_id":1}}
```

`subscription_id` is optional — the server assigns one and echoes it back in
`Subscribed` if omitted. The server replies `{"Subscribed":{"subscription_id":1}}`
on success.

Fired alerts arrive wrapped in `SubscriptionEvent`:

```json
{"SubscriptionEvent":{"subscription_id":1,"event":{"Notification":{ ...AlertEvent... }}}}
```

Other outcomes on the same subscription. Only `Notification` is wrapped in
`SubscriptionEvent` as a matter of course — `notification_relay`
(`ultros/src/web/api/real_time_data.rs`) sends `Stale` bare, the same
socket-wide convention as `Subscribed`/`Unsubscribed`:
- **`Stale`** (`{"Stale":{"subscription_id":1}}`) — the socket fell behind the
  256-slot `notifications` bus and some fires were dropped before this
  receiver read them (Postgres still has them). On `Stale` the client should
  refetch `GET /api/v1/alerts/events` and
  `GET /api/v1/alerts/events/unread_count` rather than trying to patch its
  local list.
- **Anonymous caller** → `authorize_notifications` (same file) builds a
  one-off scoped `Error` before a relay is even created, so this specific
  case *is* wrapped in `SubscriptionEvent`:
  `{"SubscriptionEvent":{"subscription_id":1,"event":{"Error":{"message":"sign in to receive notifications"}}}}`.
  The subscription is not registered.
- Exceeding **`MAX_SUBSCRIPTIONS_PER_SOCKET` (64)** active subscriptions on one
  socket (shared across every subscription kind, not just notifications)
  answers with an unscoped `Error` and the `SubscribeNotifications` call is
  rejected.

`notification_relay` (`ultros/src/web/api/real_time_data.rs`) filters the
shared `notifications` bus down to events whose `owner` matches the
authenticated user — every other user's fires on the same bus are silently
dropped for this subscription, never leaked.

Debugging by hand, mirroring the `lists-sync` recipe: log in with
`curl -c jar "$BASE/test/login?user_id=...&username=..."` on a test-auth
build, then:

```bash
websocat -H "Cookie: discord_auth=..." ws://host/api/v1/realtime/events
{"SubscribeNotifications":{"subscription_id":1}}
```

## The InApp endpoint ("This site")

Every user has exactly one `notification_endpoint` of method `InApp` — the
notification inbox itself, addressed by `owner`/`user_id` rather than an
external destination. It exists so that an alert whose *only* endpoint is the
inbox still counts as delivered:

- `deliver_to_endpoint`/`deliver_non_discord_endpoint`
  (`ultros/src/alerts/delivery.rs`) treat `EndpointConfig::InApp {}` as a
  no-op `Ok(())`. The actual inbox write happens separately, right after
  dispatch returns, via `inbox::record_fire` — regardless of this arm's
  result. Without the no-op arm, `dispatch_alert_detailed` would report
  `PermanentFailure` for "no deliverable endpoints" and the alert's cooldown
  would never advance.
- Auto-created lazily: `GET /api/v1/endpoints` calls
  `get_or_create_inapp_endpoint(user.id, "This site")` before listing, so
  every user gets one on first fetch rather than at signup.
- Dedupe is on `(user_id, method)` alone — no config payload distinguishes
  rows, since a user has at most one inbox.
- Cannot be created via `POST /api/v1/endpoints`
  (`validate_endpoint_method` rejects `EndpointMethod::InApp {}` — "auto-created
  for every user and cannot be created via this endpoint").
- Cannot be deleted: `DELETE /api/v1/endpoints/{id}` rejects a `method ==
  "InApp"` target with `ApiError::BadRequest` — deleting it would just get
  silently re-created on the caller's next `GET`, so the delete is refused
  outright instead.
- `test`/`resend` against the InApp endpoint report `delivered: true` but have
  **no side effect at all** — `deliver_to_endpoint`'s `InApp {}` arm is a bare
  no-op, and neither `test_endpoint` nor `resend_alert_event` calls
  `inbox::record_fire`. Testing (or resending) "This site" does not add
  anything to the inbox; only a real alert fire does, via `record_fire`.

## Shared predicate contract for guest evaluation

`ultros-api-types::alert::ThresholdRule` + `threshold_listing_matches` (in
`ultros-api-types/src/alert.rs`) are a pure, DB-free re-statement of an
item-price-threshold alert rule, shared between the server
(`ultros/src/alerts/price_alert_tracker.rs`) and the browser — so a guest
without an account can evaluate the identical rule locally against a
`WorldHelper` and an `ActiveListing`, and get the same answer an account-based
alert would.

Check order: item id, then world containment (`listing.world_id` resolved
through `rule.world_selector` via `WorldHelper::lookup_selector` +
`is_in`), then `hq_only`, then price (`listing.price_per_unit <=
rule.price_threshold`), then cooldown (`is_off_cooldown_at`, off-cooldown when
`last_fired_at` is `None` or `now - last_fired_at >= cooldown_seconds`).

Unlike `FilterPredicate::World`'s fail-open default (an unresolvable world
selector defaults to matching), an unresolvable `listing.world_id` or
`rule.world_selector` in `threshold_listing_matches` **never matches** —
deliberately mirroring the server's existing `rule_matches_listing` semantics
rather than the more permissive analyzer-filter behavior.

## See also

- `docs/price-alerts.md` — alert creation UI/API, delivery methods.
- `docs/push.md` — Web Push delivery method and VAPID setup.
- `docs/lists-sync.md` — the realtime socket's other subscription kinds
  (`SubscribeList`, `SubscribeListDoc`) and its debugging conventions, which
  this doc mirrors for `SubscribeNotifications`.
