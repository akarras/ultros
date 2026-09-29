# Price Alerts

Logged-in users can create per-item price-threshold alerts via the UI:
1. Add an item to a List
2. Click the bell icon on the item row
3. Pick a world/DC, set a threshold, choose Discord DM or webhook delivery
4. Manage rules + view recent fires at `/alerts`

API: `GET/POST /api/v1/alerts`, `PATCH/DELETE /api/v1/alerts/{id}`, `GET /api/v1/alerts/events`,
`POST /api/v1/alerts/events/read`, `GET /api/v1/alerts/events/unread_count`. The last three back
the notification inbox — see `docs/notification-inbox.md` for the data model, pagination/read-state
semantics, and the websocket push.

Delivery methods:
- Discord DM (default — uses your Discord OAuth identity)
- Discord channel webhook (paste a webhook URL from a channel's Integrations settings)
- "This site" — every user has one auto-created, undeletable `InApp` endpoint that delivers to
  the notification inbox itself; see `docs/notification-inbox.md`.

(Phase 4 of the original design — AI-suggested alert thresholds — was never built.)

## Retainer sale alerts

Fires when one of your claimed retainers' listings disappears and a sale with
the same world, item, quality, price and quantity is recorded within five
minutes. Sales are inferred (Universalis never says whose listing sold), so
the matcher prefers to miss a sale over reporting a wrong one: if another
seller had a listing at the same price and quantity removed in the same
window, nothing fires. Create one from the bell on `/retainers/listings`, the
Alerts page drawer, or `/ffxiv retainer add_sale_alert` in Discord.
Design: `docs/superpowers/specs/2026-09-07-retainer-sold-alert-design.md`.

## Below-median alerts

Fires when a listing appears at least N% (5–90) under the item's **30-day median for
the listing's own quality** in the alert's world, data center or region. HQ listings
are compared with the HQ median and NQ with the NQ median; "HQ only" skips NQ
listings. The median is the one the item page's confidence badge shows
(`/api/v1/item_stats`): `p50` from ClickHouse `item_stats_window`, folded across the
scope's worlds by `aggregate_item_stats_variants`.

- **Thin markets:** a median is only used when its cleaned sample has at least 10
  sales and the quality scorer hasn't marked it `unusable`. A zero MAD alone doesn't
  disqualify it. Below that the alert stays saved but quiet, and starts working once
  the item has more history. The drawer previews the median it will use.
- **Caching:** `ultros-alerts/src/median_cache.rs` fetches every referenced
  `(item, scope)` hourly (the 30-day rollup only changes every 6 hours), plus
  immediately when a new alert references a new key. The listing path never waits
  on ClickHouse.
- **ClickHouse down:** the last good median is used for up to 24 hours, then the
  alert pauses. It never fires without a baseline.
- One listing event fires at most once per alert, reporting its cheapest match.

Create one from the bell on an item or list row, the Alerts page drawer, or
`/ffxiv alert below-median` in Discord.

## Back-in-stock alerts

Fires when the alert's scope has had **no listings** of the item (no HQ listings,
with "HQ only") for at least **10 minutes** and one appears. The minimum empty time
is fixed: it absorbs Universalis reprices (a remove + add of the same listing, which
briefly empties a one-listing board) and the remove/add reordering between websocket
messages that are persisted independently.

Listing events only prompt a recount; the Postgres `active_listing` count decides.
Removals in scope trigger a recount (zero arms the alert), an add in scope fires if
the alert has been armed for 10 minutes (and disarms it either way), and every alert
is recounted hourly to recover from events Universalis dropped. The armed state is
in memory, so after a restart back-in-stock alerts need 10 minutes before they can
fire again. Create one from the same places, or `/ffxiv alert back-in-stock`.

Both triggers live in `ultros-alerts/src/market_trigger_tracker.rs`. Design:
`docs/superpowers/specs/2026-09-22-below-median-back-in-stock-alerts-design.md`.

## Cooldowns

Every alert has `cooldown_seconds` (default 3600, clamped to 60–86400). The item
price, list, list-update, below-median and back-in-stock trackers enforce it (in
memory before dispatch, and in the DB after delivery). Two don't:

- **Retainer sold** deliberately has no cooldown — each sale is its own event.
- **Retainer undercut** does not enforce it yet; its only throttle is a
  per-listing flag that resets when your own price changes, so a price war can send
  one alert per round. Enforcing it changes behavior for existing Discord undercut
  alerts (which silently carry the 3600s default), so it is a separate follow-up.
