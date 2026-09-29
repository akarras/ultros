# Below-median and back-in-stock alert triggers

Status: approved 2026-09-22 (brainstormed with Aaron; remaining open points resolved
by the recommendations below at his request).

The 2026-05-10 price-alerts design deferred two triggers: "% drop from median" and
"back in stock". This spec adds both as `AlertTrigger` variants with server-side
matching, UI, Discord commands and docs.

## Semantics

### Below median

Fire when a listing for `item_id` inside `world_selector` is priced at least
`percent_below`% under the item's **30-day median for the listing's own quality**
in that same selector scope.

- Baseline = `p50_30d` from `item_stats_window`, folded across the scope's worlds by
  `aggregate_item_stats_variants` — the exact number the item page's
  `ConfidenceBadge` / `/api/v1/item_stats` reports.
- NQ listings compare against the NQ median, HQ listings against the HQ median.
  `hq_only` skips NQ listings entirely.
- Match: `price_per_unit * 100 <= p50 * (100 - percent_below)` (integer math,
  boundary inclusive).
- `percent_below` is validated to `5..=90`.
- **Thin market:** a baseline is only usable when `cleaned_sample_size_30d >= 10`
  and the confidence band is not `unusable`. MAD = 0 alone is not disqualifying
  (a large one-price sample is a real median; a small one is already caught by the
  sample floor). Unusable baseline → the rule silently does not fire; it is saved
  anyway and starts working once history accumulates.
- **ClickHouse down:** the listener keeps its last good baseline for up to 24h,
  then treats it as missing. It never fires without a baseline.
- If one listing event carries several matching listings, one fire reports the
  cheapest.

### Back in stock

Fire when `world_selector` had **zero** listings of `item_id` (HQ listings only when
`hq_only`) for at least **10 minutes** and a listing appears.

- The minimum empty duration is fixed (not user-configurable). It absorbs
  Universalis reprices (`remove(old)` + `add(new)` with the same listing id, which
  can momentarily empty a one-listing board) and the remove/add race caused by each
  websocket message being persisted in its own spawned task.
- Events only prompt a recount; the Postgres `active_listing` count is the source of
  truth.
- State per rule: `empty_since: Option<DateTime>` (in memory only).
  - Startup / rule reload: a scope counting 0 gets `empty_since = now` (existing
    rules carry their state over by `alert_id`). Consequence: after a restart,
    back-in-stock rules are blind for 10 minutes. Accepted.
  - Remove event in scope → recount (outside the lock, deduplicated per
    `(item, worlds, hq_only)`); 0 and unarmed → arm.
  - Add event in scope: armed ≥ 10 min and off cooldown → fire (cheapest added
    listing), disarm. Armed < 10 min → disarm without firing. Unarmed → nothing.
  - Hourly reconcile recounts every rule: 0 arms an unarmed rule (lost remove),
    > 0 disarms an armed rule (lost add — prevents a stale arm from producing a
    false "back in stock" later).
  - A failed count query leaves state unchanged (`warn!`).

### Cooldown

Both triggers use `alert.cooldown_seconds` (default 3600, clamped 60–86400 by the
existing `resolve_cooldown_seconds`), enforced the way the price tracker does it:
`last_fired_at` is bumped in memory under the tracker lock before dispatch, and in the
DB after a successful delivery.

The retainer-undercut tracker's unenforced cooldown (notification-inbox spec gap 6) is
**out of scope**: enforcing it changes behavior for existing Discord undercut alerts
that silently carry the 3600s default, and it collides with the undercut-pressure work.
It stays a documented follow-up.

## Data model

Migration adds two child tables shaped like `alert_item_threshold`
(`alert_id` unique, FK → `alert.id` ON DELETE CASCADE, `world_selector` JSON):

- `alert_below_median (id, alert_id, item_id, world_selector, percent_below, hq_only)`
- `alert_back_in_stock (id, alert_id, item_id, world_selector, hq_only)`

## API

```rust
AlertTrigger::BelowMedian { item_id, world_selector, percent_below, hq_only } // "below_median"
AlertTrigger::BackInStock { item_id, world_selector, hq_only }                // "back_in_stock"
```

Shared pure helpers in `ultros-api-types::alert` (usable by server and drawer):
`baseline_is_usable`, `below_median_matches`, `BELOW_MEDIAN_MIN_SAMPLES`,
`BELOW_MEDIAN_PERCENT_RANGE`, `BACK_IN_STOCK_MIN_EMPTY_SECS`.

`create_alert` validates `percent_below`, requires non-empty `endpoint_ids` and a
resolvable selector; `list_alerts` rebuilds both variants. `UpdateAlertRequest` is
unchanged (editing the percentage = delete + recreate, as with thresholds in the UI).

## Server components (`ultros-alerts`)

- `median_cache.rs` — `MedianCache`: `(item_id, selector) → {nq, hq, fetched_at}`.
  Hourly refresh of every referenced key plus an immediate fetch of new keys when
  rules change; batched `deep_scan_batch(30, …)` chunks; keys no longer referenced
  are evicted; failures keep last good values; entries older than 24h read as
  missing. The ClickHouse fetch sits behind a `BaselineSource` trait for tests.
  `ultros-alerts` gains an `ultros-clickhouse` dependency.
- `market_trigger_tracker.rs` — `MarketTriggerListener` subscribes to listing
  Add/Remove and the `alert::Model` bus; holds the below-median and back-in-stock
  rule indexes; reads the cache synchronously (never awaits ClickHouse on the
  listing path).
- Wiring: `AlertManagerServices` gains the `ClickHouseClient` (already in scope in
  `ultros/src/discord/mod.rs`); the listener starts as a singleton.

Messages (server-side English, like every existing alert message):

- Below median — `📉 {item} at {price} gil ({pct}% below median)`, body
  `30-day median ({scope}, NQ|HQ): {p50} gil from {n} sales` + link.
- Back in stock — `📦 {item} is back in stock on {world}`, body
  `{qty}× at {price} gil[ (HQ)]` + link.
- `click_url` = `/item/{world}/{id}` for the triggering listing's world;
  push kind `AlertKind::Price`.

## UI (`ultros-ui-alerts`)

- The drawer's `AlertKind` gains `BelowMedian` and `BackInStock`, signed-in only
  (guests keep item-price only — both triggers need server state). They are
  item-scoped, so the item-page bell and list-row bell (`preset_item`) now offer
  Price / Below median / Back in stock instead of hiding the kind picker.
- Fields: item + world selector + HQ toggle (as item price); below median adds a
  percent input (default 30) and a baseline preview from `/api/v1/item_stats`
  ("30-day median on {scope}: {p50} gil ({n} sales)" or "Not enough sales history
  to alert on yet"), using the shared `baseline_is_usable`.
- The rules panel renders both variants.
- All strings through leptos-i18n in all 7 locales.

## Discord

`/ffxiv alert below-median item percent [hq] [world] [cooldown]` and
`/ffxiv alert back-in-stock item [hq] [world] [cooldown]`, mirroring
`/ffxiv alert price` (home-world fallback, DM endpoint, cooldown clamp).
`/ffxiv alert list` shows both.

## Testing

- `ultros-api-types`: `below_median_matches` (boundary, quality choice, hq_only,
  scope, unusable baseline, cooldown) and `baseline_is_usable`; serde round-trips.
- `ultros-alerts`: back-in-stock state machine as pure transitions (arm, fire after
  10 min, disarm on early add, reconcile both directions, cooldown); median cache
  staleness/eviction with a fake `BaselineSource`; message formatting.
- `ultros-db`: CRUD compiles under clippy; exercised via the API.

## Docs

`docs/price-alerts.md` gains a section for each trigger plus the undercut-cooldown
follow-up note; a changelog entry announces the feature.
