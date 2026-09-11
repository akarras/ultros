# Lists 2.0 redesign: delivery status, contracts and evidence (#1428)

Date: 2026-09-11. Parent: #1427. Epic: #1372.

This is the reconciliation #1428 asked for. It records what is built, which
check exercises it, what is deployed, and the row, estimate and undo
contracts the four tracks implemented against. The contracts were settled in
the per-track specs while the tracks ran in parallel; this document collects
them in one place and names the differences that were resolved when the
tracks were merged. It does not claim any run it does not list.

Companion documents:

- Product contract: `docs/superpowers/specs/2026-09-10-lists-2-product-design.md`
- Estimates (Track C): `docs/superpowers/specs/2026-09-10-lists-cart-estimates-design.md`
- Cart presentation (Track B): `docs/superpowers/specs/2026-09-10-labs-cart-redesign-design.md`
- Undo (Track A): `docs/superpowers/specs/2026-09-10-list-undo-keyboard-and-editing-design.md`
- Shop and companion evidence (Track D): `docs/qa/lists-2-shop-companion-evidence.md`
- Promotion gates: `docs/lists-sync.md`

## Checkout, build and deployment reviewed

| What | Value |
| --- | --- |
| Checkout reviewed | `main` at `0a18f436` (feat(lists): compact deletion feedback with undo and focus recovery (#1436) (#1451)) |
| Deployed | The production `ultros` container runs image `ghcr.io/akarras/ultros:main` with `org.opencontainers.image.revision` = `0a18f436…`, created 2026-09-11 13:34 UTC, started 13:36 UTC. Every pull request in the table below is in that build. |
| Labs state in production | `lists-sync` is opt-in (`LABS` cookie or `?labs=lists-sync`). Nothing in this document changes that. |
| Production smoke (2026-09-11, read-only, this review) | With `LABS=lists-sync` and no account: the Lists entry offered "Create a device list"; a new device list opened at `/list/device/device:<uuid>` with `list-cart`, `cart-rows`, `list-undo`/`list-redo`, `list-estimate-summary` and `cart-feedback` present; adding Maple Log produced one `li[data-item-id]` row whose controls are labelled `Select`, `Needed for`, `Quality for`, `Details for` and `Remove Maple Log`, the quantity input carried `data-committed="1"`, the estimate status read "Look up prices in Shop to estimate this list." with total "—", Undo became enabled, and the sort headers reported `aria-sort="none"`. This is a smoke of the deployed Build contract, not the #1439 journey. |

Evidence rules used below: a script named in a row is a claim about what
the script asserts, not a passing run, unless the "Runs" section lists it.
The local test reports quoted in the product design spec (2026-09-10) are
historical evidence for the guest foundation and adoption; they predate the
four tracks and are not repeated as runs here.

## Delivery matrix

Status: **built** (code on `main`, a listed check exercises it), **partial**
(built; a named part is not met), **not built**, **unverified** (built; no
listed check exercises it). Owner names the issue that carries the remainder.

### Build

| # | Requirement (product spec, "Build") | Status | Code on `main` | Check | Owner |
| --- | --- | --- | --- | --- | --- |
| B1 | One Build \| Shop switch; mode retained when opening the companion | built | account `list_view_sync.rs` (`?buy` via `filter_query_signal`), device `guest_lists.rs` (`guest-build-mode`/`guest-shop-mode`); Shop stays mounted after first open (#1442) | `lists-v2.cjs`, `list-shop-handoff.cjs` | — |
| B2 | Inline "Add an item…" catalog search: icon, name, quantity, NQ/HQ/Any; Enter adds; focus stays; Escape dismisses | built | `inline-list-add` composer inside `ListCart` (`components/cart/mod.rs`), shared by both hosts through `ListWorkspaceSource::add` (#1421) | `lists-v2.cjs` "inline add, duplicate quantities, focus, Escape and edited values survive reload"; `list-undo-keyboard.cjs` search-focus rules | — |
| B3 | Recipe adding previews finished items versus ingredients in place | built | `ListWorkspaceSource::recipe_open`/`toggle_recipe`, `add_many` (#1421) | `list-flow.cjs` "owner adds a recipe" (account, Labs and legacy) | — |
| B4 | Catalog search and row filtering are separate controls | built | `inline-list-add` versus the `lists_workspace_filter_label` search input in `ListCart` | `lists-v2.cjs` | — |
| B5 | Duplicate item+quality adds increase need and offer undo; invalid quantities and impossible HQ rejected | built | document merge in `ultros-list-doc`; `CartRow` offers HQ only when the item can be HQ; numeric editor `min` | `lists-v2.cjs` duplicate quantities; `ultros-list-doc` crate tests | — |
| B6 | Needed, owned, quality and target price edit directly; Enter commits, Escape cancels, Tab moves predictably | built | `cart/row.rs::numeric_editor` (needed), `cart/details.rs` (owned, target price), quality `<select>`; Tab order Needed → Quality → Details → Remove | `list-undo-keyboard.cjs` "Ctrl+Z in the next clean cell undoes the Tab-committed edit"; `lists-v2.cjs` | — |
| B7 | Keyboard events inside an editor never invoke document undo by accident | built | `list_doc/undo.rs::classify_target` + `data-committed` (#1446); numeric cells stop propagation only for Enter and a draft-discarding Escape | `list-undo-keyboard.cjs` (nine editor-focus scenarios) | — |
| B8 | Selection checkboxes always visible; bulk actions appear on selection | built | `CartRow` checkbox, `cart/selection.rs::CartSelectionBar` (Set HQ, Any quality, Delete selected, Clear); selection survives the id re-key a bulk quality change causes (#1450) | `lists-v2.cjs` "selecting rows reveals bulk quality actions…" | — |
| B9 | Deletion offers an undo toast; labels and focus preserved on removal | built | `cart/feedback.rs::CartFeedback` (8 s, stale-action guard, `undo_offered`, `focus_after_removal`) (#1451) | `lists-v2.cjs` "bulk delete announces the removal and its Undo restores the row", "keyboard removal restores focus and a later edit retires the stale Undo" | — |
| B10 | Sorting never moves the active editor before its edit commits | built | `ListCart` focus-in pin (`editor_drafting`); header buttons with `aria-sort` for Item, Qty, Est. cost plus a `cart_sort_by` select; account sort is the URL `sort` param, device sort a local signal | `lists-v2.cjs` "header sorting waits for the active editor to commit"; unit `header_clicks_cycle_ascending_descending_off` | — |
| B11 | Rows highlight briefly after an add | built | `highlighted` signal fed by `recently_changed` on the account host | visual only; no script asserts the highlight | #1439 (journey screenshots) |
| B12 | Legacy UI preserved; Labs boundary kept | built | non-Labs `/list/:id` → `ListView` unchanged; Labs `ListViewSync`/`DeviceEditor` mount `ListCart`; `?cart=legacy` mounts `ListBuildWorkspace` on either host | `list-flow.cjs` runs twice in `run_e2e.sh` (with and without `LABS_COOKIE`) | — |
| B13 | One cart total on the account Build view | partial | `ListCart` mounts `ListEstimateSummary`; the account host still renders the legacy whole-stack `ListSummary` panel beneath it, so two totals with different arithmetic show | read during this review (`list_view_sync.rs`, the `class:hidden=buying_view` block) | **#1457** |

### Guest and account parity

| # | Requirement | Status | Code | Check | Owner |
| --- | --- | --- | --- | --- | --- |
| P1 | Same Build presentation for device and account lists | built | one `ListCart` behind `ListWorkspaceSource`; both hosts install `undo::install` with `UndoBindings` | device: `lists-v2.cjs`, `list-undo-keyboard.cjs`; account: `list-sync.cjs`, `list-flow.cjs` (Labs pass), `list-shop-handoff.cjs` | — |
| P2 | Same Shop for both | built | `ListShop` behind account `?buy` and `DeviceShop` | `list-shop-handoff.cjs` (account), `lists-v2.cjs` (device) | — |
| P3 | Read-only shared access shows read-only chrome and no editors | built | `can_write` disables every editor, checkbox and delete; toast Undo hidden when read-only | `list-flow.cjs` reader step; `list-sync.cjs` scenario B (revocation) | — |
| P4 | Account keyboard undo across a reconnect and against remote edits | built | Loro undo is local-only (crate tests); `list-sync.cjs` presses Ctrl+Z immediately after reconnect | `list-sync.cjs` scenario A step 4 | — |

### Shop and companion

Tracked row by row in `docs/qa/lists-2-shop-companion-evidence.md` (S1–S18,
C1–C14). Open rows and their owners:

| Row | Gap | Owner |
| --- | --- | --- |
| S3 | Per-hop marginal travel savings ladder | #1334 (planner owns the model; Shop reuses it) |
| S7 | Browser coverage of excluded worlds/datacenters reaching Shop | #1439 |
| S11 | "Undo purchase" runs document undo and can revert a later Build edit | **#1458** |
| C3, C5, C11, C12 | Real Document PiP path, DPI, resizing, exclusive fullscreen against the game | #1444 |
| C7 | Browser check that sign-out and permission loss close the companion | #1439 |

## Contracts

### Default cart row

Implemented by `components/cart/row.rs::CartRow`; every per-row signal is
keyed by the document row id (`adapter::row_id` encodes item and quality, so
a quality change re-keys the row; selection and expanded state follow it).

| Slot | Default row | Accessible name | Notes |
| --- | --- | --- | --- |
| Select | checkbox, always visible | `Select {name}` | disabled when read-only |
| Identity | icon + name | link to the item | — |
| Quantity | numeric editor | `Needed for {name}` | `data-committed` = document value; Enter commits and blurs, Escape discards a draft, Tab commits |
| Quality | `<select>` NQ / HQ / Any | `Quality for {name}` | HQ offered only when the item can be HQ; commits immediately |
| Estimated line total | text | — | `LineEstimate` for the *remaining* units; "—" when unpriced |
| Details toggle | icon button, `aria-expanded`/`aria-controls` | `Details for {name}` | opens `CartRowDetails` |
| Delete | icon button, 40 px target | `Remove {name}` | immediate; toast with Undo follows |

Secondary (inside `CartRowDetails`, `cart-row-details`): owned quantity,
target price, the five cheapest matching listings (world, unit price,
stack, quality) and the pricing detail line (`cart-pricing-detail`).
Escape inside the panel closes it and returns focus to the toggle.

Always visible above the rows: `ListEstimateSummary` (`list-estimate-total`,
`list-estimate-status`, detail line). The cart total is never hidden by
filtering, sorting or selection.

Mobile (390 px): the row is a CSS grid, not a table; name, quantity,
quality and estimate fit without horizontal scroll. Desktop adds the header
row with sort buttons and the select-all checkbox.

### Estimate

Engine: `ultros_calc::list_estimate` (25 unit tests). `components/cart/estimate.rs`
is a thin adapter (`estimate_line(&ListItem, &[ActiveListing])`,
`matching_listings`) and must not grow a second engine.

- **Quantity priced:** remaining = `needed − owned`, floor 0. A row with
  nothing remaining is `Acquired` and contributes zero. `requested` is
  also on the line for presentation.
- **Any quality:** NQ rows take NQ listings, HQ rows HQ, Any rows both,
  cheapest units first with no preference.
- **Arithmetic:** listings sorted by unit price, then larger stack, then id;
  units taken until covered; partial stacks allowed. This is the cost of the
  cheapest *units* on the board, not a purchasable basket.
- **Distinction from Shop:** Shop prices *whole stacks* and reports surplus
  through `list_shop.rs::trip_totals`; the two totals are labelled
  differently and never mixed (`shop-estimate` explains the difference).
- **Supply coverage:** short lines are `PartialSupply`/`NoSupply`, carry the
  unpriced unit count, and mark the cart `incomplete`; never shown as zero
  cost or omitted.
- **Overflow:** `i64` products, `checked_add` sums; the cart saturates and
  says "Too large to total exactly."
- **Pricing scope:** account lists use the server's listings for the list's
  own world/datacenter/region scope minus the page's excluded worlds and
  datacenters, cached per scope; device lists use the listings the player
  looked up in Shop (`LookupTicket` discards late responses).
- **Freshness:** `PriceFeed` = `Loading` \| `Missing { NotRequested \| Failed }`
  \| `Observed { fetched_at, refresh_failed }`. `fetched_at` is the client
  clock at response arrival, never a listing's own timestamp. A failed
  refresh keeps the last observation and sets `refresh_failed`. No age
  threshold flips an estimate to "stale" on its own; the detail line shows
  the relative fetch time.

Status text priority is listed in the estimates spec ("Status text") and is
what production rendered in the smoke above.

### Undo

Engine: `ultros_list_doc::ListUndo` over Loro's undo manager, local
operations only. `ListUndo::MERGE_INTERVAL_MS = 0`: every committed action is
exactly one step; bulk operations stay one step through `group`.

- **Draft versus committed:** a draft is editor text that differs from the
  value in `data-committed`. Ctrl+Z on a draft is native text undo; on a
  clean list editor, on a select/checkbox/button, or outside any editable
  element it is document undo; with a modal or confirmation panel open it
  does nothing (`classify_target`).
- **Bindings:** Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y (Cmd variants on Apple, no
  Cmd+Y). One window listener per open editor, installed by both hosts
  through `undo::install(UndoBindings)`.
- **Availability:** `can_undo`/`can_redo` on both handles and on
  `ListWorkspaceSource`; toolbar buttons disable with a tooltip; a shortcut
  with nothing to do writes "Nothing to undo/redo" to the feedback line.
- **Removal toast:** `CartFeedback` offers Undo only while the toast's
  action sequence is the latest local write, `can_undo` is true and the
  source is writable; it never undoes a different action.
- **Shared edits:** undo reverts only this page's operations; a peer's
  overwrite is never resurrected. After a reconnect that merged remote
  updates the stack is intact; after a snapshot rebase it restarts and the
  buttons read disabled.
- **Not covered:** Shop's "Undo purchase" is document undo (#1458).

### States and acceptance

| State | Build contract | Where asserted |
| --- | --- | --- |
| Default | compact rows, estimate summary, undo/redo toolbar, composer, filter, sort | `lists-v2.cjs`, production smoke |
| Expanded | one or more `cart-row-details` open; expanded ids follow re-keyed rows | `lists-v2.cjs` (details toggle) |
| Selected | `cart-selection-bar` visible with counts; bulk actions | `lists-v2.cjs` |
| Removing / removed | delete button disabled in flight; `cart-removal-toast` with Undo/Dismiss | `lists-v2.cjs` |
| Read-only | editors disabled, no delete, no toast Undo | `list-flow.cjs` reader, `list-sync.cjs` B |
| Error | source error text replaces the toast; device save failure surfaces in `device-list-status` | `lists-v2.cjs` (storage), unit `undo_offered` |
| Prices missing / loading / failed / observed | `list-estimate-status` text per `PriceFeed` | `ultros-calc` tests; smoke (`NotRequested`) |
| Desktop / mobile | header row and sort buttons at `sm:`; grid rows collapse at 390 px | screenshots in the Track B PRs; #1439 journey |

## Pull requests (all merged 2026-09-11, all in the deployed build)

| Issue | PR | Delivered |
| --- | --- | --- |
| #1429 | #1441 | `undo::install(UndoBindings)`; device editor installs the listener; `list-undo-keyboard.cjs` |
| #1430 | #1446 | `data-committed`, `classify_target`, merge interval 0, `can_undo`/`can_redo`, "Nothing to undo" |
| #1431 | #1440 | `ultros_calc::list_estimate`, `ListEstimateSummary` |
| #1432 | #1447 | `PriceFeed`, `LookupTicket`, scope-keyed listings cache |
| #1433 | #1443 | `components/cart/` module, `?cart=legacy` hatch |
| #1434 | #1448 | compact rows, estimate summary mounted in the cart |
| #1435 | #1450 | details panel, selection bar, sortable headers |
| #1436 | #1451 | removal toast, stale-undo guard, focus recovery |
| #1437 | #1442 | Shop latched mount, drift/review/replan, handoff header, `list-shop-handoff.cjs` |
| #1438 | #1445 | Shop and companion evidence matrix |

Reconciled at merge time (the tracks overlapped):

- Track B's `cart/estimate.rs` estimator and `CartSummary` were replaced by
  the Track C engine and `ListEstimateSummary`; the adapter stays.
- Track A's editor contract (`data-committed`, selective `stop_propagation`)
  was ported into `cart/row.rs` because `CartRow` originally swallowed every
  keydown.
- Both tracks' `can_undo`/`can_redo` copies were collapsed into Track A's
  revision-tracking versions.
- A bulk quality change re-keys rows; selection and expanded state now carry
  over to the new ids.
- Escape in a clean numeric cell bubbles so the details panel can close.

Changelog entries exist for every visible change (`ultros-changelog/changes/`,
2026-09-10 and 2026-09-11 `labs-*`, `list-*`, `device-list-*`,
`predictable-list-undo`).

## Runs recorded for the reviewed build

| Date | Build | Command | Result |
| --- | --- | --- | --- |
| 2026-09-11 | each landed tree of the #1441 → #1451 chain, ending at `0a18f436` | fmt/clippy on the integration worktree and a `cargo leptos build --bin-features test-auth` per landed step | reported green by the landing session in the PR threads; not re-run for this document |
| 2026-09-11 | same chain, local test-auth build with the scratch Postgres | `lists-v2.cjs`, `list-undo-keyboard.cjs`, `list-sync.cjs`, `list-shop-handoff.cjs` | reported green by the landing session; `list-sync.cjs` D/E navigation timeouts roamed on two runs without a server panic (2 s pool-acquire waits from ingest, environmental until proven otherwise) |
| 2026-09-11 | production (`0a18f436`) | read-only device-list smoke described above | as described; no failures |
| 2026-09-11 | this branch (`0a18f436` + docs/driver change) | `npm --prefix integration run test:list-companion` (new script) | passed |
| 2026-09-11 | this branch | `cargo fmt --all -- --check`, `bash -n scripts/run_e2e.sh` | passed; clippy not run (no Rust change) |

Not run for this document: `./scripts/run_e2e.sh` end to end, any mobile
viewport pass, any account journey against production.

## Browser verification still owed

- The integrated journey of #1439 (add several → adjust → read cost →
  delete/undo → Shop → partial purchase → back to Build) on account and
  device lists at desktop and mobile widths, with screenshots and the exact
  build recorded.
- S7 and C7 browser coverage (above).
- `list-undo-keyboard.cjs` and `list-companion.cjs` run in the E2E driver
  from this change on; the first driver run after merge is the first
  recorded run under `run_e2e.sh`.
- The production soak, projection checker and bundle comparison in
  `docs/lists-sync.md` remain unrun; nothing here authorises promotion.

## Known defects not in a track

- Intermittent SSR panic on the account Labs page during `list-sync.cjs`
  (2 of 5 runs on 2026-09-11): `cart/mod.rs` reads `hide_acquired` after
  the owner is disposed as the client's websocket closes, and the navigation
  hangs until timeout. The legacy grid has the same read. Root cause and fix
  are PR #1456 (drain abandoned renders; GlitchTip #7269 family).
- `integration/list-bulk-edit.cjs` fails on its own add-item bodies missing
  `ListItem.id` (legacy page, pre-existing, not wired into the driver).

## Unowned gaps

None. Every unmet requirement above names #1334, #1439, #1444, #1456,
#1457 or #1458.
