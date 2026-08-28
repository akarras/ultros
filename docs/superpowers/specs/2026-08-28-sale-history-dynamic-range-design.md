# Sale history chart: dynamic default range

Date: 2026-08-28
Status: approved

## Problem

The item page's price history chart defaults to full history ("All") when the
URL carries no range params. For frequently-traded items this is misleading:
years of coarse buckets dominate the view and the recent market is invisible.
For rarely-traded items the wide view is exactly right.

## Behavior

When `?range`, `?from`, and `?to` are all absent:

- If the item's **newest sale** (from the listings payload the page already
  fetches, `CurrentlyShownItem::sales`, newest-first) is within the last
  **7 days**, the chart defaults to the **Week** window.
- Otherwise the chart defaults to **full history**, exactly as today.
- No sales at all → full history.

Explicit URL params always win; the dynamic default only fills the absence.

## `?range=all` sentinel

Because absence of params no longer means "All", the All button must write an
explicit value: clicking All writes `?range=all` (and clears `from`/`to`).
The three presets keep writing `7d`/`1mo`/`1y`. Parsing accepts `all` and
resolves it to the full-history window (`None`). Old shared links keep their
meaning; only bare item links change behavior — which is the point.

## No double fetch

When the range params are absent, the effective range is *undecided* until
the listings resource resolves. The series fetch waits for that decision
(tri-state: undecided → skip fetch; decided → fetch once with the right
window). The listings fetch is the page's primary data and resolves first
anyway, so hot items never flash the misleading all-time view and no request
is duplicated.

## UI state

While the dynamic Week default is active, the "7d" button renders pressed
(`aria-pressed=true`), so the active window is obvious. The URL stays clean
until the user clicks a control. Clicking "All" from that state writes
`?range=all` and shows full history.

## Edge cases

- Stale-selection snap-back (item/world identity change) clears `from`/`to`
  and lands on the dynamic default — correct for the new item.
- HQ-only filter: the recency check uses the unfiltered sales list; a 7d
  window can be sparse under the HQ filter. Acceptable; the user can widen.
- `preset_has_data` gating of the preset buttons is unchanged.

## Implementation shape

- Decision logic as pure functions in
  `ultros-frontend/ultros-app/src/components/chart_query.rs` (unit-tested —
  `query_signal` writes are inert in local debug builds, so tests are the
  only local verification).
- `RangePreset` gains an `All` variant (wire value `all`), excluded from the
  window-preset button row; `resolve_range` maps it to `None` (full range).
- `item_view.rs` derives the effective range tri-state from the params plus
  the listings resource, feeds it to the series fetch and the chart's
  pressed-state signal.
- Only `item_view.rs` consumes the chart's range props; no other call sites.
