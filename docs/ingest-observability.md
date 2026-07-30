# Ingest observability

Market-data ingest fails silently by default. The app keeps serving, requests
keep returning 200, and the numbers simply stop moving — serving stale data is
not an error, so nothing in the request path can notice. These metrics exist to
turn each of those silent modes into something alertable. They are exported on
the Prometheus endpoint at `:9091/metrics` (see `ultros/src/web_metrics.rs`).

## Metrics

| Metric | Type | Labels | What it means |
| --- | --- | --- | --- |
| `ultros_world_ingest_staleness_seconds` | gauge | `world` | Seconds since the newest `listing_last_updated` row for that world. The single best "is ingest alive" signal. |
| `ultros_worlds_never_ingested` | gauge | — | Worlds in `WorldCache` with no `listing_last_updated` rows at all. |
| `ultros_analyzer_bus_lagged_total` | counter | `bus` | Events the analyzer's broadcast receiver was too slow to read, and the channel discarded. Nonzero means the in-RAM caches and ClickHouse have drifted from Postgres. |
| `ultros_analyzer_skipped_events_total` | counter | `op`, `reason` | Listing/sale events dropped because a world, datacenter or region was missing from the analyzer's maps. |
| `ultros_analyzer_snapshot_rejected_total` | counter | `reason` | Startup snapshots refused (`too_old`, `unparseable_name`), causing a fall back to the Postgres reload. |
| `ultros_analyzer_snapshot_age_seconds` | gauge | — | Age of the snapshot this process booted from. Set once at startup. |
| `universalis_websocket_liveness_timeouts_total` | counter | — | Websocket connections torn down for delivering no frames within the liveness deadline. |

Pre-existing and still useful alongside these:
`ultros_websocket_rx{WorldId}`, `ultros_catchup_items_recovered{world}`,
`ultros_catchup_window_saturated{world}`.

## Suggested alerts

`ultros_world_ingest_staleness_seconds` is the one to page on. Everything else
explains *why* it went up.

- **A world went silent** — `max_over_time(ultros_world_ingest_staleness_seconds[15m]) > 3600`.
  Quiet worlds do go an hour without a listing change; a whole datacenter going
  quiet at once does not.
- **Every world went silent** — the same expression firing across most series at
  once points at the websocket or the process, not the market. Correlate with
  `universalis_websocket_liveness_timeouts_total`.
- **Silent data loss** — `increase(ultros_analyzer_bus_lagged_total[1h]) > 0` or
  `increase(ultros_analyzer_skipped_events_total[1h]) > 0`. Neither should ever
  be nonzero in steady state. Sustained `bus_lagged` means the ring sizes in
  `ultros/src/event.rs` need revisiting; sustained `skipped_events` with
  `reason="unknown_world"` means `WorldCache` is missing a world (see below).
- **Booted on stale data** — `increase(ultros_analyzer_snapshot_rejected_total{reason="too_old"}[1h]) > 0`
  is informational: the guard did its job, but the process was down long enough
  to matter.

## Known gap: `WorldCache` never refreshes

`WorldCache` is built exactly once, in `main.rs`, and on a cold database that
build races the task that populates the world table. A world it misses is absent
from the analyzer's maps for the lifetime of the process, and every event for
that world is dropped — visible as
`ultros_analyzer_skipped_events_total{reason="unknown_world"}` climbing steadily.

Until a refresh path exists, the fix is a restart once the world table is
populated. Before this change these lookups panicked instead, which killed live
sale ingestion and the ClickHouse dual-write outright while the process kept
serving.

## Tuning

| Env var | Default | Effect |
| --- | --- | --- |
| `UNIVERSALIS_WEBSOCKET_LIVENESS_TIMEOUT_SECS` | `150` | Silence tolerated on the Universalis websocket before reconnecting. 2.5× the 60s ping interval. |
| `UNIVERSALIS_WEBSOCKET_COOLDOWN_SECS` | `2` | Wait between reconnect attempts. |

Compile-time constants worth knowing about, all documented at their definitions:
`LISTINGS_BUS_SIZE` / `HISTORY_BUS_SIZE` (`ultros/src/event.rs`),
`MAX_SNAPSHOT_AGE` (`ultros/src/analyzer_service.rs`), and `REFRESH_INTERVAL`
(`ultros/src/ingest_health.rs`).
