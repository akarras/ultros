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

### Validation of the launch-review fixes

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

The broad E2E gate and optimized release comparison are still being validated;
no complete browser pass or bundle result is claimed by these focused results.
