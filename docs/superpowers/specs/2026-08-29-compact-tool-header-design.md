# Compact tool header design

**Date:** 2026-08-29
**Status:** Approved (icon-only About affordance; separate PR from the item-page refactor)

## Problem

Every analyzer-style tool renders `ToolHeader` (`ultros-frontend/ultros-app/src/components/tool_help.rs`)
as a full-width `panel` card containing only the tool's `h1` and a labeled
"About this tool" button, followed by a *separate* row for the world picker /
controls. That's ~130px of chrome before any content on all nine tools:
Flip Finder, Vendor Resale, Market Trends, Currency Exchange, Recipe Analyzer,
Leve Analyzer, Venture Analyzer, FC Crafting Analyzer, Scrip Sources.

## Design

Rework `ToolHeader` into a single compact row (~44px), no panel card:

- `h1` shrinks from `text-xl sm:text-2xl` to `text-lg sm:text-xl`. It stays an
  `h1` — SEO and a11y semantics unchanged.
- The labeled "About this tool" `btn-secondary` becomes an **icon-only ⓘ
  button** immediately beside the title. Existing i18n strings
  `tool_help_about_tool` / `tool_help_hide_info` move to `aria-label` +
  `title` (tooltip). `aria-expanded` behavior is kept.
- New **optional `controls` children slot**, rendered right-aligned
  (`ms-auto`) in the same flex row. Row uses `flex-wrap`, so on mobile the
  title sits on its own line and controls wrap beneath.
- The expandable about-panel (summary, context, help body, "Open full help"
  link) is **unchanged** and still renders in-flow below the row via the
  existing `<Show>`. No popover — nothing to clip, no new stacking issues.

### Per-page migration

- Pages with a simple world-picker row (Flip Finder, Trends, Currency
  Exchange, etc.): move the navigator (and small pills like Trends' window
  selector) into the `controls` slot and delete the now-empty row below.
- Pages with a full controls panel (e.g. Vendor Resale's presets panel):
  keep the panel where it is; the page just gets the slim title row.
  Optionally move only the world navigator up if it reads well.
- The header must stay **outside** any Suspense/Transition boundary, exactly
  as today (see the comment at `analyzer.rs` above the `ToolHeader` call) —
  the world picker has to survive loading states.

### Rejected alternatives

- **Fold the title into `ControlBar`'s summary slot** — rejected: not every
  tool uses `ControlBar`, and where it exists it lives inside the Suspense
  boundary and/or sticks; the title must be always-on-screen-independent.
- **Popover for the about content** — rejected: extra complexity, clipping
  risk, and no user benefit over the existing in-flow expand.

## i18n

No new strings. `tool_help_about_tool` / `tool_help_hide_info` are reused as
aria-label/tooltip text.

## Testing

- `./check_ci.sh` (fmt + clippy).
- Visual pass on all nine tools at 375px, 768px, and desktop widths
  (remember: the lg sidebar makes 1024px no wider than 768px).
- Verify the about-panel still expands/collapses and the full-help links work.
- Verify the world picker remains interactive during a data load (outside
  Suspense).
