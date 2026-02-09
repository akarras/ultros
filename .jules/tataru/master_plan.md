# Tataru's Master Plan for Infinite Gil

As the Treasurer of the Scions of the Seventh Dawn (and potentially the richest person in Eorzea), I, Tataru Taru, have devised a master plan to improve our investment tooling. Our goal is simple: **Maximize Gil**.

## 1. The "Gil-per-Day" Metric (Daily Profit)
**Status**: Implementation in progress.
**Concept**: "Profit" is meaningless without "Time". A 1,000,000 gil profit that takes 10 years to realize is worse than a 10,000 gil profit that happens every hour.
**Formula**: `Daily Profit = (Selling Price - Buying Price) * (Sales Per Day)`
**Requirement**:
- Integrate "Sales Velocity" into the Profit Table.
- Allow sorting by "Daily Profit".

## 2. Investment Score (The "Scion Score")
**Status**: Planned.
**Concept**: A single number to rule them all. Novice investors get confused by too many numbers. We need a simple 0-100 score.
**Formula**: A weighted average of:
- **ROI** (30% weight): Efficiency of capital.
- **Velocity** (40% weight): How fast we get paid.
- **Risk** (20% weight): Price volatility.
- **Saturation** (10% weight): Competition.

## 3. Market Saturation Index
**Status**: Planned.
**Concept**: Are we walking into a trap? If there are 100 listings and only 1 sale a week, we will never sell.
**Formula**: `Saturation = Active Listings / Weekly Sales`.
**Requirement**:
- We currently fetch `CheapestListings` (single price). We need `ListingCount` from the backend.
- Backend needs to aggregate listing counts per item/world.

## 4. Cross-World Arbitrage Route Planner
**Status**: Planned.
**Concept**: Instead of just "Item X is cheap on World A", give me a **Travel Plan**.
- "Go to World A, buy X, Y, Z."
- "Go to World B, buy A, B, C."
- "Return to Home World, sell all."
**Requirement**:
- Knapsack algorithm to maximize profit given inventory space (140 slots).

## 5. Undercut Alert System
**Status**: Planned.
**Concept**: Time is money. If I'm undercut, I'm not selling.
- Real-time alerts via Discord/Websocket when my retaining price is no longer the lowest.

## 6. Historical Trend Analysis
**Status**: Backend supported (`get_trends`), Frontend needs update.
**Concept**: "Buy Low, Sell High". We need to know if the current price is "Low".
- Compare current cheapest price to 30-day average.
- Visual indicator: "Price is 20% below average" -> BUY SIGNAL.

## 7. Tax Evasion... I mean, Tax Optimization
**Status**: Planned.
**Concept**: Some city-states have reduced tax rates due to "events" or "favors".
- Track current tax rates for each retainer city.
- Suggest where to sell based on tax rates.

---
*Signed,*
*Tataru Taru*
