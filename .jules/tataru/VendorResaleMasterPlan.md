# Vendor Resale Master Plan (Project "Easy Gil")

Buying from NPCs and selling to players? That's free real estate!

## Current State
- Identifies vendor items sold on market board.
- Basic profit calculation.

## Improvements Needed

### 1. Vendor Location (High Priority)
**Problem:** Users know *what* to buy, but not *where*.
**Solution:**
- Integrate `xiv-gen-db` map data.
- Display "Zone Name (X, Y)" for the vendor.
- Link to a map website (e.g., Garland Tools or Teamcraft) if possible.

### 2. Restricted Vendor Warning (High Priority)
**Problem:** Some vendors are locked behind quests (Beast Tribes, etc.). Users waste time traveling.
**Solution:**
- Check `Shop` data for unlock requirements.
- Add a "Restricted" tag if the vendor is not default.

### 3. "Safe Bet" Score (Medium Priority)
**Problem:** Is it worth the walk?
**Solution:**
- Calculate a score based on ROI * Sales Velocity.
- `Score = log(ROI) * DailySales`.
- Highlight high-score items.

## Implementation Specs
- **File:** `ultros-frontend/ultros-app/src/routes/vendor_resale.rs`
- **Data Source:** Need to check if `xiv_gen` exposes vendor locations and requirements.
