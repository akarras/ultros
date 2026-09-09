# Windowed listing history (#1342)

The current-only `GET /api/v1/listing_stats/{world|dc|region}` retains its response and query. An explicit `?window=1`, `7`, `30`, or `90` adds an optional `window` object to each item/HQ row. Historical-only keys are included even when their current board is empty. Unsupported windows return 400. Current-only requests use cache window 0; all four historical windows use distinct slots, with the existing single-flight/stale/failure cache behavior. A failed history query is unavailable, never successful zero history.

## Meanings and coverage

- `from`/`to` define a half-open observation window. Additions and removals count non-snapshot events; updates are neither. These are observed changes, not sale counts. Catch-up and manual changes count toward turnover but cannot be matched as websocket sales.
- `listing_coverage` and `matches.receipt_coverage` report the first/last retained observations read for that window, including up to 600 seconds of pre-window matching context. `observed_span_secs` intersects that span with the requested window. **Continuity is unverified**: quiet periods and ingestion gaps cannot be distinguished. A nominal 90-day request is not evidence of 90 days of coverage.
- The new `sale_receipts` table records socket receipt time only for sales newly inserted by websocket ingest. It has independent writer/drop metrics and a 365-day TTL. Backfill and catch-up do not manufacture receipts. Existing `sales` rows and the current sale API are unchanged. `sales_without_receipt` counts sales whose game timestamp falls in the window but whose retained receipt evidence is absent. `received_sales` instead counts receipts observed in the window; these are different cohorts.
- Matching requires world/item/HQ/per-unit price/stack quantity, receipt within ±300 seconds of a websocket removal observation, a game sale time no more than 24 hours old at receipt, and sale time at or after `reviewed_at` (60-second clock skew allowed). Same-retainer re-adds/updates of the stack within ±300 seconds exclude reprices. Each sale must have exactly one removal candidate and each removal exactly one sale candidate, including candidates from other retainers and just outside the requested window. Ambiguity is excluded before attribution; no sale is reused. This is deliberately stricter than assigning one of several same-retainer candidates.
- `age_origin` is `last_review_time`: time from the listing's last retainer review to the game's sale timestamp. It is not original listing creation time. A tolerated negative skew clamps to zero; missing/invalid review times are excluded. The median is computed across matched observations across all worlds, not averaged from per-world medians.
- The newest 600 seconds of removals remain `pending`, allowing full ±300-second sale context and the sale's full ±300-second competing-removal context. `settled_through_unix` identifies the frontier. `matched`, `ambiguous`, `repriced`, `unmatched`, and `pending` partition observed non-snapshot removals; none asserts every removal was a sale.
- `days_of_stock = aggregate alive units * window days / aggregate sold units` uses the matching `sale_stats_window`. Every selected world must have both alive and sale snapshots for that item/HQ. Missing snapshots yield `stock_status=unavailable`; known zero sale units yield `no_sales` and null duration; positive sale units yield `estimated` (including a zero duration for known zero stock). Snapshots follow the existing rollup cadence.
- Floor extrema replay each world's pre-window baseline and every transition before taking the scope minimum. Simultaneous transitions apply together. Unknown world baselines prevent a known scope floor. `floor_known_secs` includes explicitly empty intervals, `floor_empty_secs` is that subset, and `floor_unknown_secs` accounts for the remainder. Empty boards never contribute a zero-gil price. Same-second ties retain the existing conservative zero/lowest-price rule because the source has no sequence number.

## Floor API for analyzer consumers (T13)

Extend the merged floor-history API rather than adding a parallel `floor_series` implementation:

```http
POST /api/v1/floor_history/Gilgamesh
Content-Type: application/json

{"item_ids":[12,4422],"from":1788739200,"to":1788825600,"interval":"hourly","hq":false}
```

`interval` is `hourly` or `daily`. Omitted/null `hq` returns separate NQ and HQ entries. The response is `{"series":[{"item_id":12,"hq":false,"history":{"from":...,"to":...,"bucket_seconds":3600,"points":[...]},"bounds":{"min":...,"max":...,"known_secs":...,"empty_secs":...,"unknown_secs":...},"unknown_timestamps":[...]}]}`.

Prices are interval closing states, with exact starting state and a final partial-edge sample. The terminal sample is the state immediately before `to`. Extrema are from exact transitions, not sampled prices: transient changes inside an hour still affect bounds. Null prices at `unknown_timestamps` mean unknown; other null samples mean a known empty scope. Item IDs are deduplicated and response order is stable. The existing per-item chart GET, adaptive 481-point behavior, and chart wire types remain unchanged.

Requests require 1–20 positive item IDs, at most 90 days, a valid non-future unsigned-DateTime range, at most 10,000 total samples across items/qualities, and a body at most 16 KiB. These are checked before a query. Per-item cached batch entries use a separate `floor_window` namespace with exact scope, range, quality, and cadence; In-process TTL is 60 seconds; POST responses use HTTP `no-store` because a URL alone cannot identify the body. Batch cache misses have a shared four-query limit and a 15-second end-to-end timeout. Explicit historical listing requests retain the bounded stats cache. Historical ClickHouse reads throw on the 2-million-result-row, 512-MiB, or 10-second query limit rather than silently returning partial data. Whole-market 30/90-day scans still require workload validation at production volume; resource-limit failures are unavailable responses, not complete history.

## Local validation and release evidence

The opt-in `listing_history_smoke` target refuses non-loopback URLs and database names without the `ultros_t12_` prefix. Fixtures cover matching/reprice/ambiguity/receipt dedup, seed/update exclusion, all four windows, missing receipt evidence, pre-window carry-forward, empty and unknown worlds, exact cross-world floor extrema versus sampled values, hourly/daily batches, request bounds, and missing/zero/merged stock snapshots. The HTTP probe is `BASE_URL=http://127.0.0.1:18412 npm --prefix integration run test:listing-history-api`, against the seeded own-build server. Unit tests cover matching boundaries, age origin, pending status, old-wire defaults, scope/window cache isolation, and request limits.

Run against a disposable database:

```sh
ULTROS_CH_INTEGRATION=1 CLICKHOUSE_URL=http://127.0.0.1:28412 \
CLICKHOUSE_DATABASE=ultros_t12_smoke CLICKHOUSE_USER=default CLICKHOUSE_PASSWORD= \
cargo test -p ultros-clickhouse --test listing_history_smoke --test floor_history_smoke
```

Production coverage has **not** been measured. The schema proposes 365-day listing/receipt retention and no floor TTL, but CREATE IF NOT EXISTS does not alter existing deployed retention. Before release, verify at least seven days of retained listing history, actual coverage of each requested window, writer/drop rates including receipt evidence, seed continuity, floor consistency, query latency/resource usage for world/DC/region windows, and deployed retention. Missing receipt history cannot be recovered from sale timestamps. Do not close #1342 or describe production history as mature without that evidence. No production reads, writes, deployment, or retention alteration are part of this task.
