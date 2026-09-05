# Recipe Analyzer capacity model

Issue [#1245](https://github.com/akarras/ultros/issues/1245) removed the
Recipe Analyzer's 100-row display cap. The full result set remains a browser
calculation rendered through the existing virtual scroller; removing the cap
does not add a server-side per-recipe query or a DOM node per result.

The expensive part of the page is the whole-market sale-history summary. Its
capacity boundary is intentionally independent of page traffic:

- `sale_stats_window` stores one mergeable row per world, window, item, and
  quality. Scheduled refreshes scan raw `sales` for the supported 1, 7, 30,
  and 90-day windows. World rows merge cheaply into datacenter and region
  responses; medians use t-digest states while counts, sums, minima, volume,
  and last-sold timestamps compose exactly.
- A Postgres advisory lock elects one scheduler across all web replicas. The
  lock is tied to its pooled connection, releases on process or connection
  loss, and is checked every 15 seconds. A lost connection cancels that
  scheduler before the replica joins the 30-second re-election loop. Scaling
  the web tier therefore cannot multiply scheduled raw-sales scans.
- Each web replica holds at most 512 scope/window keys and 64 MiB of response
  bodies; least-recently-used bodies are evicted at the byte ceiling. Fresh
  entries live for 5 minutes, stale entries can be served for 30 minutes, cold
  misses for the same key coalesce, and at most two ClickHouse loads run
  concurrently. Loads time out after 12 seconds and failed cold loads back off
  for 2 seconds.
- Responses carry `Cache-Control: public, max-age=300, s-maxage=300,
  stale-while-revalidate=1800`, allowing a reverse proxy or CDN to absorb
  repeated traffic before it reaches a replica.
- The analyzer uses the 7-day rollup for velocity and average price as well as
  its optional sale columns. It only fetches raw recent samples when a player
  enables outlier filtering, or as a failover while a new rollup is seeding.

## Deployment and rollback

The ClickHouse migration is additive and idempotent. On startup it creates
`sale_stats_window`; the replica holding the advisory lock seeds all four
windows before entering the normal cadence. Until the first relevant scope has
rows, `/api/v1/sale_stats` returns a transient 503 rather than caching an empty
market, and the analyzer falls back to its in-memory recent-sales feed.

A rollback needs no data migration: older binaries ignore the new table and
continue using their previous query path. Leave the table in place so a later
roll-forward can reuse it. Dropping it is optional and should only happen after
all new binaries have been removed.

## Operational signals

- `ultros_sale_stats_cache_total{disposition="fresh|loaded|stale"}` shows cache
  behavior. Alert on sustained growth in `loaded` relative to `fresh`, or the
  disappearance of `stale` during a ClickHouse incident.
- `ultros_rollup_scheduler_leader` should sum to exactly 1 across healthy web
  replicas. Zero means rollups are not refreshing; greater than one indicates
  the advisory-lock invariant is broken.
- Existing `ultros_http_requests_total` and
  `ultros_http_requests_duration_seconds` metrics cover endpoint status and
  latency. Track `/api/v1/sale_stats/:scope` 5xx/503 rates alongside the cache
  counter.

Before rollout, verify the 7-day seed row count is non-zero and compare a few
world responses against raw sales. After rollout, watch ClickHouse memory and
query duration through one 15-minute and one hourly refresh boundary. The
integration smoke test can be run against a throwaway ClickHouse with:

```bash
ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test sale_stats_smoke
```
