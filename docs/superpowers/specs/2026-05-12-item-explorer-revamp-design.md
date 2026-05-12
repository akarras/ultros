# Item Explorer Revamp — Design

**Date:** 2026-05-12
**Status:** Awaiting user review
**Scope:** Remove the page-local sidebar and duplicate mobile hamburger that Item Explorer has carried since before the AppShell redesign (PR #643). Replace them with a top-of-content toolbar that uses the redesign's existing `Toolbar` primitive. No changes to routing, no changes to the underlying item-listing logic.

---

## Why

The site-wide UI redesign in [PR #643](https://github.com/akarras/ultros/pull/643) introduced an `AppShell` with a persistent 240px `SideNav` on the left and a slim `TopBar` whose `top-bar-hamburger` toggles the global navigation drawer on mobile. Every route now sits inside `app-shell-content`.

Item Explorer was not migrated. It still renders the pre-redesign layout from [item_explorer.rs:744-870](../../ultros-frontend/ultros-app/src/routes/item_explorer.rs:744):

- A 280px `<aside>` of `CategorySection` accordions (Weapons / Armor / Items / Housing / Job Sets) on `lg:` and up.
- A sticky `lg:hidden` mobile header at [item_explorer.rs:777-785](../../ultros-frontend/ultros-app/src/routes/item_explorer.rs:777) with its own hamburger that toggles the same aside as a `?menu-open=true` drawer.
- A backdrop overlay that dismisses the drawer.

Effects today:

- **Mobile:** two hamburgers stacked — the `TopBar` one for app-level nav, the page one for categories. Users have to learn which one does what, and the page-level one looks like it should be the primary nav because it sits inside the content area with a page title next to it.
- **Desktop:** two left rails — the global `SideNav` (240px) plus the page-local aside (280px). The item grid begins ~520px in. On a 1366px laptop that's 38% of horizontal space consumed by nav chrome before any item icon renders.
- **Drift from the rest of the app.** Analyzer routes use the `Toolbar` / `ToolbarPills` primitive ([toolbar.rs](../../ultros-frontend/ultros-app/src/components/toolbar.rs)) for their filter row. Item Explorer is the only "tool" page that still has a page-level sidebar.

## Non-goals

- No changes to the `/items`, `/items/category/:category`, `/items/jobset/:jobset` routes or the components that render their bodies (`CategoryItems`, `JobItems`, `ItemList`).
- No changes to the search-category or job-category data structures from `xiv_gen`.
- No changes to the global `SideNav` or `TopBar`. Approach C from the brainstorm (promoting categories into the global sidebar) is rejected; this spec keeps category browsing as page-level concern.
- No new icons or assets. Reuse `ItemSearchCategoryIcon` and `ClassJobIcon`.
- No localization changes beyond reusing the `item_explorer_*` keys already defined for the existing sidebar (`item_explorer_weapons`, `item_explorer_armor`, `item_explorer_items`, `item_explorer_housing`, `item_explorer_job_sets`, `item_explorer_categories`, `item_explorer_title_main`).

## Design

### Component shape

`ItemExplorer` becomes a thin shell:

```
<div class="item-explorer">
  <ItemExplorerToolbar />     // new — top-of-content category navigation
  <Outlet />                  // CategoryItems / JobItems / landing — unchanged
</div>
```

The `lg:hidden` mobile header, the `<aside>`, the backdrop, the `menu_open` query signal, and the `set_open` setter are all removed. `active_category_group` and the `is_open` derivation move into `ItemExplorerToolbar`.

### `ItemExplorerToolbar` layout

Two rows inside a single `Toolbar`:

**Row 1 — group selector.** A `ToolbarPills` segmented control with five buttons matching today's `CategorySection`s:

| Pill | Selected when | Click target |
|------|---------------|--------------|
| Weapons | active group = 1 | `/items` landing scrolled to weapons (see Row 2) |
| Armor | active group = 2 | same shape, group 2 |
| Items | active group = 3 | group 3 |
| Housing | active group = 4 | group 4 |
| Job Sets | active group = 5 (route has `:jobset` param) | group 5 |

`aria-pressed` reflects the active group from the existing `active_category_group` memo. Each pill is a `<button>` that sets a local `selected_group` signal — clicking does not navigate. Navigation happens when the user picks a chip in Row 2.

The default `selected_group` is whatever `active_category_group` resolves to, with a fallback to `Some(1)` (Weapons) when the route is plain `/items` and no category is active. This means a fresh visitor sees the Weapons subcategory chips, mirroring the current sidebar's default-open behavior on the landing page.

**Row 2 — subcategory chip strip.** A horizontally-scrollable `flex` row of chip buttons for the children of the selected group:

- Groups 1-4 (Weapons / Armor / Items / Housing): chips are `ItemSearchCategoryIcon` + name, sorted by `cat.order` (matches today's `CategoryView`). Click navigates to `/items/category/{name}` via `<A>` so the SPA router handles it.
- Group 5 (Job Sets): chips are `ClassJobIcon` + abbreviation, filtered and sorted using the same predicates as today's `JobsList` (`job_index > 0 || doh_dol_job_index >= 0`, non-empty name/abbreviation, sorted by `ui_priority`). Click navigates to `/items/jobset/{abbrev}`.

Active chip styling uses `aria-current="page"` from `leptos_router`'s `<A>` so the existing CSS hook `aria-[current]` works without new variants.

Overflow behavior: the chip row sets `overflow-x-auto` with a thin scrollbar; on mobile users can flick-scroll. On desktop with many chips (Job Sets has ~25), the row scrolls horizontally rather than wrapping — this keeps the toolbar to a fixed two-row height and avoids reflow when changing groups.

### Mobile

The `lg:hidden` page header disappears entirely. The `TopBar` already shows the page context via its breadcrumb/title slot; we don't need a second page title inside the content area. The two-row toolbar collapses naturally:

- Row 1 (5 pills) fits on a 360px screen at the smaller pill size used elsewhere in the app.
- Row 2 (chip strip) is already `overflow-x-auto`; identical behavior to desktop.

No `?menu-open` query param, no backdrop, no drawer. The only hamburger on the page is the `TopBar` hamburger that opens the global `SideNav`. Both UX problems (double hamburger, double sidebar) are removed by the same deletion.

### State and routing

- `query_signal("menu-open")` is removed. Any inbound deep link with `?menu-open=true` is silently ignored — that query param only ever existed to toggle the drawer.
- `SideMenuButton` (which wrapped category links in `APersistQuery` to strip `menu-open` on navigation) is deleted along with the drawer. Other callers of `APersistQuery` elsewhere in the app are unaffected.
- `active_category_group` keeps its current logic and moves into the new toolbar component.

### Files touched

| File | Change |
|------|--------|
| [item_explorer.rs](../../ultros-frontend/ultros-app/src/routes/item_explorer.rs) | Remove `ItemExplorer`'s sidebar/header/drawer markup. Remove `SideMenuButton`. Move `CategoryView`, `JobsList`, `CategorySection`, `active_category_group` into the new toolbar module or delete them (see below). Keep `CategoryItems`, `JobItems`, `ItemList`, `Items`, and the `tests` module untouched. |
| `ultros-frontend/ultros-app/src/routes/item_explorer_toolbar.rs` (new) | `ItemExplorerToolbar` component with the two-row layout described above. Encapsulates `selected_group` and `active_category_group`. Kept as a sibling module of `item_explorer.rs` to avoid converting `item_explorer.rs` into a directory module. |
| [tailwind.css](../../style/tailwind.css) | Add `.item-explorer-chip-row` utility (overflow-x-auto + scrollbar-thin + gap), and `.item-explorer-chip` for chip button styling that reuses brand colors and `aria-current` highlight. Row 1 uses the existing `toolbar` and `toolbar-pills` classes. |
| [lib.rs](../../ultros-frontend/ultros-app/src/lib.rs) | Register the new `item_explorer_toolbar` module. Route registration for `/items/*` is unchanged. |

`CategorySection` and the `<details>` accordion are deleted: chips replace them. `CategoryView` becomes a private function inside the toolbar module returning a `Vec` of chip views for a given group; `JobsList` likewise.

### Accessibility

- Row 1 pills: `role="group"` (already provided by `ToolbarPills`), each `<button>` with `aria-pressed`.
- Row 2 chips: each chip is an `<A>` link. Active chip relies on `aria-current="page"` set by the router for screen-reader announcement.
- Keyboard: Row 1 is normal tab order. Row 2 is also normal tab order; arrow-key navigation between chips is out of scope for this revamp.
- The page title (`MetaTitle` / `MetaDescription` in `CategoryItems` / `JobItems`) is unchanged, so screen reader users still announce the current category on navigation.

### Visual baseline

Two-row toolbar, ~96px tall on desktop (40px row + 8px gap + 48px row). The item grid below uses the full `app-shell-content` width minus the existing `max-w-[1600px] mx-auto` content cap. On a 1366px viewport that means ~1086px of content cell after the 240px global sidebar — vs. ~806px today.

## Risks and mitigations

- **Job Sets row width.** With ~25 jobs at icon+abbrev (~64px each), the chip row is ~1600px wide. It must scroll horizontally without breaking layout. Mitigation: `flex flex-nowrap overflow-x-auto` plus a subtle right-edge gradient mask so users see there's more.
- **Loss of "expand to see all categories at once" affordance.** Today users can scroll the aside and see Weapons + Armor + Items + Housing + Jobs simultaneously. After the change, only one group's chips are visible at a time. This is the trade-off for reclaiming sidebar space; mitigated partly by the always-visible group pills in Row 1 acting as a permanent map of what's available.
- **External bookmarks to `?menu-open=true`.** Stripped; harmless.

## Testing

- The existing `test_job_filtering` unit test in `item_explorer.rs` stays as-is — the filtering logic moves but its inputs and outputs don't.
- Manual smoke (matches the project's `scripts/run_e2e.sh` harness conventions):
  - Visit `/items` cold — Weapons pills visible, chip row shows weapon subcategories.
  - Click a subcategory chip — URL updates to `/items/category/...`, chip becomes `aria-current`, item grid renders.
  - Click Job Sets pill, then a job chip — URL updates to `/items/jobset/...`.
  - Resize to 375px width — toolbar stays usable, no horizontal page scroll, only chip row scrolls.
  - Confirm only one hamburger button visible at all viewport sizes (the `TopBar` one).
- No Puppeteer assertions added in this spec; the test plan above is what a reviewer should run before merge.

## Out of scope (possible follow-ups)

- Search within the item explorer (today's only search lives in the `TopBar`).
- Sticky toolbar on scroll. Initial implementation is non-sticky; if users complain, a `position: sticky; top: var(--top-bar-height)` is a one-line addition.
- Keyboard arrow-key navigation between chips.
- Animation for the chip row swap when the selected group changes.
