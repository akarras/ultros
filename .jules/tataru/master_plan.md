# Tataru's Master Plan for Gil Domination (v2)

## Goal
To maximize profits for all Scions (and especially Tataru) by providing superior market intelligence tools.

## Current State Analysis (Updated)
- **Trend Analysis**: Uses a simple moving average and standard deviation on the last 6 sales.
- **Resale Analysis**: Identifies arbitrage opportunities but ignores market tax (5%) and relies on a small sample size.
- **Outlier Removal**: Frontend has it, backend does not. This leads to skewed data if someone sells an item for 1 gil by mistake.
- **Previous Attempt**: Tax implementation was flawed (using floats and magic numbers). This plan aims to correct that.

## Immediate Improvements (The "Quick Gil" Phase)
1. **Tax Awareness (Clean Implementation)**:
   - The Market Board takes a 5% cut. Our ROI calculations must reflect this.
   - Profit = (Sale Price * 95 / 100) - Purchase Price. (Integer arithmetic)
   - ROI = (Profit / Purchase Price) * 100.

2. **Robust Data Analysis**:
   - Implement Interquartile Range (IQR) outlier removal in the backend.
   - This prevents "noise" (RMT transfers, accidents) from ruining the trend data.

3. **Trend Refinement**:
   - Use the filtered data for Average Price calculations.
   - This ensures we are betting on the "real" price, not the outlier-affected price.

## Future Schemes (The "Long Con" Phase)
1. **Increase History Size**:
   - 6 data points is pathetic! We need at least 20-50 to do real technical analysis.
   - *Constraint*: Database size and memory usage. Need to investigate `ultros-db` efficiency.

2. **Advanced Metrics**:
   - **RSI (Relative Strength Index)**: Are items overbought or oversold?
   - **Velocity weighted by Price**: Selling 100 items at 1 gil is less interesting than 10 items at 1,000,000 gil.

3. **Crafting Profitability**:
   - Integrate with `xiv-gen` to calculate crafting costs vs market board prices.
   - Identify items where (Mat Cost < Crafted Price * 0.95).

4. **Undercut Alerts**:
   - Notify users when they are no longer the cheapest seller.

## Implementation Details
- `ultros/src/analyzer_service.rs` is the key file for logic.
- `ultros-api-types` should eventually house the shared math logic to avoid code duplication between frontend and backend.
