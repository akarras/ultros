## 2024-05-23 - Backend Optimization: Hoisting `Utc::now()`

**Learning:** `Utc::now()` (or similar time functions) inside tight loops can be a performance bottleneck, especially when iterating over thousands of items. In `get_best_resale`, it was called for every sale of every item to calculate `SoldWithin`.
**Action:** When working with time-based filtering in loops, calculate `now` once at the start of the function and pass it down. Also pre-calculate filter thresholds (like `cutoff_date`) outside the loop.
