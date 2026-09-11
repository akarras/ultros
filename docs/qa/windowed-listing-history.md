# Windowed listing history (#1342)

An explicit `?window=1`, `7`, `30`, or `90` on `GET /api/v1/listing_stats/{world|dc|region}` reads exact observed history from a committed background snapshot. The current-only response and bounded floor-history POST endpoint remain unchanged. A first or expired snapshot can return 503 while the background worker refreshes it. A missing or failed generation never means zero history.

The [implementation contract](../superpowers/specs/2026-09-09-listing-stats-window-rollup.md) describes scheduling, atomic publication and resource limits. Snapshots preserve #1397's definitions: retry-deduplicated additions/removals; conservative one-to-one matching; exact pooled median ages from last retainer review; pending removals with insufficient context; exact scope floor extrema and the intersection of known/empty intervals. Unknown baselines remain unknown. Every generation uses one fixed `[from,to)` and exact world set, and the API returns that endpoint.

`listing_coverage` and receipt coverage describe retained observations, not continuity. `continuity_verified` remains false. No backfilled game timestamp substitutes for actual receipt evidence. Missing-receipt counts reconcile authoritative sales within bounded item batches, rather than subtracting stale rollup estimates. Stock remains the existing cadence-checked alive/sales ratio with explicit unavailable/no-sales/estimated states.

## Local correctness and API checks

Use a fresh disposable database with the required `ultros_t12_` prefix on a loopback ClickHouse server. Do not point tests at a production tunnel.

```sh
ULTROS_CH_INTEGRATION=1 CLICKHOUSE_URL=http://127.0.0.1:28412 \
CLICKHOUSE_DATABASE=ultros_t12_smoke CLICKHOUSE_USER=default CLICKHOUSE_PASSWORD= \
cargo test -p ultros-clickhouse --test listing_history_smoke \
  --test listing_snapshots_smoke --test floor_history_smoke
```

The snapshot suite covers late receipts and competing removals, retry duplicates vs genuinely distinct listings, committed-empty vs absent, incomplete/expired/future publication, disjoint world-empty intervals, missing worlds, pre-common-baseline minima, and sales-only key expiry. Existing tests retain pooled medians, floor sampling, stock boundaries and the removal partition.

Run `./check_ci.sh`, fresh `cargo leptos build`, the populated `integration/listing-history-api.cjs` probe, and the strict relevant route/browser suite. The HTTP probe accepts initial 503 warming responses for a bounded interval and still requires populated successful history, cache separation, unchanged current-wire compatibility, floor request bounds and POST no-store. Failures are not successful empty snapshots.

## Representative producer and cached-read capacity

The ignored workload now measures the background producer separately from the request read. Default: one 32-world/90-day case. Its bounded fixture sizes are 24,000 or 240,000 events per world (768,000 / 7,680,000 total), with receipts, authoritative sales, floor transitions and stock snapshots. Use the optimized profile; select the full matrix only when its additional evidence is needed.

```sh
ULTROS_CH_INTEGRATION=1 CLICKHOUSE_URL=http://127.0.0.1:28412 \
CLICKHOUSE_DATABASE=ultros_t12_workload_unique CLICKHOUSE_USER=default CLICKHOUSE_PASSWORD= \
T12_ROWS_PER_WORLD=240000 cargo test -p ultros-clickhouse \
  --test listing_history_workload --profile server-release -- --ignored --nocapture
```

The dedicated disposable server must enable query_log/log_queries; the workload flushes only its own server's query log. Record actual server configuration and memory alongside query peaks. `GENERATION` reports producer duration and complete output keys. `CASE` reports the subsequent alive/snapshot/stock read under the unchanged 12-second deadline. The separate `T12_RUN_CAP_GUARD=1` opt-in on the large fixture adds a later 2,000,001-event item, which must fail at the row cap without publishing a partial replacement. Ordinary representative producer/read runs do not insert those additional rows. `T12_FULL_MATRIX=1` expands to world/DC/region, 30/90 days and three repetitions. A failed run remains failed evidence; do not weaken assertions or increase application limits to hide it.

## Evidence provenance and remaining release work

The previous on-demand implementation and its measured capacity are preserved at `795e8b09` and the adjacent `windowed-listing-*.json` files. They are historical measurements, not validation of this background producer. The initial per-world rollup proposal and its author-reported production observations are preserved at `6e47936b`; those observations motivated the rewrite but are not independent measurements of the repaired branch.

Record the repaired head's full CI, real database tests, fresh SSR/WASM build, API/browser probes and representative producer/read result before merge. No result has been asserted in this document in advance of execution.

Keep #1342 open for actual deployed throughput/skew, concurrent replica load, cold and repeated request latency, generation failure rates, fresh-snapshot availability, retention/seed continuity, writer/drop health and floor consistency. Receipt maturity needs elapsed actual observations; synthetic timestamps cannot fill missing history. Merging this implementation does not establish production capacity or mature 7/30/90-day coverage.

## Production observation, 2026-09-11 04:00 UTC

Measured against the deployed image at main `55b422cb` (both #1397 and #1416 live) on the
production ClickHouse (`ultros` database), the production Postgres `active_listing` table,
the app's Prometheus counters and the public API. These are observations of one point in
time, not an SLA.

| Check | Observed |
| --- | --- |
| Listing-event history | `listing_events` starts 2026-09-07 18:31 UTC (11.45M-row seed, marker `_listing_events_seed`), 22.2M rows, ~3.4M websocket + ~60k catch-up rows/day since. TTL 365 days confirmed on the table. |
| Receipt history | `sale_receipts` starts 2026-09-10 04:01 UTC (the #1397 deploy). 1.37M receipts in the last 24h against 1.38M sales inserted: receipts cover sales. TTL 365 days. |
| Writer health | `ultros_clickhouse_writer_dropped_rows_total`, `..._flush_failures_total`, `ultros_listing_events_seed_failures_total`, `ultros_floor_changes_bulk_failures_total` have never incremented (no series). `..._written_rows_total` ≈ 7.46M rows/24h, queue depth 0. |
| Seed continuity | One upstream outage: 2026-09-08 06:00–19:00 UTC listing events fell to 3–30% of baseline and `sales` fell in lockstep, with worlds recovering in stages (7 → 49 → 125). Not an app or writer fault; the app did not restart. It breaks 7-day continuity for every key until 2026-09-15 19:00 UTC. `continuity_verified` is never set by the code, so this remains an out-of-band check. |
| Snapshot producer | Cold `?window=` requests return 503 and the background worker publishes the generation in 5–8 s (Gilgamesh 1/7/30/90 days: 10k–21k keys; Aether 7 days: 22.5k keys). Warm reads return 200 in 0.85–1.15 s for 15–20 MB bodies. Both snapshot tables carry the 2-day TTL. |
| `listing_alive` vs Postgres | Gilgamesh: `alive_count` equals the Postgres listing count for 15,356 of 15,532 keys; every residual is a key that changed after the last 15-minute rollup (max `computed_at` 03:51:55). |
| Floor baseline | **Defect.** 1,283,843 of 1,910,023 live (world, item, hq) keys (67%) have no `floor_changes` row at all, so their floor is "unknown" for every window, and at datacenter scope one unknown world makes the whole key unknown (17,314 of 22,004 Aether keys). Cause: the analyzer snapshot restores the cheapest map before the boot rebuild, so the boot diff runs against the restored map and never writes the per-key anchor `floor_diff` was designed to write; `reason='resync'` totals 60k rows across 15 boots. Fixed by diffing the boot rebuild against ClickHouse's latest floor per key instead. |
| Floor tie-break | 812 Gilgamesh keys have two different non-zero prices in the same latest second (a `listing` and a `refill` row). The reader's `argMax(price, (event_time, -price))` picks the cheaper one, which matched Postgres in the sampled cases. |

Observed coverage at that time: 1 day of receipt-matched sale ages, 3.4 days of listing
turnover and floor history, with one 13-hour upstream gap inside it. 7-day windows are not
mature before 2026-09-15 19:00 UTC (listings) and 2026-09-17 (receipts); 30/90-day windows
report their observed span and remain incomplete by construction until that much history has
elapsed.

## Production observation, 2026-09-11 15:00 UTC (after #1453)

Measured against the deployed image at main `0a18f436` (#1453 live; container restarted
13:36 UTC) on the production ClickHouse and the public API.

| Check | Observed |
| --- | --- |
| Boot anchor | The first #1453 boot wrote 1,285,184 `reason='resync'` rows in the 09:00 UTC hour, against ~5–9k per hour for the periodic drift resync. |
| Live-key floor coverage | 1,909,035 of 1,909,035 live (world, item, hq) keys (from `listing_alive`, `alive_count > 0`) now have a `floor_changes` row, up from 33%. |
| World scope | Gilgamesh `?window=7`: 16,303 of 17,492 keys (93%) report a `floor_min`; 16,862 have any known interval. |
| Datacenter scope | Aether `?window=7`: 9,581 of 22,538 keys (42.5%) report a `floor_min`, up from ~21%, and only 20 keys report any `floor_empty_secs`. |

The datacenter residual is the second half of the same defect. `bounds` only counts an
interval as known when every world in the scope has a floor row in effect, and a world that
has never listed an item has no `floor_changes` row at all, so it reads as unknown for the
whole window and makes the datacenter key unknown with it. The boot resync does prove such
keys empty (it diffs Postgres's complete listing set against ClickHouse's latest floor per
key), but nothing recorded *when* that proof happened. Shadow (four new EU worlds with a few
hundred listings each) makes the Europe region scope unknown for almost every key for the
same reason.

### Floor anchors

`floor_anchors (anchored_at, world_id)` records one row per world each time the analyzer's
resync completes against a ClickHouse baseline and its bulk insert succeeds. A resync that
fell back to the in-memory map (ClickHouse unreachable or slow) records nothing, because it
proves nothing about absent keys. Readers take the earliest anchor per world and seed a
synthetic empty row at that instant for any world with no observation at or before it; a real
row at the same second wins. Before a world's earliest anchor nothing is claimed, so a window
that starts before the anchor still reports that stretch as unknown.

This is what "aggregate only over active servers" means here: a world with no listings is a
known-empty board that contributes nothing to the scope minimum, not an unknown one that
voids it. The world set itself is still Universalis's (`regions_and_datacenters.rs`); the
Cloud DC test worlds sit in their own `NA-Cloud-DC` region, so they never enter a North
America scope. Expect datacenter `floor_min` coverage to converge on world coverage one
anchor after this deploys, and the Aether count above is the before-figure to compare it to.
