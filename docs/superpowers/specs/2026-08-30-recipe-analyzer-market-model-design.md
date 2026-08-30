# Recipe Analyzer market model + richer columns — design

Date: 2026-08-30
Status: approved by Aaron (chat), pending spec review
Issue: #1233 (regression report on #1202/#1206)

## Problem

1. **The #1202 feature is invisible.** Cost basis, revenue metric, and
   region/DC scope shipped in #1206 as always-visible toolbar selects, then
   the Toolbar→ControlBar migration (#1214) filed them under the `+ Filter`
   menu. #1233 reports the whole feature as gone. The backend (ClickHouse
   `bulk_sale_stats`, `/api/v1/sale_stats/{worldDcOrRegion}`) fully supports
   region/DC median/min/avg today — this half is purely a visibility defect.
2. **No travel model.** Ingredient cost always searches the full scope
   (region by default). A crafter who won't travel across NA has no way to
   say "price my ingredients from my world / my DC only".
3. **Thin table.** The flip finder and trends v2 have rich ClickHouse-backed
   signals (sales/day, last sold, volume, VWAP, % vs VWAP, confidence band)
   that the recipe analyzer lacks, and no world/DC columns or filters.

## Decisions (from chat)

- Buy side: a **World / Datacenter / Region scope**, default **Datacenter**.
- Sell side: **one specific world**, default home world. Refinement: the
  page's existing world selector already *is* the sell world (#1228 made
  revenue default to the selected world's price, net of tax) — so no second
  world picker; the existing selector is the sell world, relabeled to say so.
- World/DC columns show the **cheapest crafted-item listing's** world
  (`cheapest_world_id`, already computed), with flip-finder-style filters.
- Default-on new columns: **Sales/day (sell world)** and **Confidence**.
  Everything else starts hidden in the Columns picker.

## Phase 1 — Market menu (frontend only; the #1233 remediation)

A permanent **Market** button in ControlBar row 1 (same button+popover shape
as the flip finder's `SavedViewsMenu`; icon-only below `md`), opening:

- **Buy from** — `World | Datacenter | Region`, default `Datacenter`.
  Drives the `get_cheapest_listings` scope used to price ingredients, and
  the scope for cost-basis sale stats. `World` = the sell world only.
- **Cost basis** — Cheapest listing / Sale median / min / avg (7d).
  Unchanged semantics, evaluated over the buy scope.
- **Revenue metric** — Cheapest listing / Sale median / min / avg (7d),
  evaluated on the **sell world**. The old "Selected world listing"
  (`world-min`) option is removed: revenue is always per-world now, so
  `world-min` ≡ the new listing-min default.

The three pricing entries leave `ADDABLE_FILTERS` (they change how rows are
priced, not which rows show). Non-default values still echo as removable
chips; `clear all` resets them. The `+ Filter` menu keeps the true filters.

URL contract: new key `buy-scope` (`world|datacenter|region`, absent =
datacenter); `cost-basis` and `revenue` keep their tokens. Compat mapping at
load: `scope=datacenter` → `buy-scope=datacenter`, `scope=region` →
`buy-scope=region`, `revenue=world-min` → unset (new default). Extend the
stable-URL-contract test.

Data flow changes: cheapest-listings fetch keys off the resolved buy-scope
name (sell world name / DC name / region name); cost-basis sale stats fetch
the buy scope; revenue sale stats fetch the sell world. The existing
"world listings" secondary fetch collapses into this (buy-scope=World and
revenue-on-sell-world share the sell-world fetch).

Behavior change: the default view moves from region-wide pricing to
buy-DC / sell-home. Numbers get less optimistic by default — intended, and
gets a changelog entry.

i18n: new keys (`recipe_analyzer_market_button`, `recipe_analyzer_buy_from_label`,
buy-scope option labels, relabeled world-picker string) in all 7 locales
with real translations.

## Phase 2 — Widen sale stats + new columns

### Backend

Extend ClickHouse `bulk_sale_stats` (additive columns over the same
raw-sales scan): `last_sold_at` (max timestamp), `units_sold`
(sum quantity), `vwap` (sum(price×qty)/sum(qty)). `sales_per_day` is
`num_sold / window_days`, computed server-side. Confidence band comes from
the `item_quality_score` table; whether it joins in the same query or is a
second bulk query merged server-side is decided at plan time — constraint:
no unfiltered LEFT JOIN against large tables (per repo CH rules), and the
result must stay one response row per `(item_id, hq)`.

`/api/v1/sale_stats` response fields are serde-defaulted additions to
`ItemSaleStats` — old clients keep deserializing. Fixture-backed
`ULTROS_CH_INTEGRATION` smoke test for the widened query.

### Frontend

Recipe analyzer adopts the flip finder's Columns picker (`?cols=`), adding:

| Column | Source | Default |
|---|---|---|
| Sales/day (sell world) | widened stats, sell-world fetch | on (replaces region-wide daily sales) |
| Confidence chip | widened stats | on |
| Last sold | widened stats | off |
| Volume (units / window) | widened stats | off |
| VWAP + % vs VWAP | widened stats + current listing | off |
| Tax (5% of revenue) | computed (profit already nets it) | off |

% vs VWAP is computed client-side from the current sell-world cheapest
listing against the returned VWAP. All headers sortable via the existing
`SortableHeaderCell`. New column labels in all 7 locales.

## Phase 3 — World/DC columns + filters

- `World` and `Datacenter` columns from `cheapest_world_id` (present in
  `RecipeProfitData` today), rendered like the flip finder's.
- `world` / `datacenter` filter chips using `filter_query_signal`,
  registered in `ADDABLE_FILTERS`, hiding rows whose cheapest crafted-item
  listing is outside the chosen world/DC. Reuse flip-finder i18n keys where
  they fit.

## Testing

- Phase 1: unit tests for compat param mapping, buy-scope name resolution,
  revenue-on-sell-world selection; URL-contract test extended.
- Phase 2: CH fixture test (docker throwaway CH) covering the new columns,
  incl. VWAP weighting and last-sold; serde round-trip of widened
  `ItemSaleStats` against an old-shape payload.
- Phase 3: filter predicate tests alongside the existing filter tests.
- `./check_ci.sh` before every commit; e2e smoke via `./scripts/run_e2e.sh`
  after Phase 1 (SSR-sensitive UI change).

## Non-goals

- No changes to the flip finder, trends page, or other analyzers.
- No multi-world "shopping trip" planner (per-ingredient world sets) — the
  buy scope is the travel model.
- No changes to Phases 2–3 of the 2026-08-29 recipe-analyzer-improvements
  spec (gil/day sort, search chip, HQ toggle, expandable rows); they layer
  on top of this independently.

## Relationship to prior work

- Supersedes the in-flight "Pricing menu" draft on this branch (subsumed by
  the Market menu).
- Builds on #1228 (Phase 1 of the 2026-08-29 spec): tax netting and
  sell-on-selected-world revenue are assumed present.
