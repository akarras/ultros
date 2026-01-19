# Tataru's Master Plan for Maximizing Gil

As the chief strategist of the Scions (and your wallet), I, Tataru Taru, have devised a plan to upgrade our investment tools. The goal is simple: **Earn all the Gil.**

Current tools are... acceptable, but they lack the sophistication of a true market manipulator... I mean, investor.

## Phase 1: Philosophy and Metrics

We need to move beyond simple "Price > Average" checks. We need actionable financial metrics.

### New Metrics
1.  **Volatility (Risk)**:
    *   Formula: `Standard Deviation / Average Price`.
    *   Use: High volatility items are risky but offer great reward. Low volatility items are safe but boring.
2.  **Liquidity (Velocity)**:
    *   Formula: `Sales Per Day * Average Price`.
    *   Use: How fast can we cash out? A high price item that never sells is a trap.
3.  **Market Sentiment (Trends)**:
    *   **Spiking (Sell Signal)**: Price is > 2 Standard Deviations above mean. Sell now!
    *   **Crashing (Buy Signal)**: Price is < 2 Standard Deviations below mean. Buy the dip!
    *   **Momentum**: Is the price trend accelerating? (Requires analyzing slope of last N sales).

## Phase 2: Tooling Improvements

### `AnalyzerService` Upgrades
The current `get_trends` function is rudimentary. We will refine it:

*   **Refine "Rising Price"**: Rename conceptually to "Spiking / Sell Signal".
    *   Logic: `Price > Average + (1.5 * StdDev)` AND `Price > 1.2 * Average`.
    *   Ranking: Sort by highest premium (%) to find best items to unload.
*   **Refine "Falling Price"**: Rename conceptually to "Crashing / Buy Signal".
    *   Logic: `Price < Average - (1.0 * StdDev)` AND `Price < 0.8 * Average`.
    *   Ranking: Sort by highest discount (%) to find best bargains.
*   **New "High Potential" List**:
    *   Items with High Velocity (> 20/week) AND Moderate Volatility.
    *   These are the bread-and-butter money makers.

### Arbitrage Improvements (`get_best_resale`)
*   **Weighted Scoring**:
    *   Currently, we filter by profit.
    *   New Score: `Profit * log(SalesPerWeek)`.
    *   This prioritizes items that actually move. A 1M profit item selling once a year is worse than a 50k profit item selling 5 times a day.

## Phase 3: Frontend "Investment Dashboard"
(Future Work)
*   A new page dedicated to "Market Opportunities".
*   Columns for: ROI, Velocity, Risk (Volatility), and "Tataru Score" (Proprietary formula).
*   Visual indicators for "Buy" (Green) and "Sell" (Red).

## Immediate Actions
1.  **Update `get_trends` logic**: Make the "Rising" and "Falling" detection more statistically sound using Standard Deviation.
2.  **Improve Ranking**: Sort these lists by the magnitude of their deviation, not just velocity or price.
3.  **Documentation**: Ensure code comments reflect financial reality.

---
*Signed,*
*Tataru Taru*
