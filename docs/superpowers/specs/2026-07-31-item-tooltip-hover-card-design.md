# Item tooltip hover card + HoverCard primitive rebuild

**Date:** 2026-07-31
**Status:** Approved

## Goal

Build a reusable `ItemTooltip` hover card (icon, name, category, ilvl, stats,
description) usable on every item surface in the app except the search box.
While doing so, rebuild the shared tooltip overlay primitive: the existing
`<Tooltip/>` positioning/hover logic is buggy, and every text tooltip should
inherit the fixes and the refreshed theming. Finally, slim the item page
header, which currently duplicates what the card shows.

## Confirmed bugs in the current `Tooltip` (`ultros-frontend/ultros-app/src/components/tooltip.rs`)

1. **Mixed coordinate spaces.** `use_element_bounding` returns
   viewport-relative rects and the tooltip div is `position: fixed`
   (viewport-relative), but the flip/clamp logic compares against
   `scroll_y()` / `scroll_x()` (document coordinates). `if pos_y < scroll_y()`
   should be `if pos_y < 0.0`. On any scrolled page the "no room above" check
   is almost always true, so tooltips flip below their anchor even with room
   above.
2. **First-frame jump.** The overlay renders before `use_element_size` has
   measured it (`tooltip_width/height == 0.0` for a frame), paints in the
   wrong spot, then snaps once measured.
3. **Eager observers.** Every `Tooltip` instance starts `use_element_bounding`
   (ResizeObserver + scroll listeners) on mount whether or not it ever opens.
   Unacceptable once hover cards sit on hundreds of analyzer rows.
4. **Minor:** each open registers a fresh window-resize listener via
   `use_window_size()`; there is no vertical clamp (only horizontal), so a
   flipped tooltip near the bottom edge can overflow the viewport.

## Architecture

Three layers, all in `ultros-frontend/ultros-app/src/components/`:

```
HoverCard (primitive: portal, hover/focus state, delay, positioning)
├── Tooltip     (existing public API, thin text wrapper — restyled)
└── ItemTooltip (new: item card content over the same primitive)
```

### 1. `HoverCard` primitive

Reworked internals of `tooltip.rs` (new component, may live in the same file
or `hover_card.rs`).

**Props**

- `content: ViewFn` — arbitrary overlay content.
- `open_delay_ms: u32` (default `0`) — delay before opening; a
  leave/blur during the delay cancels the pending open.
- `class: Option<String>` — class for the anchor wrapper (parity with
  today's `Tooltip`).
- `children` — the anchor content.

**Behavior**

- Portal to `document.body`, hydrate-gated exactly like today (SSR renders
  the anchor only).
- Opens on `mouseenter`/`focusin`, closes on `mouseleave`/`focusout`/Escape
  (same events as today).
- **Lazy measurement:** no observers or listeners until the overlay actually
  opens. On open, measure the anchor via `getBoundingClientRect`; while
  open, re-position on window scroll and resize. Everything is torn down on
  close (dropped with the overlay's reactive scope).
- **No first-frame jump:** overlay is rendered `invisible` until its size has
  been measured, then positioned and revealed.
- **Correct positioning math**, extracted as a pure function so it can be
  unit-tested:

  ```rust
  /// All inputs/outputs in viewport coordinates (position: fixed).
  fn overlay_position(
      anchor: Rect,          // anchor bounding rect
      overlay: Size,         // measured overlay size
      viewport: Size,        // window inner size
      gap: f64,              // 8.0
  ) -> (f64, f64)            // (top, left)
  ```

  Rules: prefer above the anchor, centered horizontally; flip below when
  `top < 0`; clamp horizontally to `[8, viewport.width - overlay.width - 8]`;
  clamp vertically to `[8, viewport.height - overlay.height - 8]`. No scroll
  offsets anywhere — fixed positioning is pure viewport space.

### 2. `Tooltip` — API-compatible wrapper

Keeps the exact public signature (`tooltip_text: Signal<String>`,
`class: Option<String>`, `children`) so all ~20 existing call sites compile
untouched. Internally delegates to `HoverCard` with `open_delay_ms = 0` and a
text bubble as content.

**Restyled bubble** (shared card chrome, CSS-variable driven so every palette
and light mode re-tint automatically):

- Body: `bg-gradient-to-br from-brand-950/95 via-brand-900/90 to-brand-950/95`
  with `backdrop-blur-md`, `rounded-lg`.
- 1px accent hairline across the top edge:
  `bg-gradient-to-r from-transparent via-[color:var(--accent)] to-transparent`.
- Border `border-brand-400/30`; shadow `shadow-lg` +
  `shadow-[color:var(--accent-glow)]`.

### 3. `ItemTooltip` — new component

New file `item_tooltip.rs`.

**Props:** `#[prop(into)] item_id: Signal<i32>` (accepts both static ids and
signals at call sites), `children` (the anchor — row content, icon, link,
etc.).

**Content** (all read synchronously from `tracked_data()`, no fetches):

- Header: `ItemIcon` (Medium) with a soft radial `--accent-glow` bloom behind
  it; item name; category name.
- ilvl chip (same style as the item page header chip).
- `ItemStats` reused as-is (`stats_display.rs`).
- Description, `line-clamp-3`, hidden when empty.
- Card chrome identical to the restyled `Tooltip` (gradient body, accent
  hairline, glow shadow), width capped (~`max-w-sm`).

**Behavior:** `open_delay_ms = 300` so sweeping the mouse across tables does
not strobe cards. Items with no stats and no description still show the
header + ilvl (cheap and consistent). Non-hover devices: the focus fallback
applies; tap simply follows the anchor's normal action (navigation).

### 4. Rollout

- `SmallItemDisplay` (`small_item_display.rs`): wrap the row content in
  `ItemTooltip`. This covers the analyzers, lists, related items, live sale
  ticker, and other consumers in one change.
- Item explorer rows (`item_explorer.rs`): wire individually.
- Item page: the item's own header icon gets the card too (consistency).
- **Excluded:** search box results.

### 5. Item page header slimming (`routes/item_view.rs`)

- Keep: icon, name, category link, ilvl chip, `ItemStats` grid.
- Remove: the description row (`line-clamp-3` block at the bottom of the
  header grid). The description remains reachable by hovering the header
  icon (full card), and stays in the meta description for SEO.

## i18n

No new user-facing strings expected: item names, category names, stat names,
and descriptions come from game data; the `item_level` key already exists. If
any new label sneaks in during implementation, it goes through `leptos-i18n`
with real translations in all 7 locale files per CLAUDE.md.

## Error handling

- Unknown `item_id` (not in `tracked_data()`): render children with no
  hover behavior — never a broken empty card.
- SSR/hydrate: overlay code stays behind the `hydrate` feature gate as today;
  server renders anchors only.

## Testing

- Unit tests for `overlay_position`: room above, flip below, horizontal clamp
  at both edges, vertical clamp when flipped near the bottom, small-viewport
  degenerate case.
- `./check_ci.sh` (fmt + clippy) before every commit.
- E2E screenshot pass via `./scripts/run_e2e.sh`: item page header (slimmed),
  hover card open on a related-items row, at least two palettes (violet +
  one FFXIV palette) to confirm CSS-variable tinting.

## Out of scope

- Search box hover cards.
- Touch-specific long-press card gesture.
- Any change to `Tooltip`'s public API or its call sites.
- Market/price data in the card (stats only; can be layered on later).
