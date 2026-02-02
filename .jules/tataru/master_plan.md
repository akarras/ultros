# Tataru's Master Plan for Gil Maximization

## Objective
To improve the investment tools in Ultros (Flip Finder) to allow for smarter investment decisions, focusing on sales velocity and accurate profit prediction.

## Strategy

### 1. Sales Velocity Tracking
**Problem:** Currently, the "Flip Finder" predicts sales based on `avg_sale_duration` which is derived from the time between the last few transactions. This ignores the *volume* of items sold. A single transaction of 99 items is treated the same as a single transaction of 1 item.
**Solution:**
- Track `quantity` in `SaleSummary` and `SaleHistory`.
- Increase `SALE_HISTORY_SIZE` from 6 to 20 to get a better statistical sample.
- Calculate `sales_per_day` (velocity) based on total items sold over the time period.
- Expose this velocity to the frontend.

### 2. Frontend Enhancements
**Problem:** The frontend only shows "Avg Sale Time" which can be misleading for high-volume items.
**Solution:**
- Add "Sales/Day" column to the Flip Finder table.
- Allow sorting by "Sales/Day".
- This allows investors to find high-velocity items that might have lower margins but faster turnover.

## Implementation Details

### Backend (`ultros`)
- `ultros-api-types/src/recent_sales.rs`: Add `quantity` field.
- `ultros/src/analyzer_service.rs`:
    - Update `SaleSummary` to store `quantity`.
    - Increase history size.
    - Update `get_trends` to use quantity-based velocity.
- `ultros/src/web/api/recent_sales.rs`: Populate `quantity` in API response.

### Frontend (`ultros-frontend`)
- `ultros-frontend/ultros-app/src/routes/analyzer.rs`:
    - Update client-side `SaleSummary`.
    - Calculate `items_per_day`.
    - Add column and sorting logic.

## Future Plans (To be implemented later)
- **Volatility Score:** Calculate price standard deviation to warn about risky investments.
- **Undercut Alert:** Estimate probability of being undercut based on listing velocity.
- **Tax Optimization:** Factor in retainer city tax (if data available).
