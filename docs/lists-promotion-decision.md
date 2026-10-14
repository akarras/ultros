# Lists 2.0 promotion decision (#1510 soak gate)

Date: 2026-10-13. Decision for: PR
[#1627](https://github.com/akarras/ultros/pull/1627)
(`codex/lists-promotion` @ `25994a5e6ca5e81ff95f9751c06abbc183aac9ce`),
which removes the Labs gate and the legacy list UI.
Gate issue: [#1510](https://github.com/akarras/ultros/issues/1510).
Related: [#1439](https://github.com/akarras/ultros/issues/1439) (integrated
acceptance, closed), [#1479](https://github.com/akarras/ultros/issues/1479)
(projection checker, closed via #1513),
[#1512](https://github.com/akarras/ultros/issues/1512) (bundle budget, closed).

## Decision: BLOCKED — not promotion-ready under #1510 as written

#1627 must not merge on #1510 evidence that does not exist. It may merge
only after either (a) the blockers in “Remaining blockers” below are
evidenced and linked from #1510, or (b) the repository owner signs the
explicit waiver in “Waiver (unsigned)” below, accepting the named risks.
This record itself is the decision #1510’s last checkbox asks for; it does
not close #1510, and its conclusion is “blocked”, not “promotion-ready”.

Production state at audit time (verified 2026-10-13): `https://ultros.app/`
returned HTTP 200 with build stamp `d09d761`, matching `main` tip
`d09d7611b321df4b10e0cde517954eae78b200f4` (2026-10-08). The promotion is
therefore **not deployed**; production Lists remain behind Labs. Any
seven-day soak must still run against a deployed pre-promotion build.

Evidence rules for this audit: a checkbox is “Done” only with a linked,
inspectable artifact; local/synthetic runs are labelled local; elapsed
calendar time is not observation. Nothing below claims a deployment,
soak, device exercise, or production checker run that was not found in a
real artifact or run in this session (see “Closed locally in this change”).

## Per-checkbox disposition (#1510, in order)

| # | #1510 requirement (short) | Disposition | Evidence that actually exists | Gap |
| --- | --- | --- | --- | --- |
| 1 | Deployed commit, ≥7-day window, accounts/devices, cohort; note material changes | **Partially evidenced** | #1439 close-out names a candidate start: last visible Lists change #1574 deployed 2026-09-21, acceptance on prod `c7b83be` 2026-09-22. Prod today verified as `d09d761` (above). | No recorded observation end, cohort, or participating accounts/devices. Material builds landed inside any Sep 21→ window without a soak log: wasm-split pilot #1579 (2026-09-21) and its revert #1588 (2026-09-22), bundle change #1590, crate split #1597, shopping-route change #1639. >7 elapsed days with no daily record is not a soak. |
| 2 | Two browsers + phone, list shared by two Discord accounts; online/offline, reconnect, concurrent purchases, Discord bot add/remove on the deployment | **Missing** (production); partially evidenced locally | Local only: `integration/list-sync.cjs` and `integration/list-compaction-recovery.cjs` (two browser peers, offline/reconnect, additive purchases), recorded in #1439’s 2026-09-15/16 comments against local test-auth builds. #1439’s prod run (2026-09-22) was a signed-out **device** list on one client. | No artifact shows the deployed exercise with a phone, two Discord accounts sharing one list, or the bot’s `/list add_item`/`remove_item` landing live. |
| 3 | Daily review of application errors and `ultros::lists::sync` warnings (broadcast, activity, relay) | **Missing** | The soak plan names the exact GlitchTip/tracing queries (`docs/lists-sync.md`, “Historical production soak plan”). | No daily findings/resolutions are recorded in #1510, #1439, or `docs/`. An empty local log is explicitly not this evidence. |
| 4 | Authoritative manifest of every list touched, incl. deletions | **Missing** | Retention procedure exists and is sound: `docs/list-projection-checker.md` (“Week-long soak manifest”), incl. deleted-ID reconciliation and why `--updated-since` polling alone is not exhaustive. | No retained `soak-ids` manifest or action ledger exists in the repo or on #1510. Without it, checkbox 5 cannot prove completeness. |
| 5 | Read-only decoded projection checker vs that manifest; zero unexplained divergence, no data loss | **Partially evidenced** | Tooling is Done: checker merged via #1513 closing #1479, with REPEATABLE READ/READ ONLY snapshot, machine-readable statuses and exit codes 0/1/2/3 (`docs/list-projection-checker.md`). Re-verified at `25994a5e` in this session: `cargo test --locked -p ultros-db list_projection` → 7 passed, 1 ignored (isolated-PostgreSQL test); the CLI without `DATABASE_URL` exits 2 with `DATABASE_URL is required`. #1479’s close-out covers synthetic equal/divergent/malformed/missing/deleted fixtures on a disposable local DB. | The checker has **never been run against a production soak manifest** (no manifest, no production access claimed in #1479). Zero production divergence is unestablished. Synthetic passes must not be reported as soak results. |
| 6 | Snapshot growth and compaction over time (256 KiB is a trigger, not a cap) | **Missing** (production time series) | Compaction semantics and the daily size query are documented (`docs/lists-sync.md`); forced production-path compaction passed locally in `list-compaction-recovery.cjs` (#1439). | No per-list size/compaction-counter series over a deployed week; repeated-compaction-without-edits and unexplained growth are therefore unexamined in production. |
| 7 | Both accounts record close-together purchases without loss; legacy view agrees with Labs view | **Partially evidenced** (local) | Local two-peer convergence and exactly-once additive purchases (above); legacy/Labs agreement was exercised locally while both UIs existed (Phase 4 notes in `docs/lists-sync.md`). | No production two-account close-together purchase evidence. **Sequencing hazard:** #1627 deletes the legacy view and the Labs toggle, so after it merges this comparison — and soak step “turn it on under Settings › Labs” — become impossible to perform. This checkbox can only be satisfied **before** #1627 merges, or waived. |
| 8 | Controlled Phase 4 bundle comparison + separate final integrated-build measurement + #1439 acceptance | **Partially evidenced** | Historical controlled pair is Done and must keep both results: original comparison **failed** by 8,900 gzip bytes (+758,900 vs 750,000 budget); the dedicated `wasm-release` profile on the same historical revisions **passed** at +738,272 (11,728 under) — `docs/qa/lists-bundle-budget.md`, #1512 closed 2026-09-21. #1439 integrated acceptance is Done/closed with the prod `c7b83be` journey. | The **final integrated-build measurement is missing**: no bundle measurement exists for the current integrated head (`25994a5e`, after #1579/#1588, #1590 and #1627’s own legacy-UI removal). #1627 and `docs/lists-sync.md` both state no new bundle measurement is claimed. #1510 forbids substituting the historical baseline for this. It was not regenerated in this session: the documented procedure requires the frozen Linux tooling image, recorded only by local image ID (`sha256:f2f2c5ea…`, not a published reference) in `docs/qa/lists-bundle-budget.md`; an ad-hoc Windows build would be exactly the non-comparable substitute #1510 prohibits. |
| 9 | Publish promotion-ready / remaining-blockers decision; gate removal only in a separately reviewed change after evidence | **Done by this record — conclusion: blocked** | This document, stacked on #1627 for separate review, plus the draft #1510 comment below. #1627 is the separately reviewed gate-removal change in form. | In substance the ordering #1510 requires (“after the required evidence is complete”) is not met: checkboxes 1–8 are not complete. Merging #1627 now therefore skips the gate unless the waiver below is signed. |

No checkbox marked Missing above was found satisfied under another issue:
#1510 has exactly one comment (the 2026-09-16 historical bundle-failure
report, superseded for the historical pair by #1512 above) and no soak
artifacts; a repo-wide search for soak evidence on 2026-10-13 found only
the plans and tooling cited here.

## Closed locally in this change

- Projection checker re-verified at the promotion head (test and CLI
  results in row 5). This closes “does the checker still build/pass at
  `25994a5e`?” — nothing more. The ignored PostgreSQL snapshot test was
  not run here (needs an isolated disposable database via
  `MIGRATION_TEST_DATABASE_URL`; it was run for #1513 per #1479).
- Production build stamp probed and matched to `main` (above), fixing the
  audit’s baseline fact: the soak, if run, is still runnable pre-merge.
- `docs/lists-sync.md` Promotion section corrected to link this record
  and to stop presenting the undeployed branch state as a completed
  production promotion. Historical documents that describe Labs as it was
  on their own dates (specs, `docs/qa/lists-2-delivery-reconciliation.md`)
  are left intact as dated records, superseded by this decision.

Not closed locally, deliberately: the final bundle pair (row 8 — frozen
tooling unavailable; a substitute toolchain would violate the gate’s own
comparability rule) and every production-time/physical-device item below.

## Remaining blockers (exact evidence that would satisfy each)

Owner “production operator” means whoever holds production DB read access,
GlitchTip, the two Discord accounts, and a phone — currently the
maintainer (@akarras). Reviewer sign-off should be recorded on #1510.

1. **B1 — Declare the soak (checkbox 1).** Owner: production operator.
   Post on #1510 before starting: deployed commit SHA, start/end
   timestamps (≥7 days), cohort definition, account aliases and devices.
   Log every material deploy inside the window; a material Lists/bundle
   change restarts or explicitly re-baselines the window.
2. **B2 — Run the deployed exercise (checkboxes 2, 7).** Owner:
   production operator. Evidence: dated log (screenshots optional)
   covering two browsers + phone, one list shared by two Discord
   accounts, offline edits → reconnect, both accounts purchasing within
   ~1s of each other with no lost change, bot add/remove landing live,
   and legacy-vs-Labs agreement on the shared list — all against the
   B1 deployment, **before #1627 merges** (see row 7 hazard).
3. **B3 — Daily error/warning review (checkbox 3).** Owner: production
   operator. Evidence: one dated entry per day of B1 recording the
   GlitchTip query results (`list_doc`/`ListDocError`/`sync_payload`/
   `hydration` on `/list/`) and `ultros::lists::sync` warning review
   (broadcast/activity/relay), with findings and resolutions — “none
   found” per day is acceptable evidence when the query is named.
4. **B4 — Manifest + checker (checkboxes 4, 5).** Owner: production
   operator. Evidence: retained append-only ID ledger incl. deletions
   (per `docs/list-projection-checker.md`), daily + final
   `list_projection_check` JSON reports with exit codes, deployed and
   checker SHAs, and a deletion reconciliation. Pass bar: every
   non-deleted ID `equal`; every exception investigated in writing;
   reports redacted before public attachment.
5. **B5 — Snapshot/compaction series (checkbox 6).** Owner: production
   operator. Evidence: daily `list_doc` count/total-size plus per-list
   size and `changes_since_compaction` for cohort lists across B1, with
   written disposition of any repeated compaction without edits or
   unexplained growth.
6. **B6 — Final integrated bundle measurement (checkbox 8).** Owner:
   #1627 author. Evidence: the `docs/qa/lists-bundle-budget.md`
   procedure run with one identical reprovisioned tooling set (recorded
   binary hashes) for the final integrated head and its stated baseline,
   full pipeline, `gzip -n -9` sizes + SHA256s + gzip roundtrip, and the
   delta judged against the 750,000-byte budget without substituting the
   historical pair. Note #1627’s legacy-UI removal changes the artifact,
   so the measured head must be the merge candidate itself.
7. **B7 — Decision update (checkbox 9).** Owner: production operator +
   reviewer. Evidence: #1510 comment linking B1–B6 artifacts, checkboxes
   ticked only where linked, and this record amended to
   “promotion-ready” — or the signed waiver below. A sibling change owns
   the technical rollback/kill-switch path; that work is **not** evidence
   for any blocker here and does not substitute for the soak.

Also carry into the merge decision, from #1627’s own body: its broad E2E
run was not fully green (analyzer and NPC-map probes failed; a hydration
panic and shopping-focus assertion occurred and did not reproduce in
targeted reruns). Those are recorded there as intermittent, not fixed;
they are outside #1510’s checkboxes but are part of the risk a waiver
accepts.

## Waiver (unsigned — owner decision required)

The audit found no recorded owner intent to waive the soak; #1627’s body
(“does not claim new production-soak evidence”) disclaims evidence
without requesting a waiver. If the owner chooses to merge #1627 without
B1–B6, the honest form is the following explicit waiver, signed by name
and date in this file and on #1510 — not silence:

> I waive the #1510 seven-day production soak for the Lists 2.0
> promotion in #1627. I accept that: (1) existing account lists that
> have never opened through sync have no stored document
> (`missing_document` in the checker is explicitly “not a verified
> equality”), so promotion bootstraps CRDT documents en masse on first
> open with no production cohort having exercised that path under
> observation, and document/projection divergence in production is
> unmeasured; (2) guest offline preparation becomes default behaviour
> on ordinary Lists navigation for every visitor, with its bandwidth
> and service-worker cache cost unmeasured on the final bundle, whose
> size delta is likewise unmeasured; (3) the Labs flag and legacy UI
> are removed, so there is no per-user fallback — rollback means
> redeploying a prior build (plus any separately reviewed kill-switch),
> and the legacy-vs-Labs comparison can never be run after merge; and
> (4) the intermittent E2E failures recorded on #1627 ship as known
> unknowns. Rationale: <owner to state>. Signed: <name>, <date>.

Until that text (or equivalent) is signed, the disposition of this gate
is **blocked**, and #1510 stays open.

## Scoped waiver (chosen 2026-10-14 — awaiting signature)

On 2026-10-14 the owner chose a **scoped** waiver over the full waiver
above. It keeps the two checks that can find real defects and waives the
rest. It takes effect only when signed below and on #1510. Until then the
decision above stays **blocked**.

**Kept, as merge preconditions for #1627** (evidence goes on #1510):

- **B2 — deployed hands-on exercise.** Run against production with Labs
  on, before #1627 merges, because #1627 deletes the legacy view and the
  comparison becomes impossible afterwards. Cover two browsers and a
  phone, one list shared by two Discord accounts, offline edits followed
  by a reconnect, both accounts purchasing within about a second of each
  other with no lost change, bot `/list add_item` and `remove_item`
  landing live, and the legacy and Labs views agreeing on the shared
  list. A dated checklist is the evidence; screenshots are optional.
- **B4, reduced — projection checker over every production account
  list.** Run `list_projection_check` once before #1627 merges, and again
  3–7 days after deploy. A retained manifest is not required. The
  checker has no implicit "all lists" mode, so build the ID file from the
  database at run time (`SELECT id FROM list ORDER BY id` into
  `all-ids.txt`) and pass `--ids-file all-ids.txt`. A list deleted between
  the export and the check reports as deleted, which is fine. Pass bar: every row is `equal`
  or `missing_document` (never opened through sync). Any `divergent` or
  `malformed` result blocks the merge, or after the merge triggers
  `ULTROS_LISTS_READONLY`. The post-deploy run is the check on the mass
  first-open bootstrap risk (waiver item 1).

**Replaced by a lighter check:**

- **B6 → bundle sanity check.** One wasm-release gzip measurement of the
  merge candidate against `main`, on the same current toolchain. It is
  recorded on #1510 and judged against the 750,000-byte budget. It is
  not comparable with the historical pair, which stays as it is.
- **B1, B3, B5 → one week of monitoring after deploy.** Daily look at
  GlitchTip (`list_doc` / `ListDocError` / `sync_payload` / hydration
  on `/list/`) and the `ultros::lists::sync` warnings. A one-line #1510
  comment per day, "none" included. Any data-loss signal means flipping
  `ULTROS_LISTS_READONLY` first and investigating second.

**Waived outright:** the declared ≥7-day pre-merge soak window and cohort
(B1), the retained append-only manifest (B4's ledger), the per-list
snapshot/compaction time series (B5), and the frozen-toolset bundle pair
(B6 as written).

**Rollback for the waived risks:** `ULTROS_LISTS_READONLY` (#1658), an
env flip and restart of the same image, then the tier-2 revert runbook in
`docs/lists-sync.md`.

> I accept the scoped waiver above for the Lists 2.0 promotion in #1627,
> including risks (1)–(4) named in the full waiver text, with the kept
> checks as merge preconditions. Signed: ________, ________.

## Draft #1510 comment (paste-ready; does not close the issue)

> Promotion decision for #1627 is recorded in
> `docs/lists-promotion-decision.md` (stacked PR against
> `codex/lists-promotion`): **blocked, not promotion-ready.**
>
> Per-checkbox: (1) partially evidenced — candidate start 2026-09-21
> (#1574) but no recorded window/cohort, and material builds (#1579/
> #1588, #1590, #1597, #1639) landed unlogged; elapsed time is not a
> soak. (2) Missing on the deployment (local two-peer evidence only).
> (3) Missing — no daily GlitchTip/`ultros::lists::sync` reviews.
> (4) Missing — procedure exists, no retained manifest. (5) Partially
> evidenced — checker (#1513) re-verified at `25994a5e` (7 tests pass,
> CLI guards intact) but never run against a production manifest.
> (6) Missing — no deployed size/compaction series. (7) Partially
> evidenced locally; note #1627 deletes the legacy view, so this must
> be evidenced pre-merge or waived. (8) Partially evidenced —
> historical `wasm-release` pair passes (+738,272 ≤ 750,000; original
> +758,900 failure preserved) and #1439 acceptance is closed, but no
> final integrated-build measurement exists for the merge candidate.
> (9) Done by the decision record — conclusion: blocked.
>
> Production verified 2026-10-13 as `d09d761` (= `main`): the
> promotion is not deployed and the soak is still runnable. Remaining
> blockers B1–B7 in the record name owners and the exact artifacts
> that would satisfy each checkbox. The alternative is the explicit
> signed waiver drafted in the record (en-masse CRDT bootstrap,
> offline-by-default cost, no flag/fallback, unmeasured final
> bundle). Please do not close this issue until one of those two
> paths completes; no production soak is claimed by this comment.
