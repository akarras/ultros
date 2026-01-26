# Tataru's Master Plan for Infinite Gil

Listen up, minions! To maximize our profits, we need to stop bleeding gil on bad math! Here is the plan to overhaul our investment strategies.

## Phase 1: The Basics (Stop Losing Money)

### 1. Market Tax Implementation
**Problem:** We are currently pretending the market tax doesn't exist! The Market Board takes a 5% cut of every sale. We are overestimating our profits!
**Solution:**
- Update `get_best_resale` in `ultros/src/analyzer_service.rs`.
- Apply a 5% tax reduction to the `est_sale_price` before calculating profit.
- Formula: `Net Sale Price = Sale Price * 0.95`.
- `Profit = Net Sale Price - Purchase Price`.

### 2. Teleportation Costs
**Problem:** Traveling between worlds to buy cheap items costs gil! Small margin flips might actually be losses if we spend 1000 gil teleporting.
**Solution:**
- Add a configurable `teleport_cost` parameter (defaulting to a safe estimate like 500 gil).
- Subtract this from the profit calculation.

## Phase 2: Advanced Analytics (Smarter Investments)

### 3. Velocity & Liquidity Scoring
**Problem:** A 100k profit is useless if the item takes a year to sell! We need to prioritize items that move fast.
**Solution:**
- Integrate `sales_per_week` metric into the `ResaleStats`.
- Create a "Score" that weights Profit against Time-to-Sell.
- `Score = Profit * (Sales Per Week / Desired Turnover Time)`.

### 4. Volatility & Risk
**Problem:** Some prices jump around like a Miqo'te on catnip. High volatility means high risk.
**Solution:**
- Use Standard Deviation (already calculated in `get_trends`) to flag high-risk investments.
- If the price variance is high, require a higher ROI to justify the investment.

## Phase 3: UI Improvements (User Experience)

### 5. Better Filtering
**Problem:** The current filters are too basic.
**Solution:**
- Allow filtering by "Minimum Sales Per Week".
- Allow filtering by "Maximum Investment Amount" (for poor people).

---

## Implementation Plan for Phase 1 (Tax)

I will implement the tax fix immediately.

1.  Modify `AnalyzerService::get_best_resale` in `ultros/src/analyzer_service.rs`.
2.  Adjust the profit calculation to: `let profit = (est_sale_price as f32 * 0.95) as i32 - cheapest_price.price;`
3.  Adjust the ROI calculation to reflect the taxed return.
