# Windowed listing history (#1342)

The current-only `GET /api/v1/listing_stats/{world|dc|region}` retains its response and query. An explicit `?window=1`, `7`, `30`, or `90` adds an optional `window` object to each item/HQ row. Historical-only keys are included even when their current board is empty. Unsupported windows return 400. Current-only requests use cache window 0; all four historical windows use distinct slots, with the existing single-flight/stale/failure cache behavior. A failed history query is unavailable, never successful zero history.

## Meanings and coverage

- `from`/`to` define a half-open observation window. Additions and removals count non-snapshot events; updates are neither. These are observed changes, not sale counts. Catch-up and manual changes count toward turnover but cannot be matched as websocket sales.
- `listing_coverage` and `matches.receipt_coverage` report the first/last retained observations read for that window, including up to 600 seconds of pre-window matching context. `observed_span_secs` intersects that span with the requested window. **Continuity is unverified**: quiet periods and ingestion gaps cannot be distinguished. A nominal 90-day request is not evidence of 90 days of coverage.
- The new `sale_receipts` table records socket receipt time only for sales newly inserted by websocket ingest. It has independent writer/drop metrics and a 365-day TTL. Backfill and catch-up do not manufacture receipts. Existing `sales` rows and the current sale API are unchanged. `sales_without_receipt` counts sales whose game timestamp falls in the window but whose retained receipt evidence is absent. `received_sales` instead counts receipts observed in the window; these are different cohorts.
- Matching requires world/item/HQ/per-unit price/stack quantity, receipt within ±300 seconds of a websocket removal observation, a game sale time no more than 24 hours old at receipt, and sale time at or after `reviewed_at` (60-second clock skew allowed). Same-retainer re-adds/updates of the stack within ±300 seconds exclude reprices. Each sale must have exactly one removal candidate and each removal exactly one sale candidate, including candidates from other retainers and just outside the requested window. Ambiguity is excluded before attribution; no sale is reused. This is deliberately stricter than assigning one of several same-retainer candidates.
- `age_origin` is `last_review_time`: time from the listing's last retainer review to the game's sale timestamp. It is not original listing creation time. A tolerated negative skew clamps to zero; missing/invalid review times are excluded. The median is computed across matched observations across all worlds, not averaged from per-world medians.
- The newest 600 seconds of removals remain `pending`, allowing full ±300-second sale context and the sale's full ±300-second competing-removal context. `settled_through_unix` identifies the frontier. `matched`, `ambiguous`, `repriced`, `unmatched`, and `pending` partition observed non-snapshot removals; none asserts every removal was a sale.
- `days_of_stock = aggregate alive units * window days / aggregate sold units` uses the matching `sale_stats_window`. Every selected world must have both alive and sale snapshots for that item/HQ. Missing snapshots yield `stock_status=unavailable`; known zero sale units yield `no_sales` and null duration; positive sale units yield `estimated` (including a zero duration for known zero stock). Snapshot age may not exceed its existing scheduler cadence: 15 minutes for alive/1-day sales, 1 hour for 7-day sales, and 6 hours for 30/90-day sales. The exact age cutoff is inclusive; snapshots computed after the response's `to` are unavailable. Positive sale snapshots must also have their last sale in `[from,to)`, because a group that disappears from a rolling refresh can leave an old positive row behind. A delayed refresh can therefore temporarily yield `unavailable`; absence or staleness never invents a zero-sales snapshot.
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

## Release-evidence inventory (2026-09-09)

| Evidence | What can establish it | Current access or history blocker |
| --- | --- | --- |
| Deployed listing/floor retention and seed completion | Read-only `SHOW CREATE TABLE`, `system.parts` metadata, and the small `_listing_events_seed` marker | This task has no configured production ClickHouse URL/user, workspace `.env`, ClickHouse client configuration, or monitoring connector. Repository schema is intended DDL, not deployed evidence. An SSH session environment does not identify or authorize a database target. |
| Existing listing/floor writer health | Already-collected monitoring rates for `ultros_clickhouse_writer_written_rows_total`, `dropped_rows_total`, `flush_failures_total`, plus seed/bus-lag metrics | No configured production Prometheus/Grafana URL or read-only access. A current counter snapshot cannot establish historical absence of drops or restarts. |
| Existing floor consistency | A bounded, explicitly selected item/world comparison between retained floor transitions and the alive snapshot at a common observation time | No configured production database access. Local exact-floor fixtures passed; that does not prove live ingest consistency. |
| Durable receipt writer health and receipt coverage | Deployment containing the `sale_receipts` writer, followed by observed receipts and its `table="sale_receipts"` writer metrics | This PR has not been deployed by this task. Old sales/backfill have no durable receipt evidence. Creating a table or inserting synthetic old timestamps cannot establish historical receipt coverage. |
| Seven-day listing continuity and 30/90-day maturity | Retained observations plus monitoring/log history covering the interval and seed/restart boundaries | First/last observations only establish a span. Quiet intervals, losses before metrics existed, and missing receipts remain unknown. Elapsed live history is necessary; a nominal request window is insufficient. |
| Whole-market world/DC/region capacity | Owned synthetic workloads first; later an approved representative production-volume replica or existing bounded query telemetry | Synthetic scale can measure resource limits and correctness, but actual deployed volumes, hardware, and concurrency are not available here. Do not run uncached whole-market 30/90-day production queries to fill this gap. |

The local workload harness is an ignored test, so ordinary CI does **not** claim to exercise it. It refuses non-loopback endpoints, requires a fresh `ultros_t12_workload_*` database, and accepts only its two bounded fixture sizes. It invokes the real alive/history/stock query functions and asserts removal classification, 30/90-day turnover counts, complete floor baselines, and stock availability on successful responses. Resource-limit errors remain failures to provide history, not passing market-data results.

To reproduce on a **dedicated disposable** ClickHouse server, enable `query_log` and the user's `log_queries=1`; use `max_threads=1`, a 1-GiB default query ceiling and a 16-GiB server ceiling to match this run. Historical SQL retains its own stricter 512-MiB/10-second/2-million-result-row settings. Give the owned native client the same local credentials as the HTTP client. The harness performs `CREATE TABLE`/inserts and `SYSTEM FLUSH LOGS` only in this disposable environment; never point it at a tunnel to a deployment. It does not start the app, ingest, or rollup workers.

```sh
export CH_WORKLOAD_BIN=/path/to/owned/clickhouse
flock /tmp/ultros-analyzer-heavy-checks.lock bash <<'WORKLOAD'
set -euo pipefail
export CLICKHOUSE_URL=http://127.0.0.1:28412 CLICKHOUSE_USER=default CLICKHOUSE_PASSWORD=
export CARGO_BUILD_JOBS=8 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
for build_profile in test server-release; do
 for fixture_rows in 24000 240000; do
  export CLICKHOUSE_DATABASE="ultros_t12_workload_$(date +%s%N)_${fixture_rows}"
  "$CH_WORKLOAD_BIN" client --host 127.0.0.1 --port 29412 \
    --query "CREATE DATABASE $CLICKHOUSE_DATABASE"
  T12_ROWS_PER_WORLD="$fixture_rows" cargo test -p ultros-clickhouse \
    --test listing_history_workload --profile "$build_profile" -- --ignored --nocapture \
    >"/tmp/${CLICKHOUSE_DATABASE}.log" 2>&1
 done
done
WORKLOAD
```

Each dataset has 32 synthetic worlds, 1,000 item IDs, both qualities, 2,000 output keys, 64,000 alive boards, and two sale-stat snapshots per board. Listing triplets (add/update/remove) span 90 days; 20% of cycles concentrate on one hot item, 10% lack receipt evidence, and a small receipt retry population exercises deduplication. Per-cycle retainer IDs prevent unrelated cycles from masquerading as same-retainer reprices. Floor transitions include explicit empties and a complete 91-day baseline. A synthetic baseline proves fixture semantics, not historical ingest maturity.

For each tier, the 1-world, 8-world DC and 32-world region queries run for 30 and 90 days, three times each. `CASE` lines report full Rust alive/history/stock duration and success or explicit resource-limit error; `QUERY` lines report server duration, scanned/result rows, peak query memory, and exception code from the owned server's query log. `PROCESS` records Linux process high-water RSS cumulatively, not per-query allocation. The first run follows insertion and is **not** a cold-disk benchmark; no shared caches are flushed. The final harness applies the same 12-second deadline as `StatsCache` and fails if local processing delays an error beyond 14 seconds. Both debug and `server-release` runs represent serial cache misses, excluding HTTP serialization and cache hits, not production p95 or concurrency evidence.

## Regional capacity fix and current measurements

The regional row-limit failure below is resolved at the measured fixture volume. The loader first reads scoped per-item row counts, then fetches bounded item batches instead of whole-region raw results. Each item retains **all selected worlds**, both qualities, the full matching context, and every floor transition plus its pre-window baselines. Exact scope floors and pooled age medians are calculated once per item; per-world summaries are never combined. The count query also discovers baseline-only keys, and the final missing-receipt aggregation retains sales-only keys.

Batches target at most 500,000 rows per raw query and 128 items, with at most two batches in flight. A hot item stays whole and isolated even when it exceeds the target. Counts guide planning only: concurrent inserts cannot bypass the unchanged SQL limit. Every query still throws at 2,000,000 result rows, 512 MiB, or 10 seconds; the full cache loader retains its 12-second deadline. A failed batch discards the entire load, including earlier completed items. Compact event projections avoid unused identity strings, and hash-based receipt deduplication preserves the earliest actual receipt. Cooperative yields and dropping the bounded future stream preserve deadline cancellation without detached batch tasks. This changes no schema, retention, response semantics, or cache policy.

All **36 optimized workload calls** now return complete history, with exact turnover counts, all 2,000 item/quality keys, complete fixture floor coverage and stock estimates. The same matrix also runs in debug mode to check cancellation; an explicit debug timeout is unavailable and is not counted as successful history. Each range below is three serial calls. Query peak is an individual query, not the combined memory of two in-flight queries.

| Profile | Events/world/90d | Scope | Window | Seconds (min–max) | Outcome | Query peak MiB | Max rows returned by one query |
| --- | ---: | --- | ---: | ---: | --- | ---: | ---: |
| test | 24,000 | world (1) | 30d | 0.202–0.216 | complete | 4.34 | 2,210 |
| test | 24,000 | world (1) | 90d | 0.327–0.385 | complete | 4.68 | 7,248 |
| test | 24,000 | dc (8) | 30d | 0.625–0.651 | complete | 6.94 | 17,680 |
| test | 24,000 | dc (8) | 90d | 1.607–1.621 | complete | 8.38 | 57,984 |
| test | 24,000 | region (32) | 30d | 2.157–2.189 | complete | 12.79 | 70,720 |
| test | 24,000 | region (32) | 90d | 5.787–5.857 | complete | 38.56 | 231,936 |
| test | 240,000 | world (1) | 30d | 0.786–0.834 | complete | 16.34 | 23,947 |
| test | 240,000 | world (1) | 90d | 2.127–2.145 | complete | 40.95 | 72,480 |
| test | 240,000 | dc (8) | 30d | 4.796–4.875 | complete | 25.80 | 191,576 |
| test | 240,000 | dc (8) | 90d | 12.135–12.175 | deadline | 128.08 | 499,200 |
| test | 240,000 | region (32) | 30d | 12.005–12.008 | deadline | 92.71 | 511,712 |
| test | 240,000 | region (32) | 90d | 12.068–12.147 | deadline | 163.45 | 1,536,000 |
| server-release | 24,000 | world (1) | 30d | 0.119–0.148 | complete | 4.33 | 2,210 |
| server-release | 24,000 | world (1) | 90d | 0.152–0.213 | complete | 4.68 | 7,248 |
| server-release | 24,000 | dc (8) | 30d | 0.184–0.192 | complete | 6.94 | 17,680 |
| server-release | 24,000 | dc (8) | 90d | 0.330–0.344 | complete | 8.38 | 57,984 |
| server-release | 24,000 | region (32) | 30d | 0.441–0.465 | complete | 12.79 | 70,720 |
| server-release | 24,000 | region (32) | 90d | 1.023–1.046 | complete | 38.56 | 231,936 |
| server-release | 240,000 | world (1) | 30d | 0.282–0.299 | complete | 16.34 | 23,947 |
| server-release | 240,000 | world (1) | 90d | 0.533–0.565 | complete | 40.94 | 72,480 |
| server-release | 240,000 | dc (8) | 30d | 0.988–1.039 | complete | 25.80 | 191,576 |
| server-release | 240,000 | dc (8) | 90d | 2.373–2.489 | complete | 128.08 | 499,200 |
| server-release | 240,000 | region (32) | 30d | 3.064–3.536 | complete | 92.71 | 511,712 |
| server-release | 240,000 | region (32) | 90d | 9.171–9.421 | complete | 175.41 | 1,536,000 |

A separate probe inserts 2,000,001 events for one later item after the successful workload matrix. Both profiles return code 396 without returning a partial map: test 8.300s, server-release 1.122s. This intentional single-item safety failure is separate from the regional capacity results. A 270-item database regression spans multiple batches and verifies a pooled median of 20 seconds from world ages [10,20] and [100], plus exact scope extrema that exclude a 999-gil world spike hidden by a cheaper world. Existing matching, reprice, pending, coverage and boundary tests still apply.

Pre-deduplication machine-readable evidence is [windowed-listing-capacity-2026-09-09.json](windowed-listing-capacity-2026-09-09.json), including all timings, scanned/result rows, query memory and safety probes. Cumulative application peak RSS (KiB): `test/24000` = 56,796, `test/240000` = 274,080, `server-release/24000` = 60,008, `server-release/240000` = 322,228. This resolves the demonstrated implementation gap without raising limits or narrowing scope. It does not establish production maturity, deployed scale/skew, cold-disk behavior or concurrent-request capacity; the release-evidence inventory remains applicable.

## Historical baseline: initial whole-scope workload

Measured on 2026-09-09 with ClickHouse **25.4.13.22**, an AMD Ryzen Threadripper 3970X shared host, one ClickHouse query thread, and unoptimized Rust test code. Each row below represents three serial runs. These initial measurements apply the production **SQL** limits but time the complete loader without the cache deadline; they identify which cases need the separate 12-second cancellation check. They are not successful HTTP response times.

| Events per world over 90 days | Scope | Window | Full loader seconds (min–max) | Largest query peak MiB | Observed result |
| ---: | --- | ---: | ---: | ---: | --- |
| 24,000 | World (1) | 30d | 0.147–0.155 | 4.77 | Complete, counts verified |
| 24,000 | World (1) | 90d | 0.304–0.362 | 6.67 | Complete, counts verified |
| 24,000 | DC (8) | 30d | 0.650–0.667 | 16.64 | Complete, counts verified |
| 24,000 | DC (8) | 90d | 1.706–1.736 | 26.27 | Complete, counts verified |
| 24,000 | Region (32) | 30d | 2.372–2.402 | 76.72 | Complete, counts verified |
| 24,000 | Region (32) | 90d | 6.581–6.646 | 104.25 | Complete, counts verified |
| 240,000 | World (1) | 30d | 0.831–0.877 | 16.31 | Complete, counts verified |
| 240,000 | World (1) | 90d | 2.198–2.255 | 40.89 | Complete, counts verified |
| 240,000 | DC (8) | 30d | 5.412–5.475 | 105.10 | Complete, counts verified |
| 240,000 | DC (8) | 90d | 15.713–15.896 | 358.74 | Correct full load, **over cache deadline** |
| 240,000 | Region (32) | 30d | 7.829–7.867 | 10.90 | **Unavailable**, result limit (396) |
| 240,000 | Region (32) | 90d | 7.813–7.830 | 10.83 | **Unavailable**, result limit (396) |

The smaller dataset has 768,000 listing events, 232,681 receipt rows, 256,000 sales and 320,000 floor rows; the larger has 7,680,000 / 2,326,812 / 2,560,000 / 2,624,000 respectively. Both have the same 64,000 alive boards and 128,000 sale-stat snapshots. The larger DC/90-day request returned 1,920,000 event rows; its first event query scanned 6,639,616 rows in 4,455 ms, and its floor query returned 656,000 rows at a 376,169,054-byte peak. Thus SQL duration/memory alone does not measure application readiness. The process high-water RSS reached 478,080 KiB during the larger matrix, separate from ClickHouse's query memory cap.

The larger region/30-day interval contains approximately 2.56 million event rows and the 90-day interval 7.68 million. All six regional attempts raised `TOO_MANY_ROWS_OR_BYTES` (code 396) at the two-million-result-row limit. The Rust loader returned an error and no history map; it did not return truncated statistics. The existing web error mapping produces HTTP 500 for the initial ClickHouse error or `StatsCache` deadline; coalesced/follow-up cold requests during the cache failure backoff receive HTTP 503. A previously cached response may instead be served with the explicit stale disposition. Those status mappings are established by code/unit tests; this matrix invokes query functions and does not itself issue HTTP requests.

This historical result established the implementation capacity gap addressed by the item partitioning measured above. It remains recorded as the before-change baseline, not as a current regional failure or successful historical data response. Production-volume/skew and concurrency evidence still requires a representative isolated host.

The first deadline-aware run exposed a separate implementation problem: the large DC/90-day load returned its 12-second timeout after **15.959 seconds**, failing the harness's 14-second responsiveness assertion. Synchronous receipt deduplication, grouping and per-item calculations prevented the runtime from polling the deadline. `listing_history::window` now yields every 4,096 rows during deduplication/grouping and between item calculations. It preserves the same matching and floor semantics; it does not turn a timed-out partial result into successful history or raise a resource limit. The failed run remains recorded at `/tmp/ultros-t12-workload-starved-deadline.log` on the validation host.

## Historical baseline: whole-scope deadline and optimized-profile results

At commit `12a3160`, before item partitioning, the harness passed its safety assertions for both fixture tiers in both profiles with the SQL limits, 12-second loader deadline, count/coverage assertions, and responsiveness assertions unchanged. These are 72 measured calls: complete history and explicit unavailable outcomes are distinguished below. Each range is three serial runs; query memory is the maximum individual query peak, not application RSS.

| Profile | Events/world/90d | Scope | Window | Seconds (min–max) | Outcome | Query peak MiB | Max rows read by one query |
| --- | ---: | --- | ---: | ---: | --- | ---: | ---: |
| test | 24,000 | world (1) | 30d | 0.164–0.172 | complete | 4.77 | 408,256 |
| test | 24,000 | world (1) | 90d | 0.312–0.369 | complete | 6.64 | 686,080 |
| test | 24,000 | dc (8) | 30d | 0.672–0.685 | complete | 16.64 | 408,256 |
| test | 24,000 | dc (8) | 90d | 1.745–1.778 | complete | 26.26 | 702,464 |
| test | 24,000 | region (32) | 30d | 2.497–2.517 | complete | 76.72 | 408,256 |
| test | 24,000 | region (32) | 90d | 6.663–6.753 | complete | 102.64 | 768,000 |
| test | 240,000 | world (1) | 30d | 0.878–0.918 | complete | 16.33 | 3,056,032 |
| test | 240,000 | world (1) | 90d | 2.286–2.347 | complete | 40.92 | 6,311,936 |
| test | 240,000 | dc (8) | 30d | 5.552–5.669 | complete | 105.89 | 3,170,720 |
| test | 240,000 | dc (8) | 90d | 12.465–12.650 | deadline | 358.20 | 6,639,616 |
| test | 240,000 | region (32) | 30d | 8.012–8.038 | result_limit_396 | 11.20 | 2,795,232 |
| test | 240,000 | region (32) | 90d | 7.840–7.951 | result_limit_396 | 11.63 | 2,027,680 |
| server-release | 24,000 | world (1) | 30d | 0.067–0.077 | complete | 6.68 | 408,288 |
| server-release | 24,000 | world (1) | 90d | 0.106–0.159 | complete | 6.64 | 686,080 |
| server-release | 24,000 | dc (8) | 30d | 0.174–0.184 | complete | 16.64 | 408,288 |
| server-release | 24,000 | dc (8) | 90d | 0.362–0.370 | complete | 26.26 | 702,464 |
| server-release | 24,000 | region (32) | 30d | 0.528–0.543 | complete | 76.72 | 408,288 |
| server-release | 24,000 | region (32) | 90d | 1.204–1.237 | complete | 103.76 | 768,000 |
| server-release | 240,000 | world (1) | 30d | 0.285–0.315 | complete | 16.34 | 3,056,032 |
| server-release | 240,000 | world (1) | 90d | 0.610–0.632 | complete | 40.93 | 6,311,936 |
| server-release | 240,000 | dc (8) | 30d | 1.105–1.121 | complete | 107.43 | 3,170,720 |
| server-release | 240,000 | dc (8) | 90d | 2.901–2.956 | complete | 357.26 | 6,639,616 |
| server-release | 240,000 | region (32) | 30d | 1.058–1.087 | result_limit_396 | 11.20 | 2,795,264 |
| server-release | 240,000 | region (32) | 90d | 1.068–1.083 | result_limit_396 | 11.62 | 2,027,424 |

The cooperative-yield change was verified by the same deadline regression that failed at 15.959 seconds before the fix. This is cooperative cancellation evidence for the measured workload, not a hard CPU-preemption guarantee for arbitrary single-item skew. The optimized run uses the repository `server-release` profile; the test binary uses the default allocator, while the web server enables jemalloc. Concurrent requests, a cold disk, actual deployed data skew, and the deployed server configuration still require an approved isolated representative environment.

Historical machine-readable fixtures, all three timings, outcomes, memory and scanned-row figures are checked in at [windowed-listing-workload-2026-09-09.json](windowed-listing-workload-2026-09-09.json). Process high-water RSS (KiB): `test/24000` = 210,656, `test/240000` = 479,272, `server-release/24000` = 216,908, `server-release/240000` = 473,276.

## Bounded release-validation procedure

An operator with an already-authorized read-only target should record the deployment revision, database identifier, server version, observation time, and scope/item IDs alongside every result. Do not run the application, migrations, backfill, seed helpers, refreshes, or sweeps as an inspection tool. Do not change TTLs. A timeout or row-read limit is an **incomplete check**, not a reason to remove its bound.

1. Inspect metadata only: `SHOW CREATE TABLE <db>.listing_events`, `<db>.floor_changes`, and (if present) `<db>.sale_receipts`. Record actual TTL expressions and engines. Inspect `system.parts` for only these tables and `active=1`, grouped by table/partition, with `max_execution_time=2`, `max_rows_to_read=100000`, `read_overflow_mode='throw'`, and `max_memory_usage=67108864`. Partition names/row totals establish coarse retained volume, not exact scope coverage or continuity. Read `SELECT seeded_at, rows_streamed FROM <db>._listing_events_seed ORDER BY seeded_at DESC LIMIT 1` with the same bounds. A completion marker does not rule out later gaps.
2. From existing monitoring history, inspect `increase(ultros_clickhouse_writer_written_rows_total{table=~"listing_events|floor_changes|sale_receipts"}[1h])`, equivalent drop/flush-failure increases, and `max_over_time(ultros_clickhouse_writer_queued_rows[1h])`, split by instance/table. Also inspect seed failures, analyzer bus lag, and per-world ingest staleness. Record scrape gaps and process resets; absent series are unknown, not zero. Review existing seven-day history before claiming seven-day health. The receipt label requires the new writer to have actually run.
3. Choose at most two item IDs and one world already identified in existing telemetry. On each relevant raw table, read `count()`, `min(timestamp)`, and `max(timestamp)` for that exact item/world and a one-hour interval, with `max_execution_time=2`, `max_rows_to_read=250000`, `read_overflow_mode='throw'`, `max_result_rows=100`, `result_overflow_mode='throw'`, and `max_memory_usage=67108864`. Timestamp columns are `event_time` for listing/floor events and `received_at` for receipts. Stop on a limit error. These are samples, not proof of whole-world coverage. Do not infer receipt time from `sold_date` or `inserted_at`.
4. For those same item/world/HQ keys, capture `computed_at` and `floor_alive` from `listing_alive FINAL`. Using that timestamp as the common upper bound, inspect the corresponding `floor_changes` last state with the same two-second/250,000-row bounds and the shared conservative tie rule `argMax(price_per_unit, tuple(event_time, -toInt64(price_per_unit)))`. Record explicit empty states and unknown baselines, plus rollup lag and non-atomic read timing. A mismatch needs investigation; do not refresh either source to make it disappear.
5. Establish receipt deployment time and the earliest retained **actual** receipt observation from existing telemetry or bounded item/world reads. Report unsupported portions of each requested window. Wait for the required live interval; no reconstruction procedure can fill absent receipt evidence.
6. For capacity, reproduce the owned-fixture checks below on a representative isolated host. Obtain actual volume/skew and concurrency estimates from existing partition/query metadata or approved exports, then use an isolated replica or synthetic equivalent. Record cold-cache and repeated latency separately, scanned/result rows, ClickHouse peak memory, application RSS, timeout/limit errors, and concurrent scope requests. Whole-market production scans and production cache misses remain outside this procedure. Keep #1342 open until substantive production coverage and capacity requirements are met. Merging this API implementation does not establish deployed history maturity: continuity remains unverified, missing receipts and floor baselines remain explicit, and no frontend consumer promises complete historical coverage.
