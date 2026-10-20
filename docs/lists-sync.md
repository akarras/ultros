# Lists local-first sync

`/list/:id` runs on a Loro CRDT document by default
stored in the browser and merged through the server. Spec:
`docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`.

## Document compatibility

`ListDocument::from_snapshot` is the shared boundary for account cache opens,
device backups/opens, server stored documents and recovery snapshots. It accepts
schema 1 or the schema-less legacy layout; every other explicit version is an
actionable compatibility error. Legacy rows may omit `item`, `quality`, `need`
and `acquired`: identity comes from the key and missing quantities read as zero.
Loading a legacy document adds no operations and does not rewrite its schema.
An account handle without a cache starts as an operation-free `empty_peer`, so
it does not label the incoming legacy document as schema 1 before the handshake.
Present fields must still have their declared types. Schema 1 requires these
row fields. `name`, `scope`, and row `target` remain optional.

Only root maps `meta` and `rows`, their documented fields, canonical row keys
and matching item/quality metadata are accepted. Item ids are positive and fit
the browser's reversible `item * 4 + quality` row id. Scope ids are canonical
nonnegative i32 values; existence in today's world catalog is a separate concern.
Need and target are signed i64 integers. Acquired must be a finite integral Loro
counter total within the f64 representation of the i64 endpoints. Negative
acquired values from concurrent undo and historical signed quantities are valid:
the document preserves them, while relational/display projection continues to
clamp need/acquired to 0..i32::MAX. Targets retain their signed i64 value. There
is no floating-point rounding of malformed quantity fields on import. Unknown
fields are rejected instead of silently dropping an unsupported extension.

Peer updates are first applied to an isolated snapshot copy and validated there.
Rejected updates never reach the live document, its observers, undo history,
durable snapshot or relational projection. Dependency-incomplete updates return
`ImportReport { pending: true }` without parking any operations in the live
document; callers must fetch a complete snapshot. This prevents a later valid
dependency from activating unvalidated queued data. Snapshot loads reject missing
history outright. Full and shallow snapshots use the same validation.

Local typed edits and complete undo groups also validate an isolated same-peer
candidate before touching the live document. The saved result must reopen within
the same 8 MiB snapshot, 32 MiB decoded-block and 100,000 retained-operation
budgets. Undo reserves conservative capacity before popping its stack and can
refuse an action that would not fit. Rejection preserves the last accepted work.
Causal peer identities are retained without a lifetime writer-count ceiling;
encoded snapshot and transport limits still apply.

Once a server response acknowledges the exact current version, clients can trim
50,000 or more retained operations to a shallow snapshot. This resets local undo
and uses the durable replacement fence before editing or sending resumes. Pending
local work is never trimmed. A disconnected list can reach its finite history
budget and must sync before accepting more edits; its existing snapshot remains
exportable. Legacy ancestry is proved by shared operations, not a schema marker.

An unsupported or damaged account cache is retained exactly, with edits, sync and
saves paused. The actual failure displays an export-recovery action; ordinary
lists have no additional controls. Device restore validates before creating a
record and leaves the source backup and existing records intact. Device conflicts
and account merge-save validate the stored snapshot independently before merging,
so newer valid operations cannot conceal an incompatible saved version.

Regressions: `ultros-list-doc/tests/validation.rs`, account handle/store unit tests,
and the opt-in database test
`unsupported_schema_rejects_every_projection_and_preserves_stored_bytes`.
The browser probe is `node integration/list-document-compatibility.cjs` against
a fresh test-auth build. Its byte fixtures can be regenerated with
`cargo run -p ultros-list-doc --example compatibility_fixtures`.

## Readable cached contents and price coverage

REST permission alone does not initialize a document. Build estimates, Shop,
and editing wait for a validated cached copy or a completed document handshake,
including an explicit UpToDate response. A genuinely empty readable list costs
zero; an operation-free receiver waiting for its first snapshot is unavailable.

New account-cache index entries bind readiness to the snapshot's causal version
and revocation generation. Saves stage at most an incoming and previous proof
before replacing raw bytes, so a failed write preserves the previous usable
copy. Snapshot saves, permission updates and index cleanup share the account
Web Lock. Revocation still fences and removes raw bytes synchronously. Invalid
or mismatched explicit provenance never falls back to assuming complete contents.
The raw snapshot format is unchanged, and pending recovery keeps its provenance.

Legacy caches without readiness metadata use causal history, which survives
shallow compaction. An ambiguous operation-free legacy cache remains preserved
but needs a successful handshake; a newly confirmed zero-operation empty list
can reopen offline using its explicit readiness proof.

Build and Shop share the actual Build estimate. Price coverage identifies only
item IDs returned by a successful account response or included in a successful
device request. An added, unfetched item is “not requested”; a successfully
checked item with no matching offers is “no supply.” Refresh failures retain
same-list prices and their served scope. A new desired scope never relabels an
older response, and fully owned rows need no market request to cost zero.

## Bundle measurements

| build | raw | gzip -9 |
|---|---|---|
| before (prod, main @ 14ff1fc) | 18,108,088 | 5,681,660 |
| after (Phase 4 @ 67eeba30) | 20,138,107 | 6,427,736 |
| growth | +2,030,019 | +746,076 (limit 750 KB per the spec) |

These are historical measurements reported for the listed revisions. The
baseline predates unrelated frontend changes, so the difference cannot be
attributed solely to Loro or certify the final Phase 4 bundle budget. The
reported isolated Loro measurement is 1.99 MB raw and 703 KB compressed.
The subsequent controlled direct-base/reviewed-head comparison exceeded the
conservative 750,000-byte budget by **8,900 bytes**. A frontend-only Loro
compiler-setting experiment saved **20,883 gzip bytes** on that same head.
The dedicated `wasm-release` profile was then exercised on both historical
revisions: **738,272 bytes** of gzip growth, **11,728 below the budget**.
Commands, exact revisions, checksums and the distinction between historical
failure, candidate evidence and final integrated validation are recorded in
[`qa/lists-bundle-budget.md`](qa/lists-bundle-budget.md). This does not certify
the final integrated build or complete the production soak.

The 2026-10-18 current-toolchain sanity comparison separately measured the
integrated implementation `1eafc0f1` against `main` at `0254444f`: 5,529,702
versus 5,679,018 bytes with `gzip -n -9` after the production name-strip step,
a reduction of 149,316 bytes. See
[`qa/lists-launch-bundle-2026-10-18.json`](qa/lists-launch-bundle-2026-10-18.json)
for exact inputs and limitations. This is the unsigned scoped-waiver proposal's
lighter check; it does not reproduce the historical frozen Linux toolchain or
exercise Docker's immediate-abort build.

## How it works

- One Loro document per list. The server keeps the merged state in
  `list_doc` (`snapshot`, `version`, `changes_since_compaction`) and keeps
  `list_item` and the `list` name and scope as a projection of it. The REST
  handlers, the Discord bot and the socket
  all call `ListSync` (`ultros-lists/src/sync.rs`), and the old row writers
  were deleted from `ultros-db/src/lists.rs`. This prevents callers using
  those helpers; it does not prevent direct entity writes. List deletion
  intentionally removes the relational rows outside `ListSync`.
- The browser stores one snapshot per user and list under
  `ultros.listdoc.v1.{user_id}.{list_id}` in localStorage, with a per-user
  index at `ultros.listdoc.index.v1.{user_id}` (last use and last known
  permission and readiness provenance). These are primary offline copies,
  so they are not automatically evicted. Sign-out keeps the snapshot, so an offline
  edit survives a session expiry. A permission denial or a deleted list
  purges it.
- Sync rides the existing websocket. `SubscribeListDoc` carries the
  client's version vector; the server replies with a snapshot, the missing
  updates, or up-to-date, then relays every other peer's updates. Bytes are
  base64 inside the JSON frames. Offline edits need no queue: every
  reconnect re-runs the handshake with the current version and the client
  sends whatever the server lacks.
- If the server cannot take the client's history (it compacted past it, or
  a non-owner renamed the list), it answers with a fresh snapshot; the
  client rebases its local rows onto that snapshot as new operations and
  sends them once more. A second rejection of the same server version stops
  the loop and shows the page as offline.
- List broadcasts share a fixed one-second revalidation window. The first
  broadcast schedules the permission probe; subsequent broadcasts cannot
  postpone that deadline. Continuous updates therefore keep checking access.
  The maximum normal scheduling delay is 1,000 ms, plus request latency;
  suspended/background tabs can be delayed by browser timer throttling.
  Fired timers release their slot and route cleanup cancels pending timers.
  Results still require the active route and the exact open document; transport
  failures or an expired session never purge the saved copy as a denial.
  Price loads and permission probes also share a response watermark: a reply
  older than the latest applied authoritative reply cannot restore stale write
  access or apply an obsolete denial. Merely starting a newer request does not
  discard useful responses, so slow overlapping requests cannot starve updates.
- Undo is Loro's undo manager: local operations only, 100 steps, per open
  page. Every committed action (an applied `Edit`, a rename, a purchase) is
  exactly one step; there is no time-based merging, and multi-row edits are
  one step through an explicit group. Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y (Cmd
  on Apple) are ignored while a modal is open. Inside a form control the
  rule is draft versus committed: a list editor renders `data-committed`
  with the document's value, and while its live value matches, the shortcut
  is a document undo; while it differs (a draft), the browser's own text
  undo keeps the keys. Selects and checkboxes never hold a draft; inputs
  without the attribute, textareas and contenteditable regions always keep
  native undo. Account and device lists install the same window listener
  (`list_doc::undo`), one per open document, removed with the page or editor
  that installed it. Both handles expose reactive `can_undo`/`can_redo`; the
  toolbar disables an unavailable action and a shortcut that finds nothing
  to do says so in the workspace feedback line. A snapshot rebase restarts
  the stack.
- Shop's **Undo purchase** reverses the most recent available local Shop
  purchase as a new grouped counter delta, preserving later Build edits.
  Account, device, and companion controls share this behavior. The receipt
  journal retains up to 100 purchases for the open document and resets on
  reload or snapshot rebase. Ctrl+Z/redo tracks purchases and their reversals,
  so a purchase already undone from Build cannot be reversed twice. A
  removed, replaced, or quality-moved row is skipped, as is a receipt whose
  quantity is no longer owned; Undo purchase never redirects to a different
  quality or makes Owned negative. The button is disabled without an
  available receipt.

## Debugging

- Handshake by hand: log in with `curl -c jar "$BASE/test/login?user_id=...&username=..."`
  on a test-auth build, then `websocat -H "Cookie: discord_auth=..." ws://host/api/v1/realtime/events`
  and send `{"SubscribeListDoc":{"subscription_id":1,"list_id":ID,"version":""}}`.
  The reply is a `ListDocSubscribed` with a `Snapshot` payload and the
  server's version.
- A list whose page looks wrong: run the read-only decoded
  [projection checker](list-projection-checker.md) against explicit list IDs.
  It reads documents, list metadata and rows in one REPEATABLE READ, READ ONLY
  database snapshot and compares each projected field with the writer's rules.
  Its JSON distinguishes differences, decode failures, missing documents and
  absent/deleted lists. A mismatch requires investigation and is not by itself
  proof of a bypassing writer. Snapshot sizes and row counts alone do not
  establish equality. The checker is evidence only: it repairs nothing and
  stops no writer. On a `different` or `decode_error` result, do not treat
  the REST projection the read-only routes display as independently trusted
  truth — it is one side of the divergence — and follow the writer
  containment procedure under [Rollback](#rollback).
- Compaction: a snapshot is replaced by a shallow one after 5,000 changes or
  256 KiB. This is a compaction trigger, not a stored-size cap: shallow
  snapshots can exceed it when the live document is large. Clients that
  start from a shallow snapshot still sync both ways.
- Bus lag: the socket answers a lagged relay with `Stale` and the client
  re-runs the handshake with its current version. Resubscribing with the same ID
  replaces and immediately drops the previous relay, including any pending
  permission lookup. Unsubscribe, denied access and socket close also dispose
  their relays; the 64-subscription limit bounds retained streams.
  Run `BASE_URL=http://127.0.0.1:8080 node integration/list-socket-lifecycle.cjs`
  against an isolated `test-auth` build to check repeated handshakes,
  unsubscribe/navigation churn, cross-list ID reuse and failed replacements
  over a real socket.
  Native `real_time_data::tests` additionally count receivers/authorization
  calls and cover pending lookups, failed handshakes, access denial and lag.
- Browser side: `localStorage.getItem("ultros.listdoc.index.v1.<user_id>")`
  lists the cached lists, their last use and last known permission.
- The same socket also carries the notification inbox (`SubscribeNotifications`).
  See `docs/notification-inbox.md` for its handshake, the `websocat` recipe,
  and how it reuses this socket's `Stale`/subscription-limit conventions.
- Divergence query for the soak:
  `SELECT l.list_id FROM list_doc l WHERE l.updated_at > now() - interval '1 day'`
  gives recently touched lists, not divergence. Retain the union of IDs
  across the entire soak and run the decoded projection checker above;
  account separately for lists deleted during the soak.

## Promotion

> **Gate status (2026-10-18): production checks remain open.** The current
> integration and local bundle result are recorded in
> [`lists-promotion-decision.md`](lists-promotion-decision.md). The deployed
> two-account/device/bot exercise and production projection check have no
> evidence here, and the scoped waiver remains unsigned. Production was
> inspected at `0254444` with Lists still behind Labs. Do not merge the
> promotion until that record's blockers are evidenced or its waiver is signed.

In the promotion branch, Lists 2.0 is the default for account and device
lists. The Labs switch, original REST-driven list editor and directory, and
`?cart=legacy` comparison grid have been removed. Old `LABS` cookies and
`?labs=lists-sync` bookmarks no longer select a different implementation.
Existing account lists use the same document bootstrap and REST projections;
device lists keep their existing browser storage and IDs. REST endpoints
remain available for the Discord bot, imports, sharing and other callers.

The guest Build/Shop and account adoption product contract is in
[`2026-09-10-lists-2-product-design.md`](superpowers/specs/2026-09-10-lists-2-product-design.md).
The Shop and companion requirements from that contract are tracked item by
item, with the checks that exercised each one, in
[`qa/lists-2-shop-companion-evidence.md`](qa/lists-2-shop-companion-evidence.md);
its open rows (physical game validation, the
per-hop savings ladder) are not promotion blockers but must stay visible.
The Build side (compact cart, estimates, undo) is reconciled the same way in
[`qa/lists-2-delivery-reconciliation.md`](qa/lists-2-delivery-reconciliation.md),
which also records the deployed build and the browser verification still owed.
New UI strings, including companion labels and offline boot messages, must
remain translated in all seven supported locales. Verify keyboard focus
through a committed row edit on both device and account lists when changing them.

The existing root service worker also serves push notifications. Worker
activation and control of open tabs are site-wide; only guest offline cache
preparation starts on the Lists directory or a device-list route. Its fetch handler can use an anonymous guest shell
and public assets only after a cache generation has been prepared. It never
caches authenticated page HTML, account APIs, or market responses. The guest
store, offline helper, and companion modules ship with the versioned WASM
package so an older cached static helper cannot change a deployed module API.

`integration/lists-default.cjs` verifies default SSR, hydration and client-side
navigation, including stale Labs cookies and comparison bookmarks.
`integration/lists-help.cjs` and `lists-v2.cjs` cover anonymous device lists
without opting in. The list-flow suite now runs once against the default editor.
The historical soak plan and Phase 4 evidence below are retained for context;
this promotion does not claim new production-soak evidence. The separate local
bundle sanity comparison above is not the historical frozen-toolset pair.

## Rollback

The promotion deleted the Labs toggle and the legacy editor, so rollback is
deliberately two-tiered. Tier 1 needs no rebuild; tier 2 is a full revert.
Neither tier touches list data — and **neither tier is an all-writer
containment**. Tier 1 pauses only the promoted browser surfaces; tier 2
restores the previous browser UX. REST, the Discord bot, imports and other
server-side `ListSync` writers continue under both, by design: they predate
the promotion and share its merge and projection code. Match the response
to the incident class:

- **Browser-only incident** (the promoted routes or socket editing misbehave,
  with no sign the shared server merge or projection is wrong): tier 1
  first, tier 2 if the old UX is wanted back.
- **Suspected shared merge/projection corruption** — a checker `different`
  or `decode_error` result, or any other sign the code every writer shares
  is corrupting documents or projections: neither tier contains it. Use
  the writer containment procedure below; tier 1 may be flipped alongside
  it, but does not substitute for it.

### Tier 1: read-only kill-switch (`ULTROS_LISTS_READONLY`)

Set `ULTROS_LISTS_READONLY=true` in the server environment and restart the
process (`ultros-server-core/src/lists_readonly.rs`; same boolean spellings
as `ULTROS_DISABLE_UNIVERSALIS_WEBSOCKET`). No image rebuild, no migration.
While it is on:

- The SSR bootstrap carries `lists_readonly` to the browser. `/list/:id`
  renders the REST projection read-only (`routes/list_readonly.rs`),
  `/list` renders a read-only account directory, and `/list/device/:id`
  shows only the paused banner. No list document is opened on any route.
- The realtime socket refuses `SubscribeListDoc` handshakes and rejects
  `ListDocUpdate` merges with a scoped `lists are temporarily read-only`
  error. The restart drops every live socket, so a tab loaded before the
  flip cannot resubscribe or keep writing after it reconnects.
- Guest offline-cache preparation is skipped, so no new service-worker
  generation staging an editor shell is published while paused.

What it does **not** do — it is not a site-wide or all-writer freeze:

- REST list writers (Discord bot, imports, sharing, and any other API
  caller) keep working through `ListSync`: they predate the promotion and
  are not the layer it changed. Every server-side writer keeps merging and
  projecting while the switch is on.
- The site-wide service worker is not disabled or unregistered; only new
  guest offline-cache preparation is skipped.
- A previously prepared offline device editor is not stopped (see limits
  below).

If the incident implicates the shared server merge or projection itself,
this switch does not contain it, and neither does tier 2. Use the writer
containment procedure below.

Limits to know during an incident: the switch takes effect on each
browser's next full page load (already-hydrated tabs keep their in-memory
editor until reload, but their writes stop at the socket refusal above),
and a previously prepared offline guest generation can still boot the
device editor *offline* — device documents are local-only and use the
unchanged store format, so those edits merge normally once editing
resumes. To restore editing, unset the variable (or set it to `false`)
and restart; no data step is needed in either direction.

Verify after flipping: load `/list` and one account `/list/:id` — both
show the “temporarily paused” banner and the account list's projected
rows; the browser console shows no `SubscribeListDoc` handshake; and
`localStorage` snapshots (`ultros.listdoc.v1.*`) are untouched. In a
suspected divergence those projected rows are display only, not a trusted
record of the list — treat the banner as the verified fact, not the rows.

### Writer containment: suspected shared merge/projection corruption

This is an operational stop, separate from both tiers. There is no
all-writer freeze flag, and none of the steps here is implemented in code.
Use it for a checker `different` or `decode_error` result, or any other
evidence the merge/projection code shared by every writer is corrupting
data:

1. **Stop every affected writer process.** Through the operator's own
   deployment controls (this runbook does not know the production
   topology, so it names no command), stop or isolate every application
   process and replica that can write lists — web/API replicas serving
   REST and the document socket, the Discord bot, and any import or other
   server-side `ListSync` caller. Flipping `ULTROS_LISTS_READONLY` or
   redeploying the pre-promotion image does **not** do this: the switch
   guards only the promoted browser routes and socket, and the old image
   runs the same merge code without the switch (see tier 2).
2. **Isolate traffic** so no request can still reach a surviving writer
   while processes are being stopped.
3. **Retain evidence before changing anything:** the checker JSON report
   and exit code, the affected list IDs, timestamps, the deployed image
   digest and Git SHA, and the relevant logs. The checker is read-only
   and safe to re-run for evidence; it does not repair anything.
4. **Leave the database untouched.** Do not delete, rewrite or hand-repair
   snapshots or projections, and do not present the projected rows as
   trusted while divergence is known.
5. **Deploy a verified fix**, re-run the checker over the affected IDs,
   and only then restart writers. Restoring the old browser UX (tier 2)
   may accompany the fix, but is not itself the fix for a shared-code
   fault.

### Tier 2: revert the promotion

Before promotion, record the currently deployed image's immutable digest
and Git SHA as the tier-2 target, and confirm that exact image is still
pullable. Redeploy that exact image for tier 2; the floating `main` tag
can already point at the promotion and is not a rollback target.

Tier 2 is a **browser UX rollback, not a containment**. The pre-promotion
image still runs the document merge/projection pipeline through REST, bot and
import writers, while omitting the launch-review locking and account/session
fixes (see “Downgrade compatibility and limitations”). A fault in shared code
can therefore keep running on the old image.
The pre-promotion image also predates the `ULTROS_LISTS_READONLY` guards,
which the promotion added, so it may not enforce the pause flag at all:
setting the variable on the old image must not be assumed to pause
anything. For a suspected shared merge/projection fault, tier 2 alone
stops no writer — use the writer containment procedure above and deploy
a verified fix.

If an image is unavailable, make a rollback branch from current `main`
and revert the actual landed promotion commit through the normal checks
and deployment pipeline. For a squash commit use `git revert <commit>`;
for a merge commit use `git revert -m 1 <merge-commit>`. If the stack lands
as separate commits, revert its children before their parents. Historical
branch SHAs are not a substitute for the deployed commit: integration
merges and squash merges change that history. There is no database migration
to undo, and no list snapshots or projections should be deleted.

### Downgrade compatibility and limitations

This section describes **format compatibility**, not preservation of every
later safety fix. The launch-review follow-up changes document locking and adds
optional account-binding fields to socket messages and account-bound REST
reads; it does not change schema-1 snapshots or relational row formats. Older
servers ignore those optional guards, and older clients do not recognize the
local write-pause marker for rejected offline work. A pre-fix image can therefore
reintroduce the locking/account/session defects even though it can decode the
data. Prefer a rollback image containing the safety fixes. Never use an older
image as containment for one of those defects: stop the affected writers and
deploy a verified fix as described above. Account switching and rejected-work
recovery must be checked against the actual image being considered.

Verified against `main...codex/lists-promotion` (25994a5e):

- **Documents**: `ultros-list-doc`, `ultros-lists`, `ultros-db`, and
  `ultros/static/guest-list-store.mjs` are byte-identical before and
  after the promotion. The schema-1 snapshot boundary
  (`ListDocument::from_snapshot`), the guest IndexedDB
  (`ultros-device-lists-v1`, version 1), and the account localStorage
  snapshots (`ultros.listdoc.v1.*`) are unchanged, so documents written
  by the new UI open in the pre-promotion build unchanged.
- **Projections / REST**: the legacy editor read only REST
  (`get_list_items_with_listings` over `list_item`), and every accepted
  document update still maintains that projection (`project_rows` /
  `project_meta` in `ultros-db/src/list_doc.rs`, covered by
  `a_peer_update_projects_into_rows_and_back_again`). Lists edited only
  in the new UI therefore appear complete in the old build. Confirm with
  the decoded [projection checker](list-projection-checker.md) over the
  incident's list IDs before and after a tier-2 deploy.
- **Service worker**: the promotion changed one comment in
  `service-worker.js` and removed the Labs gate from
  `guest-offline.mjs` cache preparation. Fetch handling is
  network-first with cache fallback only on network failure, and
  generations are never modified in place. After a downgrade the browser
  installs the old worker script and claims tabs as usual; online
  navigations serve the old build immediately. A generation prepared by
  the new build can still answer *offline* device-list navigations with
  the new build's shell until the old build prepares its own (which, for
  a never-Labs user, it will not) — that shell reads the same IndexedDB
  format and caches no account data, so the worst case is the new device
  editor remaining available offline, not data loss.
- **Active browsers**: a downgrade does not instantly replace code already
  running in browsers. Already-hydrated tabs keep the new build's editor
  in memory until reload, and a prepared offline generation can keep
  answering device-list navigations with the new shell as above. Data
  formats are compatible in both directions, so this
  is a UX-consistency limit, not a data risk — but do not claim every
  client is on the old build the moment the old image is deployed.

### Alternatives considered

Resurrecting the full legacy editor behind a flag was rejected: it is
~1,800 lines of route plus ~2,000 lines of deleted components, and its
REST write paths were partly removed with it — re-adding it would ship a
second editor to maintain for an incident-only surface. A cookie/query
flag in the Labs style was rejected because it is per-browser (an
incident switch must take effect server-side for the promoted browser
clients, including ones that never opted in) and it reads as a permanent
user-facing toggle. A gradual cohort was rejected for the same reason
the promotion deleted
Labs: the document layer underneath both UIs is identical, so a cohort
would roll out only the editor skin while complicating every route. The
env switch covers the failure the promotion actually introduces — the
new browser surfaces misbehaving — at the smallest surface area.

## Historical production soak plan

The deployed seven-day observation and promotion decision are tracked in
[#1510](https://github.com/akarras/ultros/issues/1510).
[#1439](https://github.com/akarras/ultros/issues/1439) owns the integrated product
acceptance and readiness assessment; local fixtures do not close the soak.

Run after the branch is deployed, over at least a week with the toggle on
for the maintainer's own shared lists. Retain the authoritative list-ID manifest
and reconcile deletions using the [checker workflow](list-projection-checker.md).
Note this plan can only run on a **pre-promotion** build: the promotion
branch deletes the Labs toggle and legacy view that steps 1 and 3 use, so
the soak (or a signed waiver) must precede that merge. Current gate
disposition: [`lists-promotion-decision.md`](lists-promotion-decision.md).

1. Turn it on under Settings › Labs on two browsers and one phone, and
   share one list between two Discord accounts.
2. Daily: GlitchTip has no new issue whose title contains `list_doc`,
   `ListDocError`, `sync_payload` or `hydration` on `/list/`;
   inspect warning events from tracing target `ultros_lists::sync`, including
   broadcast, activity-recording and relay failures in `ListSync::publish`
   (filtering only for "list document" misses several of these);
   `SELECT count(*), pg_size_pretty(sum(length(snapshot))) FROM list_doc;`
   grows with use, not runaway. Track per-list size and compaction counters
   over time, investigating repeated compaction without meaningful edits
   or unexplained growth; do not treat 256 KiB as a hard cap. The Discord
   bot's `/list add_item` and `remove_item` land on the
   Labs page live.
3. Exit criteria: zero divergence between `list_doc` and `list_item` for
   every list touched during the soak; no data loss on the shared list,
   including both accounts ticking the same item within a second of each
   other; the legacy page, opened without the toggle, always matches the
   Labs page.

When all three hold, open the redesign spec (Lists 2.0 sub-project 2). The
toggle stays on for the maintainer until the redesign ships on top of it.

## Known gaps carried out of Phase 4

- Phase 4 commit `59fbc662` guards list loads and delayed permission
  responses with the active route and exact document-handle identity,
  preventing stale responses from changing or purging a successor list.
  The navigation regression probe covers delayed permission success and
  denial in both load and background-revalidation paths, across navigation
  and unmount; all eight cases passed. Full CI, fresh SSR/WASM builds and
  browser checks passed for convergence, offline reconnect, undo, REST and
  legacy interoperability, revocation/deletion purge, account isolation,
  and physical interactions in plain and Labs list flows. Two initial
  fixture-startup failures passed on targeted rerun; push-subscription
  creation was skipped because VAPID was unconfigured. These local checks
  do not replace the production soak.
- Broadcast-driven revalidation has no maximum debounce wait (see above).
- A row added locally shows no price until a market event or an import
  bumps the listings cache.
- `MakePlaceImporter` still writes over REST; its rows reach the document
  through the socket. Labs recipe previews now add their rows through the
  document as one undoable operation.
- The bulk-edit Puppeteer suite (`integration/list-bulk-edit.cjs`) sends
  add-item bodies without `ListItem.id` and fails on `main` too; it is not
  in the e2e gate.
