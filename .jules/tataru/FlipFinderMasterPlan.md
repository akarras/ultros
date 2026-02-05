# Flip Finder Master Plan (Project "Shiny Coin")

The current Flip Finder is... acceptable. But it can be GREATER!

## Current State
- Finds price differences between worlds.
- Basic filtering (Profit, ROI).
- Basic outlier filtering.

## Improvements Needed

### 1. Velocity-Based Sorting (High Priority)
**Problem:** Users see items with 1000% ROI but don't realize they haven't sold in a year.
**Solution:**
- Calculate `SalesPerDay` based on `avg_sale_duration`.
- Add a column "Daily Sales" to the table.
- Allow sorting by "Daily Sales".
- **Formula:** `Daily Sales = 86,400,000 / avg_sale_duration_ms`

### 2. Supply/Demand Warning (Medium Priority)
**Problem:** Users buy items to flip, but the destination market is flooded.
**Solution:**
- Fetch the *number of current listings* on the destination world.
- Calculate `DaysToSell = Current Listings / Daily Sales`.
- If `DaysToSell` > 30, show a warning icon (⚠️).

### 3. Bulk Profit Calculator (Medium Priority)
**Problem:** Buying 1 item is boring. I want to buy 99!
**Solution:**
- Show "Stack Profit": Profit if you buy all available cheap stock (up to a limit) and sell it.
- Requires checking the *quantity* of cheap listings, not just the price.

### 4. Teleport Cost Adjustment (Low Priority)
**Problem:** Teleporting costs ~1000 gil round trip.
**Solution:**
- Add a "Transport Cost" input field (default 1000 gil).
- Subtract this from the profit of the *first* sale (or amortize it).

## Implementation Specs (for my minions)
- **File:** `ultros-frontend/ultros-app/src/routes/analyzer.rs`
- **Struct:** `SaleSummary` needs a `daily_sales` field.
- **UI:** Add "Daily Sales" column to `AnalyzerTable`.
