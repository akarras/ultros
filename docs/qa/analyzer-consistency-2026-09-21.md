# Analyzer consistency validation

Validation uses the Windows worktree `eea6/ultros`, disposable PostgreSQL and
ClickHouse containers bound only to loopback, dummy Discord credentials, and the
`test-auth` market-isolation feature. Production was inspected separately by the
parent audit; browser regression fixtures never write to production.

## Commands and environment

Rust: `1.100.0-nightly (f7d782a3b 2026-08-19)`, toolchain
`nightly-2026-08-20`. The installed MSVC linker is `14.34.31933`.

Run from Git Bash, with Strawberry Perl before Git's Perl:

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:/usr/bin:$PATH"
export CARGO_BUILD_JOBS=4 OPENSSL_RUST_USE_NASM=0 CARGO_PROFILE_TEST_DEBUG=2
cargo() {
  command cargo --config profile.dev.package.ultros-app.debug=0 \
    --config profile.test.package.ultros-app.debug=0 "$@"
}
export -f cargo
./check_ci.sh > target/analyzer-ci-final.log 2>&1
```

The package-specific debug override is a Windows validation workaround. The
initial native test build emitted a 5,267,707,586-byte `ultros-app` archive and the
server test link failed with 185 unresolved references, including generated
`game_sources` functions and generic `std`/`drop_glue` symbols. Reducing only the
app's debug information allowed that link to complete. It does not change code,
assertions, overflow checks, enabled features, or test selection. Other packages
retain debug level 2 so native dependencies can be reused by the preview build.
Cold builds used two jobs; final warm validation used four with more than 20 GB
of free memory available.

The client Rust build completed through `cargo leptos build --bin-features
test-auth`. GitHub access was needed once to download cargo-leptos's Tailwind
executable. Its native build was subsequently invoked separately to preserve
the existing compiler-cache flags:

```bash
export CARGO_ENCODED_RUSTFLAGS=$(printf -- "--cfg=web_sys_unstable_apis\037--cfg=erase_components")
cargo leptos build --server-only --bin-features test-auth \
  --bin-cargo-args="--config=profile.dev.package.ultros-app.debug=0"
```

Cargo-leptos otherwise appends a second, semantically redundant
`--cfg erase_components` to the two flags already present in
`.cargo/config.toml`, invalidating the native dependency cache. The encoded
flags preserve both existing cfg values exactly; no SSR/client rendering mode
is changed. The argument to `--bin-cargo-args` must use `--config=...` with no
space, because cargo-leptos passes it as a single token.
For the subsequent frontend-only invocation, unset `CARGO_ENCODED_RUSTFLAGS`
to reuse the client cache produced by cargo-leptos's normal flags.

Both the server-only build and the subsequent cached frontend-only build passed.
The latter restores site assets cleared by cargo-leptos's server-only invocation.
The development WASM's `name` custom section occupied 600,487,483 bytes. Before
browser validation, the installed tool removed debug names from generated assets
only (no optimization or source changes):

```bash
cp target/site/pkg/ultros.wasm target/ultros-with-debug-names.wasm
wasm-opt target/ultros-with-debug-names.wasm --strip-debug --all-features \
  -o target/site/pkg/ultros.wasm
```

The final functional build's served artifact was reduced from 694,068,858 to 93,320,840
bytes; its original is `target/ultros-with-debug-names-final.wasm`. Both original
build artifacts remain under ignored `target/`. The local server runs the fresh `test-auth`
binary on `127.0.0.1:61335`; `METRICS_PORT=0` avoids another worktree's existing
metrics listener. Its dummy Discord bot authentication error is expected and
does not stop HTTP service.

## Results

- JavaScript deterministic regression command `node --test integration/*.test.cjs`:
  **106 passed**, no failures.
- The complete root `./check_ci.sh` gate **passed (exit 0)**: formatting,
  all-targets Clippy, explicit `xiv-gen` feature Clippy, workspace unit tests,
  list convergence/validation, deterministic Universalis tests, feature-gated
  `xiv-gen` tests, pack sanity, and all required vendored reactive-graph tests.
  Its final 52 suite invocations report **2,586 passed**, **87 existing ignored**, and
  **6 explicitly filtered live Universalis tests**. Counts include tests repeated
  by required commands. The workspace phase alone passed **2,439 tests**,
  including all **662 app tests** and **475 server tests**. This final run
  includes the Recipe comparator and sort-schema presentation-invalidation
  regressions and the final editable Vendor Sell/Vendor Resale/Trends default
  semantics and the final seven-locale wording correction; its complete log is
  `target/analyzer-ci-final.log`.
- Both final functional client and server builds **passed** after the
  performance/refetch and editable default fixes (server 2m31s, client Rust
  1m41s plus 36s wasm-bindgen). A new
  `integration/analyzer-consistency.cjs` reproduces the exact Ruthenium Vambraces
  item with a 999,999,999-gil listing and checks recommended/default views,
  unknown evidence retention, scope selection, and heading/menu sort parity.
- The new consistency harness passed, including mobile Views bounds on both
  Ventures and Flip Finder after the CSS fix. Screenshots are under
  `integration/artifacts/analyzer-consistency/`.
- One complete `scripts/run_e2e.sh` invocation finished with exit 1. It exposed
  obsolete scope selectors (updated), empty-database sale-statistics 503s, and a
  real Trends window-refetch regression. Its unrelated Lists/Auth/group,
  recipe-planner, search, and dashboard suites completed. Listing/history
  horizontal-scroll and expansion probes explicitly skipped their populated-data
  checks because those unrelated tables are empty.
- The full Currency Exchange and market-window suites passed after the
  performance and Trends refetch fixes. The latter verifies direct SSR/hydration,
  7d/30d/90d changes, late-response races, world navigation, pending filters and
  sorts, hidden-column requirements, and restored views.

The final functional browser driver used:

```bash
REUSE_SERVER=1 SKIP_BUILD=1 BASE_URL=http://127.0.0.1:61335 \
E2E_BLOCK_EXTERNAL=1 CONCURRENCY=2 \
RUN_LISTS_V2=0 RUN_RECIPE_PLANNER=0 CHECK_ANALYZER_ROUTES=1 \
./scripts/run_e2e.sh
```

`LEPTOS_FEATURES` was unset for this reuse invocation, so the already-tested
authentication/group block was not repeated. This was the known fresh server
from this exact worktree, not an arbitrary existing instance. Strict console,
content, and overflow checks remained enabled.

This invocation completed with **exit 1**, recorded in
`target/analyzer-scoped-e2e-final.log`. Desktop, mobile, and wide route smoke;
search; FC breakdown/world; analyzer grids and restored views; all eight shared
analyzer route checks; market-window; consistency; and dashboard checks passed.
Two harness assumptions failed and were corrected, then rerun independently:

- `analyzer-world-urls.cjs` expected Vendor Sell to fetch world recent sales.
  Its fixed NPC payout uses regional bulk statistics. The corrected test retains
  region statistics/listing requests, scope labels, world/cookie precedence,
  encoded query, history, and saved-view assertions. The complete suite passed:
  `target/analyzer-world-final.log`.
- The Trends test left Columns open, then clicked a filter editor obscured by
  that picker. The failure screenshot and changed `cols` parameter showed the
  click toggled Current listing instead of submitting the filter. The test now
  closes Columns with its button and verifies the picker is hidden. The complete
  Trends suite passed without sleeps or weaker assertions: false-to-true changes
  the two-row cohort to three; Clear includes all four; Recommended returns
  three; clearing Recommended restores four, with exact API flags checked.
  Log: `target/analyzer-trends-guard-final.log`.

Standalone final reruns (all exit 0):

```bash
BASE_URL=http://127.0.0.1:61335 CHECK_ANALYZER_ROUTES=1 ANALYZER_TOOLS=trends \
  npm --prefix integration run test:shared-analyzer-data
BASE_URL=http://127.0.0.1:61335 npm --prefix integration run test:analyzer-world-urls
BASE_URL=http://127.0.0.1:61335 npm --prefix integration run test:currency-exchange
BASE_URL=http://127.0.0.1:61335 npm --prefix integration run test:analyzer-consistency
```

The consistency suite also passed within the driver. It verifies exact Ruthenium
listing exclusion, unknown-evidence retention, custom default/reset, negative
Vendor Sell rows restored by Unrestricted, inflated Vendor Resale rows restored
by clearing Recommended, world/DC/region requests, header/menu sort agreement,
and mobile Views bounds on Ventures and Flip. New populated screenshots include
`vendor-unrestricted-losses.png` and `vendor-resale-cleared-guard.png` under
`integration/artifacts/analyzer-consistency/`.

There is no claim that the composite driver itself returned success: its failed
components were fixed and passed independently. Populated item-view horizontal
scroll/expansion remains untested because the unrelated listing/history tables
were empty; the runner reports these skips. Live Universalis tests, optional
database integration targets, security audit, and opt-in Lists acceptance were
not added to this local deterministic/browser validation.

Browser validation also exposed a hidden Vendor Sell `profit > 0` candidate
filter that prevented Unrestricted from showing losses. That gate was removed;
the explicit Recommended profit filter now owns this choice. Vendor Resale and
Trends similarly encode their suspicious-row guard in Recommended rather than
treating an absent URL flag as a hidden filter. Dedicated browser assertions
cover clearing these guards as well as restoring Recommended.

Visual inspection then caught the stale Vendor Sell phrase "every profitable
listing" above unrestricted losses. It was changed to "every eligible listing"
in all seven locale files. The final root CI rebuild includes that copy-only
change. Browser interaction results and screenshots precede this final text
correction; the preview was not rebuilt solely to recapture the wording.

The isolated database now has one synthetic `sale_stats_window` row per world
and supported window, so the final smoke run can keep strict console checks.
The application deliberately returns 503 for empty rollups; no handler or console
allow-list was changed. The seed SQL is retained in
`target/analyzer-isolated-sale-stats-seed.sql` and targets only the disposable
`ultros-analyzer-eea6-ch` container.

The Trends regression was reproduced in a bounded three-case browser probe:
implicit sort direction lost the 30d-to-7d API refetch on both direct hydration
and client entry, while an otherwise identical `dir=desc` URL fetched exactly
once and displayed 7d rows. This is tracked through final verification rather
than weakening the request/value assertions. After the sort-schema memo fix,
all three cases emit exactly one 7d request and display the same correct 7d
values (28/222/373), with no page errors. Logs:
`target/analyzer-trends-probe.log` and `target/analyzer-trends-probe-final.log`.

A separate 15-second Recipe probe counted 2,201 outside-reactive-context warnings
before the performance fix and 131 afterward. Residual diagnostics are setup or
column-level reads; the largest locations account for 34 and 30 messages, rather
than a per-row warning storm. In a fresh browser with no competing checks, a
canonical Profit-descending sort of **9,640 unrestricted fixture-backed Recipe
rows settled in 151 ms**, requiring the URL, `aria-sort`, and first displayed row
to change. No page errors occurred. This is a development-build observation,
not a production benchmark. Logs: `target/analyzer-warning-probe.log` and
`target/analyzer-warning-probe-final.log`.

The locale build reports new English strings falling back to English in other
languages. Translation completeness is not claimed by this validation.

## Cleanup

After the final root gate passed, the worktree's QA server (PID 102016) was
stopped and the two disposable containers `ultros-analyzer-eea6-pg` and
`ultros-analyzer-eea6-ch` were removed. Follow-up process/container queries
confirmed none remained. Other worktree processes and databases were untouched.
Logs, generated build assets, seed SQL, and screenshots were preserved under
`target/` and `integration/artifacts/`; the local preview is no longer running.

## Follow-up: Currency Exchange toolbar (#1492)

The follow-up removes Currency Exchange's extra Full Results heading/panel and
redundant home-world note. Its Views menu now belongs to the shared ControlBar,
alongside Columns and filters, and QueryGrid's separate Views row is disabled.

The first hydrated regression run caught a real remaining placement problem:
providing saved-view storage had not moved the rendered menu into ControlBar.
The new assertion correctly failed because Views was outside the registered
filter bar. The route was then corrected, retaining the same assertions.
The original failed result is preserved in
`target/analyzer-currency-1492-browser-before-toolbar-fix.log`.

Validation for this narrowly scoped follow-up uses the same isolated service
names, loopback ports, compiler settings, and generated-asset-only debug stripping
described above. Its final logs use the `target/analyzer-currency-1492-` prefix;
the broad audit's earlier run scope and limitations remain unchanged.

Final follow-up results:

- `./check_ci.sh` passed after the rendered Views placement fix: **2,586 passed**
  across **52 suite invocations**, **87 existing ignored**, **6 live tests
  filtered**. Log: `target/analyzer-currency-1492-ci.log`.
- JavaScript regressions: **106 passed**, no failures.
  Log: `target/analyzer-currency-1492-js.log`.
- Fresh server and client builds passed: server **2m36s**, client Rust
  **1m37s** plus **37s** wasm-bindgen. Logs:
  `target/analyzer-currency-1492-server-build.log` and
  `target/analyzer-currency-1492-client-build.log`.
- Generated WASM debug names were stripped without optimization, reducing
  693,787,108 bytes to 93,265,432 bytes. Original preserved as
  `target/ultros-currency-1492-with-debug-names.wasm`.
- `BASE_URL=http://127.0.0.1:61335 npm --prefix integration run test:currency-exchange`
  passed with the unchanged new toolbar assertions and the existing estimates,
  raw listing, NQ statistics, window, filters, saved columns, quantity, and mobile
  alignment assertions. Log: `target/analyzer-currency-1492-browser.log`.

Desktop (1800px) and mobile (393px) captures were visually reviewed by both the
validation agent and parent: Views is beside Columns/clear/filter in the shared
toolbar, the duplicate heading/panel and redundant note are gone, and mobile
actions fit while the grid retains horizontal scrolling. Fresh screenshots:
`integration/artifacts/currency-exchange/desktop.png` and
`integration/artifacts/currency-exchange/mobile.png`.

Only this targeted Currency browser suite was rerun for #1492; the full browser
driver was not repeated. These fresh client/server builds include the prior
locale-only correction, although the new screenshots exercise Currency rather
than Vendor Sell. After verification, server PID 98868 was stopped and both
named disposable containers were removed again; follow-up queries confirmed
none remained. Logs, fixtures, generated assets, and screenshots were preserved.
