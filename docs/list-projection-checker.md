# Decoded Lists projection checker

`list_projection_check` compares stored Loro documents with the relational
projection. It makes no repairs, never initializes missing documents, and does
not run migrations, the application, the Discord bot or market ingestion.
Use a PostgreSQL role with SELECT access to `list`, `list_item`, and `list_doc`.
The connection URL comes only from the exported `DATABASE_URL`; the tool does
not load `.env`, print the URL, or include raw database errors in its output.

```sh
cargo run --locked -p ultros-db --bin list_projection_check -- --ids 123,456 > check.json
cargo run --locked -p ultros-db --bin list_projection_check -- --ids-file soak-ids.txt > check.json
# Optional discovery, unioned with retained explicit IDs:
cargo run --locked -p ultros-db --bin list_projection_check -- \
  --ids-file soak-ids.txt --updated-since 2026-09-15T00:00:00Z > check.json
```

The ID file is positive decimal IDs separated by whitespace or commas.
Repeated IDs are deduplicated. There is no implicit "all lists" selection.
`--updated-since` includes documents updated at or after its RFC3339 timestamp.
Use the built binary directly for repeated checks after recording its Git SHA;
do not paste credentials into commands, issue comments, or retained artifacts.

Every invocation opens **one REPEATABLE READ, READ ONLY transaction** for the
selection and all document, metadata, and row reads. Concurrent application
transactions can commit without causing a mix of old document/new rows in the
report. The transaction is rolled back after reading; decoding then occurs in
memory. A failure of any query produces an operational error, not a partial
"equal" report. Run bounded manifest sizes to avoid holding long MVCC snapshots.

The comparator calls `ListDocument::from_snapshot`, then compares each natural
key (item and Any/HQ/NQ), quantity, acquired count, target price, name and scope.
It shares the writer's integer clamping helper (0 through i32::MAX), relational
NULL defaults (quantity 1 and acquired 0), and scope priority (region, then
datacenter, then world). Target prices remain signed i64. A legacy document
without a scope means "preserve the relational scope" in the writer; its report
sets `scope_compared: false`. Investigate these separately if the soak requires
every participating list to have an explicit document scope.

## JSON and exit codes

Stdout is one JSON object, with `format_version: 1`, `checked_at`, sorted
`selected_ids`, and a result for each ID. `checked_at` is report-generation time,
not a historical query timestamp. Result statuses:

- `equal`: all applicable projected fields match.
- `different`: `differences` includes `metadata`, `missing_row`, `excess_row`,
  `different_row`, or `duplicate_key`, with expected/actual values where relevant.
- `decode_error`: snapshot bytes or document schema cannot be decoded/validated.
  Snapshot bytes and decoder text are not echoed. Investigate using the reviewed
  document decoder; never overwrite the original bytes as a "repair".
- `missing_document`: the list exists but has no stored document. This may be a
  legacy list never opened through sync; it is not a verified equality.
- `missing_list`: no list metadata exists at the read snapshot. This includes
  deleted IDs and IDs that never existed; the checker cannot distinguish them
  without external evidence. The result includes whether any orphan document or
  relational rows remain.

Exit **0** means every selected ID is equal; **1** means at least one finding,
including missing/deleted lists; **2** means invalid input or operational failure;
**3** means an empty selection. A zero-row document with a present list and no
relational items is valid; an empty selection is not evidence for promotion.
JSON includes list names and item contents when mismatched: treat reports as
private operator artifacts and redact them before public attachment as needed.

## Week-long soak manifest

For the deployed seven-day soak in [#1510](https://github.com/akarras/ultros/issues/1510),
begin with a manifest of every participating account list ID and keep
an append-only action/event ledger with list ID, timestamp, operation and actor
alias. Record an ID **before** creating/editing/deleting it in the controlled
soak, or immediately after creation returns its assigned ID. Retain deleted IDs
and their deletion evidence; do not remove them from the manifest to get exit 0.
For broader live traffic, use authoritative action/event records that capture
every participating ID and deletion. This tool does not install an audit log.

Maintain `soak-ids.txt` as the union of that ledger throughout the entire week.
Daily `--updated-since` discovery and the report's `selected_ids` can supplement
the manifest, but **polling alone is not exhaustive**: a list can be created and
deleted between polls, and timestamps do not prove every mutation was observed.
A sampled union must not be described as every list touched during the soak.

Run the checker against the full retained manifest daily and at the end of the
week. Retain every JSON report and process exit code, invocation timestamps,
deployed/server Git SHA, checker Git SHA and manifest/action-ledger versions.
Classify `missing_list` results using the deletion ledger. Expected deletions
remain visible findings; verify there are no orphan docs/items and attach the
reconciliation alongside the final report. Investigate decode failures, missing
documents, scope omissions, and every difference; do not auto-repair them.
Attach deployed soak reports and the promotion decision to #1510, and link
relevant local/integrated acceptance evidence from
[#1439](https://github.com/akarras/ultros/issues/1439). A passing synthetic test is
not production-soak evidence, and running the tool once does not complete the
week-long criterion.

## Tests

The normal `./check_ci.sh` gate covers comparison fixtures and CLI parsing.
The opt-in PostgreSQL regression creates two connections and private temporary
test tables in a uniquely named schema. It commits a writer between the reader's
document and row reads, verifies the old and next snapshots are both consistent,
detects deliberately divergent rows, and proves READ ONLY rejects writes.
Run only against an isolated disposable test database, never production:

```sh
MIGRATION_TEST_DATABASE_URL="$TEST_DATABASE_URL" CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --locked -p ultros-db concurrent_edit_cannot_mix_document_and_projection_versions -- --ignored
```

The test drops only its generated schema on success. If assertions abort, retain
the failing schema for diagnosis, then remove that specifically named schema
from the disposable database; no application data is used by the fixture.
