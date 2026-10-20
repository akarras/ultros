# Dalamud review repairs — 2026-10-20

The seven existing PRs (#1672, #1674–#1679) were reviewed independently by GPT-6.1-Sol and Muse Wonder Full. Ten confirmed findings were repaired. Muse follow-up reviews of the affected areas found the original findings resolved; runtime verification remains distinct from static review.

## Fixes

| Finding | Layer | Result |
| --- | --- | --- |
| D1672-1: checkpoint limits and lifetime writer cap | #1672 | Typed edits/groups preflight before mutation. Undo reserves capacity before consuming a step. Exactly acknowledged history can compact after 50,000 retained operations, fenced on durable storage. The whole causal vector is preserved without a lifetime peer cap. Snapshot, decoded-byte and retained-operation limits remain enforced. |
| D1672-2: related legacy snapshots rejected | #1672 | Shared causal operations establish ancestry for supported schema-less documents; unrelated lookalikes still require review. |
| D1674-1: legacy owner notifications lose their ping | #1674 | An explicitly authorized legacy owner/channel destination permits only the owner mention. Normal endpoints suppress mentions. |
| D1674-2: private registration freezes public retainer city | #1674 | Positive public city metadata corrects stubs; unknown city zero and repeated private registration cannot degrade authoritative data. |
| D1675-1: queued commands can target another list | #1675 | Manual and recovery actions carry the account/device/list identity the player saw and revalidate it when executed. |
| D1675-2: stalled response body outlives timeout | #1675 | Ordinary HTTP requests have one 15-second deadline covering headers and bounded body reads. SSE retains its separate subscription lifetime. |
| D1675-3: minimum-version rejection deletes refresh credentials | #1675 | A definitive refresh 426 retains the protected refresh credential and clears access with update guidance. Known-unsent outages retain the old credential; verification outages after known rotation retain only the saved new generation for retry. Ambiguous rotation still fails closed. |
| D1675-4: release provenance excludes compiled inputs | #1675 | Build and publisher use whole-repository cleanliness, including tracked and nonignored untracked inputs, with Git failures rejected. Development builds remain supported. |
| D1678-1: partial inventory save loses/replays a batch | #1678 | One bounded batch retains its cursor across persistence failure, saves the mutated delta before its tail, revalidates/clamps untouched rows and allows normal sync after non-storage refusal. |
| D1679-1: sales/deltas falsely renew market review age | #1679 | A separate non-backfilled full-board review timestamp is written only after successful full reconciliation. Sales and individual listing events cannot renew price suggestions. |

The market review timestamp describes Ultros's reconciliation of one item/world board; it is not a seller upload timestamp or a claim of global ingest freshness.

## Verification

Validated integrated source checkpoint: `0fec07839818eda0004e3dc8c12f7e45ddd65415`, tree `69e353eae801e624ae73b4ecfa475613e09a0542` (Lists main `09af481faff565226080acbd8917d510ee1a2404` included).

- Root `check_ci.sh` passed after all repairs, and again after the isolated fixture's startup city seed correction. The integrated gate reported 3,055 successful test executions and zero failures; these are executions rather than unique tests.
- All 203 JavaScript regressions passed, with zero failures/skips.
- Two opt-in plugin database tests and all eight Lists document database regressions passed against an isolated loopback PostgreSQL cluster. The plugin fixture covers city correction, private/public isolation, full-board freshness semantics, and an injected review-marker write failure that must leave catch-up eligibility intact.
- Final managed client-only smoke passed, including destination binding, stalled bodies, refresh 426 handling, inventory persistence/retry and stale-tail cases.
- The synthetic release-source Git fixture passed, including compiled API types, toolchain/lock/configuration, untracked inputs and ignored output handling.
- `cargo leptos build --bin-features test-auth,plugin-localhost --bin-cargo-args=--config=profile.dev.package.ultros-app.debug=0` passed for SSR and WASM. The running QA binary/WASM hashes were recorded and verified unchanged after all browser checks.
- The managed plugin Release build passed against the installed Dalamud API 15 references with zero managed warnings/errors. The optimized Windows x64 native DLL, managed/native ABI, browser compatibility fixtures, concurrent merges, duplicate import, undo/redo, finalizer cleanup, failed-save recovery and native shadow-loading checks passed. The package was staged with localhost support and was not published. Its `SourceCommit` was the clean local checkpoint above; publication must rebuild from the selected release commit.
- Real C# HTTP integration passed: browser consent/pairing, scoped access, concurrent/checkpoint sync, Rust estimate, private retainer upload/lookup/delete, SSE wake-up/reconnect/acknowledgment and disconnect.
- The broad driver passed 54 route/viewport visits (18 each at desktop, mobile and wide), plus startup/search, guest/account/group Lists, compaction/conflict recovery, plugin installation/consent/private-retainer and accessibility cases. Optional public listings/history row-layout probes skipped for empty market data. Priced purchase assertions were subsequently exercised with real synthetic server/database rows.

### Browser DNS failures and follow-up evidence

The original broad driver returned 1: `analyzer-grids.cjs` and `list-sync.cjs` collected DNS errors in strict console assertions. Those failures remain in `e2e-debug.log`. A bounded two-page controlled DNS reproduction identified Google Tag Manager's `/gtag/js`; both pages returned 200 and hydrated with zero first-party failed requests. A later trace covering separate browser contexts identified the public Discord default-avatar image path in the synchronization assertion.

The analyzer rerun passed with analytics isolated. Synchronization then passed with analytics and `cdn.discordapp.com/embed/avatars/*` isolated, including all eight stale load/revalidation × 200/403 × navigation/unmount cases. Unknown console locations and all first-party errors remained subject to the original strict assertions. No application source was changed for these reruns.

The required priced aggregate initially recorded 11 of 12 suites passed. Its real market matrix completed all 32 cases with zero application errors. The remaining synchronization suite passed in the separate context-aware rerun above. Thus every required suite has a passing observation on the same runtime build; the original aggregate is retained as failed, with the successful follow-up recorded separately in `functional-results.json`. This is debug SSR/WASM functional evidence; no new optimized-WASM or production-soak result is claimed.

The adjacent `dalamud-browser-external-isolation.cjs` preserves the exact scoped browser policy used for the successful sync follow-up. It blocks the two observed Google analytics hosts and the public default-avatar path through CDP, without changing fixture request handlers. It logs only origins/paths and error text, excluding request headers, bodies and query strings.

To use it against a fresh disposable loopback `test-auth,plugin-localhost` server with `ULTROS_TEST_MARKET_ISOLATION=true`:

```powershell
$env:BASE_URL='http://localhost:55438'
$env:E2E_BLOCK_EXTERNAL='1'
$env:QA_ISOLATE_EXTERNAL='1'
$qaPreload=(Resolve-Path docs/qa/dalamud-browser-external-isolation.cjs).Path.Replace('\','/')
$qaPreviousNodeOptions=$env:NODE_OPTIONS
try {
    $env:NODE_OPTIONS='--require="'+$qaPreload+'"'
    $env:LISTS_ACCEPTANCE_BUILD='<fresh server/WASM revision and fingerprint>'
    node integration/lists-acceptance.cjs
} finally {
    $env:NODE_OPTIONS=$qaPreviousNodeOptions
}
```

### Source and artifact continuity

App binary SHA-256: `BD1613784062F48DF04FEB74684B1EEBFED8726504F9786477494D2EB9345AC8`.

WASM SHA-256: `5E2B05C0884364FD72D6531D6364CF2797891B8403B2FD0C70976860F3E84D38`.

Staged native DLL SHA-256: `731AB47289E1491CC8E36D49B77C410BDD63D7654DA0F26686709A50E564594F`; ABI 2, protocol 1, x86-64 CPU baseline, panic unwinding, no reused installed native binary.

The seven delivery branches preserve the original commits and merge their repaired prerequisite, including Lists main. Each delivery tree passes root `check_ci.sh` before its new commit. The composed intermediate managed layers also receive locked Release builds/client smoke. The final runtime source is checked against the validated checkpoint before adding these QA records and the generated analyzer-column-snapshot `.gitignore` rule; the latter keeps known runtime cache outputs outside the release source boundary.

Local detailed logs and synthetic browser artifacts are retained under `target/qa-dalamud-review-2026-10-20/`. Tests use explicitly configured disposable services and dummy credentials, not production lists/accounts or private production telemetry. Browser market fixtures are synthetic local data. Owned app, PostgreSQL and ClickHouse QA services were stopped after verification; generated local analyzer caches were archived under the QA target directory.

## Remaining release gates

Actual FFXIV/Dalamud in-game acceptance has not been performed by these automated checks. No plugin release was published or deployed. Each PR still requires its own current hosted CI and required review. The Lists #1665 review override does not authorize an override for this stack.

The Lists production observation and operator follow-ups retain their original dates and owners; these plugin checks do not complete the seven-day observation or the production projection recheck.

## Reconciled publication checkpoint and crash-prevention lints

The final runtime was reconciled with the separately restacked PRs at clean local checkpoint `130717cbfd9d2fcf809ebb64091c3bb3557dcc2e`, tree `7fd88b0a78fdbba8a4af4a87ed6d5345a33510c9`. Current remote commits are retained as ancestors of their respective delivery updates. This section supersedes earlier validation counts for this checkpoint without replacing the historical failed logs above.

The shared `ultros-list-doc` and native `ultros-list-doc-ffi` libraries deny production uses of `unwrap`, `expect`, explicit panic/unreachable/todo/unimplemented, unchecked indexing/string slicing, arithmetic side effects and process exit. The core forbids unsafe code; the FFI requires documented unsafe blocks and explicit unsafe operations. Root CI runs a separate non-test library Clippy gate so the test-only exemptions cannot mask production code. Fallible construction, poisoned-undo handling, duplicate row rejection and saturating bound calculations remove avoidable panic paths. Three narrow core `expect` exceptions document pinned-Loro fresh-map/trusted-constructor and integer-only growable-buffer serialization invariants; the native open path uses the fallible constructor. These lints and the FFI unwind boundary cannot guarantee immunity to dependency bugs, process aborts or allocation failure.

Reconciliation also retains the legacy v1 retainer response shape while moving rich private observations to v2; adds CIDR-aware trusted proxy handling; prevents botless workers from consuming Discord delivery attempts; fences queued commands by cart instance and recovery generation; safely retries known-unsent refreshes and verification of known rotated credentials; retries profile ownership during hot reload; and aligns bounded HTTP/WebSocket/native causal-version envelopes for actual 5,000-writer histories. Existing inventory persistence cursors, stale-tail revalidation, notification authorization/deduplication, full-board freshness and release-source provenance remain intact.

Validation of this exact runtime:

- Root `check_ci.sh`: 3,063 successful test executions, zero failures (104 ignored; 70 result blocks). Each of the seven published delivery commits additionally requires its own passing root gate before commit.
- 203 JavaScript regressions passed. Locked managed Release build: zero warnings/errors. Client-only smoke passed.
- Disposable real PostgreSQL: three plugin tests, one independent legacy v1/v2 response compatibility test and eight Lists document tests passed. No private production database was accessed.
- `cargo leptos build` produced the debug SSR and hydration client with `test-auth,plugin-localhost`. The managed/native staged Release package passed ABI/concurrency/undo/persistence/finalizer/lock/corruption checks, ten native unit tests and cart-command tests. Real C# HTTP integration passed browser consent/pairing, scoped APIs, checkpoint/concurrent sync, estimate, private captures, SSE reconnect and disconnect.
- All 12 required Lists acceptance suites passed in one aggregate run with no skips/allow-empty override, including 32 priced cases with zero application errors. This is local debug SSR/WASM evidence, not optimized-WASM, physical FFXIV or production-soak evidence.
- The full broad driver passed all 54 route/viewport visits and the plugin/Lists/permission/install/private-retainer/accessibility scenarios, but returned 1 for two analyzer probes: `Node is detached from document` in the vendor filter and a shared-analyzer reload/sort timeout. The unchanged shared-analyzer probe passed in a focused retry. The vendor issue was isolated to menu setup: grid scrolling intentionally dismisses its popup. A test-only setup repair waits for stable grid geometry and opens at a visible heading point, retaining real input events and all filter/world/history/scroll assertions. The successful five-tool follow-up is recorded separately; the original broad run remains failed. A locator-only attempt that failed to repair dismissal is retained. One diagnostic accidentally selected a different loopback server because `BASE_URL` was missing; it was stopped and excluded from accepted evidence.

The scoped third-party isolation policy above remains in use. JS-disabled SSR probes intentionally report the unexecuted JS request as failed; the named build's JS resource was independently HTTP 200 and normal hydration scenarios passed. These are distinct from application errors and from deliberate fault-injection failures.

Reconciled artifact fingerprints:

- SSR executable: `E74D67A25578ACAA5D323783E07A4B503187509593F1999D90BF5EA91B81193A`.
- Hydration WASM: `B3892B26149A9C7689F115A43CD9418573603F80F3B17114048F4E229078B1E0`.
- Native DLL: `2C8859038CF8114933CE324F72120F480BA366D2CDCD97649668011A4ADBE5C1`.

Staged build metadata records clean source `130717cb`, ABI 2, protocol 1, x86-64 baseline, panic unwinding and no reused installed native binary. This localhost-enabled staging package was not installed or published. Production packaging must rebuild from its selected clean release commit. The final delivery differs from this runtime checkpoint only in this QA record and the analyzer probe setup. Detailed local logs and dated machine-readable reports remain under `target/qa-dalamud-review-2026-10-20/`.

Actual in-game acceptance and required GitHub review remain outstanding; the prior Lists review override does not apply to these PRs. This work does not complete any private production sync review, projection recheck or seven-day observation.

## Current-main reconciliation after the first admin merge

The dependency and toolchain changes merged in #1680, #1682 and #1681 are
included through authoritative main `03cb75cd54d498792f14cffde9742e7fcd095162`
(the #1672 squash). The combined backend initially failed both Docker and
required Rust CI because its new `ultros-db` lock entry still referenced
`sha2 0.11.0`, while the package set pins 0.11.1. Commit
`8a0c56db92f1534be8344967aca5337ae061119a` corrects that one dependency edge;
all locked package identities remain unchanged from the automatic merge.
The `--locked` checks remain enabled.

Root `./check_ci.sh` passed before that backend commit on exact tree
`b161b6a89d39721a1900a8b7bce3276af2dda6a4`: 3,064 successful test executions,
zero failures and 104 ignored. The production crash-prevention lint gate
for `ultros-list-doc` and its FFI crate passed on nightly-2026-10-13.
The remaining layers incorporate that validated backend while preserving
their previous published heads. Their current-base gate logs and source
refs are retained under `target/qa-dalamud-review-2026-10-20/new-main-*.json`
and `check-ci-new-main-*.log` in the delivery worktree.

The earlier `130717cb` staged DLL, native HTTP, and browser results above
used the old dependency/toolchain set. They are not fresh validation of
this combined base. Current hosted CI, fresh combined-stack browser
validation, and clean current-source package production are tracked
separately. In-game acceptance and public plugin package publication
remain pending. No prior result establishes those steps as completed.

### Fresh current-base browser evidence

The composed stack at `82b2a6875f732b54a6690d8dbc9170d8fdc04d07`
(tree `fbd84fca2ebdacc26cba0ba0a585f215a6033bcc`) was rebuilt with
nightly-2026-10-13 using `cargo leptos build`, locked dependencies and the
local-only `test-auth,plugin-localhost` server features. The owned loopback
server and hydration client were identified by source and SHA-256 before
testing. The fresh SSR executable hash is
`F2FDFAF95708680FB11152B93D4D05B5BA3F126070E92558CFBB76C043867E76`;
the fresh WASM hash is
`53A0CBB68F43D871A7EA02CB18F67D803EFDED3A5A20584AE8AF92583495840B`.

The broad driver passed all 54 route/viewport visits, the two previously
failing analyzer probes, and its first account-adoption execution. It
returned 1: the continuous-broadcast probe was not foregrounded; a later
required-acceptance adoption execution stalled on hydration; and the
accessibility probe raced picker setup and modal focus restoration. The
initial required-acceptance report passed 10 of 12 suites. Original logs
and failures remain in `new-main-e2e.log`, `new-main-e2e-exit.json` and
`new-main-required-acceptance-results.json`; no clean broad aggregate is
claimed.

The test-only follow-up brings the timing-sensitive editor to the
foreground and waits for both three probes and twelve broadcasts within
the existing deadline. Timing and permission assertions are unchanged.
That complete sync suite passed: the first probe took 1,002ms and the
Write-to-Read UI update took 733ms. Accessibility setup now observes the
picker's animation-frame selection and actual filtered option, waits for
the old search dialog to restore focus, and checks the new menu's modal
focus before traversing all 30 Tab steps. A focused run passed all original
assertions. These changes do not modify application code. A fresh required
12-suite aggregate is tracked separately, including its original failed
adoption run; a passing retry must not erase that failure.

All five opt-in plugin browser suites passed against the same fresh build:
bootstrap, protocol/release availability, desktop/mobile guide, consent
links, and private retainer capture/account isolation. The release fixture
was deliberately empty, so this does not validate a newly packaged ZIP.
This build does not validate a current-toolchain native plugin DLL or
physical FFXIV acceptance. Package production and in-game acceptance remain
separate release work.

The next required aggregate passed 11 of 12 suites, including adoption and
sync, but `device-build-prices.cjs` timed out waiting for 40 gil after its
availability simulation; the rendered subtotal was 20 gil. A focused
diagnostic passed with a captured server quantity of four and the same
quantity after the fixture's restoration. That retry does not establish
the failed run's root cause. The fixture previously observed only local
pricing before capturing a full server row for several REST replays. It
now requires the server projection to contain the quantity-four edit and
the newly added Iron row before capturing that snapshot. This adds a
bounded assertion and retains the final 40-gil assertion. Both failed
aggregate reports remain recorded; final aggregate status is tracked
separately rather than inferred from passing individual retries.
