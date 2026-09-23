# Item page verdicts: sell and craft

Two compact cards at the top of `/item/:world/:id` that answer "what should
I list this at?" and "should I craft this or buy it?" without scrolling.

This salvages the "verdict hero" idea from the deleted
`2026-07-26-item-view-layout-design.md`. The `?lens=` page reordering from
that spec is **not** built and is not wanted; only the verdicts are.

## Why

Everything the two answers need is already on the page, but spread out:

- Real price sits in the listings panel header (`RealPriceSummary`), the
  floor in the listings table and the first chart stat card, recent sales in
  the sale history. Nothing turns them into a price to list at, and nothing
  says how much stock is ahead of a new listing.
- Each recipe card in `#sources` (`ultros-ui-crafting/.../related_items.rs`)
  already runs `compute_cost` and shows "Est. cost" / "Est. profit" chips,
  but at the bottom of the page, per recipe, without sub-crafts, and without
  dividing by the recipe's yield.

## Placement

A new `ItemVerdicts` component inside `#overview`, directly after
`DecisionHeader` (freshness/cadence badges and the "cheapest on another
world" callout) and before the listings/sales tables.

- Two cards side by side, **Sell** then **Craft**, stacking to one column on
  narrow containers. Uses a container query like the tables below it
  (`@container`), not a viewport breakpoint.
- An item with no recipe renders the sell card alone at half width on wide
  containers, full width when stacked.
- Card styling matches the existing surfaces: `item-surface`-style panel,
  `text-brand-200` heading, muted labels, the existing emerald / red / amber
  pill classes for verdict chips, `<Gil>` for amounts.

Relationship to other surfaces:

- **Undercut pressure** (`claude/undercut-pressure`,
  `2026-09-22-undercut-pressure-design.md`): its pane and four cards live
  under the price chart at world scope and answer "is this market contested,
  will my listing survive". The sell verdict answers "what price, how much
  stock ahead". No value is shown in both. The two features touch different
  parts of `item_view.rs` (`ChartWrapper` there, `ListingsContent` here).
  Once both have merged, a follow-up can add a "price war active" chip to the
  sell card linking to the pane; this work does not depend on that branch.
- **Recipe cards in `#sources`** are unchanged. The craft card links to
  `#sources` for the breakdown and labels itself "incl. sub-crafts" so a
  number that differs from the per-recipe chips is explained.

## Code layout

- `ultros-frontend/ultros-calc/src/verdict.rs` (new, `pub mod verdict` in
  `lib.rs`): pure functions and result types, no Leptos, fully unit-tested.
  Inputs are plain tuples/slices so the module does not need the page's
  wire types beyond what `ultros-calc` already depends on
  (`ultros-api-types`).
- `ultros-frontend/ultros-app/src/routes/item_view_verdicts.rs` (new, beside
  `item_view_scope.rs` / `item_view_sections.rs`): the `ItemVerdicts`,
  `SellVerdictCard` and `CraftVerdictCard` components. `item_view.rs` only
  gains the mount in `ListingsContent`.

## Sell verdict

### Which world

1. The page scope is a single world → that world.
2. Otherwise (datacenter/region page) → the viewer's home world
   (`use_home_world()`), if it is one of the page's worlds.
3. Otherwise (no home world set, or it is outside the page scope) → the card
   shows one muted line, "Pick a world to see where to list", with no numbers.

Undercutting is per world, so the card never aggregates listings across
worlds.

### Inputs

From `listing_resource` / `filtered_listings`, already loaded by the page:
the chosen world's listings `(price_per_unit, quantity, hq)` and the scope's
recent sales `(price_per_item, quantity, hq, world_id, sold_date)` (up to 200
across the page scope, from `world_item_listings`), plus the item's vendor
price for `real_price`'s laundering guard.

### `sell_verdict(...) -> Option<SellVerdict>`

- **Headline quality** (shared by both cards, so their quality chips always
  agree): `real_price(page-scope sales).primary()`'s quality, i.e. whichever
  quality has more sales, NQ on a tie; NQ when there are no sales.
- **Real price.** `real_price` on the chosen world's sales, at the headline
  quality. If the world has no sales of that quality, use the page-scope
  estimate instead and label it with the scope name. No estimate either
  way → the card shows "No recent sales" in place of the real price and the
  warning chip is skipped.
- **Floor.** Cheapest listing of the headline quality on the world.
- **Wall.** Sort that quality's listings by price ascending. The wall starts
  at the floor and extends while `next_price <= prev_price * 1.05`. Reports
  listing count, unit total, and price range (low–high).
- **Gap.** The first listing above the wall: its price and
  `(gap_price - wall_high) / wall_high` as a percent. Absent when the whole
  board is one wall.
- **Fast price.** `floor - 1` (never below 1).
- **Patient price.** `gap_price - 1`, shown only when a gap exists. By the
  wall rule it is always more than 5% above the top of the wall.
- **Sale rate.** Window = from the oldest sale in the *scope* buffer to now,
  capped at 7 days (the buffer is scope-wide, so a busy datacenter's 200
  sales covering 2 days means we only know 2 days on every world). Units =
  sum of `quantity` of sales on the chosen world, headline quality, inside
  the window. Rate = units / window days. Fewer than 3 matching sales → rate
  is "too few sales".
- **Days of stock.** Units listed at the headline quality on the world ÷ rate
  (the same formula as the server's `set_stock`, fed from recent sales).
- **Patient ETA.** Wall units ÷ rate: roughly how long until the wall ahead
  of a patient listing clears.
- **Warnings** (one chip at most, relative to real price):
  - floor < 0.7 × real price → "Floor is N% under real price"
  - floor > 1.5 × real price → "Floor is far above recent sales"
- **Empty board** for the headline quality → no fast/patient/wall lines;
  the card says "No NQ listings on Gilgamesh" (quality and world
  substituted) and shows real price as the reference.

All thresholds (`WALL_STEP = 1.05`, `RATE_WINDOW_DAYS = 7`,
`MIN_RATE_SALES = 3`, `FLOOR_LOW = 0.7`, `FLOOR_HIGH = 1.5`) are named
constants at the top of `verdict.rs`.

### Card

```
Sell on Gilgamesh                         [NQ]
Fast      198   undercut floor
Patient   213   behind 5 units · ~0.6 d
────────
Wall: 3 listings, 5 units, 199–200
Gap above: +7% → 214
Real price 182 · ~2.1 days of stock
[warning chip, if any]
```

## Craft verdict

### When shown

Only when at least one recipe has `item_result == item_id`. With several
(different jobs), the cheapest per-unit craft cost wins.

### Pricing

- `compute_cost` over the `CheapestPrices` context (the viewer's price
  zone), the same source ingredients are priced from on the recipe cards.
- `max_subcraft_depth: 2`, with a `recipes_by_output` map built only for the
  items reachable within two levels of the candidate recipes (not all
  recipes, as the recipe analyzer does).
- Honors the existing `CraftOptions` cookie (exclude crystals, use on-hand)
  and `OnHandMap`, like the recipe cards; vendor ingredient prices via
  `vendor_price_map()`.
- `require_hq` follows the headline quality (defined under the sell
  verdict).
- **Per unit:** `craft_unit = cost / amount_result.max(1)`.

### Buy price

- Zone cheapest comes from `CheapestPrices` for the headline quality. For HQ
  with no HQ listing, fall back to the NQ listing and label the buy line
  "NQ", as the recipe cards do.
- When the headline quality is NQ and the item is vendor-sold,
  buy = `min(zone cheapest, get_vendor_price(item_id))`. Vendors only sell
  NQ, so the vendor price is not considered for an HQ verdict.
- The buy line's label names the winner: "cheapest in {zone}" or "vendor".

### `craft_verdict(craft_unit, buy, unpriced_lines) -> CraftVerdict`

- `unpriced_lines > 0` (from `CostBreakdown::unpriced_market_lines`) or no
  buy price → **Incomplete**: show craft cost (if any) and "Some ingredients
  have no listings", no verdict word.
- `craft_unit < buy × 0.97` → **Craft saves** `buy - craft_unit`
  (`(buy - craft_unit) / buy` %), emerald chip.
- `craft_unit > buy × 1.03` → **Buying is cheaper by** `craft_unit - buy`
  (`(craft_unit - buy) / craft_unit` %), red chip.
- Otherwise → **About even**, neutral chip.

`EVEN_BAND = 0.03` is a named constant.

### Card

```
Craft or buy                              [NQ]
Craft / unit                       18
Buy · cheapest in North-America    20
[Craft saves 2 (10%)]
incl. sub-crafts · crystals excluded
See recipe breakdown ↓   (links to #sources)
```

## Hydration and failure

- Both cards render inside a `Transition` over `listing_resource`, as
  `DecisionHeader` does, and build their i18n with
  `i18n_fallback::use_i18n_or_default()` (the `RealPriceSummary` precedent,
  GlitchTip #7294).
- Anything that depends on `now` (sale-rate window, days of stock, ETA) or
  on `CheapestPrices` (the whole craft card) renders only after a
  `hydrated` flag flips in an `Effect`, with a same-shape skeleton before
  that. This is the existing idiom (#740, #732, `DecisionHeader`'s
  zone-savings gate) and avoids SSR/CSR DOM mismatches.
- Listings failing to load → neither card renders (the page already shows
  the error). `CheapestPrices` failing → the craft card is hidden.
- No new requests: the sell verdict uses the page's listings payload; the
  craft verdict uses `CheapestPrices`, which the page already demands.

## i18n

Every label, verdict phrase, hint, warning chip and aria-label is a key
prefixed `item_verdict_*`, added to all seven locale files
(`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with real translations.
Reuse existing keys where the meaning is identical (`hq`, `nq`, `real_price`).

## Cleanup

- `routes/item_view_scope.rs:27`: drop "and `?lens=` once the lens work
  lands" from the `item_href` doc comment.
- `routes/item_view_sections.rs:3-4`: drop the "Later lens work reorders the
  rendered sections…" sentence from the module doc.

## Tests

`verdict.rs` unit tests:

- Wall: single listing; whole board within 5% (no gap); a step exactly at
  1.05× stays in the wall, just above it breaks; quality filter excludes the
  other quality.
- Fast price clamps at 1; patient price absent without a gap.
- Sale rate: window capped at 7 days; window shorter than 7 days when the
  scope buffer is shorter; only the chosen world and quality count; fewer
  than 3 sales → too few.
- Days of stock and ETA with a normal rate; no division when rate is absent.
- Real price falls back to scope-wide sales when the world has none.
- Warnings at, just below and just above 0.7× and 1.5×.
- Empty board for the headline quality.
- Craft: per-unit yield division; ±3% band edges; incomplete on unpriced
  lines or missing buy; buy = vendor when vendor is cheaper, zone when not;
  vendor ignored for an HQ verdict; HQ→NQ buy fallback.
- Headline quality: more HQ sales → HQ; tie → NQ; no sales → NQ.

Frontend:

- A test that `ItemVerdicts` builds without an i18n context (same shape as
  `real_price_summary_builds_without_an_i18n_context`).
- World-choice helper as a pure function with tests: world page, DC page
  with home world inside, DC page with home world outside, no home world.

Manual, on a local build in the browser: a world page, a datacenter page
with and without the home world in scope, a craftable item, a multi-yield
recipe, a vendor-sold item, a non-craftable item; console free of hydration
warnings.

`./check_ci.sh` green before committing.

## Out of scope

- Excluding the viewer's own retainers from the wall (so "fast" never
  suggests undercutting yourself).
- The "price war active" chip from undercut pressure (after both merge).
- Fixing the recipe cards' "Est. profit" chips, which compare a single-unit
  market price with a whole-craft cost and ignore `amount_result`. Tracked
  as a separate fix.
- Any per-item listing-stats endpoint; days of stock is estimated from the
  page's recent sales.
- The `?lens=` page reordering.
