# Recipe Analyzer improvements — design

Date: 2026-08-29
Status: approved by Aaron (chat), pending spec review

## Problem

The recipe analyzer's rankings are quietly wrong and its top results are not
actionable. Four confirmed pain points: numbers feel untrustworthy, top results
aren't what you'd actually craft, too much fiddling before it says anything
useful, and no depth once you pick a recipe.

Root causes found in code:

- `cost_per_unit = craft_cost` in `recipe_analyzer.rs` ignores recipe yield
  (`Recipe::amount_result`). The TODO claiming yield "isn't exposed" is stale —
  the subcraft path in `crafting_cost.rs` already divides by it. Multi-yield
  recipes (much CUL food, etc.) have cost overstated by the yield factor, and
  many are wrongly dropped by the `cost >= market_price` cutoff.
- Ingredient cost is always the cheapest **market** listing; NPC vendor prices
  are ignored, overstating costs and skewing the craft-vs-buy subcraft choice.
- Revenue defaults to the scope-wide (region/DC) cheapest listing; you sell on
  *your* world. World-min exists but is a buried opt-in.
- The 5% market-board tax is not deducted from revenue.
- Sell price uses `lowest_gil()` (usually NQ); crafted output mostly sells HQ.
- Sortable by profit or velocity, but not by their product — the actual
  decision metric (gil/day).
- `compute_cost` already produces a full per-ingredient `CostBreakdown`, but
  the analyzer discards everything except `sub_crafts`; there is no way to see
  *why* a row costs what it costs.

## Approach

Three staged PRs, each shippable and reviewable alone. Correctness first so a
math regression is never tangled with UI churn.

### Phase 1 — Make the numbers true (no new UI surfaces)

1. **Yield division.** `cost_per_unit = craft_cost / recipe.amount_result.max(1)`
   (integer division, same as the subcraft path). Profit and ROI computed from
   the per-unit cost. Rows with `amount_result > 1` show a small "×N per craft"
   note next to the cost.
2. **Vendor price floor.** Build a `HashMap<item_id, vendor_price>` from
   `gil_shop_items` × `Item::price_mid` (same construction as
   `vendor_resale.rs`). `compute_cost` takes a vendor-price lookup (mirroring
   the existing `is_shard` closure parameter) and prices each ingredient at
   `min(market, vendor)`. `IngredientLine` records the chosen source
   (`Market | Vendor | Subcraft | OnHand`) so Phase 3 can display it. The
   subcraft comparison also competes against the vendor price.
3. **Market tax.** Revenue = listing price × 0.95 before profit/ROI. The
   `CalculationSummary` formula/details text says so.
4. **Revenue default = selected world.** `RevenueMetric`'s default becomes the
   selected world's cheapest listing (current opt-in "world-min"), falling back
   to scope-wide when the world has no listings (existing fallback logic).
   Scope-wide min stays available as an explicit option. Behavior change on
   bookmarked URLs without `revenue=` — intended; changelog entry on merge.

Tests: yield division (incl. `amount_result == 0` guard), vendor floor beats
market / market beats vendor, tax applied once, world-default fallback.

### Phase 2 — Make the ranking actionable

5. **Gil/day column + default sort.** New `SortMode::GilPerDay`; value =
   `profit × daily_sales` with the same semantics as `analysis::profit_per_day`
   (0 when no sale history). Becomes the fallback/default sort. Column shown on
   md+ like the other stat columns.
6. **Item-name search chip.** New filter (`FILTER_SEARCH`, URL key `search`,
   registered in `ADDABLE_FILTERS`) doing case-insensitive substring match on
   the localized item name; uses `filter_query_signal` like every other filter.
7. **"Assume HQ sale" toggle.** New boolean filter chip. When on, revenue uses
   `price_preferring_hq()` for items with `can_be_hq`, falling back to the LQ
   listing when no HQ listing exists.

### Phase 3 — Depth on a pick

8. **Expandable row.** Clicking a row expands an inline breakdown panel:
   - ingredient table: icon/name, qty, unit price, source badge
     (market/vendor/subcraft/on-hand), line total;
   - shard line when excluded ("shards excluded: Xg");
   - revenue assumption line (which price, tax, HQ assumption).
   `RecipeProfitData` carries the full `CostBreakdown` (Arc'd) instead of just
   `sub_crafts`. VirtualScroller: expansion uses `variable_height` support or
   an overlay/detail panel — decided at plan time by what the scroller
   supports; the breakdown content is the requirement, the container is not.
9. **Craft-list handoff.** The expanded panel gets a prominent "Add to craft
   list" button opening the existing `AddRecipeToListModal`, pre-seeding its
   HQ toggle from the analyzer's require-HQ / assume-HQ-sale state.

## Non-goals

- No changes to the sale-stats endpoints or ClickHouse queries.
- No retainer/inventory integration beyond the existing on-hand map.
- No changes to the other analyzers (leve, venture, FC) in these PRs, even
  where they share the same yield/tax gaps — follow-ups if wanted.

## Conventions that apply

- Every new user-facing string in all 7 locale files, real translations.
- Filter URL keys are a stable contract — extend the
  `filter_registry_keys_are_a_stable_url_contract` test.
- `filter_query_signal` for all new filters (no history spam / scroll-to-top).
- `./check_ci.sh` before every commit.
