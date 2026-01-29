# Tataru's Master Plan for Infinite Gil

Greetings, scions! As the Treasurer of the Scions of the Seventh Dawn, I, Tataru Taru, have devised a master plan to ensure our coffers are always overflowing! We must improve our investment tools to maximize efficiency and profit. Time is money, literally!

## Vision

To build the ultimate "Flip Finder" and market analysis tool that allows any adventurer to become a Gillionaire. We will use advanced investment mathematics and intuitive UI to identify the best opportunities.

## Proposed Improvements

### 1. Sales Velocity (Sales Per Day)
**Status**: Planning / Implementation
**Description**: Currently, we only show "Average Sale Duration". This is confusing! We should show "Sales Per Day". High sales velocity means quick turnover, which is crucial for flipping.
**Math**: `Sales Per Day = Number of Sales / Time Range (Days)`
**Action Item**:
- [ ] Update `analyzer.rs` to calculate and display Sales Per Day.
- [ ] Add a filter for "Minimum Sales Per Day".

### 2. Investment Confidence Score
**Status**: Research
**Description**: A single score (0-100) that rates how safe and profitable an investment is.
**Math**: `Score = f(ROI, Sales Velocity, Price Stability, Market Saturation)`
- High ROI + High Velocity = High Score
- High ROI + Low Velocity = Medium Score (Risk of holding bag)
- Low ROI + High Velocity = Low Score (Not worth the effort)
**Action Item**:
- [ ] Research a formula for this score.
- [ ] Implement a "Confidence Score" column.

### 3. Market Saturation
**Status**: Future Work
**Description**: We need to know supply vs. demand. Even if an item sells well, if there are 1000 listings, we will get undercut instantly.
**Math**: `Saturation = Current Listings Count / Sales Per Day`
- Low saturation is better.
**Action Item**:
- [ ] Need to fetch listing counts (currently we only get cheapest price).
- [ ] Update API to provide listing counts in `CheapestListings`.

### 4. Price Trend Analysis
**Status**: Future Work
**Description**: Is the price crashing or spiking?
**Math**: Linear regression slope of the last N sales.
- Positive slope: Price rising.
- Negative slope: Price falling.
**Action Item**:
- [ ] Implement trend indicator (Arrow Up/Down) next to price.

### 5. Undercut Alerts
**Status**: Future Work
**Description**: Notify users when they are undercut.
**Action Item**:
- [ ] Requires user listing tracking (out of scope for public analyzer but good for user dashboard).

## Immediate Next Steps
1.  Implement **Sales Velocity** in the Flip Finder. This is the low-hanging fruit that yields the most gil!
2.  Add a **Minimum Sales Per Day** filter so we can filter out items that never sell.

Let's get to work! Chop chop!
