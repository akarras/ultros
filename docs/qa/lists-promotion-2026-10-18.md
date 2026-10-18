# Lists 2.0 integration verification — 2026-10-18

Candidate: [#1665](https://github.com/akarras/ultros/pull/1665), combining
[#1627](https://github.com/akarras/ultros/pull/1627) and #1658–#1661 with current
`main`. The original promotion head alone does not include the integration fixes.

## Source and environment

- Base: `0254444f2e4180233b93924dea9dfb002ad0518a`.
- Imported combined head: `05f9b50a89bbc439cf935576a34bbb8df3da34a5`.
- Frozen implementation tree, before adding this evidence note:
  `a1ae6956979b4074e81ab56122325b45c7f1c0e4`. The build footer names the base
  commit because the integration was verified before committing it.
- Windows, the repository's `nightly-2026-09-20` toolchain, cargo-leptos 0.3.1,
  wasm-bindgen 0.2.129, and the locked Puppeteer dependency.
- An isolated local PostgreSQL 14 database, synthetic test-auth accounts,
  invalid Discord credentials, disabled market ingestion, and no telemetry DSN.
  `ULTROS_TEST_MARKET_ISOLATION=true` prevents background market sweeps from
  racing deterministic fixtures. A loopback-only ClickHouse 24.8.14.39 service
  supplies the analytics dependency for the broad route suite.
  This does not use production accounts or write production data.
- Local dev builds omit frontend debug symbols and use
  `profile.dev.package.ultros-app.debug=0` for the native app to avoid Windows
  archive limits. Native checks reuse the exact static OpenSSL installation
  produced by the first vendored build, with `OPENSSL_NO_VENDOR=1`,
  `OPENSSL_STATIC=1`, and `OPENSSL_DIR` set to the absolute path of
  `target/qa-openssl`.
- Tested artifact SHA-256 values (the debug artifacts are not release-bundle
  measurements):
  - Native server: `121718433E636470ECBA34D2D2DB4FCEE19CA103D44FD293E985515DE8AEC219`.
  - WASM: `EEEF01C064ED8551B83FA01E0C77F9A39D36054642EDAC3F4F3FAF18EBDEA4F5`.

## Verification

- `node --test integration/*.test.cjs`: **181 passed**, no failures or skips.
- `npm --prefix integration run test:list-companion`: **passed**.
- `npm --prefix integration run test:account-list-storage`: **passed**.
- Changed browser scripts and the E2E driver: **syntax checks passed**.
- Full `./check_ci.sh`: **passed**, including workspace Clippy, the
  feature-gated xiv-gen checks, deterministic Rust tests and vendored reactive
  graph regressions. Repeated after the cart and focus fixes.
- Fresh WASM compilation and wasm-bindgen packaging: **passed**. Full
  `cargo leptos build --bin-features test-auth` with the local debug overrides:
  **passed**. Both targets were rebuilt after the focus fix.
- `test:lists-default`: **passed**. Fresh and retired Labs cookies, direct
  account/device routes, client navigation and removed Labs settings work
  without an opt-in. Obsolete `labs`/`cart=legacy` URLs keep working.
- `test:lists-v2`: **passed**. Includes same-count selection replacement and
  quality-rekey disarming bulk deletion, restored row actions, sorting,
  keyboard focus, wide/desktop/phone row geometry, offline cold reload,
  cross-tab drafts, portable restore and companion lifecycle.
- `test:list-priced-acceptance`: **32 scenarios passed**, with no application
  or cleanup errors. Includes real database stock, desktop/mobile direct and
  client navigation, exclusions, shared readers, transient sync recovery,
  companion access changes, delayed-confirmation focus, partial purchases,
  unavailable prices and changed/disappeared listings. The first run after the
  responsive fix reached the collaborator-focus assertion, where it failed.
  Diagnostics distinguished editor replacement from focus loss: the same
  editor and draft survived, but focus moved to `shop-change-route`. The
  unguarded delayed trip-confirmation callback caused this race. It now
  preserves newer focus, matching route adoption's existing guard, and the
  browser regression deliberately holds the callback until typing starts.
  An earlier isolated rerun passed the full focus/keyboard sequence, then
  exposed a stale device-pricing test assumption: its own Build-reference
  checks started a trip, intentionally pausing eager repricing. The probe now
  adds the unpriced row, checks the known subtotal, and explicitly refreshes
  before asserting lookup failure. No application repricing behavior changed.
  The full corrected matrix passed against the focus fix. Its final stale
  English assertion was also updated from "3 units still unpriced" to the
  current "Unpriced units: 3". Earlier failures are retained separately.
- `test:list-flow`, `test:list-sync`, `test:shared-list`,
  `test:list-estimate-allocation`, `test:list-sort-accessibility`: **passed**.
  Covers account editing/deletion, permissions and stale responses across
  account switches/navigation, invite/sharing flows, quality-aware allocation,
  keyboard sorting, the accessibility tree and mobile controls.
- Broad `scripts/run_e2e.sh`: **not green**. The desktop route pass reported
  13 console errors from 503 responses on market-history consumers. Direct
  probes confirmed 503s from `sale_stats` and windowed `listing_stats` against
  the empty local analytics database. Search navigation also timed out and
  the NPC map-preview image assertion failed. These are unresolved local
  results, not evidence of passing the broad browser gate.
- `test:lists-readonly`: **passed**. This probe covers direct
  SSR/hydration and client navigation, desktop/mobile paused surfaces, readable
  purchase progress, rejection of stale-client subscriptions and updates, and
  unchanged projected rows. Screenshots belong under
  `integration/artifacts/lists-readonly/`.

The first priced run failed the horizontal-overflow assertion and screenshot
review found hidden item names. The cart now switches layouts using the
available panel width rather than the viewport; compact row actions wrap and
the quality picker retains a readable width. A geometry probe initially
measured an intermediate layout immediately after resizing; it now waits two
animation frames before measuring, with the same visibility assertions.

The initial shared-cache frontend attempt failed to resolve compiled crates;
the fresh worktree-local frontend build passed. Initial local-server attempts
also exposed an absolute Windows asset-root incompatibility and a metrics-port
collision; the tested servers use `target/site` and separate metrics ports.
The paused wire probe initially counted the socket greeting as a command
reply; it now excludes that greeting and asserts both actual refusals.
Starting a server-only cargo-leptos build during a later browser run cleared
`target/site`, invalidating that run with missing JS/WASM assets. It was
stopped and the full assets were regenerated. A setup-only retry found the
interrupted fixture's remaining market rows; its exact owner/token and owned
lists were cleaned before the fresh priced run. No builds ran during the
final browser verification. Failed setup is not counted as acceptance.
Interrupted builds and failed attempts are not counted as passing gates.
Local logs use the prefix `%TEMP%/ultros-lists-promotion-`.

## Release handoff

Production was inspected read-only at `/list?labs=lists-sync&lang=en`, showing
footer `0254444`. This is a directory smoke check, not shared-account, offline,
phone, Discord-bot or production projection evidence.

The [release decision](../lists-promotion-decision.md) retains the unsigned
2026-10-14 scoped-waiver proposal. Its remaining pre-merge work is the deployed
hands-on exercise, the production projection check, a same-toolchain release
bundle sanity comparison against `main`, and the owner's decision on that
proposal. These local checks do not sign the waiver or complete #1510.
The broad browser gate also remains open: repeat it with populated analytics
and resolve or reproduce the separate search/map failures before claiming a
complete browser pass.

Before go-live, retain the prior production image's immutable digest. The
[rollback procedure](../lists-sync.md#rollback) covers the runtime pause switch
and a full image rollback without deleting list data.
