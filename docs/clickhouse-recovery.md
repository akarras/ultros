# ClickHouse recovery

Postgres `sale_history` is authoritative. The live ClickHouse writer has a
10,000-row queue plus one 1,000-row retry batch. It retains the complete batch
until the insert is acknowledged, with a ten-second attempt timeout. An
unavailable startup retries schema initialization every minute. Rollups wait
for that initialization. Graceful SIGTERM/Ctrl-C shutdown stops the analyzer,
waits for its in-flight sale batch and final snapshot, and drains rows already
sent to the writer within the process's overall 30-second shutdown deadline.

This does not drain the sale-history broadcast bus. Producers and the analyzer
share a cancellation signal, so the history consumer can exit while committed
sale events are still queued, or before an in-flight producer publishes its
last event. Those events reach neither the final RAM snapshot nor the writer's
drain. Reconciliation can therefore be necessary even after an orderly shutdown
that finishes within the deadline; successful joins and flushes do not prove
Postgres/ClickHouse parity.

This is not durable replication. Queue overflow, broadcast lag, an unclean
process exit, undrained shutdown events, or expiry of the shutdown deadline can
still leave gaps. Retrying
an ambiguous insert can temporarily create physical duplicate rows; analytics
queries use `FINAL` to deduplicate them before aggregating.

## Detect and reconcile gaps

Monitor increases in:

- `ultros_clickhouse_writer_dropped_rows_total`, labeled `reason` with
  `queue_full`, `queue_closed`, or `shutdown_unflushed`.
- `ultros_clickhouse_writer_flush_failures_total` and
  `ultros_clickhouse_writer_migration_failures_total`.
- `ultros_analyzer_history_recovery_required_total` for broadcast lag.

`ultros_clickhouse_writer_queued_rows` reports pending channel rows sampled by
the worker. It excludes the retained retry batch and is not a durable backlog.
These metrics do not count sale events abandoned on the broadcast bus during
shutdown. Use parity checks after deployments as well as after reported drops.

1. Restore ClickHouse availability and verify new inserts succeed.
2. Run `cargo run --bin clickhouse_parity_check -- <start-year>` against the
   intended deployment credentials. This is a potentially expensive historical
   scan; choose the smallest affected year range. The command uses logical
   (`FINAL`) sales counts, not physical pre-merge duplicate counts.
3. Identify affected `(world_id, year_month)` chunks from the output. Existing
   `_backfill_state` markers cause the backfill tool to skip completed chunks,
   including a chunk that later acquired a gap. Clear only those markers before
   rerunning the backfill; for example, in the intended ClickHouse database:

   ```sql
   ALTER TABLE _backfill_state DELETE
   WHERE world_id = 40 AND year_month = 202609
   SETTINGS mutations_sync = 1;
   ```

4. Run `cargo run --bin clickhouse_backfill -- <start-year>`, then rerun parity.
   Backfill overlaps are safe because both paths use the real Postgres sale id.
   Counts from a live system can move between the Postgres and ClickHouse reads;
   investigate persistent drift rather than assuming every live difference is
   a lost row. Repairing ClickHouse does not rebuild the analyzer's RAM history.

## Follow-up: durable replication

Add a Postgres outbox entry in the same transaction that inserts each sale.
Poll pending outbox rows in bounded batches with a replica-safe lease; write
ClickHouse first, then acknowledge the exact outbox ids only after insert
completion. An ambiguous acknowledgement replays the same sale identity. Use
lease expiry to recover a crashed worker, instrument oldest pending age, and
prune acknowledged entries in bounded batches. Cover failure before/after the
ClickHouse acknowledgement and concurrent transactions with tests against real
Postgres and ClickHouse.

Do not use `max(sale_history.id)` alone as a permanent checkpoint: PostgreSQL
transactions can commit out of id order. A sold-date lookback also misses old
sales imported late. Existing historical gaps still need a scoped one-time
reconciliation when introducing an outbox; avoid adding an unbounded full-table
scan to every running replica.
