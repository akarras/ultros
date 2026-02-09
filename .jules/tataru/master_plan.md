# Tataru's Master Plan for Infinite Wealth

## Objective
To ensure that the Scions of the Seventh Dawn (and specifically Tataru) have enough funding for all future endeavors, we must optimize our investment strategies.

## Analysis of Current Tools
The current `AnalyzerService` provides basic tools for identifying:
1.  **Trends**: High velocity, rising prices, and falling prices.
2.  **Resale Opportunities**: Buying low in one region and selling high on a home world.

However, there are critical gaps:
-   **Tax Evasion... err, Awareness**: The profit calculation does not account for the standard 5% Market Board tax. This leads to overestimating profits!
-   **Velocity of Money**: We prioritize raw profit per item, but if an item sells once a year, it's a bad investment. We need to prioritize items that sell *fast* and have *good* profit.

## Proposed Improvements

### 1. The "Render unto Caesar" Adjustment (Tax Calculation)
We must subtract 5% from the estimated sale price before calculating profit.
`Profit = (Est. Sale Price * 0.95) - Purchase Price`

### 2. The "Time is Money" Metric (Velocity Weighting)
We should introduce a "Gil Per Day" (GPD) or similar metric.
`GPD = Profit * Sales_Per_Day`

We can estimate `Sales_Per_Day` from the `SoldWithin` data.
-   Today: ~1+ / day
-   Week: count / 7
-   Month: count / 30

### 3. Future Plans (Not implemented yet)
-   **Undercut Alert**: notification when someone undercuts our investment.
-   **Crafting Profitability**: Analyze cost of materials vs finished product.

## Implementation Steps
1.  Modify `ultros/src/analyzer_service.rs` to include tax in `get_best_resale`.
2.  Add velocity estimation to `get_best_resale`.
3.  Verify with tests.
