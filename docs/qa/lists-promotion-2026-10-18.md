# Lists 2.0 integration verification — 2026-10-18

Candidate: [#1665](https://github.com/akarras/ultros/pull/1665), combining
[#1627](https://github.com/akarras/ultros/pull/1627) and #1658–#1661 with current
`main`. The original promotion head alone does not include the integration fixes.

Earlier sections retain results for their named revisions. The final candidate
section records the later implementation, bundle comparison and current limits.

## Original candidate source and environment

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

## Original candidate verification

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

## Original candidate release handoff

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

## Launch review follow-up (original candidate `82891f5e`)

Muse Wonder Full, Opus 5.5, and a fresh Sol 6.1 each reviewed the complete
implementation independently. Three new tasks then gave each model all three
reports and the parent's objections for source-based cross-review. The initial
candidate stayed unchanged throughout those six reviews and the reproductions.
Agreement was not treated as runtime proof.

The surviving launch blockers were:

| Finding | Evidence on the original candidate | Remediation |
| --- | --- | --- |
| SOL-01: first-touch locking | Three PostgreSQL connections reproduced an empty locking read followed by an unlocked ordinary read; another writer could commit before the stale snapshot overwrote it. | One returning `SELECT ... FOR UPDATE`, including the post-insert reload. Real PostgreSQL addition and deletion regressions both fail the old sequence and pass the fix. |
| SOL-03: offline edits after Write-to-Read | Real browser: acquisition `1` was persisted offline, owner downgraded to Read, reconnect deleted the snapshot; server remained at `0`. | Separate edit refusal from lost Read, preserve and durably pause the local branch, show the authorized server copy, and require explicit retry after Write returns. |
| OPUS round-2 NEW-1: expired session | Real browser: remove the auth cookie while offline, reconnect anonymously, receive a read-denial and lose the persisted snapshot. | Anonymous document subscriptions now report a lapsed session; close sync and keep the original account's snapshot. |
| SOL-02: surviving-tab account binding | Two tabs in one disposable profile: A's tab survived B's login and socket reconnect, then client-navigated to B's private list. B's document was saved under A's key. A's separate REST request was correctly denied with 403. | Bind subscription/update traffic and account-bound REST reads to the snapshot's account. A mismatch detaches without purging and asks for a full reload. |

The account reproduction kept the original page document (no reload), rendered
B's private name and row, received its document handshake, populated
`ultros.listdoc.v1.A.<B-list>`, and left B's corresponding key empty. This was
a browser namespace violation; the server correctly authorized the actual
cookie's B session. Fixtures and disposable browsers were cleaned with no
cleanup errors. The first attempt stalled with the source tab in the background;
bringing it to the foreground made the reconnect deterministic. That stalled
attempt is not counted as a successful reproduction.

Two additional fixes survived cross-review:

- OPUS-1: identical worker preparation fetched all seven fixture resources again,
  including WASM with `cache: reload`, and replaced the generation. Reuse now
  requires the complete canonical manifest, locale, shell version and every
  cached dependency. Changed or partially evicted generations still stage
  transactionally. World topology is refreshed with the generation rather than
  on each visit; no claim is made about measured browser transfer bytes yet.
- MUSE-1 / OPUS-2 and SOL-04: the incident guidance overstated browser-only pause
  and image rollback, and named the old tracing target. It now requires stopping
  all affected writers for shared merge/projection faults, corrects the target
  to `ultros_lists::sync`, and distinguishes format compatibility from retention
  of the new safety fixes. No all-writer flag was added and no waiver was signed.

Cross-review downgraded the ordinary in-flight revocation window: merely moving
the permission read into the document transaction cannot serialize it with
independent share/group revocation. No claim of closing that window is made.
The large-document capacity concern still needs measurement; transport has a
default limit, so it is not literally unbounded. A missing projection on
`Updated` remains a hardening concern for already-divergent data; `Removed`
with an already-absent row is not divergent. The first-touch fix removes the
demonstrated source of that divergence. Production checker differences still
block launch. The stale-world fallback translation and stale-tab pause messaging
remain nonblocking follow-ups. LWW/remove-wins quality-change semantics remain
intentional and should be included in the hands-on exercise.

Validation of the revised implementation is recorded separately below; the
original passing checks above do not validate these follow-up changes. All
production-only evidence and owner decisions in the release handoff remain
open unless explicitly updated with their own evidence.

A fresh post-fix source review found the original P1 paths closed. Follow-up
changes preserve the visible saved branch when a paused document has no server
snapshot after reload, fence cached REST permissions by viewer, refresh the
read-only server copy from relay deltas, and track the paused state when disabling
controls. The account regression now remounts from a non-list route before
detecting the stale identity, and asserts no permission-only index entry is
created under the previous account. The rollback limitations in that review
are covered by the updated format-compatibility section of the runbook.

One deliberate recovery limitation remains: a user who stays at Read retains
the saved branch and its review banner; export is available, but there is no
discard action in this state. Restoring Write alone does not resend it; an
explicit retry is required. A separate confirmed-discard flow needs the same
durable replacement ordering as ordinary recovery. Storage failure is still
reported: successfully saving the snapshot does not report Saved unless the
write-pause marker also sticks. No claim is made that a browser can persist a
new marker when its storage refuses writes.

### Earlier validation of the launch-review fixes

The implementation tree before this evidence-only addition is
`11a7ee5769e331d052d67cc8fca7e3ac761fcdac`, on top of `82891f5e`.
The additional Opus source review inspected the earlier indexed tree
`5ef6b8a6288871648395c5c2f3a4edd4147df379`; its follow-up changes and
the browser-discovered issue below were made afterward.

- `./check_ci.sh` passed after the final Rust change, including workspace
  library/binary tests, CRDT convergence, the non-default CSV feature and
  vendored reactive-graph regressions. Existing ignored tests remain ignored.
- The DB worker separately ran 20 focused document/adoption/projection tests,
  including the eight opt-in PostgreSQL cases, in disposable private schemas.
  Both new three-connection race regressions fail the old lock/read sequence.
- JavaScript unit regressions: **199 passed, 0 failed, 0 skipped**. The real
  production-worker/helper browser fixture also passed a disconnected cold
  reload with anonymous shell and durable device contents.
- `cargo leptos build --bin-features test-auth` passed for server and client,
  with the same local debug overrides described above. The final debug server
  SHA256 is `1183ec305f4a62eac2fcab9fd4dcae05b7e5607315786c7cf20591b6b23d67b0`;
  delivered debug WASM is
  `70b5fafbfe53cfa574d4abd0dbb2967375736d8157cd27098d3f928360fa1a79`.
  These are not optimized release-bundle measurements.
- The isolated account-lifecycle browser run passed all cases with **zero
  page errors**: Write-to-Read keeps and exports the branch, a socket-blocked
  reload shows the saved copy, a server snapshot restores the authorized view,
  regrant alone never sends rejected edits, explicit retry converges, a lapsed
  session preserves work, and a stale tab cannot write document or permission
  metadata under the former account. Reload and subsequent login put each
  account's data in its own namespace and sync the former account's saved edit.
  All owned list fixtures and disposable browsers were cleaned up.

The first lifecycle attempt exposed a real cart teardown panic when the account
warning conditionally removed the workspace during queued cart updates. The
warning now hides the detached workspace until route teardown. A fresh build
and the complete lifecycle run then passed without panics. That failed attempt
is retained as a failure, not counted as a pass. Its separate undefined-valued
Puppeteer object assertion was corrected to serialize an explicit null.

The NPC map-preview probe passed at 390 and 1440 pixels on the fresh build.
Search reproduced its earlier timeout: the selected `/about` became
`/about?lang=en`, but the test demanded an empty query string. The corrected
probe checks the destination and all its query parameters while requiring the
current locale to survive; its desktop/mobile Enter/click matrix passed.
This was an assertion correction, with no search application change.

Those focused results did not establish a complete browser pass or bundle result.
Later validation is recorded below.

## Final candidate validation (`1eafc0f1`)

The implementation is frozen at
`1eafc0f1f1af0b388afd276b190e4fbc9e34ac60`. Later changes only refine test
synchronization and record evidence. The full CI script passed before each
implementation commit. A fresh `cargo leptos build --bin-features test-auth`
with the same local debug overrides passed for both targets. The matching
server SHA256 is
`f615608585b34991881ae2ec59c12ef7cda796c130cdb8cc7e8f450b10b4f105`;
debug WASM is
`c1a6c843c05a9874b813b0d99c3e3212d6cbd83c050396f3aab8bed6aa22e75e`.

After the artifact freeze, remote `main` advanced from `0254444f` to
`d57d4970` (#1671, retainer setup/Discord crate extraction). A non-mutating
`git merge-tree --write-tree HEAD origin/main` check completed without conflicts
and produced tree `d7d973f305653c9b28d2e8b91dce5c2660c79462`. That newer main
is not included in these artifact measurements; hosted checks must validate
the PR against its current base. The recorded comparison remains against its
exact named baseline, not an unspecified moving main tip.

The complete `lists-v2.cjs` run passed on that fresh debug client, including
desktop/mobile layout, quality changes, offline cold reload, cross-tab drafts,
undo and companion lifecycle. It first exposed a real native-select layout bug:
after switching from compact to wide, the quality select's `display: contents`
parent left it in the wrong grid cell. A normal wrapper now owns its container
query order; the unchanged geometry assertions pass after the fresh build.

The priced acceptance matrix passed **32 scenarios with zero application or
cleanup errors**. Its browser now honors `E2E_BLOCK_EXTERNAL=1`, as the documented
driver does. An earlier attempt stalled after the shared reader; it was stopped
and its four owned list fixtures and market fixture were cleaned. No cause is
claimed for that stalled attempt. The corrected device-price probe also passed
twice in fresh normal invocations: it waits until the Remove button is a real
pointer target rather than clicking through a toast, and waits for the specific
denied permission response instead of mistaking a Suspense loading detach for
denied availability. The original remount, draft-reset, validity, trip retention,
no-purchase and remote-completion focus assertions remain intact.

### Current-toolchain bundle sanity check

Both separate source checkouts were clean and `git lfs fsck` passed. Their
toolchain, lockfile, build/data input hashes, source trees, output hashes and
cache handling are retained in
[`lists-launch-bundle-2026-10-18.json`](lists-launch-bundle-2026-10-18.json).
Both used `cargo leptos build --release --frontend-only --lib-cargo-args=--locked`,
then the repository's `wasm-symbols` extraction/name stripping and GNU
`gzip -n -9` for the budget comparison.

| Stripped client | `main` (`0254444f`) | candidate (`1eafc0f1`) | delta |
| --- | ---: | ---: | ---: |
| WASM bytes | 17,921,737 | 17,353,083 | -568,654 |
| GNU gzip bytes | 5,679,018 | 5,529,702 | -149,316 |

The lighter 750,000-byte growth check passes with 899,316 bytes of headroom.
The candidate also emitted 3,560,867-byte Brotli and 5,536,328-byte helper gzip
sidecars; the latter uses a different compressor from the GNU budget result.
Build durations are not comparable because the candidate reused valid external
compiler cache after explicitly cleaning every workspace package. This local
`wasm-release` pipeline retains the panic hook and does not exercise Docker's
build-std/immediate-abort setting. It is not the historical frozen Linux pair
and does not sign the scoped waiver.

### Optimized-client browser validation

The matching `1eafc0f1` debug test-auth server served the candidate's optimized,
stripped client and generated compression/symbol sidecars from a separate site
root. A real HTTP fetch of `/pkg/1eafc0f1/ultros.wasm` returned gzip and decoded
to exactly 17,353,083 bytes with SHA256
`b83728ad1ae1fb110689380b0ef766cd0a4ad00e0ba61e8f52e8c82039aad430`,
matching the measured artifact. Every probe used fresh disposable browser
profiles. This checks the optimized client against a matching-commit debug SSR
server, not the production Docker server or immediate-abort runtime.

All of these normal invocations passed:

- `lists-default.cjs`: default SSR, hydration, client navigation, old Labs
  cookies/bookmarks and removed settings.
- `lists-v2.cjs`: complete desktop/mobile, editing, offline, cross-tab,
  backup and companion suite.
- `list-sync.cjs`: convergence, offline/reconnect and undo, REST updates,
  revocation/deletion, stale responses across navigation, and the complete
  account lifecycle regression. Under sustained 100ms broadcasts, the first
  access probe started at 1,004ms and Write-to-Read controls updated in 1,065ms.
  Held stale Write, 403 and 404 responses could not undo newer Read access.
  Downgrade preserved and exported the offline branch; blocked-socket reload,
  server-copy restoration, explicit retry after regrant, expired-session
  preservation and the original fresh-mount account-switch case all passed.
- `list-priced-acceptance.cjs`: **32 passed, zero application/cleanup errors**,
  using real local database market fixtures.
- `device-build-prices.cjs`: device and account coverage, unavailable controls,
  safe remount/draft reset, remote-completion focus and incompatible-peer
  companion closure.
- `list-flow.cjs`: account composer/recipe, exclusion bookmarks, keyboard
  acquisition, rename, invite/read-only sharing and deletion.

The flow probe first reproduced its earlier navigation timeout. An instrumented
run observed a real `beforeunload` dialog at both deliberate reloads. The test
now accepts those navigations, as the other account probes already do; the fresh
normal invocation passed. It and the sync probe also honor the documented
`E2E_BLOCK_EXTERNAL=1` setting. The two failed flow runs' owned lists (197, 248)
were explicitly cleaned and verified absent. Other owned fixtures and browsers
were cleaned by their test finalizers.

Failed attempts remain failures. One debug full-sync rerun timed out acquiring a
held response; setup acquisition now uses the outer fixture timeout while actual
Write-to-Read and overlapping-probe assertions keep their 2,500ms budgets. A
later run timed out waiting for a newer overlapping probe; an isolated debug run
and the complete optimized run passed without relaxing that assertion. Another
lifecycle run reached the real 32-second socket-constructor reconnect backoff
while its outer timeout was 30 seconds; the test now explicitly awaits the
handshake for up to 40 seconds before checking the server copy.

### Broad browser gate and remaining release work

The full `REUSE_SERVER=1 SKIP_BUILD=1 E2E_BLOCK_EXTERNAL=1
LEPTOS_FEATURES=test-auth CONCURRENCY=2 ./scripts/run_e2e.sh` run against the
earlier launch-review-fix build exited 1. It is **not a complete browser pass**.
The following failures remain open in that broad run:

- The desktop route runner reported 18 console assertions from 503 responses
  on item/category, flip-finder and recipe-analyzer routes; desktop/mobile
  dashboard Trends also received 503s.
- Analyzer world-URL/grid/shared-data probes timed out on missing profit,
  amount or price-scope controls; the virtual-grid probe also failed.
- The item-source navigation probe timed out on its clipboard selector.

A local ClickHouse process was healthy, but that does not establish a populated
analytics environment or explain every missing control. Repeat these probes in
populated QA and resolve or reproduce their failures before claiming the full
browser gate is green. The broad run's Lists geometry, flow, sync and device-price
failures have the focused fresh-client reruns and dispositions above. The separate
search locale assertion and NPC map preview also passed their focused reruns.

Final deterministic verification runs `./check_ci.sh` and
`node --test integration/*.test.cjs` before committing this test/evidence update.
The latter covers 199 JavaScript regressions; the DB worker's real PostgreSQL
race/adoption/projection checks remain separately recorded above.

The remaining production preconditions in
[`lists-promotion-decision.md`](../lists-promotion-decision.md) are the deployed
two-account/two-browser/phone/Discord-bot and legacy-versus-Labs exercise, the
read-only checker over every production account list, and the owner's scoped
waiver decision. Retain the previous image's immutable digest and Git SHA before
promotion. After deploy, repeat the projection check in 3–7 days and review
GlitchTip and `ultros_lists::sync` daily for a week. The pause flag only pauses
browser routes/socket edits; suspected shared merge/projection corruption
requires stopping every affected writer process and replica. No deployment,
merge, production checker run, operator exercise or waiver signature is claimed.
