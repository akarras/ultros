# Vendor Sell analyzer — design

Date: 2026-09-18
Status: Approved
Issue: none (idea raised in chat)

## Why this spec exists

Some market-board listings are priced below what an NPC vendor pays for the same
item. Buying such a listing and selling it straight to any vendor is a
zero-risk profit, but nothing in Ultros surfaces it. Vendor Resale answers the
mirror question (buy from an NPC, sell on the board); this page answers "buy
off the board, sell to an NPC".

## Scope

One new analyzer page, **Vendor Sell**, at `/vendor-sell` and
`/vendor-sell/:world`, built on the analyzer kit like Venture Analyzer.

In scope:

- Region-wide scan of the cheapest listings for the selected world's region.
- Profit per unit after the buyer-side 5% market tax.
- Grid with sorting, category chip, min-profit filter, saved views, realtime
  status, world picker.
- Sidebar entry, home-page tool chip, help topic, changelog entry, i18n keys in
  all seven locales, e2e grid smoke coverage.

Out of scope (YAGNI):

- Sales velocity / recent-sales columns.
- Per-city tax rates (the real rate is 3–5%; we assume the worst case).
- Stack quantity (cheapest listings carry no quantity; rows are per unit).
- A separate landing page for the bare route; `use_analyzer_world` resolves
  the home world like Venture Analyzer does.

## Inputs

| Source | Field | Use |
|---|---|---|
| `get_cheapest_listings(region)` | `item_id, hq, cheapest_price, world_id` | one candidate per (item, hq) |
| `xiv_gen::Item` | `price_low` | vendor sell-back price, NQ |
| `xiv_gen::Item` | `item_search_category` | `!= 0` means marketable |

An item is a candidate only if `item_search_category != 0` and `price_low > 0`.

## Math

New helpers in `ultros-frontend/ultros-calc/src/formula.rs`, next to
`net_after_tax`:

```text
purchase_tax_for(listing) = ceil(listing * 5 / 100)     // integer math, i64 intermediate
vendor_sell_line(listing, vendor_price):
    tax    = purchase_tax_for(listing)
    cost   = listing + tax
    profit = vendor_price - cost
    margin = return_on_investment(profit, cost)          // existing clamped f64 helper
```

Rules:

- HQ listings are valued at the NQ `price_low`. HQ vendors for at least that
  much, so the number never overstates.
- Rows with `profit <= 0` are dropped from the table outright.
- There is no tax toggle. The buyer really pays the tax at purchase.

Worked example: listing 101, vendor 120 → tax 6, cost 107, profit 13,
margin 12%.

## Page structure

`ultros-frontend/ultros-app/src/routes/vendor_sell.rs`, registered in
`lib.rs` beside `venture-analyzer`.

- `VendorSell` component: `use_analyzer_world("/vendor-sell")`,
  `use_region_for_world`, one `ArcResource` for the region's cheapest listings,
  `ToolHeader` (title, summary, context, help link, formula, assumptions,
  `WorldOnlyPicker`, market-scope label), `Suspense` around the table.
- `VendorSellTable` component: builds rows, registers filters, renders
  `ControlBar` + `MarketGrid` with the same plumbing as Vendor Resale
  (`use_market_data`, `RealtimeStatus`, `GridSavedViews`, `Calculation`).
- Pure, testable pieces:
  - `VendorSellRow { item_id, hq, world_id, listing, tax, cost, vendor_price, profit, margin }`
  - `build_rows(listings: &CheapestListings, vendor_prices: &HashMap<i32, i32>) -> Vec<Arc<VendorSellRow>>`
    (applies the drop rule; deterministic order by item id then hq)
  - `vendor_prices_from_data() -> HashMap<i32, i32>` (the marketable +
    `price_low > 0` gate over `tracked_data().items`)
  - `sort_rows(&mut Vec<..>, SortMode, SortDir)`

### Columns

| id | Header | Sortable | Notes |
|---|---|---|---|
| `hq` | HQ | no | badge when `hq` |
| `item` | Item | no | icon, link to `/item/{world}/{id}`, `AddToList`, `Clipboard` |
| `world` | World | yes | world name via `use_world_helper` |
| `listing` | Listing | yes | `Gil` |
| `tax` | Tax | yes | `Gil` |
| `vendor-price` | Vendor price | yes | `Gil` |
| `profit` | Profit | yes (default, desc) | `Gil`; min-profit `ColumnFilter` |
| `margin` | Margin % | yes | `roi_badge_class` |

Filters registered through `register_filters`: `profit` (min, numeric) and
`category` (chip over `item_search_categorys`, reusing the interned-token
pattern from Vendor Resale — move `category_id_token` into
`analyzer_kit/filters.rs` so both pages share it).

Calculation strip terms: Result = profit, Revenue = vendor price,
Tax = tax, Cost = listing.

### Assumptions shown in the header

1. Tax is a flat 5% rounded up; your retainer city may charge less.
2. HQ listings are valued at the NQ vendor price.
3. Each row is one unit; the listing may be gone before you arrive.

## Discovery

- `components/side_nav.rs`: `SideNavItem` after Vendor Resale, icon
  `i::FaCashRegisterSolid` (or nearest available), label `vendor_sell`.
- `routes/home_page.rs`: `ToolChip` with `vendor_sell` / `vendor_sell_desc`.
- `routes/help.rs`: `HelpTopic` slug `vendor-sell`, category "Market analysis".
- `ultros-changelog/changes/2026-09-18-vendor-sell-analyzer.json`, category
  `features`, importance `medium`.

## i18n

New `vendor_sell_*` keys in every file under
`ultros-frontend/ultros-i18n/locales/` (`en, fr, de, ja, cn, ko, tc`) with real
translations: page title, summary, context, help body, calc title/formula/
details, three assumptions, column headers, filter labels, results count,
meta title/description, tool-chip description, empty-state text.

## Errors and edge cases

- Listings fetch error → the same red error line Venture Analyzer shows.
- No world resolvable (no home world, no param) → the region signal is
  empty and the page shows the world picker with an empty grid, as Venture
  Analyzer does.
- `price_low` is `u32`; cast to `i32` after the `> 0` gate. Listing prices are
  `i32`; the tax helper uses `i64` intermediates so `i32::MAX` cannot
  overflow.

## Testing

`ultros-calc` unit tests:

- `purchase_tax_for`: 0→0, 1→1, 19→1, 20→1, 21→2, 99→5, 100→5, 101→6,
  `i32::MAX` does not panic.
- `vendor_sell_line`: the worked example; profit 0 and negative cases.

`routes/vendor_sell.rs` tests (SSR, `#[cfg(test)]`):

- unmarketable and `price_low == 0` items never become rows;
- `profit <= 0` rows are dropped;
- an HQ listing is valued at the NQ vendor price and keeps `hq = true`;
- `world_id` is preserved;
- default sort is profit descending, `asc` reverses it.

E2E: add `['vendor-sell', `/vendor-sell/${WORLD}`, 'profit', 'profit']` to
`integration/analyzer-grids.cjs` and the route to
`integration/shared-analyzer-data.cjs`.

CI gate: `./check_ci.sh` before commit.
