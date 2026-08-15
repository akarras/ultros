# Currency Exchange: rebuild on the flip-finder UI kit

**Date:** 2026-08-14
**Issue:** #1128 (phase 5 of umbrella #1133)
**Scope:** `/currency-exchange/:id` (the per-currency results page) only. The
currency-picker landing page at `/currency-exchange` keeps its current grid.

## Goal

The results page reimplements the flip finder's entire control-surface concept
from scratch: a hand-rolled filter disclosure with an active-count badge, its
own `push_chip` chip row, per-column `FilterModal` popups, `QueryButton` sort
headers with no direction arrows, index-based responsive column hiding, and a
`<Loading />` spinner. Replace all of it with the shared kit — `ToolHeader`,
`ControlBar` + `FilterChip`, the `?cols=` column model, `SortHeader`, and
`TableSkeleton` — while keeping the dense spreadsheet density from #1097 and
changing nothing about the data pipeline.

Guiding principle: **the defaults must answer the question with zero
interaction** — best trade on top, profit visible at every viewport width,
filters invisible until used — and every escalation (scroll, sort, filter,
columns) looks exactly like the flip finder, so recognition carries the UX.

## Non-goals

- No changes to `compute_prices`, shop scanning, the 60-day stale cutoff, or
  the no-home-world banner.
- No `VirtualScroller` — this page renders dozens of rows, not 20k.
- No shared data-table extraction (#1080); this page follows the analyzer's
  conventions directly, as #1128 instructs when #1080 hasn't landed.
- Landing page untouched.

## Design

### 1. Page frame

Replace the parent route's bare `<h3>` link and the bespoke header panel with
`ToolHeader`: title "{currency} — Currency Exchange", new locale keys for the
summary/help body, help link into the existing help page.

The **currency-quantity input lives in the `ToolHeader` row** (right side,
next to the About button), not in the control bar. It is the page's primary
input — "how much of this currency do you have?" — and needs its label visible
at every width; the control bar's height lock hides button labels below `md`,
which would leave an unexplained bare number box on phones. Header = what
you're asking; bar = how you're viewing the answer.

### 2. Control bar

One `ControlBar` replaces the filter disclosure, count badge, chip row, and
`FilterModal`s:

- **Summary:** "N trades" result count.
- **Filters:** eight `FilterOption`s — min/max for price-per-item,
  qty-received, profit, hours-between-sales — each rendering as a numeric
  `FilterChip`. The `+ Filter` menu lists them grouped in column order with
  long explanatory labels; the chips themselves use comparison-shaped labels
  ("Profit ≥ 5000", "Hours/sale ≤ 12") — terser and self-explanatory. That
  long-menu/terse-chip split is what `FilterOption` vs the chip label is for.
- **Query keys are kept verbatim** (`price_per_item_min`, `total_profit_max`,
  …) so existing deep links and bookmarks keep working. Bindings move from
  plain `query_signal` to `filter_query_signal` (`replace: true`), fixing the
  scroll-to-top + history-spam bug on every filter edit.
- **Clear all** clears the eight keys.

### 3. Column system

Analyzer model, verbatim:

- **Required columns**, always rendered, leading the table in this order:
  **item, qty received, profit**. This inverts today's order (shops and cost
  currently lead) — on a 375px phone the visible slice must be the answer,
  not the trivia. This is what lets us delete the breakpoint arithmetic.
- **Optional columns**, trailing, all default-on, stable IDs serialized to
  `?cols=`: `price_per_item`, `shops`, `cost`, `hours_between_sales`.
  Toggleable in the Columns picker, with Reset. Absent `?cols=` = defaults;
  explicitly set (even to "") = respected exactly, same as the analyzer.
- `column_visibility()` and its `hidden lg:table-cell` tiers are deleted.
  Every switched-on column renders at every width; the table is a horizontal
  scrollport (`overflow-x-auto` on the table wrapper only — nothing with
  popovers inside it, per the overflow-clipping rule).
- **Edge fades** on the scrollport (the flip finder's #1057 affordance) so a
  clipped right edge visibly reads as "more table this way".

### 4. Sorting

A `SortMode` enum — `Profit` (fallback), `PricePerItem`, `QtyReceived`,
`HoursBetweenSales` — implementing `SortColumn`, rendered with `SortHeader`,
giving the asc/desc arrows today's `QueryButton` headers lack.

**`HoursBetweenSales` gets an ascending initial direction.** The kit's
best-first-descending convention is wrong for this one column — descending
hours puts the slowest sellers on top. If `SortHeader` can't express a
per-column initial direction yet, extend it minimally rather than inverting
the metric (the filter keys stay `hours_between_sales_*` either way).

The `SortableVec`/`FieldLabels` derives and the `?sorted-by` string param are
removed. Old `?sorted-by` links fall back to the default sort (profit desc),
which is what most of them encoded anyway.

### 5. Loading and empty states

- `<Loading />` → `TableSkeleton`, columns derived from the visible set so
  the placeholder matches the table that loads in (SSR-deterministic widths,
  per the skeleton conventions).
- **Empty state:** when active filters match zero rows, render a short
  "no trades match your filters" line with a clear-filters action instead of
  a silently empty `<tbody>`.

## Error handling

Unchanged: the no-home-world banner and the sales-fetch error panel keep
their current behavior. Filter parsing stays lenient (unparseable query
values = filter inactive).

## Testing

- Rewrite the two layout tests: the "phone keeps the answer columns" test
  becomes "required columns are not in `ALL_OPTIONAL_COLS` and lead the DOM
  order"; the `FILTER_QUERY_KEYS` sync test pins the keys against the
  `FilterOption` list.
- `?cols=` parse/serialize round-trip tests mirroring the analyzer's.
- Keep the two locale-regression tests (category IDs) untouched.
- Note: CI never runs `cargo test` — run locally; `ultros-app` tests needing
  signals wrap in `Owner::new()`.

## i18n

New keys (picker labels, chip labels, filter-menu labels, help blurb, empty
state) added to **all seven** locale files with real translations, snake_case,
`currency_exchange_` prefix. Obsolete keys from the removed modal/chip UI are
dropped from all locales.

## Verification

`./check_ci.sh` (exit code checked directly, not through a pipe) before
commit; visual check of phone-width layout against the reordered columns.
