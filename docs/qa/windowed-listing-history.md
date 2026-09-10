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
