# Lists 2.0 launch validation — 2026-10-19

This follows the [2026-10-18 record](lists-promotion-2026-10-18.md) for
[PR #1665](https://github.com/akarras/ultros/pull/1665). Historical failed attempts
and the unsigned [release decision](../lists-promotion-decision.md) remain part
of the record. No merge, deployment, historical soak, or owner waiver signature
is established by the local checks below.

## Candidate and local environment

- Current `main`: `89b24c88f78e6ad457e302b2f4134ea9b38abcf7`.
- Original integration head: `c941061eb4347759953ad30dd6cd0433b5fb5692`.
- Integrated candidate: `c908431db73f34ee12e7261cf6952c08e52129e1`, tree
  `b1fe009442df9f2f2b77e5106b0bbf32925377dc`.
- Before the test repairs, the merged tree was
  `8883470b7f3263c7b81e5bc966af732e65fd2c54`, identical to GitHub's fetched
  prospective merge `a881e34c6d6546b6683cb1a2c4c3b9c9bf37624e`.
  That commit adds four browser test repairs. Subsequent browser-test/evidence
  changes also leave the integrated Rust code identical to the build used here.
- Windows, pinned `nightly-2026-09-20`, `cargo-leptos 0.3.1`. The native
  build uses a verified OpenSSL 3.6.3 static install with
  `OPENSSL_NO_VENDOR=1`, avoiding the local Git Bash Perl installation's
  missing `Locale::Maketext::Simple`. This changes local build configuration,
  not repository dependency versions.
- PostgreSQL and ClickHouse are disposable instances created for this worktree.
  Dummy Discord credentials cause the documented background bot startup panic;
  the web server stays running. Upstream market writers and local error-reporting
  DSNs are disabled. No production database or sibling QA service is used.
- For the broad browser run, empty sale statistics initially returned 503.
  A synthetic ClickHouse fixture supplies item 5364 on 126 worlds for each
  1/7/30/90-day window (504 rows). Listing snapshots are warmed through the
  normal API/worker. This is local synthetic analytics, not live market coverage.
  PostgreSQL market listings remain empty except during owned test fixtures.

The initial server/client build completed at 18:05–18:07 UTC using
`cargo leptos build --bin-features test-auth` with `profile.dev.debug=0` for
both targets. The running server is this worktree's `target/debug/ultros.exe`
on `127.0.0.1:55438`, using this worktree's `target/site` assets. It was
started after the build; no client rebuild runs against those assets during
browser verification. Its footer reports the pre-commit `89b24c88`, so the
footer alone does not identify this uncommitted integration build:

- Server SHA256:
  `3937e89b809b43971f7ef94dc2f0981cadc940e350e8a41759f7031f3e9cfd20`.
- Client WASM SHA256:
  `50407679af2a41512ef5ca3a369a9469c8d5341ec1a6ade67c419479af8e3155`.
- Build-time index tree:
  `3bb9d354b2530e2b047ff1fce41fb23269bec2d6`; the subsequent accessible-name
  selector repair affects JavaScript tests only.

These are debug/test-auth builds. They do not exercise Docker's
`build-std`/`panic=immediate-abort` production pipeline.

For the clean final browser run, both targets were rebuilt after committing
the implementation. The owned server at `127.0.0.1:55438` and its client now
embed `c908431d`, verified at 18:54 UTC:

- Server SHA256:
  `371dbb486ae42f4cb7df096a377e1cf1a74f30fa55e724f45b5486955fc75eb5`.
- Client WASM SHA256:
  `3f663187e630f0f44f9923c5f8ac3788008c24b9e64c9aa6646c4a26e20d1ea4`.

Only owned QA processes were stopped for that rebuild; browser tests run
after it completes and assets stay fixed during the run.

## Deterministic checks

- Root `./check_ci.sh`: passed after all four test repairs, before committing
  `c908431d`. Includes formatting, workspace Clippy, the explicitly enabled
  CSV feature, workspace unit tests, CRDT convergence integration tests,
  deterministic Universalis tests, pack sanity, and vendored reactive-graph
  regressions. The completed run records 2,960 successful test executions
  across 66 result blocks, zero failures, and 100 ignored test executions;
  these are execution counts, not a count of unique tests.
  The final browser-test/evidence changes must pass the same root gate again
  before their follow-up commit; the updated PR records that run's result.
- `node --test integration/*.test.cjs`: 199 passed, zero failed/skipped.
- Real PostgreSQL `ultros-db list_doc::tests:: -- --ignored`: eight passed,
  including both three-connection first-touch addition/deletion races.
  The first attempt ran before app migrations and failed six fixture setups
  with `relation region does not exist`; both race tests passed then. The
  migrated rerun passed all eight. The failed setup run is not a product pass.

## Browser test repairs and focused results

The earlier record misidentified the clipboard timeout as item-source
navigation. The failing script was `analyzer-actions.cjs`, on Flip Finder:
recommended landing filters excluded its deterministic fixture rows. The
test now uses an explicit `v=1` query while preserving its clipboard,
dialog, keyboard, mobile geometry and action assertions.

Three other probes still targeted the retired filter form/operator controls.
They now use the current Min/Max and presence controls, accessible names for
icon buttons, and the registry's readable URL aliases when those aliases
represent a filter exactly. Assertions still cover the actual filtered rows,
URL preservation, reload, saved-view restoration and hidden filters.

Focused fresh-browser results:

- `analyzer-actions.cjs`: ten tools at both 1440px and 390px passed.
- `analyzer-grids.cjs`: all eight tools, header filters, desktop/mobile
  alignment, cookie restoration, explicit URLs and cross-session fallback passed.
- `analyzer-world-urls.cjs`: all five tools, world precedence, regional
  ingredients, sales scopes, encoded filters and history/reload passed.
- `shared-analyzer-data.cjs`: shared registry, simultaneous bounds,
  pre-calculation inputs, missing/partial values, delayed statistics,
  sort/keyboard/touch behavior, hidden filters and mobile wrapping passed.
  An intermediate run exposed the remaining text-only selector for the
  now-icon-only Hide column button; the accessible-name repair passed.
- `virtual-grid.cjs`: both-axis virtualization/alignment, live updates,
  automatic/manual sizing, resize cancellation, insertion/reordering,
  persistence/history, keyboard and actual emulated phone touch gestures passed.
  It now reads the current `col-order`, `col-widths` and `show-cols` URLs and
  accessible menu names. Intermediate runs exposed the retired packed-layout
  expectation and an incorrect assumption that blurring an edited width would
  discard it. The updated probe verifies that the explicit width persists,
  then resets it and verifies automatic sizing resumes with the latest data.
- `fc-crafting-world.cjs`: a focused rerun passed the route/cookie precedence,
  regional ingredients/statistics, world sales, reload/history and encoded
  query checks. The broad attempt found the SSR control was inside a streamed
  template while JavaScript was disabled. Its selected region was already
  correct in the response; the assertion now inspects the template content.
- `list-account-identity.cjs`: the new regression uses `9007199254740993`
  as a string, above JavaScript's exact-integer boundary. Bootstrap and
  current-user JSON remain exact; the raw browser WebSocket frame carries the
  exact account guard, the server accepts the subscription, and the bound
  REST read succeeds. The fixture account/list are removed. It is included in
  both the authenticated driver and the required Lists acceptance wrapper.
- The route runner now requires the app's `ultros:hydrated` event before its
  existing content, geometry and strict console assertions. Live polling made
  `networkidle0` repeatedly time out on otherwise healthy pages. The earlier
  route pass completed all 54 desktop/mobile/wide visits; the clean full rerun
  also passed with the explicit hydration requirement.
- `lists-readonly.cjs`: passed against the same binary on a separate local
  port with `ULTROS_LISTS_READONLY=true`. Directory, account projection and
  device routes agree before/after hydration at desktop/mobile widths;
  stale clients cannot subscribe or send document updates. REST fixture writes
  deliberately remain supported. This is not an all-writer containment test.

The first complete `REUSE_SERVER=1 SKIP_BUILD=1 E2E_BLOCK_EXTERNAL=1
LEPTOS_FEATURES=test-auth CONCURRENCY=2 ./scripts/run_e2e.sh` attempt exited 1:
all 54 route visits and the other suites completed, but the FC streamed-SSR
assertion above failed. Its focused repaired rerun passed. This remains a
failed broad attempt, not a complete pass.

The clean complete rerun against the matching `c908431d` server/client
**passed (exit 0)**, finishing at 19:18 UTC. It retained strict console,
content and overflow assertions and completed all 54 route visits, the
authenticated Lists/sync/group/account flows, notifications, dashboard and
accessibility probes. `REUSE_SERVER` points only to the owned build identified
above. Default smoke still reports empty-market branches and skips VAPID
subscription creation without VAPID configuration; priced acceptance is the
separate required gate below.

The subsequent optimized run exposed late preference setup in two fixtures.
`list-sort-accessibility.cjs` changed `HIDE_ADS` while the initial `/list`
document was still hydrating: the instrumented run recorded an SSR ad rail,
`hydrated: false`, then the empty-fallback marker panic before entering the
test list. Symbols locate the failure in the global ad-rail `Show` boundary.
Both it and `list-shop-handoff.cjs` now seed ad/locale preferences before
the first SSR request and await initial hydration before changing the dynamic
world preference. The unchanged scenario/error assertions passed focused
reruns against both debug and optimized clients. No runtime Rust change was
made for these fixture timing corrections.

## Current-main bundle sanity

The fresh local pair passes the scoped proposal's 750,000-byte growth budget:

| Build | Implementation | Stripped WASM bytes | `gzip -n -9` bytes |
| --- | --- | ---: | ---: |
| Main | `89b24c88` | 17,912,469 | 5,678,271 |
| Candidate | `c908431d` | 17,344,160 | 5,527,844 |

The candidate is **150,427 gzip bytes smaller**. Full source/tree, input/tool
and output hashes, commands, gzip roundtrips and limitations are in
[`lists-launch-bundle-2026-10-19.json`](lists-launch-bundle-2026-10-19.json).
Both source checkouts are clean. The tool binaries and build/data inputs match;
both stripped modules retain their expected embedded revision.

The comparison uses local `wasm-release`, wasm-bindgen 0.2.129, explicitly
pinned Binaryen 123, the shared `wasm-symbols` postprocessor and GNU gzip 1.14.
It retains the local panic hook and does not reproduce Docker's
`build-std`/`panic=immediate-abort` pipeline or the historical frozen Linux pair.

Two setup artifacts were excluded: an initial main build selected PATH
Binaryen 116, and the first candidate reused the baseline's build-script version
stamp. The latter failed the embedded-revision check. Before the final candidate,
all 50 workspace packages were explicitly cleaned for `wasm-release`, with and
without `--target wasm32-unknown-unknown`; the regenerated module passed the
revision guard. Failed provenance checks are not accepted measurements.

The optimized acceptance server is a copy of the matching `c908431d`
test-auth binary above, on `127.0.0.1:55445`, serving the stripped release
client from the candidate checkout. It uses a separate fresh PostgreSQL database,
`ultros_lists_opt_qa`, so its fixture writes cannot race the broad run's fixtures.
At 19:15 UTC the WASM named in its SSR response was fetched through HTTP and
matched the measured candidate SHA256 exactly:
`706d85e9cea14d2f945e7744a679802b7f0b9ee1e481f46534d8476595785af9`.
No assets are rebuilt while that server is under browser test.

The final required acceptance wrapper **passed all 12 suites**, starting at
19:27:23 UTC and finishing at 19:31 UTC against that named build. All **32
priced scenarios passed**, with zero application or cleanup errors. The wrapper
forbids skip/allow-empty overrides and rejects smoke-only substitutions. Its
other suites cover handoff, account/device editing, undo, adoption, companion,
sync/account lifecycle, exact account identity, account storage/UI, sorting
accessibility and device prices.

The first optimized attempt passed ten of twelve suites and all 32 priced
scenarios, but failed handoff and sorting because of the late preference setup
described above. Its reports/logs are retained separately. After those fixture
corrections, both focused debug/optimized reruns and the complete optimized
wrapper passed with the original error assertions intact. Neither attempt is
a production soak or a physical FFXIV exercise.

## Production observations

Fresh anonymous, read-only browser loads of `/`, `/list`, `/settings` and
`/item/Gilgamesh/5364` at 18:20:24–18:20:28 UTC returned HTTP 200, completed
hydration and served `89b24c8`. They recorded no page errors, failed
first-party resources or horizontal overflow at 1280px. Third-party HTTP
requests were blocked for this smoke. Read-only cheapest/Gilgamesh and
sale_stats/North-America requests also returned 200 at 18:19 UTC. These
observations do not exercise a signed-in production sharing/sync journey.

The 19:35:49–19:35:53 UTC refresh repeated all four browser routes with the
same result: HTTP 200, hydrated `89b24c8`, no page errors, failed first-party
resources or horizontal overflow, with third-party HTTP requests blocked.

An older collaborative-browser tab stopped responding: navigation timed out
after 15 seconds and reopening stalled. A fresh repository-Puppeteer session
completed the production checks above. The automation failure is retained
separately from the measured application results.

After the owner signed in to GlitchTip, a read-only review included ignored
groups. The newest 100 groups were ordered by last occurrence and reached
back to September 22, covering the 14-day window. Forty-four groups had been
seen during that window; no group title/culprit matched Lists document/sync
signals. Three groups were active during the last 24 hours, all already
ignored socket-disconnect groups: #7290, #6896 and #7209. Their displayed
counts are lifetime counts, not 24-hour event counts. The latest generic API
event in #8009 was the older `DELETE /api/v1/current_user` failure. This is
a bounded reported-group review, not a complete event/transaction or private
server tracing scan. No GlitchTip group was changed.
The 18:36 UTC refresh still showed 44 groups in 14 days and no Lists title/
culprit matches. The same three ignored socket groups were active in 24 hours;
BrokenPipe #7209 had a newer occurrence at 18:29 UTC. This does not claim that
socket errors stopped or that ignored groups are necessarily harmless.
At 19:36:11 UTC, the newest 100 groups still covered back to September 22:
44 groups in 14 days, no Lists title/culprit matches and the same three ignored
socket groups in 24 hours. ConnectionReset #6896 most recently occurred at
19:03:49 UTC; BrokenPipe #7209 remained at 18:29:23 UTC and SendError #7290 at
17:20:10 UTC. No issue status was changed.

Earlier production evidence remains in PR #1665: the owner attests the
deployed hands-on exercise was completed; the checker built from `c941061e`
checked all 23 account lists at 03:21:56 UTC, finding 11 `equal`, 12
`missing_document`, zero `different`/`decode_error`/`missing_list`. Missing
documents are allowed by the scoped proposal and are not verified equality.
No repeat physical-device exercise is being requested by this record.

The current pre-promotion image was verified available in GHCR and its
configuration revision matches the served `89b24c88`:

`ghcr.io/akarras/ultros@sha256:00c56ef355eed314c1a8c941cf2789a39f96575b87b399a9eeeaac62c395974a`

Its amd64 manifest is
`sha256:c8692c97febfccf0d053dacd743433e15fb2ac486eb00f3cd64ad645e56f7f5d`.
Registry availability was checked; the image was not pulled, started or
used for an actual rollback in this session. Rolling back the browser UI
does not stop other writers when merge/projection corruption is suspected.

Hourly bounded site/GlitchTip monitoring is enabled. Private production
server logs and database access are not established in this session.
After deployment, the scoped proposal still calls for daily error/sync-warning
review for a week and another production projection check in 3–7 days.

## Remaining release work

The broad browser run, optimized required acceptance and current-main bundle
sanity pair have passed. Publish the updated candidate after the final local
CI gate, then require hosted checks on its actual head. Main requires one
approving review. The release decision explicitly
requires the owner's scoped-waiver signature in the record and on #1510;
that signature is still absent. Local tests and monitoring do not sign it.
