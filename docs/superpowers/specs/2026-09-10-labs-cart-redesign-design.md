# Labs cart redesign (Track B of #1427)

Issues: #1433, #1434, #1435, #1436. Epic #1372. Delivered as four stacked
pull requests, one per issue, in that order.

## Outcome

While assembling a list under the `lists-sync` Labs experiment, a player sees
a compact cart: item, quantity, quality, estimated line cost and a small
delete control per row, with the estimated cart total always visible.
Owned quantity, target price and listing detail open per row on demand.
Deleting a row is immediate and reversible from a compact toast. The
non-Labs `ListView` route and the legacy Labs `ListBuildWorkspace` grid
stay in the tree; neither is promoted, deleted or migrated.

## Boundaries with the other tracks

The shared prerequisite (#1428), the estimate track (#1431/#1432) and the
undo track (#1429/#1430) had no pull requests when this work started. Track
B therefore defines the smallest contracts it needs, keeps them in one file
each, and states the assumptions so the owning track can replace them:

- **Estimate** (`components/cart/estimate.rs`): pure functions over the
  row's `ListItem` and its already scope-filtered `ActiveListing`s. Build
  estimates the *remaining* quantity (needed minus owned, floor 0), filling
  from the cheapest listings whose quality matches the row (NQ rows take NQ
  only, HQ rows HQ only, Any rows both). Arithmetic is `i64` with checked
  multiplication and saturating sums. A line reports how many units the
  observed supply covers; the cart total is a lower bound whenever any line
  is short and is labelled so. No new pricing engine: the listings are the
  ones the page already fetches for the row.
- **Undo availability**: `ListWorkspaceSource` gains `can_undo` and
  `can_redo` signals derived from the document's `ListUndo`. Track A owns
  the editor semantics of Ctrl+Z; the cart only reads availability and calls
  the existing `undo`/`redo` callbacks.

## Architecture

New module `ultros-frontend/ultros-app/src/components/cart/`:

| File | Owns |
| --- | --- |
| `mod.rs` | `ListCart` host: add composer, filter, sort, row list, mounts the parts below |
| `row.rs` | `CartRow`: identity, quantity editor, quality control, line estimate, details toggle, delete button |
| `summary.rs` | `CartSummary`: estimated total, coverage state, item/unit counts |
| `details.rs` | `CartRowDetails`: owned, target price, matching listings, pricing detail |
| `selection.rs` | `CartSelectionBar`: bulk controls revealed by selection |
| `feedback.rs` | `CartFeedback`: deletion toast with Undo, stale-action guard |
| `estimate.rs` | line/cart estimate contract and tests |

`ListCart` takes the existing `ListWorkspaceSource` (extended, never
duplicated) plus the caller's `selected_items` and `highlighted` signals.
Both hosts mount it where `ListBuildWorkspace` was mounted: `ListViewSync`
(account document, REST fallback, permission capabilities) and
`DeviceEditor` (device document, always writable). Document, sync, guest
storage and authorization lifecycles are untouched.

`ListWorkspaceSource` additions, all optional-by-default so the legacy grid
keeps compiling:

- `can_undo`, `can_redo: Signal<bool>` (PR 1)
- `remove_many: Callback<Vec<i32>>`, `set_quality_many: Callback<(Vec<i32>, Option<bool>)>` (PR 3)
- `sort: Signal<Option<SortSpec>>`, `set_sort: Callback<Option<SortSpec>>` (PR 3)

Row identity is the document row id. Every per-row signal is keyed by id,
so reactive updates, sorting and deletions never move state between rows.

## Per-PR design

### PR 1 — #1433 separate presentation

`ListCart` reproduces the current Labs Build behaviour and accessible names
one-for-one (so `lists-v2.cjs` and `list-flow.cjs` pass unchanged), split
across `mod.rs`, `row.rs` and `summary.rs`. `estimate.rs` lands with tests
and a fixture helper but is not yet rendered. Both hosts switch to
`ListCart`; `ListBuildWorkspace` and `BuildListRow` stay compiled and
reachable through the tester query `?cart=legacy` on either host, so the
two presentations can be compared on the same list without a new Labs
token. Undo/Redo toolbar buttons disable when the document reports
nothing to undo or redo. No changelog entry: nothing visible changes.

### PR 2 — #1434 compact rows and total

Default row: icon and name, quantity input (`Needed for {name}`), quality
control (select with NQ/HQ/Any; HQ is offered only when the item can be HQ),
estimated line total, a details toggle, and an icon-only delete button
labelled `Remove {name}` with a 40px hit target. Owned and target price move
into the row's details panel (minimal in this PR; PR 3 expands it).
`CartSummary` sits above the rows and stays visible while building: total,
`covers X of Y units` when short, `no listings for N items` when unknown.
Rows are a CSS grid list, not a fixed-width table, so 390px shows name,
quantity, quality and estimate without horizontal scroll. Add composer and
filter remain separate inputs; recipe add is a secondary button. Delete
still calls `source.remove`; the toast comes in PR 4. Changelog entry.

### PR 3 — #1435 details and bulk actions

`CartRowDetails`: `aria-expanded`/`aria-controls` toggle, panel with owned,
target price, the five cheapest matching listings (world, unit price,
stack, quality) and the pricing detail the estimate used. Escape inside the
panel and the toggle both close it and return focus to the toggle. Expanded
ids live in a `HashSet<i32>` keyed by row id. Selection checkboxes stay
visible; `CartSelectionBar` appears only when the selection is non-empty
and offers Delete selected, Set HQ, Any quality and Clear. Selection is
pruned whenever rows disappear. Sorting: header buttons (name, quantity,
estimate) with `aria-sort`, keyboard operable, backed by the URL `sort`
param on account lists and a local signal on device lists; the existing
focus-in pin keeps an active editor's row in place until it commits.
Listings are already part of the row source, so details need no fetch and
the UI does not claim lazy loading. Host headers lose their separate bulk
toolbars. Changelog entry.

### PR 4 — #1436 deletion feedback, undo, focus

`CartFeedback` is a polite live region under the summary. After a single or
bulk removal it shows `Removed {name}` / `Removed {n} items` with Undo and
Dismiss, auto-dismissing after 8 seconds. A local action counter increments
on every write through the source; the toast remembers the counter at its
deletion and hides Undo when any other local write (or a newer deletion)
has happened since, so it can never undo the wrong action. Undo is also
hidden when `can_undo` is false or the source is read-only. Focus after a
removal goes to the next row's delete button, else the previous row, else
the add composer; after Undo it goes to the restored row's quantity input.
The delete button disables while its removal is in flight. Errors from the
source replace the toast text. Both hosts behave the same; the device host
surfaces its `apply` error the same way. Changelog entry.

## Testing

- Unit: `estimate.rs` (quantities, qualities, partial supply, mixed lines,
  overflow), feedback stale-action guard, selection pruning.
- Browser: `integration/lists-v2.cjs` extended for quality change, visible
  estimates, details toggle, bulk actions, delete, undo and focus;
  `list-flow.cjs` updated for the details toggle. Desktop and mobile
  screenshots captured through the existing harness.
- `./check_ci.sh` and a fresh `cargo leptos build` per PR.
