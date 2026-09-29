# Analyzer rebase validation — 2026-09-22

This report records new validation after rebasing the analyzer consolidation
onto `origin/main` at `ef1dda33`. Earlier results remain in
`analyzer-consistency-2026-09-21.md`; they are not counted as validation of the
rebased tree.

## Initial environment inspection

- Installed toolchain: `nightly-2026-09-20`, including rustfmt, Clippy, rust-src,
  and the `wasm32-unknown-unknown` target required by the new upstream pin.
  `rustc --version`: `1.100.0-nightly (feaadeeac 2026-09-19)`.
- Git Bash, Strawberry Perl, cargo-leptos 0.3.1, wasm-opt, Node, and Puppeteer
  25.10.0 are available. `npm --prefix integration ls --depth=0` passes.
- All 16 Git LFS assets are hydrated, including seven new `xiv-startup` packs,
  seven full `xiv-db` packs, icons, and maps.
- The prior `target/` directory is absent. Native and WASM validation require
  fresh compilation; old ignored build logs cannot be reused as evidence.
- Inspection found approximately 673 GB free disk and 20 GB free memory.
- No prior task-specific QA containers or listeners remain. Cached
  `postgres:17-alpine` and `clickhouse/clickhouse-server:24.8-alpine` images are
  available for new isolated loopback services.

## Required validation scope

The `AGENTS.md` and gate scripts were read directly from `origin/main` before
builds. Required checks remain root `./check_ci.sh`, separate client validation
through cargo-leptos, and relevant browser E2E. The upstream vendored
reactive-graph gate additionally includes `effect_disposed_first_run` alongside
the existing concurrent memo and asynchronous source-walk tests.

Upstream market requests now use `format=columnar` for cheapest listings,
recent sales, sale statistics, and listing statistics. Browser fixtures must
provide the new wire shape while preserving the existing row-value and
interaction assertions. Upstream also adds startup game-data packs and a
`wasm-symbols` tool for generated symbol maps; these are distinct from the
previous branch's validation environment.

The cold Windows validation uses command-only
`CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`, consistently across
CI and preview builds. This avoids the prior MSVC archive-size issue and reduces
debug artifact size. Assertions, overflow checks, features, and test selection
remain unchanged; repository Cargo profiles are not edited. Initial parallelism
is four compiler jobs, with resource pressure monitored.

## Validation results

- Root `./check_ci.sh` **passed** on the reconciled source: **2,640 tests
  passed** across **56 suite invocations**, **87 existing ignored**, and **6
  explicitly filtered live Universalis tests**. Counts include tests repeated
  by required commands. All-targets Clippy and explicit xiv-gen feature Clippy
  passed. Native app tests passed **666/666**, server tests **488/488**.
  Startup-pack projection/reference checks and the new
  `effect_disposed_first_run` gate passed. Log: `target/analyzer-rebase-ci.log`.
- `node --test integration/*.test.cjs`: **127 passed**, no failures, including
  the five columnar market-wire fixture tests. Log:
  `target/analyzer-rebase-js.log`.
- After the browser readiness test change, the required final root gate
  **passed again**: **2,640 passed / 56 suite invocations / 87 ignored /
  6 live tests filtered**. The JavaScript suite **passed again, 127/127**, and
  the changed browser file passed `node --check`. Final logs:
  `target/analyzer-rebase-final-ci.log` and
  `target/analyzer-rebase-final-js.log`.
- Fresh native preview build **passed** with `test-auth` in 6m29s. Log:
  `target/analyzer-rebase-server-build.log`.
- Hydrated client build **passed**: Rust compilation took 9m02s, followed by
  successful wasm-bindgen 0.2.126 generation and Tailwind compilation. Log:
  `target/analyzer-rebase-client-build.log`.
- Four-case history matrix **passed** unchanged on matching server/client
  commit `6c953c08`: bare entry, explicit-locale Recommended, remembered view,
  and chosen default overriding the remembered view. Every case preserves
  the complete previous URL, exactly one added history entry, and client-side
  Back/Forward. Log: `target/analyzer-rebase-matched-history.log`.
- Matching scoped browser driver completed with **exit 1**. Desktop/mobile/
  wide smoke, eager search, projected startup/deferred detail, item layout,
  FC breakdown/world, analyzer world URLs/grids/restoration, market-window,
  expanded analyzer-consistency, and all eight dashboard screenshots passed.
  Only shared-analyzer-data failed: its mobile pointer click targeted a header
  while asynchronous auto-fit moved it, opening Partial feed instead of
  Amount. The complete standalone component subsequently **passed** after its
  fixture opener waited for the existing `data-auto-fitted` completion
  counter; no application code or substantive assertions changed. This is
  a failed aggregate followed by a clean affected-component rerun, not a
  claim that the aggregate returned zero. Driver log:
  `target/analyzer-rebase-matched-e2e.log`.
- Complete shared-analyzer-data standalone **passed** with
  `CHECK_ANALYZER_ROUTES=1`: deterministic query/header/keyboard/touch tests,
  Flip sale columns, populated Flip/Recipe/Leve/Venture/FC/Vendor Resale/
  Vendor Sell/Scrip columns, and Trends aliases/window/suspicious control.
  Log: `target/analyzer-rebase-shared-data.log`.
- Focused Currency Exchange browser checks **passed**, including populated
  columnar data, native estimate/raw listing distinctions, selected-window
  statistics, legacy bounds, saved columns/quantity, shared Views placement,
  desktop/mobile menu interaction, and mobile cell/header alignment. Log:
  `target/analyzer-rebase-currency.log`.

Commands use the verified Git Bash and Strawberry Perl paths:

```bash
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:/usr/bin:$PATH"
export CARGO_BUILD_JOBS=4 OPENSSL_RUST_USE_NASM=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
./check_ci.sh
export CARGO_ENCODED_RUSTFLAGS=$(printf -- "--cfg=web_sys_unstable_apis\037--cfg=erase_components")
cargo leptos build --server-only --bin-features test-auth
unset CARGO_ENCODED_RUSTFLAGS
cargo leptos build --frontend-only --bin-features test-auth
```

The explicit native flags preserve the repository's two cfg values and avoid
cargo-leptos adding a redundant erase-components flag that invalidates the
native cache. The server's test-auth override also avoids optional jemalloc on
MSVC. Neither adjustment changes the native gate's test selection.

For local browser loading, `wasm-opt --strip-debug --all-features` removed only
debug/name sections from the generated asset: 678,346,807 bytes became
47,243,264 bytes. No optimization or application logic transformation was
requested. The original asset remains in ignored
`target/ultros-rebase-with-debug-names.wasm`; log:
`target/analyzer-rebase-wasm-strip.log`. This preview does not exercise the
release symbol-map packaging workflow.

The preview runs on `http://127.0.0.1:61335` against disposable loopback
Postgres and ClickHouse containers named `ultros-analyzer-eea6-pg` and
`ultros-analyzer-eea6-ch`. Test market isolation and disabled websocket ingest
prevent background market writes. One synthetic item-2 sale-statistics row
per world/window makes the empty-database smoke API available; populated
analyzer assertions use intercepted deterministic columnar fixtures. No
production database is involved.

The first browser attempt exposed a preview setup mismatch: the server was
built before the implementation commit (`ef1dda33`), while the client was
built afterward (`6c953c08`). Market API `x-ultros-commit` headers therefore
correctly triggered `ReloadWhenStale`, and Back became a document navigation.
A no-default `/items?v=1` control reproduced it; `/help` stayed client-side.
The mismatched driver was interrupted and its results are not counted.
Rebuilding the native stamp (41.42s), restoring cached frontend assets
(1.50s Rust build plus 30s wasm-bindgen), and restarting the preview resolved
all four unchanged history assertions. No source fix or assertion weakening
was needed. Matching build logs use the `analyzer-rebase-matched-` prefix.

## Browser commands and scope

The matching preview is the explicitly identified binary and asset directory
from this worktree, so reusing it does not test another worktree's server.
Browser components run sequentially; the route screenshot runner uses two
pages concurrently. Strict console and content/overflow assertions remain on.

```bash
BASE_URL=http://127.0.0.1:61335 node integration/back-nav-history.cjs
export REUSE_SERVER=1 SKIP_BUILD=1 BASE_URL=http://127.0.0.1:61335
export E2E_BLOCK_EXTERNAL=1 CONCURRENCY=2 CHECK_ANALYZER_ROUTES=1
export RUN_LISTS_V2=0 RUN_RECIPE_PLANNER=0
unset LEPTOS_FEATURES
./scripts/run_e2e.sh
BASE_URL=http://127.0.0.1:61335 CHECK_ANALYZER_ROUTES=1 npm --prefix integration run test:shared-analyzer-data
BASE_URL=http://127.0.0.1:61335 npm --prefix integration run test:currency-exchange
```

The scoped driver retains desktop/mobile/wide smoke, search responsiveness,
upstream projected game-data startup/deferred detail, item layout, FC
breakdown/world, analyzer world URLs/grids/last views/shared data/window/view
consistency, and dashboard checks. Unrelated Lists, authentication, and recipe
planner flows are intentionally excluded from this rebase pass. Live
Universalis and database-dependent ignored Rust tests, a dependency security
audit, and release packaging are not claimed as locally executed.

The item-layout component explicitly skipped populated listing/history
horizontal probes and greater-than-ten-row expansion probes because those
tables are empty in the disposable database. The remaining layout assertions
passed at every guarded width.

Fresh Currency screenshots were visually reviewed and copied to
`docs/reviews/2026-09-20-analyzer-audit/24-currency-shared-toolbar-desktop.png`
and `25-currency-shared-toolbar-mobile.png`. Their footer is `6c953c08` and
their market values are deterministic test fixtures, not live market quotes.
Views appears beside Columns in the shared toolbar, the duplicate results
heading is absent, and mobile controls fit within 393 pixels; the grid retains
its intentional internal horizontal scrolling.

## Cleanup

Validation is complete. The verified worktree preview process (PID 79984)
was stopped, its port 61335 listener is absent, and the exact disposable
`ultros-analyzer-eea6-pg` / `ultros-analyzer-eea6-ch` containers were removed.
Logs, ignored generated assets, browser artifacts, and the two tracked review
screenshots are preserved. No production services or other worktrees were
modified by cleanup.
