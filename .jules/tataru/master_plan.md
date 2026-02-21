# Master Plan: Market Investment & Analysis Improvements

## Overview
This document outlines plans to improve the investment tools in Ultros, focusing on usability, financial mathematics accuracy, and profit maximization. As Tataru Taru, I will ensure these plans lead to maximum gil acquisition.

## Current State Analysis
- **ROI Calculation:** Currently calculated as `((est_sale_price / cheapest_price) * 100) - 100`. This is a basic percentage return.
- **Profit Calculation:** `est_sale_price - cheapest_price`.
- **Trends:** Uses sales per week, velocity, and price standard deviation to identify "Rising" and "Falling" prices.
- **Resale Analyzer:** Finds arbitrage opportunities between worlds.
- **Frontend:** Provides filters for Min Profit, Min ROI, Min Sales, Max Predicted Time, etc.

## Proposed Improvements

### 1. Refined ROI Metrics (Plan A)
- **Problem:** Simple ROI doesn't account for time. A 100% ROI over a year is worse than 50% ROI over a day.
- **Solution:** Implement **Annualized ROI (CAGR)** or **Velocity-Adjusted ROI**.
    - Formula: `(Ending Value / Beginning Value) ^ (1 / Years) - 1`
    - Or simply `ROI / Expected Sale Time (days)` to get "Daily ROI".

### 2. Opportunity Cost & Capital Efficiency (Plan B)
- **Problem:** Users might lock up capital in slow-moving items.
- **Solution:** Add a metric for **Gil Per Day** or **Profit Per Day**.
    - `Profit / Average Sale Duration`

### 3. Risk Assessment (Plan C)
- **Problem:** High profit often comes with high risk (e.g., price volatility, low liquidity).
- **Solution:** Introduce a **Risk Score** based on:
    - Price Volatility (Standard Deviation / Mean)
    - Sales Velocity (Sales per day)
    - Number of competing listings.

### 4. Portfolio Management (Plan D)
- **Problem:** Users look at single items.
- **Solution:** Suggest a "Basket" of items to diversify risk. (Maybe too advanced for now, but good for specs).

### 5. Frontend Enhancements (Plan E)
- **Problem:** Users need to manually calculate if a trade is "worth it" based on their own time/effort.
- **Solution:**
    - "Profit per Teleport" (if buying from multiple worlds).
    - Bulk deal finder.

## Immediate Action Items (Implementation)
1.  **Implement "Daily Profit" Metric:** Add `profit_per_day` to `ResaleStats` and expose it in the frontend.
    - Logic: `Profit / (Average Sale Duration in Days)`. If duration is < 1 day, cap it or use 1.
2.  **Enhance Trend Analysis:** refine the "Rising" logic to be more robust, perhaps using Moving Averages if data allows (though we only have 6 history points... limitation).
3.  **Frontend Sorting:** Allow sorting by `Profit Per Day`.

## Detailed Specs

### Feature: Daily Profit Metric
- **Backend:** `ultros/src/analyzer_service.rs`
    - In `get_best_resale`, calculate `avg_sale_duration` for the item on the destination world.
    - Calculate `profit_per_day = profit / max(avg_sale_duration_days, 1)`.
    - Add field to `ResaleStats` struct.
- **Frontend:** `ultros-frontend/ultros-app/src/routes/analyzer.rs`
    - Update `CalculatedProfitData` to include `profit_per_day`.
    - Add column to table.
    - Add sort option.

### Feature: Risk Score
- **Backend:** Calculate volatility.
    - We already compute `std_dev` in `get_trends`. We can reuse this logic or expose it in `ResaleStats`.
    - `Volatility = StdDev / AveragePrice`.
    - `Liquidity = SalesPerDay`.
    - `Risk = f(Volatility, 1/Liquidity)`.

## Roadmap
1.  Implement Daily Profit Metric (High Value, Low Effort).
2.  Research better Trend Analysis algorithms given limited history.
3.  Draft UI for Risk Assessment.
