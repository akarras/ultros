# Tataru's Master Plan for Infinite Gil

As the project manager for Ultros's investment division, I, Tataru Taru, have devised the following plan to maximize profits for all minions... er, users.

## Phase 1: Enhanced Investment Metrics (The "Know Your Gil" Phase)
*Priority: High*
*Status: In Progress*

We need better numbers to make better decisions. ROI is good, but we need more.

### 1. Sales Velocity (Sales Per Day)
Currently, we show "Avg Sale Time" (e.g., "3h 20m"). This is hard to read.
*   **Proposal**: Add "Sales/Day" metric.
*   **Formula**: `Sales / Days Range`.
*   **Why**: "5.2 sales/day" is instantly understandable. "Low velocity" items are high risk.

### 2. Profit Margin vs ROI
We currently show ROI (`Profit / Cost`). We should also show Profit Margin (`Profit / Revenue`).
*   **Why**: High ROI on a cheap item is good, but if the margin is thin, a slight price drop wipes it out.
*   **Formula**: `(Estimated Sell Price - Buy Price) / Estimated Sell Price`.

### 3. Price Volatility (Risk Score)
How stable is the price?
*   **Proposal**: Calculate Standard Deviation of recent sales prices.
*   **Metric**: "Volatility: High/Med/Low".
*   **Why**: Avoid items that fluctuate wildly unless you like gambling.

## Phase 2: Portfolio Management (The "Hoarding" Phase)
*Priority: Medium*

Users buy things but forget to sell them. We need to track it.

### 1. Investment Tracker
*   User enters what they bought, for how much, and where.
*   Ultros tracks current price on their home world.
*   **Alerts**: "Sell now! Price is peaking!" or "Undercut warning!".

## Phase 3: Route Optimization (The "Efficient Errand Boy" Phase)
*Priority: Low (but fun)*

If I need to visit 5 worlds to buy 5 different items, what is the best order?
*   **Feature**: Shopping List Route Optimizer.
*   **Math**: Traveling Salesman Problem (implied, though Teleport costs are uniform-ish, load times are the real cost). Minimizing world hops.

## Phase 4: Crafting vs Flipping
*Priority: Medium*

Should I flip the raw mat, or craft it and flip the final product?
*   **Feature**: Integrated Crafting Profit Calculator in the Analyzer.
*   **Logic**: Check recipe. `Cost(Mats) < Price(Product)`.
*   Compare `Profit(Flipping Mats)` vs `Profit(Crafting)`.

---

## Implementation Plan for Phase 1 (Immediate Action)

I will personally oversee the implementation of:
1.  **Sales Per Day** column in the Flip Finder.
2.  **Profit Margin** tooltip or display.
