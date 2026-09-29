# Exact windowed listing snapshots (#1342)

The HTTP listing-statistics endpoint reads a committed snapshot for the exact sorted world set and 1/7/30/90-day window. Current-only requests remain unchanged. The implementation replaces the proposed per-world summary in PR #1416 because that summary cannot reconstruct exact cross-world floor extrema/empty durations or an exact pooled median, and a one-way outcome cursor cannot reconcile late writer evidence.

## Authoritative computation

The background producer retains #1397's exact full-scope reducer. It deduplicates complete stored listing rows before projection; preserves every requested world and both qualities within each item batch; retains matching context; computes exact pooled ages and the common floor timeline. Every recomputation sees newly arrived historical evidence, including a delayed competing removal that revokes an earlier match.

Sales-only item keys participate in batch planning. Missing-receipt reconciliation reads authoritative `sales FINAL` and receipts inside the same bounded item batches, replacing the previous whole-scope anti-join that exhausted query memory. Receipts observed at or after the generation endpoint cannot cancel its missing-receipt count, even if their game sale time is earlier. It does not subtract unrelated or stale sale-rollup counts. Raw planning counts are conservative; actual queries retain their limits.

The event, receipt, floor and sale tables are not changed by this repair. No one-shot removal-outcome cursor or lossy per-world listing summary is installed.

## Atomic publication

`listing_window_snapshot_rows` stores immutable `(scope, window, generation, item, quality, payload)` rows. A generation uses a newly generated UUID string. Rows are inserted in small typed batches; only after every insert succeeds does the producer append a completion manifest with the fixed observation endpoint and expected row count. The reader selects one manifest and that exact generation, checks its row count, unique keys, payload endpoints and window, and rejects incomplete or mixed data.

The manifest also represents a genuinely empty generation. Absence of a manifest, expiry, future timestamps or incomplete rows return unavailable, never a fabricated zero-history response. Orphan generations and old manifests expire after two days. Snapshot freshness is measured from the observation endpoint, not publication time. A failed replacement leaves the earlier complete generation selected; existing StatsCache stale disposition may serve a previous cached response according to its unchanged policy. A lost manifest acknowledgement can still leave a valid generation published; logs only claim publication when confirmed.

All fields in a generation cover the same `[from,to)` and exact world set. Replacing a generation naturally removes expired keys; per-item tombstones are unnecessary. A changed datacenter/region world membership produces a different scope key.

## Scheduling and bounds

The app owns one cancellable producer task per process, separate from the sequential market-rollup scheduler. Reads register demand in a bounded 64-key coalescing queue. Repeated requests cannot bypass a failed job's 60-second backoff. Active keys refresh before their window cadence expires (1 day: 15 minutes; 7 days: one hour; 30/90 days: six hours). Keys stop refreshing after 24 hours without demand. Multiple app replicas retain their own bounded queues; this is not a cluster-wide admission controller.

There is one generation in flight per process. Each SQL read retains 512 MiB, 10 seconds and two million result rows with overflow=throw. A whole generation has a separate 120-second cooperative background deadline and cancellation (synchronous single-item reduction is not CPU-preempted); no HTTP loader waits for it. Individual row/manifest writes have ten-second acknowledgement deadlines. Snapshot payloads are limited to 64 MiB, snapshot SQL responses to 96 MiB including framing/keys, and planning to 100,000 item IDs. The request-side StatsCache deadline stays 12 seconds. A very hot item that cannot fit an individual query remains unavailable; limits are not raised or hidden by partial results.

A first request can receive HTTP 503 while the first generation is built. Once available, request work reads only its manifest/payload rows plus the existing alive/stock snapshots. Stock remains a current cadence-checked estimate, as before; the historical window carries its own fixed snapshot endpoint. Request availability at representative production scale still requires measurement under #1342.

## Validation

The restored historical smoke tests remain the semantic oracle. `listing_snapshots_smoke` adds committed-empty vs absent, atomic generation selection, incomplete/future/expired manifests, canonical scope identity, late receipt and competitor corrections, retry deduplication, cursor-edge pending behavior, disjoint empty intervals, a missing world, prices before the common baseline, and sales-only key expiry. Queue unit tests verify coalescing, backoff and admission bounds.

The restored ignored workload defaults to one representative 32-world/90-day producer and cached-read case. `T12_FULL_MATRIX=1` enables the larger matrix intentionally. It reports producer duration separately from the unchanged request deadline and verifies exact counts and floor coverage. Only an explicit `T12_RUN_CAP_GUARD=1` on the large fixture adds the later oversized-item probe; it must fail with the actual row-limit error and leave the old committed generation readable. No synthetic result establishes production coverage, continuous ingest or historical receipt maturity.
