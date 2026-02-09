# Tataru's Master Plan for Gil Maximization

## Objective
To enable all minions (users) to earn maximum gil by improving the investment analysis tools in Ultros. We need to move beyond simple "buy low, sell high" and introduce sophisticated market metrics.

## Strategy

### 1. Investment Maths Overhaul
The current tool relies on basic price difference and a rough "average sale duration". We will introduce:

- **Sales Velocity (Sv)**: The number of items sold per day. This is a crucial metric for turnover.
  - Formula: `Total Sales / (Time Range in Days)`
  - Implementation: `calculate_sales_velocity` in `math.rs`.

- **Volatility Index (Vi)**: A measure of risk. High volatility means prices fluctuate wildly.
  - Formula: `Coefficient of Variation = Standard Deviation / Mean`
  - Implementation: `calculate_volatility` in `math.rs`.

- **True Cost Accounting**:
  - **Tax Rate**: Users should be able to set their own tax rate (e.g., 0% for tax-free cities/retainers, 3% for companion app).
  - **Teleport Costs**: A flat fee deduction for the cost of traveling to buy the item.

### 2. Analyzer Tool Enhancements
The `Analyzer` (Flip Finder) is our primary tool. We will upgrade it:

- **Data Structure**: Update `SaleSummary` to store `Sv` and `Vi`.
- **Logic**: Update profit calculation to:
  `Profit = (Estimated Sell Price * (1 - Tax Rate)) - (Buy Price + Teleport Cost)`
- **UI**:
  - Replace the boolean "Tax" toggle with a numeric input.
  - Add a numeric input for "Teleport Cost".
  - Display "Sales Velocity" (e.g., "5.2/day") prominently.
  - Display "Volatility" or "Risk Score".

## Future Ideas (Out of Scope for now)
- **Opportunity Cost**: Calculate what else you could have done with that gil.
- **Market Saturation**: How many listings are currently up vs. daily sales.
- **Undercut Probability**: Probability of being undercut based on listing activity.

## Implementation Plan
1.  **Math**: Implement `sales_velocity` and `volatility` in `math.rs` with tests.
2.  **Logic**: Update `analyzer.rs` to compute and use these new metrics.
3.  **UI**: Add new inputs and columns to the Analyzer table.
4.  **Verify**: Ensure all tests pass and the UI works as expected.
