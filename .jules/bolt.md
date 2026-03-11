## 2026-03-11 - Preallocating Vec for large list processing
**Learning:** In `analyze_sales`, pre-allocating the `prices` vector using `Vec::with_capacity` when iterating over large `sales_data` slices avoids multiple reallocations and improves performance.
**Action:** Always consider `Vec::with_capacity` instead of `Vec::new()` when the final size or an upper bound is known in advance.
