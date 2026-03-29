
## 2025-01-28 - Precomputing strings in UI iterators
**Learning:** Leptos filters in UI inputs (like dropdown selections) are hot paths that shouldn't do per-keystroke allocations, especially for large game item databases.
**Action:** Always extract `to_lowercase()` or other string conversions into an upstream `Memo` cache instead of invoking them inside `filter_map` or `filter` on every input update.
