# Flip Finder: full column filter coverage

**Date:** 2026-07-30
**Status:** Approved

## Problem

The Flip Finder (`ultros-frontend/ultros-app/src/routes/analyzer.rs`) renders
15 columns but its chip-based filter registry only covers some of them.
Auditing every column against the registry:

| Column | Filterable today | How |
|---|---|---|
| Profit | yes | `profit` (min) |
| Profit/day | yes | `ppd` (min) |
| Velocity / Sales-per-day | yes | `vel` (min floor, CH rate first) |
| Buy Price | yes | `min-buy` + `max-price` |
| Last Sold | yes | `last-sold` |
| World / Datacenter | yes | chip via row click |
| Item (name) | partial | category chip only — no name search |
| HQ | **no** | — |
| Drift | **no** | — |
| Confidence | partial | only the coarse `show-suspicious` toggle |
| Volume 30d | **no** | — |
| Trend (sparkline) | no (by design) | visual-only; drift is its numeric proxy |

## Goal

Close the five gaps — HQ, item name, drift, confidence band, 30d volume —
inside the existing chip system. No new UI paradigm: each filter is a
registry entry, a URL param, a chip, and a `+ Filter` menu row, so it is
URL-persisted and works with saved views for free.

Trend stays unfiltered deliberately: a sparkline has no typeable value and
drift already captures direction.

## New filters

Each id doubles as its `query_signal` URL key, matching the existing
registry convention.

| Filter | URL key | Semantics | Menu seed value |
|---|---|---|---|
| Quality | `quality` | `hq` or `nq`; param absent = both | `hq` |
| Item name | `name` | case-insensitive substring match on item name | empty; chip opens in edit mode |
| Min drift | `drift` | keep rows with drift ≥ X% (floor; excludes crashing markets) | `-10` |
| Min confidence | `confidence` | keep rows whose band ≥ X; values `low` / `medium` / `high` | `medium` |
| Min 30d volume | `min-volume` | keep rows with ≥ X units sold in 30 days | `10` |

All five are appended to `ADDABLE_FILTERS`, get a `default_filter_value`
entry, a `filter_label` arm, an `add_filter` arm, an `active_filters` entry,
a chip in the sticky bar, and a `clear_all_filters` reset — the same seven
touch points every existing filter has.

## Unknown-data semantics

Two rules, chosen to match the precedents already in the file:

- **Universal-coverage metrics fail the floor when uncomputable** (the `vel`
  filter's existing rule). Applies to:
  - **Drift** — computed client-side from the row's price buffer
    (`price_drift_pct`); rows with too few sales to compute a drift fail an
    active drift floor.
  - **Confidence** — prefer the ClickHouse `confidence_band`, fall back to
    `derived_confidence` (the same preference the Confidence column and
    badge use), so effectively every row has a band. Band ordering:
    Unusable < Low < Medium < High. The CH `Unknown` variant (pass-1, no
    deep-scan data yet) is not a verdict — it triggers the derived
    fallback rather than failing the floor; `Unusable` fails any active
    floor.
- **ClickHouse-only metrics keep unknown rows** (the suspicious filter's
  existing rule). Applies to:
  - **Volume 30d** — CH rollup covers ~7% of items *and* enrichment loads
    lazily per visible window. If unknown-fails, the un-enriched initial
    table filters to zero rows, the virtual scroller then fetches nothing,
    and the page deadlocks empty. So `min-volume` drops only rows whose
    *known* volume is below the floor; un-settled and uncovered rows pass.

Quality (`hq`/`nq`) and name filters always have data (row key / game data)
so no unknown case exists.

## FilterChip extension

`components/filter_chip.rs` currently renders text, numeric, or readonly
chips. Two additions:

1. **Select variant** — an `options: Vec<(&'static str, String)>`
   (value, localized label) prop that renders an inline `<select>` instead
   of a text input. Used by Quality (`hq`/`nq`) and Confidence
   (`low`/`medium`/`high`). Clearing with `x` still removes the filter.
2. **Start-in-editing flag** — the name filter seeds empty, and an empty
   resting chip would render as a bare label; a `start_editing` prop makes
   the chip mount directly in its editing state so the user can type
   immediately after picking it from the `+ Filter` menu.

Both are additive props; existing call sites are untouched.

## Hydration safety (name filter)

This is the first filter that matches a *localized* string. Per this repo's
established failure class, SSR renders game data in English while the
client hydrates in the visitor's locale, so a `?name=` URL could produce
different row sets server- vs client-side → hydration panic.

Implementation must:

1. Verify whether the analyzer table SSRs any rows at all (the virtual
   scroller's initial `visible_range` of `(0, 0)` may already mean zero
   rows server-side, making this moot).
2. If rows do SSR, gate the name predicate behind the existing
   post-hydration pattern used elsewhere in the app (Effect-driven
   `hydrated` signal), so the SSR row set ignores `?name=` and the filter
   applies after hydration.
3. Either way, add a note at the predicate explaining the constraint.

Matching is against the item name from `tracked_data().items` — the same
source the Item column renders — lowercased on both sides. No diacritic
folding in v1.

## i18n

Every new user-facing string (menu labels, chip labels, select option
labels) is added to **all seven** locale files (`en`, `fr`, `de`, `ja`,
`cn`, `ko`, `tc`) with real translations, following the existing
`analyzer_*` key prefix.

## Testing

- Unit tests alongside the existing pure-function tests in `analyzer.rs`
  for: quality predicate, name matching (case-insensitivity), drift floor
  (including the uncomputable-drift case), confidence band ordering +
  fallback, and min-volume keep-unknown semantics.
- The predicates are extracted as plain functions (like
  `passes_velocity_floor`) so they test without a reactive runtime.
- Manual verification of the SSR/hydration behavior for `?name=` per the
  section above.

## Out of scope

- Sort support for Velocity / Drift / Buy Price / Last Sold (only
  Profit, Profit/day, ROI are sortable today) — separate change.
- Filtering the Trend sparkline.
- Diacritic-insensitive name matching.
