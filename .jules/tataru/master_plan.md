# Tataru's Master Plan for Gil Maximization

Hello minions! This is your project manager Tataru. I have analyzed our current tools and found them... lacking. We are leaving too much gil on the table!

Here is the specification for the next generation of investment tools.

## 1. Smarter Valuation Logic (The "Greedy but Wise" Algorithm)

**Problem:** Our current "Flip Finder" (`get_best_resale`) is too cowardly. It estimates the sale price based on the *minimum* price of the last few sales. If one person got lucky and bought a `Golden Beaver` for 1 gil, we assume we can only sell it for 1 gil. This is unacceptable!

**Solution:**
- We shall use the **Median** or **Weighted Average** of recent sales.
- Specifically, we should look at the sale history (last 20 sales if possible, currently we store 6).
- **Algorithm Update**:
    - `EstimatedSalePrice = min(CurrentCheapestListing - 1, Median(RecentSales))`
    - If `CurrentCheapestListing` doesn't exist (market is empty), use `Median(RecentSales) * 1.2` (Monopoly pricing!).
    - If `RecentSales` is empty, ignore the item (too risky).

## 2. The "Tataru Score" (Investment Grading)

**Problem:** Minions get confused by "Profit" vs "ROI". A 100% ROI on a 1 gil item is useless. A 1,000,000 gil profit on an item that sells once a year is a trap.

**Solution:**
- Introduce a composite score: `TataruScore`.
- `TataruScore = Log10(Profit) * (SalesPerWeek ^ 0.5) * Reliability`
- **Reliability**: A factor (0.0 to 1.0) based on price volatility.
- Display this score in the UI and allow sorting by it. "Sort by Best Opportunity".

## 3. Advanced Market Trends

**Problem:** "Rising Price" just means "Current > 1.5 * Average". This is too simple.

**Solution:**
- Implement **Standard Deviation** checks.
- **Spike Detection**: Price > Average + 2 * StdDev.
- **Crash Detection**: Price < Average - 2 * StdDev.
- **Volatility Index**: High StdDev means high risk (or high reward for brave traders).

## 4. Vendor Resale "Cash Flow"

**Problem:** Vendor resale list is clogged with items that never sell.

**Solution:**
- Default sort by `WeeklyProfit = UnitProfit * SalesPerWeek`.
- Highlight items that can be bought from a vendor in *housing districts* (Material Suppliers) vs those that require travel to obscure zones.

## Implementation Plan

1.  **Upgrade `analyzer_service.rs`**:
    - Modify `get_best_resale` to calculate Median price instead of Min.
    - Modify `get_trends` to include Standard Deviation calculation.
2.  **Upgrade `analyzer.rs` (Frontend)**:
    - Expose the new valuation in the UI.
    - (Optional) Add the "Tataru Score" column.

---

*Signed,*
*Tataru Taru*
