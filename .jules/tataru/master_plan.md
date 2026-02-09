# Tataru's Master Plan for Gil Domination

Greetings, minions! I, Tataru Taru, have devised a plan to maximize our profits. We need to upgrade our investment tools to be sharper, faster, and more profitable!

## 1. The "Daily Profit" Metric
**Problem:** A high profit per item is useless if it never sells! A 1,000,000 gil profit item that sells once a year is worse than a 10,000 gil profit item that sells 20 times a day.
**Solution:** specific "Daily Profit" column.
**Math:** `Daily Profit = (Profit Per Unit) * (Sales Per Day)`
**Implementation:**
- Add `daily_profit` to `CalculatedProfitData` in `analyzer.rs` and `vendor_resale.rs`.
- `Sales Per Day` can be derived from `avg_sale_duration`.
- Add a column to the table and allow sorting by it.

## 2. "Stack Size" & Inventory Cost
**Problem:** Buying 99 of an item to flip might lock up our inventory and gil for too long.
**Solution:** Analyze typical stack sizes.
**Implementation:**
- Analyze sales data to see average stack size sold.
- Warn if we are buying a stack of 99 but average sale stack is 1.

## 3. Market Saturation (Competition)
**Problem:** If there are 50 other people selling the same item, we will be undercut constantly.
**Solution:** Show the number of current listings.
**Implementation:**
- We already have `CheapestListings`, which might contain the number of listings? Need to check. If not, we might need to fetch it.
- If we have it, display "Competition: X listings".

## 4. Volatility Index
**Problem:** Prices fluctuate. Is this a stable price or a spike?
**Solution:** detailed price history analysis.
**Implementation:**
- Compare current price to 30-day average.
- If current > 150% of average, warn "Price Spike".

## Immediate Action Items (The "Easy" Gil)
1.  **Implement `Daily Profit` sorting and display.** This is the low-hanging fruit.
2.  **Improve UI for "Next Sale".** Color code it.
