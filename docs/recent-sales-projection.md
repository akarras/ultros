# Analyzer recent-sales projection

A cold analyzer boot used to rank the entire `sale_history` table to keep six
sales per item, world, and quality. `recent_sale_history` holds those samples
explicitly. Once seeded, the six-sample startup query is a plain scan of the
compact table; it never ranks historical sales. Smaller requested limits rank
only the compact table. Requests above the retained limit use the legacy query.
The analyzer and projection share `RECENT_SALE_LIMIT` (currently six).

Each accepted sale is inserted into the projection and trimmed under the same
board lock and PostgreSQL transaction as its source sale and ingest marker.
Retries do not add samples twice. Trimming orders by sale date, then source ID,
so timestamp ties retain exactly six deterministic samples. Historical source
sales are retained unchanged. The projection does not depend on a time cutoff:
a quiet market's last sale from years ago still belongs in it.

## Rollout and seeding

The migration creates empty projection and progress tables. It does not scan
`sale_history`, add a historical index, or run a backfill at application startup.
Until the progress row is complete, cold boots retain the legacy query and log
that the projection needs seeding. Existing analyzer disk snapshots still work.

1. Deploy the migration and **all sale-writing instances** from this version.
   Wait for older binaries and their in-flight transactions to stop. An old
   writer does not maintain the projection; leaving one running during or after
   backfill can leave sales behind the cursor unprojected. Include any other
   importer that writes source sales directly in this requirement.
2. Build the maintenance tool: `cargo build --release -p ultros-db --bin recent_sales_backfill`.
3. Export the intended `DATABASE_URL` in the operator environment. The tool does
   not load `.env`, run migrations, initialize the app, or fetch game data.
4. Inspect progress: `target/release/recent_sales_backfill --status`.
5. Run a bounded batch, then repeat it as needed:
   `target/release/recent_sales_backfill --max-boards 1000 --statement-timeout-ms 10000`.
   Stop when the JSON status reports `"complete":true`. Each run resumes from
   the last committed `(item, world)` cursor. Interrupting a run is safe.

The command defaults to 1,000 board attempts and ten seconds per SQL statement within each attempt,
with one pooled connection. A timeout or other failure exits with code 2; rerun
to resume committed progress. For a particularly large board, raise the timeout
explicitly (maximum 300,000 ms). A board without one quality may need to inspect
its whole history to establish that fact. Read the PostgreSQL logs when a board
repeatedly fails; the CLI avoids printing connection strings or raw DB errors.

The existing `(sold_item_id, world_id, sold_date DESC)` index supports cursor
seeks and per-quality `ORDER BY sold_date DESC,id DESC LIMIT 6` reads. Historical
reads finish before the board's write lock is taken. Their results merge with
any newer live samples, and the cursor advances in the same transaction as the
merge. Competing operators use a compare-and-swap on the cursor; losing attempts
roll back and count toward the command's finite attempt budget. Lock waits and
queries have transaction-local PostgreSQL deadlines, including before the board
lock is acquired. Only one connection is held at a time.

After completion, newly appearing boards are projected by the writer even if
their keys sort behind the cursor. No recurring full-history rebuild is needed.
This change does not make the analyzer event bus durable or change its snapshot
restore policy.

## Maintenance and verification

The runtime source-sales path is append-only. If an operator deletes or edits
historical sales, or bulk-loads history without the transaction helper, the
projection must be rebuilt as part of that operation. Stop maintenance commands,
mark the progress row incomplete **before** changing source/projection rows, and
stop or restart analyzers that might be loading it. Clear the projection, set
both cursor columns to NULL, keep `complete=FALSE`, then run the seed command
again with all writers upgraded. Do not reset progress while another backfill
is running. A change to the retained limit also needs an explicit projection
migration/rebuild; a mismatched retention value keeps the reader on its legacy
fallback and prevents the maintenance tool from claiming completion.

For a binary rollback, deploy writers that no longer reference the projection
before dropping its tables. Keep the projection when a rollback is temporary;
re-seed before trusting it again if any old writer has run.

The explicitly opted-in PostgreSQL regression covers old quiet sales, quality
separation, timestamp ties, live writes during a staged seed, cursor resumption,
competing attempts, marker rollback, deadline-limited lock contention, and a
one-connection maintenance pool. Its ready-read EXPLAIN is a scan of
`recent_sale_history` with no historical relation or WindowAgg. This verifies the
query shape on a disposable fixture, not production latency or backfill duration.

```sh
ULTROS_PLUGIN_TEST_DATABASE_URL=postgres://ultros_plugin@127.0.0.1:PORT/ultros_plugin_projection_fixture \
  cargo test -p ultros-db projection_backfill_resume -- --ignored --nocapture
```

Use a dedicated disposable database: this test clears projection state and its
world-30001 source fixtures. The fixture helper refuses remote or non-test URLs.
