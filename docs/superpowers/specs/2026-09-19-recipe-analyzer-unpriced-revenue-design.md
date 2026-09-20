# Recipe analyzer: no listing fallback for sale-stat revenue, evidence filters

Date: 2026-09-19
Status: approved design, not yet implemented

## Problem

On `/recipe-analyzer/Gilgamesh?revenue=sale-median&window=90` the top row was
Cauldronmaster's Jackboots (item 13047) at a price of 24,999,999 and a profit of
23.7M. The item has no listings on Gilgamesh and no sale there since
2026-03-26. ClickHouse, Postgres and Universalis agree on that; no data is
missing.

The number is the only Aether listing for the item (Faerie, world 54).
`SignalView::quality` in `ultros-frontend/ultros-calc/src/pricing.rs` layers
three sources: sell-world listings, then buy-scope listings, then the
selected sale statistic on top. When the sale statistic has no row the row
keeps whichever listing it found. Under a sale-stat revenue that turns "no
sales in the window" into "some other world's asking price" and lets it
lead the profit ranking. The `revenue_fell_back` flag and the small
"listing" tell exist, but the row still sorts first.

The fallback was written for listing-based revenue so rows with no
sell-world listing would not vanish as unpriceable. That reasoning does not
carry to a sale statistic: absence of sales is the signal.

## Decisions

### 1. Sale-stat revenue never falls back to a listing

When the selected revenue signal is a sale statistic (sale min, median or
average) and the sell place has no stat row for the page window and either
quality, the row has **no revenue**.

- Price, profit and ROI render "—". Nothing falls through to the sell
  world's listing or the buy scope's listing.
- The row is kept in the table. It is not subject to the
  `CostAtOrAboveNet` drop rule, which has no net to compare.
- Unpriced rows sort after every priced row under every sort mode and in
  both directions. Profit descending and profit ascending both keep them at
  the bottom. Within the unpriced block the existing secondary order
  applies.
- Metric thresholds on price, profit or ROI treat "—" as not matching, so
  a `profit>=0` filter hides them with no new code.
- The alternative-revenue columns (`rev-listing-min`, `rev-sale-*`) already
  render "—" for a missing stat and are unchanged.
- Cost-side behaviour is unchanged: an ingredient with no sale row still
  prices from the buy-scope listing, because a missing ingredient price
  would fake a free craft.

**Listing-based revenue is unchanged.** Under `revenue=listing-min` the
sell-world listing still falls back to the buy-scope listing with the
"listing" tell. That fallback is also questionable and should be revisited,
but it is out of scope here.

### 2. Two evidence filters in the filter bar

Both are row-shaping filters: visible in the bar, cleared by "Clear all",
not seeded by default, and part of the stable URL-key contract.

**"Sold in window"** — URL key `sold`, boolean toggle.
Hides rows whose sell place has no sale row for the page window under the
selected revenue signal. Under a sale-stat revenue this is exactly the set
of unpriced rows from decision 1. Under a listing revenue it hides rows
whose sell-place sale stats have no row for the window, so the toggle means
the same thing whichever signal is selected.

**"Last sold within"** — URL key `last-sold`, number of days.
A `FilterAlias` over the existing Last sold column, the same mechanism
`min-sales` uses over Daily sales. Inclusive (`<=` days ago), matching the
repo's range-filter convention. A row with no last-sale time never matches
when the filter is set.

A "Min sales in window" count filter was considered and rejected: once the
fallback is fixed, "no sale row" equals "0 sales", and the existing
`min-sales` (daily sales) filter is the same quantity divided by the window
length. Recency is the dimension the bar cannot express today.

### 3. Data model

`RecipeProfitData` today carries `market_price: i32` and
`revenue_fell_back: bool`. The change introduces an explicit revenue state
so "—" is a value rather than a sentinel:

```rust
enum Revenue {
    /// The selected signal at the sell place.
    Priced { price: i32, fell_back: bool },
    /// Sale-stat revenue with no sale row for the window.
    Unpriced,
}
```

The profit, tax and ROI figures move behind one `line: Option<ProfitLine>`
field, `None` for `Unpriced` rows, rather than three separate options. Every reader that formats or
sorts these fields handles the `None` arm; the grid's numeric `CellValue`
already has a missing form used by the late-enrichment columns.

The row also records `last_sold_unix: Option<i64>` from the sell-place
stats so the `last-sold` alias has a column to read; the Last sold column
already renders from the same source.

## Testing

Unit tests live next to the existing pricing and registry tests in
`ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`.

- **Fixture reproduces prod.** A recipe whose output has one buy-scope
  (datacenter) listing on another world at 24,999,999, no sell-world
  listing and no sale rows. Under `revenue=sale-median` the row is present,
  `Revenue::Unpriced`, profit `None`. Under `revenue=listing-min` the row
  is priced at 24,999,999 with `fell_back = true` (unchanged behaviour).
- **Sort.** Two priced rows plus the fixture row: the fixture sorts last
  under profit descending, profit ascending, velocity and price.
- **Drop rule.** The fixture row is not removed by the drop rule even though
  its cost is positive.
- **Filters.** `sold=true` hides the fixture row and keeps a row with sales.
  `last-sold=90` hides a row whose last sale is 176 days old and keeps one
  at 30 days. Both keys extend the
  `filter_registry_keys_are_a_stable_url_contract` assertion.
- **Thresholds.** A `profit` threshold hides the unpriced row.
- **i18n.** New labels (`recipe_analyzer_filter_sold_in_window_label`,
  `recipe_analyzer_filter_last_sold_within_label`, and any tooltip text) are
  added to all seven locale files with real translations.

## Out of scope

- Buy-scope listing fallback under listing-based revenue.
- Seeding either new filter by default, and the last-view cookie restoring
  an emptied filter set as a preference. Both were discussed and deferred.
- Other analyzers (Flip Finder, vendor resale, FC crafting) that price from
  sale stats. Once this lands the same `Revenue` shape can be applied there.
