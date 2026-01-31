# Tataru's Master Plan for Infinite Gil

**Author:** Tataru Taru
**Goal:** Earning ALL the Gil!

## Executive Summary
To support the Scions of the Seventh Dawn, we must optimize our market board operations. The current tools are functional but lack the sophisticated financial modeling required for maximum profit extraction. We need to move from "checking prices" to "portfolio management" and "risk assessment".

## 1. Investment Math Upgrades

### 1.1 Sales Velocity (Turnover)
**Current State:** The Flip Finder uses "Average Sale Duration" (e.g., "1 sale every 4 hours").
**Improvement:** Switch to **Sales Velocity (Items Sold Per Day)**.
**Reasoning:** It's easier to compare "10 sales/day" vs "0.5 sales/day" than comparing durations. It allows for calculating "Days to Sell Stock".
**Implementation:**
- Adopt `analysis::SalesStats` logic into `analyzer.rs`.
- Display "Daily Sales" column.

### 1.2 Risk Assessment (Volatility)
**Current State:** No risk metric.
**Improvement:** Add a **Volatility Score** or **Risk Rating**.
**Math:** Coefficient of Variation ($CV = \frac{\sigma}{\mu}$).
- Low Risk: Low price variance.
- High Risk: High price variance (prices jump up and down).
**Display:** A simple indicator (Low/Med/High) or the raw % deviation.

### 1.3 Opportunity Cost & ROI Adjustments
**Current State:** ROI is simple $\frac{Profit}{Cost}$.
**Improvement:** **Annualized ROI** or **Time-Adjusted Return**.
- A 10% profit in 1 hour is better than a 50% profit in 1 month.
- Formula: $ROI_{adjusted} = ROI \times Velocity$.
- Show "Gil Per Day" potential for a given investment.

## 2. Tooling Enhancements

### 2.1 The "Sack of Gil" (Portfolio Manager)
**Concept:** A tool to track active investments.
**Features:**
- Input: "Bought 99x Iron Ore at 100g on Server A".
- Tracking: Current price on Server B (Target).
- Status: "Ready to Sell", "Hold", "Stop Loss".
- Profit/Loss realization.

### 2.2 Watchlists & Alerts
**Concept:** Passive monitoring.
**Features:**
- "Notify me when [Item] drops below [Price] on [Region]".
- "Notify me when [Item] ROI > 50%".

### 2.3 Bulk Analyzer
**Concept:** For crafters and gatherers.
**Features:**
- Paste a list of items (e.g., from a Teamcraft list).
- Output: Best world to buy each item, total savings vs home world.

## 3. New Tools

### 3.1 Desynthesis Profit
**Concept:** Buy items -> Desynth -> Sell materials.
**Math:** $\sum (Probability_{mat} \times Price_{mat}) - Price_{item}$.

### 3.2 Retainer Ventures
**Concept:** "Passive Income" generator.
**Features:** Calculate the most profitable venture per hour based on current market prices.

---

## Immediate Action Items (The "Quick Gil" Plan)

1.  **Refactor Flip Finder (`analyzer.rs`):**
    -   Integrate `SalesStats` to show **Daily Sales**.
    -   Add **Volatility** calculation to identifying risky flips.
    -   Allow sorting by **Sales Velocity**.

2.  **Visual Polish:**
    -   Add "Confidence" indicators based on sample size.

*Tataru Taru, Project Manager*
