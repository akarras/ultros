# Investment Tools Improvements - Master Plan
**Author:** Tataru Taru
**Date:** 2024

## Overview
To maximize gil acquisition for the Scions (and myself), we need to upgrade our market analysis tools with proper investment mathematics. The current "Flip Finder" is good, but it lacks risk assessment and capital efficiency metrics.

## Proposed Metrics

### 1. Velocity (Sales Per Day)
*   **Concept:** How fast does the item sell? High profit is useless if it takes a year to sell.
*   **Formula:** `Count of Sales / Time Range (Days)`
*   **Implementation:**
    *   Derived from existing `avg_sale_duration`.
    *   `Velocity = 1 / avg_sale_duration (in days)`
    *   If `avg_sale_duration` is None (no sales), Velocity is 0.

### 2. Volatility (Risk)
*   **Concept:** How stable is the price? We want predictable returns. High volatility means the price might crash before we sell.
*   **Formula:** Coefficient of Variation (CV) = `Standard Deviation / Mean`
*   **Implementation:**
    *   Calculate `mean` price of recent sales.
    *   Calculate `standard_deviation` of recent sales.
    *   `Volatility = (std_dev / mean) * 100` (expressed as percentage).
    *   Low Volatility (< 10%) = Safe.
    *   High Volatility (> 30%) = Risky.

### 3. Profit Per Day (PPD)
*   **Concept:** Capital Efficiency. This combines Profit and Velocity.
*   **Formula:** `Profit * Velocity`
*   **Why:** An item with 10k profit selling 10/day (100k/day) is better than an item with 50k profit selling 1/day (50k/day).

## UI Improvements

### Analyzer Table
*   **New Columns:**
    *   **Sales/Day**: Show the calculated Velocity.
    *   **Risk**: Show Volatility (maybe color-coded: Green/Yellow/Red).
    *   **Profit/Day**: Show the PPD value.

### Filters & Sorting
*   **Sort by:**
    *   `Profit/Day` (Default for aggressive traders)
    *   `Risk` (Ascending for conservative traders)
*   **Filters:**
    *   `Min Sales/Day`: crucial to avoid "dead stock".

## Future Ideas
*   **Portfolio Mode**: Suggest a basket of items to buy with a given budget (e.g., "I have 1 million gil").
*   **Price History Graphs**: Sparklines in the table rows.
*   **Undercut Alert**: Notification if someone undercuts your investment (requires retainer integration).
