# Flip Finder spreadsheet redesign — design

Date: 2026-07-30
Status: approved by user (chat), pending spec review

## Problem

The flip finder (`/flip-finder/:world`, `ultros-frontend/ultros-app/src/routes/analyzer.rs`)
renders as a card ("well") containing a window-scrolled infinite list. Four
pain points, in the user's words:

1. Columns aren't adjustable — no way to squeeze the name column or remove a
   column from the header itself.
2. Horizontal scrolling is broken-feeling: on mobile the header and body
   slide at different speeds. (Root cause: header and body are two sibling
   scrollports whose `scrollLeft` is mirrored by JS listeners at
   `analyzer.rs:875-922`.)
3. "Select World:" doesn't say it's the world you *sell* on.
4. The default view has no filters, which overwhelms new users with junk rows.

Goal: make the page look and behave like a spreadsheet — one contained,
grid-lined table pane — with resizable/removable columns and a sensible
default filter set.

## Decisions (user-confirmed)

- **Scroll model**: contained spreadsheet pane (table owns its scrolling; the
  page itself no longer scrolls).
- **Column widths persist** in localStorage, not the URL.
- **Column menu**: header context menu (right-click / long-press) *plus* the
  existing columns picker popover.
- **Default filters**: seed the existing "Realistic flips" preset whenever the
  URL arrives with no filter/sort params (same mechanism as today's
  `next-sale=1d` seeding).

## 1. Contained spreadsheet pane

The world view becomes a non-scrolling flex column:

```
┌ page (h-viewport flex flex-col, no scroll) ┐
│ title + world navigator                    │
│ filter bar (static, no longer sticky)      │
│ ┌ table pane (flex-1 min-h-0) ───────────┐ │
│ │ single scrollport, overflow: auto      │ │
│ │ ┌ header row (sticky top-0) ─────────┐ │ │
│ │ ├ virtualized rows ──────────────────┤ │ │
│ └────────────────────────────────────────┘ │
└────────────────────────────────────────────┘
```

- `VirtualScroller` switches from `ScrollSource::Window { sticky_offset }` to
  `ScrollSource::Container`, and the column header moves into the scroller's
  existing `header` prop, which already renders sticky inside the scroll
  container (`virtual_scroller.rs:123-125`). One scrollport for both axes
  ⇒ header/body horizontal desync is impossible by construction.
- Delete: the `scrollLeft` mirroring effect (`analyzer.rs:875-922`), the
  separate `.analyzer-hscroll` header scrollport, and the
  `ScrollSource::Window`/`STICKY_BAR_HEIGHT` scroll-offset coupling on this
  page. (`ScrollSource::Window` itself stays — other routes use it.)
- **VirtualScroller change**: `viewport_height: f64` is fixed at mount. Add a
  reactive path (accept `Signal<f64>`, e.g. via `#[prop(into)]` or a new
  optional prop) so the pane can fill the remaining viewport and respond to
  resizes. Height measured with `use_element_size` on the pane wrapper.
  Existing call sites keep working (plain `f64` converts `.into()`).
- The filter/sticky bar stays at the top of the page but no longer needs
  `position: sticky` (the page doesn't scroll). Popovers/menus anchored to it
  keep working.
- Mobile: the pane fills the viewport below the bar; native two-axis panning
  in one container.

### Spreadsheet styling

- Hairline column separators and row borders (theme-consistent, subtle).
- Denser rows (keep `row_height` 40px or tighten slightly — pick what reads
  well with gridlines; whatever the choice, header/body use the same border
  math so virtualization stays exact).
- Tabular numerals for numeric cells (already partially applied).
- The outer "well" card chrome around the table is dropped in favor of the
  pane filling the available width.

## 2. Data-driven column registry + resize

Today each column's width lives in three places: a Tailwind class on the
header cell, the same class repeated on the row cell, and
`extra_column_width_px` (`analyzer.rs:602`). Replace with one registry:

```rust
struct ColumnSpec {
    id: &'static str,        // existing COL_* ids; required cols get ids too
    default_width: f64,      // px, taken from current effective widths
    min_width: f64,          // px; name column gets a small min + ellipsis
    resizable: bool,
    optional: bool,          // participates in ?cols= visibility
}
```

- Widths render as CSS custom properties on the pane element
  (`--colw-<id>: 224px`); header and body cells both use
  `width: var(--colw-<id>)`. Row min-width becomes the sum of visible column
  widths (one memo), replacing both `extra_column_width_px` and the
  breakpoint-tuned `--analyzer-row-min-width` calc in `tailwind.css`.
- Breakpoint-based column hiding (`hidden md:flex` etc.) is replaced by the
  visibility system: `?cols=` remains the source of truth for which optional
  columns show. (Default visible set stays as today.)
- **Resize interaction**: each resizable header cell gets a drag handle on its
  right edge — thin visual affordance, wider hit area (~12px, larger on
  touch), `cursor: col-resize`. Pointer events (`pointerdown` +
  `setPointerCapture`) so mouse and touch share one code path. During drag,
  write the CSS var directly on the pane element (no reactive churn); on
  `pointerup`, commit to the widths signal. Clamp to `min_width`.
- **Persistence**: `ultros.flipfinder.colwidths` in localStorage,
  `HashMap<String, f64>` of only the columns the user has touched, via
  `use_local_storage_with_options` with `delay_during_hydration(true)`
  (same pattern as `saved_views.rs:104-109`). Effective width =
  stored override or `default_width`.
- Unknown/stale ids in storage are ignored; "Reset column width" removes the
  entry; "Reset all widths" clears the map.

## 3. Header context menu

- `on:contextmenu` (prevent default) on header cells opens a small anchored
  popover; long-press (~500ms pointer hold without move) triggers the same on
  touch. Reuse the app's existing popover styling (`.sticky-bar-popover`).
- Items, contextual per column:
  - Sort ascending / Sort descending — only for the columns that map to a
    `SortMode` variant (Profit, ROI, Profit/day today; no new sort modes in
    this project).
  - Hide column — optional columns only; writes `?cols=`.
  - Reset column width — resizable columns with a stored override.
  - Reset all column widths.
  - Manage columns… — opens the existing columns picker.
- One menu open at a time; closes on outside click, Escape, or scroll.
- All labels via `leptos-i18n`, added to **all** locale files
  (`en, fr, de, ja, cn, ko, tc`) with real translations.

## 4. World picker copy

- `analyzer_select_world`: "Select World:" → **"Sell on world:"**
- `analyzer_index_choose_world`: "Choose a world to get started:" →
  **"Choose the world you'll sell your flips on:"**
- Both re-translated in every locale. No behavior change to the picker or
  navigation.

## 5. Default filters = Realistic flips

- In `AnalyzerWorldView` (next to the current `seed_query_default` call,
  `analyzer.rs:2242`): if the incoming URL contains **none** of the filter,
  sort, or chip params (the full registry at `analyzer.rs:505-535` plus
  `sort`/`dir`), seed the Realistic preset into the URL:
  `min-buy=5000`, `last-sold=1d`, `roi=30`, `sort=profit-per-day`, plus the
  existing `next-sale=1d`.
- Seeded values appear as removable chips exactly like `next-sale` today.
- URLs with any explicit filter/sort param are untouched (bookmarks, shared
  links, saved views).
- "Clear all filters" behaves as today: chips clear for the session; a fresh
  parameterless page load re-seeds. (Accepted tradeoff, consistent with the
  existing `next-sale` behavior.)
- The `seed_query_default` caveat holds: seeding stays in the route component,
  outside `Suspense`.

## Error handling / edge cases

- SSR/hydration: localStorage reads are hydration-delayed; server render uses
  default widths, so no hydration mismatch.
- Resize during active drag of the pane (window resize): viewport-height
  signal updates; virtualization recomputes; in-progress column drag keeps
  its pointer capture.
- Very narrow viewports: min widths keep every visible column tappable; the
  pane scrolls horizontally as one unit.
- Empty result set: pane shows the existing empty state inside the scrollport
  (header remains visible).

## Testing

- Unit: column registry width-sum memo (visible set × overrides), width
  clamping, `?cols=` round-trip (existing tests keep passing), and the
  "URL has no filter params" seeding predicate (empty, one filter, chip-only
  params, `cols`-only — `cols` alone must NOT suppress seeding since it's not
  a filter… decision: `cols` and region toggles do not count as filters; only
  the filter registry + `sort`/`dir` suppress seeding).
- E2E: `./scripts/run_e2e.sh` screenshot pass over the flip finder page.
- Manual: mobile Safari/Chrome — two-axis pan in the pane, no header desync;
  touch drag-resize; long-press menu.
- `./check_ci.sh` before every commit.

## Out of scope

- New sort modes for currently unsortable columns.
- Filter semantics, enrichment fetching, saved-views behavior.
- Other analyzer-style routes (`vendor_resale`, `venture_analyzer`, …) — they
  keep `ScrollSource::Window`; a follow-up could adopt the pane if this lands
  well.
