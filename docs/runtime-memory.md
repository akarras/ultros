# Bounding server memory

Repeated `Memory cgroup out of memory` kills mean the deployment's cgroup
limit was reached. The kernel log's RSS figures describe resident memory;
`total-vm` is virtual address space. Read the configured container/cgroup limit
alongside RSS rather than treating virtual size as RAM usage.

## HTTP metrics

`ultros_http_requests_total` and `ultros_http_requests_duration_seconds` use
only method, route template and status labels. Unmatched routes share
`<fallback>` if this middleware is applied to them. Raw paths and raw user
agents must not become metric labels: both are client-controlled and create
new series retained by the in-process recorder and Prometheus.

`ultros_http_user_agents_total{agent="..."}` is an independent request counter
with nine fixed families: `chrome`, `firefox`, `safari`, `edge`, `opera`, `bot`,
`tool`, `other`, `missing`. Browser versions and unknown strings do not create
new families. This intentionally measures broad categories, not individual
crawlers or browser versions. Agent classification is heuristic, not identity.

Queries or dashboards selecting/grouping the old `user_agent` label need to
switch to the separate counter. Existing Prometheus history retains its old
series until normal retention removes them.

## Websocket ingest

`ULTROS_WEBSOCKET_MAX_IN_FLIGHT` controls accepted concurrent event tasks;
default **25**. It must be a positive integer; invalid values warn and use the
default. Each event can perform several database writes, so tune it against
the database pool and observed ingest throughput, not just CPU count.

When all slots are occupied, the listener stops dequeuing. The Universalis
client already has a 100-event bounded channel, which then backpressures its
socket reader. Payloads cannot collect in unlimited tasks waiting for pool
connections. Prolonged saturation can still cause an upstream disconnect;
market reconciliation remains necessary and this is not a durable queue.

`ultros_websocket_in_flight` reports occupied task slots, including completions
not yet reaped. `ultros_websocket_task_failures_total` counts panicked tasks.
Shutdown stops accepting websocket events and drains accepted writes before
cancelling the analyzer/mirror consumers, within the existing 30-second
shutdown budget. Events still queued upstream are not accepted for that drain.

## Analyzer snapshots

New files are `snapshot-<unix seconds>.columns.gz`. The internal `ULTCOL01`
header versions a sequential column-page format, not Parquet. Each page has
at most 1,024 item/quality keys. It contains typed columns of item IDs, packed
quality bits, listing prices/worlds, or sale counts/prices/timestamp seconds
and nanoseconds. All selectors and the exact order/precision of recent sales
are preserved, including partially filled buffers and empty maps.

A blocking worker copies one page under a short read lock, releases it,
and streams columns through gzip to a temporary file. Additional write
memory is bounded by the page and fixed IO/compression buffers rather than
the analyzer's total size. There is no full state clone, full uncompressed
archive, or full compressed file in memory. Page reads can observe different
moments during live ingest; snapshots remain restart caches, not transactional
market backups.

After gzip finishes and the file is synced, an atomic rename publishes it.
The existing 15-minute/shutdown cadence, keep-four rotation and three-hour age
limit are retained. Restores stage the maps until every page and the gzip
checksum validate, then apply them; a corrupt newest file falls back to an
older one. Streaming restores avoid whole-file buffers but still allocate
the restored maps. Legacy rkyv `.bin`/`.bin.gz` files remain readable; those
restores still need an uncompressed archive for rkyv validation. Older binaries
cannot read the new format and need a retained legacy snapshot or a database
reload when rolling back.

The last successful snapshot publishes:

- `ultros_analyzer_snapshot_uncompressed_bytes`
- `ultros_analyzer_snapshot_compressed_bytes`
- `ultros_analyzer_snapshot_write_duration_seconds`

File size depends on market contents. The deterministic comparison test uses
a multiworld fixture and reports both old/new raw and gzip sizes; it is not
a production RSS measurement. Use process/container memory telemetry to
confirm that runtime growth and snapshot spikes improve after rollout.

The 8,193-key fixture (three populated listing selectors and two sale worlds,
with mixed quality and zero through six sales per key) produced:

| Format | Uncompressed bytes | Gzip bytes |
| --- | ---: | ---: |
| Legacy rkyv | 1,450,540 | 444,628 |
| Column pages | 1,168,705 | 276,835 |

That is about 19% less uncompressed data and 38% less disk space. The test
asserts that both sizes improve, without depending on exact codec output.
