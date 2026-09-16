# Lists local-first sync

`/list/:id` behind the `lists-sync` Labs toggle runs on a Loro CRDT document
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
Before promotion, rebuild Phase 4's direct base and final reviewed head with
the same release pipeline, toolchain, features and data. Record commands,
artifact checksums and raw/gzip sizes; compare that delta with the budget.

## How it works

- One Loro document per list. The server keeps the merged state in
  `list_doc` (`snapshot`, `version`, `changes_since_compaction`) and keeps
  `list_item` and the `list` name and scope as a projection of it. The REST
  handlers, the Discord bot and the socket
  all call `ListSync` (`ultros/src/lists/sync.rs`), and the old row writers
  were deleted from `ultros-db/src/lists.rs`. This prevents callers using
  those helpers; it does not prevent direct entity writes. List deletion
  intentionally removes the relational rows outside `ListSync`.
- The browser stores one snapshot per user and list under
  `ultros.listdoc.v1.{user_id}.{list_id}` in localStorage, with a per-user
  index at `ultros.listdoc.index.v1.{user_id}` (last use and last known
  permission). At most 20 snapshots per user; least recently used evicted
  first. Sign-out closes the document but keeps the snapshot, so an offline
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
- A list whose page looks wrong: snapshot byte length and relational rows
  alone cannot establish projection equality. A read-only checker must
  read `list_doc.snapshot`, its `list_item` rows and `list` metadata at one
  consistent database snapshot, decode with `ListDocument::from_snapshot`,
  then compare `rows()` and `meta()` against the projection. Compare each
  natural key (item and HQ), quantity, acquired count, target price, list
  name and scope, using the projection's integer clamping rules. No such
  operator checker is supplied here; implementing and validating it is a
  promotion prerequisite. A mismatch requires investigation and is not by
  itself proof of a bypassing writer.
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
- Divergence query for the soak:
  `SELECT l.list_id FROM list_doc l WHERE l.updated_at > now() - interval '1 day'`
  gives recently touched lists, not divergence. Retain the union of IDs
  across the entire soak and run the decoded projection checker above;
  account separately for lists deleted during the soak.

## Promotion

Lists 2.0 adds guest Build/Shop and account adoption under the same Labs
preference; its product contract is in
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
through a committed row edit on both device and account lists before promotion.

The existing root service worker also serves push notifications. Worker
activation and control of open tabs are site-wide; only guest offline cache
preparation is Labs opt-in. Its fetch handler can use an anonymous guest shell
and public assets only after a cache generation has been prepared. It never
caches authenticated page HTML, account APIs, or market responses. The guest
store, offline helper, and companion modules ship with the versioned WASM
package so an older cached static helper cannot change a deployed module API.

Promotion out of Labs deletes `LAB_LISTS_SYNC`, the `LabsSettings` section
when the registry is empty, the legacy `ListView`, and the REST-driven
actions it owns. It requires the soak below to pass. Promotion is blocked
pending the projection checker, controlled bundle comparison and the
production soak. Phase 4 validation is recorded below. This document
does not record a completed production soak.

## Production soak

Run after the branch is deployed, over at least a week with the toggle on
for the maintainer's own shared lists.

1. Turn it on under Settings › Labs on two browsers and one phone, and
   share one list between two Discord accounts.
2. Daily: GlitchTip has no new issue whose title contains `list_doc`,
   `ListDocError`, `sync_payload` or `hydration` on `/list/`;
   inspect warning events from tracing target `ultros::lists::sync`, including
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
