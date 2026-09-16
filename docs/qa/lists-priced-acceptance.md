# Required priced Lists acceptance

This gate implements #1478's executable checks for #1439. It does not establish
a production soak, projection agreement, Labs promotion, or physical FFXIV/PiP
behavior. Keep those decisions and evidence separate.

## Environment

Use an **isolated disposable PostgreSQL database**, a fresh server and WASM build
with `test-auth`, and Labs `lists-sync`. Never aim fixture setup at production.
The server must have its normal catalog, regions, datacenters, worlds, and
retainer cities initialized. Start this exact server with
`ULTROS_TEST_MARKET_ISOLATION=true`. This opt-in exists only in a `test-auth`
build and freezes once before market workers start. It suppresses Universalis
websocket ingest, recent-item catch-up and automatic reconciliation, and rejects
manual HTTP refreshes and Discord market sweeps before work begins. Metadata
initialization, real listing reads, analyzer and unrelated services stay active.
The existing websocket/reconciliation flags alone do not isolate the recent-item
loop. Unexpected stock invalidates the run rather than being erased.

`POST /test/list/market-fixture` inserts real database rows. The unchanged
account-list and bulk-listings APIs read them, including SSR requests. No browser
network interception supplies acceptance prices. The module and routes are
absent from builds without the compile-time `test-auth` feature.

The fixture owns three synthetic retainers (one per world) under an authenticated-owner/random-token
namespace. Setup refuses any other listing of items 5056/5057 in the selected
region. Setup and exact-owner cleanup are transactional; concurrent fixture
setup in the same region is serialized. Cleanup deletes only that fixture's
retainer and listings, never all stock for an item/world. The harness compares
every returned offer ID, world, quality, quantity, and price with its manifest to
detect interference. Keep the logged token for cleanup after an interrupted run:
authenticate as its owner and DELETE `/test/list/market-fixture/<token>`.

Baseline uses Bronze Ingot (5056), with the name resolved from the installed game catalog: two NQ at 10 and two HQ at 20 on world A, three
NQ at 12 on world B (same DC), and four HQ at 25 on world C (another DC).
Item 5057 intentionally has no supply. Partial supply is two NQ at 10. Empty
supply has none. Changed-price retains the first listing identity at 30;
disappeared removes that exact first fixture listing. Actual world/listing IDs
are recorded in each run's logs and results, not hardcoded to a mutable DB.

## Commands

Run the mandatory repository gate before committing, then build both sides from
the final source:

```bash
./check_ci.sh
# The default gate cannot see test-auth-only fixture/isolation code.
cargo clippy --locked -p ultros --features test-auth --all-targets -- -D warnings
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p ultros --bin ultros --features test-auth test_market_isolation
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p ultros --bin ultros --features test-auth list_market_fixture
CARGO_INCREMENTAL=0 cargo leptos build --bin-features test-auth
```

Start the freshly built binary from its own worktree with its own site directory
and ignored `.env` (including the isolated database credentials). Set
`QA_SERVER_BINARY` to the absolute path of that verified fresh executable, then:

```bash
LEPTOS_SITE_ROOT=target/site ULTROS_TEST_MARKET_ISOLATION=true \
  PORT=53131 HOSTNAME=http://127.0.0.1:53131 \
  "$QA_SERVER_BINARY"
```

Run from that worktree's root. The runtime currently prefixes its bundle disk
path with `./`, so use the relative `target/site` runtime setting even if the
build used an absolute site root. The server mounts `/pkg/<embedded revision>/`
onto the unversioned `target/site/pkg` directory; do not copy bundles into a
version-named subdirectory. Before acceptance, require HTTP 200 and matching
hashes for the actual versioned JS/WASM URLs, plus the browser's real
`ultros:hydrated` event. Use the embedded revision, which can differ from HEAD
after a history-only re-anchor of identical source.

The same startup flag is required for subsequent #1439/#1480/final real-fixture
acceptance servers. A production build ignores this test-only flag entirely.
Never point another ingest-enabled process at the disposable fixture database.
Setup refuses to seed without isolation attestation; the harness also requests
`/item/refresh/<world>/<item>` and requires the isolation-specific 409 rejection.
It still checks the exact database stock after ordinary listing queries.
Then run:

```bash
BASE_URL=http://127.0.0.1:53131 \
  LISTS_ACCEPTANCE_BUILD='<exact server and WASM source revision>' \
  npm --prefix integration run test:lists-acceptance

# The required repository E2E driver can also invoke the gate. Reuse a server
# only when its branch/build ownership is verified, especially with worktrees.
BASE_URL=http://127.0.0.1:53131 REUSE_SERVER=1 \
  LISTS_ACCEPTANCE=1 LISTS_ACCEPTANCE_BUILD='<exact revision>' \
  ./scripts/run_e2e.sh
```

`lists-acceptance.cjs` requires a build label, refuses skip/allow-empty switches,
and fails if any required suite is missing, fails, or reports a skipped path.
`list-shop-handoff.cjs` remains usable as smoke without `LISTS_ACCEPTANCE=1`, but
prints `SMOKE ONLY` and the omitted priced assertions when data is absent.
Smoke cannot satisfy the priced gate.

Required suites include server-backed account/device pricing, quality overlap,
world/DC exclusions (recording every current-stop stack before advancing and
checking the exact full set of fixture worlds, with home not the lowest ID),
partial/no supply, price change/disappearance, shared
read-only purchases, desktop/mobile and direct/client entry, priced account
handoff/Undo/replacement, device offline/restore, keyboard Undo, Make online,
Shop focus, companion module behavior, and the permission/lifecycle probes
inside `list-sync.cjs` (#1473). The Shop focus implementation is #1474; device
Build lookups are #1471. Integrate these dependencies before attempting the gate.

## Evidence

The runner saves every suite's log and result under
`integration/artifacts/lists-acceptance/`. The market matrix saves screenshots,
fixture identity, viewport, route, and results under
`integration/artifacts/list-priced-acceptance/`. Record the commit, build mode,
actual command, fixture accounts/IDs, and any failed suite in #1439. A failing
unrelated route/search probe still means the aggregate driver failed: retain
that result alongside scoped Lists results.

No browser or Rust execution is certified by this document alone. Final results
must be attached after the fresh build and complete required suites execute.

The gate also covers #1506: Shop's reference must match the actual Build total,
coverage description and incomplete flag for the source from which the trip was
planned. Whole-stack totals stay separate. In the world-excluded fixture the
Build subtotal is136gil with1unit unpriced; the whole-stack trip costs136gil but
has4missing units because surplus in quality-specific purchases is not reused
by the Any row. Recording a purchase does not rewrite the frozen Build reference.


The named `account-two-item-priced-edit-delete-undo-purchase-build` scenario
starts from one empty account list, adds both catalog items through the UI,
changes quantity and quality, deletes the second row and undoes it, records a
partial HQ purchase, then returns to Build. Expected totals are 46 → 34 → 54,
40 after deletion, 54 after Undo, and 34 after the purchase. The frozen trip
keeps its original 54-gil reference. The separate `multi_item` fixture adds a
five-unit second-item NQ stack at 7 gil to baseline stock, then resets baseline.
This journey uses one planned world and makes no home-first travel claim.

`open-companion-revoke`, `open-companion-delete`, and `open-companion-signout`
each start with a fresh, actionable real companion containing an unsubmitted
purchase draft. They assert closure, the correct cache retention/removal, and
no unintended server purchase. Downgrade remains covered separately by1473.

Required handoff, sync and priced suites now inventory only their own created
list IDs, attempt all necessary deletions, and verify absence. Cleanup failures
fail the gate while all browser closes are attempted; original assertion and
cleanup errors are preserved together. This is executable preparation until
these named scenarios run on the fresh integrated build.


## Terminated subscriptions (#1518)

The same real-price matrix opens a fresh real companion for each actual revoke,
list deletion, and sign-out. An unsubmitted quantity draft must never become a
purchase, and access loss must close the companion within the existing 15-second
browser oracle. Cleanup still removes only the caller's registered fixtures.

The separate transient-recovery cases deliberately end a real document/activity
relay with `Unsubscribe`, await its real acknowledgement, then inject the exact
scoped lookup-error frame and two temporary REST 503 responses. These are
**synthetic transport faults**, not evidence of actual revocation. Successful
REST replies, prices, same-ID subscription handshakes, and subsequent remote
updates come from the actual server. The native case keeps one local purchase
unsent until the app's own current-version handshake converges it exactly once.
Both cases retain the focused draft, cached document, and frozen priced trip.
No successful pricing response is replaced by the harness.
