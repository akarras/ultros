# VirtualGrid and spreadsheet-style column controls

Status: implemented. Native/WASM builds and `check_ci.sh` pass; browser validation is pending recovery of the local PostgreSQL/Docker test environment.

## Goal and agreed direction

Give Flip Finder a spreadsheet-style viewport with native horizontal and vertical scrolling, virtualization on both axes, frozen headings, and customizable columns. Introduce `VirtualGrid`, migrate Flip Finder, then remove window-scroll mode from `VirtualScroller`. Keep `VirtualScroller` as a container-scrolling list for its remaining callers.

This supersedes the earlier proposal for a page-scrolling sticky virtual surface. The grid owns vertical scrolling; the page owns the surrounding navigation and controls.

## Starting implementation

- `routes/analyzer.rs` is the only production caller of `ScrollSource::Window`. It synchronizes two horizontal scroll containers for headings and rows.
- `components/virtual_scroller.rs` supports both container and window scrolling. Its window-mode row container spans the entire virtual list height, putting its scrollbar at the bottom of that height.
- `style/tailwind.css` defines `.tool-hscroll` and shared fixed column widths for Flip Finder.
- `analyzer_kit/columns.rs` already describes column labels, value extraction, sorting, and data dependencies. Widths currently live in CSS classes, and required columns can have empty URL IDs. The new grid needs explicit geometry and a unique ID for every column.
- Flip Finder's `cols` query parameter describes a set, serialized in fixed definition order. It cannot represent user ordering or widths.
- `components/saved_views.rs` saves the URL query string. Layout can participate in saved views without replacing their storage schema.

## User experience

| Interaction | Proposed behavior |
| --- | --- |
| Scroll | One native container scrolls both axes. One horizontal scrollbar; column headings remain frozen vertically. |
| Resize | Drag a column's trailing border. Both header and cells resize live, constrained by per-column minimum and maximum widths. |
| Auto-fit | Double-click the trailing border to fit the heading and currently available formatted values. Include padding, icons, sort indicators, and menu space. |
| Header menu | Right-click a heading or use its visible menu button. Keyboard users can open it with the context-menu key or Shift+F10. |
| Insert column | Choose “Insert column before” or “Insert column after”, then select a hidden metric from a searchable picker. Already-visible metrics cannot be inserted twice. |
| Reorder | Drag the heading's grip with an insertion marker and edge scrolling. Menu actions “Move left” and “Move right” provide keyboard and touch alternatives. |
| Hide | Optional columns can be hidden from their menu. Required columns stay visible but can still be resized and reordered. |
| Explicit sizing | Menu actions provide “Auto-fit column”, “Set width…”, and “Reset column width”. Touch users need no double-click gesture. |
| Reset | “Reset column layout” restores default visibility, order, and widths. Clearing filters does not reset column layout. |
| Save | Save view captures filters, sorting, visibility, column order, and widths. Existing saved views retain their existing behavior. |

“Insert” means adding one of the tool's available metrics at that position. User-authored formulas, blank editable columns, and spreadsheet data editing are outside this first release.

Separate header interactions: clicking the sort label sorts; dragging the heading past a movement threshold reorders; dragging or double-clicking the border resizes. Resizing, reordering, and opening the menu must never accidentally sort. Escape cancels an active drag and restores the prior layout; pointer capture handles movement outside the header.

Keep the filter bar outside the grid's clipping area. Fit the grid to available space below the controls, with a usable minimum height on short screens. Measure the viewport with `ResizeObserver`; account for browser chrome and toolbar wrapping. There is no table-header dependency on the filter bar's hardcoded 76px sticky offset. Small result sets and empty results should not leave a large blank panel.

## Component design

### One geometry model

Create `components/virtual_grid/` with geometry and layout state separated from component rendering and interactions. The public component accepts rows with stable keys, column definitions, controlled column layout, row height, header/cell rendering callbacks, measurement support, and visible-range reporting. Persistence, sorting, and market-data fetching remain the caller's responsibility.

Each column has a unique stable ID (including required columns), label, default/min/max width, cell renderer, auto-fit measurement support, and capabilities such as sortable, hideable, resizable, and reorderable. Adapt existing analyzer metadata rather than making a second registry of labels or metric semantics. Fixed width and shrink rules must move out of header/cell CSS into the grid's geometry.

Column state is an ordered collection of IDs and widths, separate from the immutable definitions. Prefix sums of resolved visible widths determine total content width, column positions, hit testing, and the horizontal virtual range. Fixed row height determines vertical positions. Fixed-height rows are sufficient for Flip Finder; existing variable-height lists remain in `VirtualScroller`.

### Rendering and scrolling

Use one `overflow: auto` element containing a logical canvas with the full row height and column width. Render only visible rows and columns plus independent overscan on each axis. Keep the header in the same horizontal coordinate space, frozen vertically with native sticky positioning. Do not create an overflow container on the header or row area and do not mirror horizontal scroll positions.

Keep the sticky header's containing block spanning the full logical content width, so it does not stop painting partway across the grid. Header height, body origin, native scrollbar gutters, and row offsets must use the same geometry.

Coalesce range updates to animation frames, measure viewport changes, and avoid updating URL state on every scroll or drag frame. Retain native scroll offsets during layout changes; the browser clamps offsets after hiding columns or shrinking results. Filtering or sorting resets vertical position to the top while preserving the horizontal layout. Live price updates should not reset scrolling.

SSR and the first hydration render use the same deterministic initial ranges and normalized URL layout. Browser measurements only alter those ranges after hydration. Dispose observers, listeners, pointer capture, and queued animation frames on unmount. Preserve existing visible-row enrichment, request caps, caching, and sorting semantics; horizontal virtualization must not change which data is used to sort or filter.

Frozen data columns can be added later. They are not required for this release; if added, they must use the same scroll offsets rather than independently scrolling panes.

### Auto-fit without defeating virtualization

Auto-fit measures the header and the current filtered rows whose values are already available. It does not use only mounted DOM cells, mount the entire dataset, or fetch data to measure hidden metrics.

For text and numeric columns, measure the displayed strings with the actual font and add known cell adornments and padding. Custom cells provide an intrinsic measurement function or a documented preferred width (for example, a sparkline). Lazy values are measured only when already cached. Process large measurements in cancellable chunks and cache repeated strings. Invalidate measurements when font or locale changes.

Clamp the resulting width to the column's bounds. Auto-fit is a one-time sizing action: newly arriving values can truncate until the user fits again, rather than continually moving surrounding columns. Menu help should explain that sizing uses available results. Tests must include a longest value outside the rendered window.

### Focus and accessible controls

Implement the interactive grid keyboard model alongside the grid, not merely a `role="grid"` attribute: one entry point, arrow-key cell navigation, Home/End and Page Up/Down, and an explicit mode for entering a cell's links/buttons and returning with Escape. Keep stable focused row/column identities across virtualization, reveal destinations before moving focus, and choose a nearby valid target when a focused row or column disappears.

Expose total logical row/column counts and correct indices for rendered cells, with `aria-sort` on the active sort heading. All column actions are available through a keyboard/touch-accessible menu. Provide non-drag alternatives for reordering and setting width. Render menus outside the clipping container; dismiss or reposition them when their anchor moves or disappears, and restore focus on dismissal.

References: [WAI-ARIA grid pattern](https://www.w3.org/WAI/ARIA/apg/patterns/grid/), [virtual grid counts and indices](https://www.w3.org/WAI/ARIA/apg/practices/grid-and-table-properties/), and [alternatives to dragging](https://www.w3.org/WAI/WCAG22/Understanding/dragging-movements).

## URL and saved-view compatibility

Retain `cols` as the existing optional-column visibility contract. Add a versioned `layout` parameter containing ordered column IDs and width overrides, including IDs for required columns. `cols` determines visibility; `layout` determines ordering and sizing. Missing layout uses the current default order and widths.

Normalize layout at one boundary shared by SSR and client: discard unknown IDs, remove duplicates, append missing known columns in default order, clamp finite widths, bound payload size, and fall back to defaults for malformed or unsupported versions. Hidden columns retain their order and width for re-enabling; inserting a hidden column explicitly moves it to the requested location. Old `cols` links continue to work unchanged.

During drag, update local layout only. Commit one URL update on release, auto-fit, or a discrete menu action, with page scrolling disabled. Browser back/forward restores both layout and column visibility. Saved views and saved defaults naturally capture layout through the existing query string. Cover reset, malformed input, new/removed metrics, and older saved views in compatibility tests.

## Delivery sequence

1. **Grid foundation:** add normalized column state and tested X/Y geometry. Build a browser fixture with enough rows and columns to exercise both axes, frozen headings, viewport resizing, keyboard focus, and deterministic hydration. Establish that scrolling uses one native container before migrating the page.
2. **Flip Finder migration:** adapt all required and optional cells to column definitions; use `VirtualGrid`; preserve sorting, filters, quality/item actions, live updates, empty states, and enrichment. Replace duplicated hardcoded widths and remove page-owned horizontal synchronization. Validate at desktop and phone widths.
3. **Column UX and persistence:** implement resizing, double-click auto-fit, header menus, insertion, reordering, accessible alternatives, reset, URL layout state, and saved-view compatibility. Validate cancellation and interaction conflicts as well as the happy paths.
4. **Remove the broken mode:** after Flip Finder passes its grid checks, delete `ScrollSource::Window`, window-scroll geometry/listeners, split-scroll `list_ref` plumbing where no longer needed, `.tool-hscroll`, and stale sticky-height coupling. Remove the `ScrollSource` enum if only container behavior remains. Retain variable-height behavior, native container scrolling, scrolling to an index, minimum content width, and visible-range reporting needed by other callers. Update obsolete window-mode tests and docs; run checks for remaining list/analyzer consumers.

## Acceptance and validation

- Header/body alignment stays within one CSS pixel after diagonal scrolling, resize, reorder, insert/hide, viewport changes, and browser zoom. Exactly one horizontal scroll container serves the grid.
- Browser tests prove DOM cell counts remain bounded by the two visible ranges and overscan for a large synthetic dataset, while the final row and column remain reachable. Confirm horizontal virtualization independently of the small production column count.
- Dragging toward the viewport edges advances horizontal scrolling; border double-click never sorts; Escape restores the original size/order; touch/menu alternatives provide all column operations.
- Auto-fit measures an off-screen longest value and formatted adornments, respects bounds, and performs no additional data requests.
- Saved views, defaults, old links, reload, and history restore valid layouts; malformed layout parameters fall back safely.
- Direct SSR loads and client navigation produce the same initial structure; route teardown leaves no listeners or pending callbacks. Focus survives range changes and can enter existing cell actions.
- Use deterministic market fixtures for essential browser regressions instead of allowing empty live data to silently skip the grid checks.
- Run `cargo leptos build`, the existing Flip Finder hydration/mobile probes, new grid browser tests, applicable JavaScript regressions, `./check_ci.sh`, and `./scripts/run_e2e.sh` before merging implementation. Run targeted Rust geometry/state tests as each layer lands.
- Add player-facing changelog entries when the grid behavior ships. This planning document itself needs no changelog entry.
