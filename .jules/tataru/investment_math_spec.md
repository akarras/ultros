# Investment Math Specifications

## 1. Profit Per Day (The "Tataru Index")
The most important metric for an active trader. It balances profit margin with sales frequency.

### Formula
$$ \text{Daily Profit} = \text{Profit per Unit} \times \text{Daily Sales Velocity} $$

Where:
*   `Profit per Unit` = `Estimated Sale Price` - `Purchase Price`
*   `Daily Sales Velocity` = `86,400,000` / `Average Sale Duration (ms)`

### Implementation Details
*   If `Average Sale Duration` is undefined (no recent sales), `Daily Profit` is 0 (or marked as High Risk).
*   This metric should be the *default* sort order for advanced traders.

## 2. Risk-Adjusted ROI
Current ROI is `(Profit / Cost) * 100`.
We should introduce a "Confidence Interval" based on price volatility.

### Formula
$$ \text{Conservative Price} = \text{Avg Price} - (1.5 \times \text{Standard Deviation}) $$

If the `Conservative Price` is still higher than the `Purchase Price`, it's a "Safe Bet".

## 3. Capital Turnover Rate
How fast can we free up our gil?

$$ \text{Turnover Days} = \frac{\text{Average Sale Duration} \times \text{Stock Held}}{24 \text{ hours}} $$
