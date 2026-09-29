# Lists 2.0: guest lists, Build, Shop and companion

Date: 2026-09-10. Epic: https://github.com/akarras/ultros/issues/1372.
Status: implemented behind Labs; local validation recorded below.

## Outcome

A player can open Ultros, create a list without signing in, add items in
place, reload without losing them, and later add the same list to a new or
existing account. Build helps assemble the project; Shop helps finish it.
All new list flows ship behind `lists-sync` while the existing experience
remains available. The toggle now changes both the data layer and the
presentation, exposing device lists and distinct Build and Shop views.

## Guest ownership and offline use

- Creating, naming, editing and checking off a device list requires no login
  and no authenticated list endpoint. Guest creation must be reachable from
  the Lists entry point while Labs is enabled, before the auth gate.
- Use stable random device-list IDs in a distinct namespace, never a fake
  account ID or a negative server ID. Account caches remain user-scoped and
  must never be exposed as guest lists after sign-out.
- Device lists are primary data, not a cache. Never apply the account
  cache's 20-list LRU eviction to them. Storage failure must surface as
  "Changes aren't saved on this device" with retry/export, not success.
- Show "Saved on this device" and a secondary "Sync to an account" action.
  Explain browser-data deletion in the storage details; provide a portable
  export and restore path. Do not make authentication the recovery path for
  a failed local write.
- Previously saved data and cached item search remain usable offline.
  Offline reload additionally needs the app shell and game-data resources
  cached; localStorage alone does not make the site available offline.
  First-ever navigation without internet is not promised. Acceptance tests
  must distinguish an already-open disconnected tab from a cold reload.
- Prices display their observation time. Fresh market lookups and refreshed
  shopping plans need connectivity; a saved list never waits on them to
  render. Missing cached catalog data must not prevent editing saved rows.
- Concurrent tabs must merge document updates rather than overwrite each
  other's snapshots. Guest storage needs transactional read/merge/write or
  a cross-tab lock around that operation; a storage event alone is not a
  concurrency guarantee. The PiP window shares its opener's live document.

## Account adoption

Signing in preserves the guest workspace. Offer "Add these N device lists
to your account" with list selection and an explicit destination account.
This applies equally to first sign-in and an existing account. Matching
names never cause a silent merge with existing account lists.

For each selected list, persist an adoption attempt before contacting the
server. Bind its random idempotency key to the authenticated destination
account and the device-list ID. The server creates a fresh account list and
records the key-to-list mapping atomically. Retry returns the same list;
timeouts, two tabs, or reloading after the server commits must not create
duplicates. Authentication and normal validation still apply; possessing a
device ID is not authorization.

Retain the guest document until the server confirms persistence and the
client durably records the destination. Preserve edits made during the
request: either merge the intervening document changes into the destination
and acknowledge them before retirement, or retain the local source with a
clear pending state. Never delete based only on an HTTP success for an older
snapshot. Account switching cancels application of stale responses; it must
not retarget an in-flight attempt to the new account. A partially completed
multi-list adoption is retryable per list.

## Build

- One Build | Shop switch with mode retained when opening the companion.
- Permanent "Add an item..." catalog search above the rows. Results remain
  within the page, with icon, name, quantity and NQ/HQ/any selection. Enter
  adds; focus remains ready for the next item. Escape dismisses results.
- Catalog search and filtering existing rows are visibly separate actions.
- Added rows highlight briefly; duplicate item+quality adds increase need
  and offer undo. Validation prevents invalid quantities or impossible HQ.
- Needed, owned, quality and target price edit directly. Enter commits,
  Escape cancels, Tab moves predictably. Keyboard events inside an editor
  must not accidentally invoke document-level undo.
- Selection checkboxes stay visible; bulk actions appear on selection.
  Deletion offers an undo toast. Sorting must not move the active editor
  before its edit commits. Preserve accessible labels and focus on removal.
- Recipe addition previews finished items versus ingredients in place.
  Retaining recipe contribution relationships is a later document-schema
  extension, not implicit in the current flat rows.

## Shop

Use the recipe planner's purchasing/travel engine after its dependency is
verified, with choices for home world, fewest stops and lowest total cost.
Explain marginal travel value: "One extra world saves 18,400 gil; a third
saves 600." Estimates are based on observed listings, not reservations.

Compute whole-stack purchase costs and show surplus. Respect quality,
remaining quantities, allowed worlds/datacenters and travel constraints.
Show missing supply explicitly, not as zero cost or a completed basket.
Group the active trip by world with stack sizes, expected cost, copy-name
and partial-purchase controls. Purchase progress immediately updates the
remaining work. Offer Undo for a purchase.

Freeze the active trip's presentation as market data changes. Offer a
reviewable refresh, and a "Listing is gone" replacement action, rather
than moving targets underneath the player. A manual completion records the
player's report; it is not proof of an in-game purchase.

## Shopping companion

Build a compact reusable Shop view for a second monitor, ordinary browser
window and Document Picture-in-Picture. Preferred action: "Pop out shopping
companion". Initial requested viewport approximately 340 by 440 CSS pixels;
the layout must accommodate browser clamping and user resizing.

Content: current world, progress, remaining items at this stop, quality,
whole stacks and expected price, Copy name, Bought, Undo and Next world.
Keep settings and list construction in the main view. Guest lists work too.

Implementation contract:

- Feature-detect `window.documentPictureInPicture`; open with
  `requestWindow({ width, height })` directly from the player's click in an
  HTTPS top-level page. Handle rejection and offer the normal-window view.
- Render real interactive HTML, load the shared styles, and use the same
  document/actions as the opener. Avoid a second independent snapshot or
  socket for the PiP view. Dispose subscriptions on close.
- The companion cannot outlive the originating document. Closing/reloading
  that page closes it. Route changes, sign-out, permission loss and list
  deletion must close or safely retarget the view; never leave stale edits
  attached to a disposed handle. Closing the companion does not lose edits.
- Browser-owned window chrome stays. The website cannot force screen
  coordinates; size is a request. No click-through overlay, game input
  injection or global game-hotkey promise. Interactions may take game focus.
- A normal-window fallback provides the same compact content but has no
  web-platform always-on-top guarantee. Communicate this distinction.
- Target FFXIV borderless/windowed initially. Test actual Windows game
  visibility, focus, copying names, marking purchases, DPI and resizing.
  Exclusive fullscreen support is unverified and must not be advertised.

References: [Chrome implementation guide](https://developer.chrome.com/docs/web-platform/document-picture-in-picture),
[MDN API overview](https://developer.mozilla.org/en-US/docs/Web/API/Document_Picture-in-Picture_API),
[requestWindow restrictions](https://developer.mozilla.org/en-US/docs/Web/API/DocumentPictureInPicture/requestWindow).

## Delivery and acceptance

### Initial foundation implementation

`ultros/static/guest-list-store.mjs` implements an IndexedDB store named
`ultros-device-lists-v1`, separate from authenticated localStorage. Records
have random `device:<UUID>` IDs, names, opaque Loro snapshot bytes and a
monotonic storage revision. Create, save and delete report success only on
transaction completion. Save/delete compare the expected revision inside
the write transaction; conflicts are returned to the caller for Loro merge
and retry. This prevents silent overwrites but does not itself merge Loro.

No automatic eviction or default-empty recovery is implemented. Damaged
records stay stored and are reported individually so healthy lists remain
discoverable. Versioned backup helpers preserve the name and snapshot but
omit device/account identity; restore validates the Loro bytes and creates
a new local list. The JS layer validates the envelope, not the Loro schema.

The Leptos `list_doc::guest` runtime validates snapshots with `ListDocument`,
merges conflicts, subscribes to cross-tab changes and exposes save failures.
`/list/device/:device_id` opens guest lists without authentication. The Labs
directory exposes creation and backup restore before the existing account
list section. Browser storage remains subject to browser/user deletion;
do not promise immunity to eviction or private-session cleanup.

Offline preparation caches only public, versioned app assets and the item
catalog, then publishes a generated anonymous shell. A staged cache generation
becomes active with one pointer write; failed asset or pointer writes retain
the prior working generation. Origin Web Locks serialize preparation and
cleanup. Unsupported or restricted browser storage reports preparation failure
without deleting device documents. Neither authenticated SSR nor account API
responses enter this cache.

Account adoption uses a validated row projection, not untrusted Loro history.
The transaction creates the account list, row projection, server document and
receipt together. The authenticated owner plus device identity are deduplicated
even when separate tabs generate different attempt keys. Receipts survive
list deletion to prevent a late retry resurrecting a deleted destination.
The client retains its device source after adoption and explicitly identifies
newer local edits as separate pending work. Retrying an immutable receipt
does not silently overwrite the account list with a later local version.

Build and Shop share document callbacks across account/device lists. The
companion is an alternate view of the live document; it does not open another
guest storage handle. Active shopping trips keep per-offer ordering and
exclude recorded purchases from replanning. Physical FFXIV window/focus
testing remains separate from browser automation and is not claimed here.

The shared `ListWorkspaceSource` presentation contract feeds one Build
workspace: inline item/recipe adding, undo/redo, filtering and the editable
price grid. Routes retain their storage, authorization and account-only
activity/settings lifecycles. Rows are keyed by document row ID and cells
read independent reactive values. Sorting and hiding completed items wait
until focus leaves the grid; actual deletion and loss of access still take
effect immediately. Account purchases apply acquired-count deltas so
concurrent purchases can merge.

Adoption is a one-time copy, not ongoing synchronization of the retained
device source. An uncertain transfer retry keeps its original snapshot and
recovers the same receipt. Newer edits remain in the device list; the UI
links to the account copy and explains how to restore a backup as a separate
device list when another independent transfer is wanted. Creation activity
commits with the first import and is not duplicated by retries.

The root push service worker controls tabs site-wide. Labs gates anonymous
offline cache preparation, not worker activation. The three guest/companion
JavaScript helpers are packaged with the versioned WASM build. All seven
locales cover the workspace, adoption, companion, persistence messages and
offline boot shell.

Run `node --test integration/guest-list-backup.test.cjs` for portable backup
checks and `node integration/guest-list-store.cjs` for real Chromium
IndexedDB tests. The latter starts its own loopback fixture and needs no
Ultros server, account, Postgres or market data. It exercises reload,
simultaneous writers, stale deletion, quota/transaction aborts, no app-level
eviction, account-cache separation, restore and corruption isolation.

`integration/lists-v2.cjs` exercises the actual app from homepage navigation
through anonymous creation, inline keyboard editing, reload, disconnected
cold reload, catalog search, backup restore and companion lifecycle. Its local
proxy disconnects the service worker's network too. `integration/list-adoption.cjs`
covers single and batch adoption, exact quantities, same-name separation,
account switching, immutable retries and edits made during transfer. Both
passed against the isolated test-auth server during implementation. The normal
E2E driver includes these flows; adoption requires `test-auth`.

Validation in the isolated local test environment passed `check_ci.sh`, the
native and browser Leptos builds, all 93 JavaScript unit tests, and the
database concurrency regression for account adoption. Real-browser guest
and adoption flows passed against that build. Desktop and mobile screenshots
were inspected. The Labs list flow passed inline item and recipe adding,
quantity editing, sharing and read-only access. Shared-list checks passed
live/offline convergence, undo, passive access revocation and deletion,
account isolation, and all eight delayed-response navigation cases. Shop
purchase and undo controls were exercised at mobile width. Selected modes,
route choices, disabled controls and touch targets received a visual polish
pass; optional route comparisons now sit below the shopping checklist.
The full-site E2E run is not green: the empty market fixture
produces no profitable FC projects, unavailable ClickHouse causes market API
errors, and the route sweep reports a WebSocket/BFCache navigation warning.
These results do not establish production readiness or physical game overlay
compatibility.

### Acceptance gates

1. **Guest foundation:** separate durable storage and IDs, storage error and
   corruption handling, cross-tab merging, export. Test reload, quota
   failure, account isolation and no eviction of primary data.
2. **Guest vertical slice:** Labs entry bypasses auth for device lists;
   create, inline-add, edit, reload. Deliver cached-shell/catalog offline
   behavior and test a disconnected reload, not just in-memory editing.
3. **Account adoption:** transactional server idempotency plus client
   lifecycle. Test timeout after server commit, retries, simultaneous tabs,
   account switching and edits during adoption with no loss or duplicates.
4. **Build polish:** editable grid, selection, sorting, keyboard/undo and
   recipe preview. Browser checks cover focus and SSR/hydration boundaries.
5. **Shop and companion:** verify planner dependency, whole-stack plans and
   stable active trips; PiP feasibility probe in the actual game followed
   by the shared compact view and fallback. Test opener lifecycle and guest
   parity. Physical game testing is separate from automated browser tests.

The existing production promotion gates (projection checker, controlled
bundle comparison, production soak) remain in `docs/lists-sync.md`. New
features do not imply those gates have passed. Per shipped player-visible
slice add its own changelog entry and run `check_ci.sh`, a fresh
`cargo leptos build`, and relevant browser regressions.

PR review validation passed the full `check_ci.sh` gate, fresh native and
WASM builds, and 95 JavaScript tests. Browser checks passed for guest
Tab-after-save and preservation of an uncommitted draft during a background
tab update, disconnected reload/catalog use, backup restore, companion
cleanup, single/batch adoption, and account Build editing/sharing. All seven
locales rendered translated Build/Shop labels and persistence status with
the helpers loaded from the versioned package. The database regression
covered concurrent receipts, batch/empty imports and nonduplicated creation
activity. Shared-list convergence, revocation/deletion, account isolation
and all eight delayed-navigation cases passed. Current-price display and
Shop purchase/undo were checked visually at desktop/mobile sizes.
The native Windows build required disabling incremental compilation after
an unresolved-symbol linker failure. The full-site E2E limitations recorded
above remain separate from this passing Lists validation.

## Later

Shared claims to avoid duplicate buying, recipe contribution tracking,
recurring restock targets, API tokens and Dalamud inventory/purchase sync.
These extend the model and permission rules and are not prerequisites for
the guest list or browser companion.
