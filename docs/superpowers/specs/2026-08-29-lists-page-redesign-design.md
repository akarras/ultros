# Lists page redesign — design spec (2026-08-29)

## Problem

The list detail page (`/list/:id`, `ultros-frontend/ultros-app/src/routes/list_view.rs`) has three issues:

1. **Exclude-datacenters is effectively broken.** The `excluded-datacenters` query param is only passed
   to `BuyingView`; the main list table applies only `filter_excluded_worlds`. Toggling a DC chip does
   nothing outside the buying view, while exclude-worlds works everywhere.
2. **The layout feels disorganized.** The experimental AutoMarkPurchases `<details>` accordion occupies
   the top of the page; the filters panel has three separately-labeled clusters (world exclusion, DC
   chips, sort/hide-acquired) with no visual grouping.
3. **No snapshot/regression tests** cover the page's functionality or layout.

## Design

### 1. Unified exclusion filtering (bug fix)

Replace `filter_excluded_worlds` with a single pure function:

```rust
fn filter_excluded(
    items: &[(ListItem, Vec<ActiveListing>)],
    excluded_worlds: &HashSet<i32>,
    excluded_datacenters: &HashSet<String>,
    world_helper: &WorldHelper,
) -> Vec<(ListItem, Vec<ActiveListing>)>
```

A listing is dropped when its world id is in `excluded_worlds` **or** its datacenter name is in
`excluded_datacenters` (via the existing `ActiveListing::is_datacenter_excluded`). Applied once in
`ListView` before the table rows, summary, and buying view. `BuyingView` keeps its
`excluded_datacenters` prop for now (harmless double-filter) — no behavioral change there.

Result: excluding a DC changes prices/rows in the main table, matching exclude-worlds semantics.

### 2. Unified filter row

One panel row with three labeled groups (all state stays in the existing `filter_query_signal` URL
params — shareable links unchanged):

- **Exclusions** — one shared label. DC chips first, then the world `<select>` and removable world
  chips + clear button.
- **Sort** — the existing sort `<select>`.
- **View** — the hide-acquired toggle.

### 3. Auto-mark purchases: accordion → toolbar button + modal

- Remove the top-of-page `<details>` accordion.
- `AutoMarkPurchases` re-renders as: a `sticky-bar-button` in the toolbar's right group (next to
  Subscribe) that opens a `Modal` containing the character-name input, experimental badge, and
  start/stop watching button.
- While watching is active the toolbar button carries the active styling (`bg-brand-900` /
  `border-brand-500`) so the running watcher stays visible after the modal closes.
- Watching state and the realtime subscription stay inside `AutoMarkPurchases`; only presentation
  changes. `apply_purchase_to_list` and its tests are untouched.
- New page order: toolbar → filter row → table.

### 4. Tests — three layers

1. **Logic unit tests** (cargo, in-crate `#[cfg(test)]`):
   - `filter_excluded`: world-only, DC-only, both, empty sets, unknown world id.
   - `sort_list_items`: each key asc/desc, ties fall back to item id.
   - `remaining_quantity`, `cheapest_price_per_unit` edge cases.
   - `IdList` / `NameList` `FromStr`/`Display` round-trips (dedupe, sort, junk input).
2. **Rust render snapshots** (`insta` dev-dependency, new to the repo): SSR render-to-string of the
   filter row and the auto-mark modal content, wrapped in a Leptos `Owner` (per the known
   test-Owner/arena trap). Snapshots live next to the tests; reviewed via `cargo insta`.
3. **Puppeteer** (`integration/`): extend `list-flow.cjs` — open the auto-mark modal, toggle a DC
   chip and assert the `excluded-datacenters` URL param plus a visible listing change, and capture
   screenshots of the redesigned page. The `.list-toolbar` querySelector hook is preserved.

### 5. i18n

New keys (unified "Exclude" label, auto-mark button/modal strings) added to **all 7 locale files**
with real translations, per repo policy.

## Out of scope

- No backend/API changes.
- No buying-view internal changes beyond receiving already-filtered items.
- No changes to list index page (`lists.rs`).
