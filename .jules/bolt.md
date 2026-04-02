## 2025-04-02 - Replace sort_by_key with sort_by_cached_key
**Learning:** In Rust, `.sort_by_key()` calls the key extraction function `O(N log N)` times. For keys that involve expensive operations like map lookups (`ItemSortKey::from`), this can cause a noticeable performance hit.
**Action:** Use `.sort_by_cached_key()` instead when the sort key extraction is non-trivial, reducing lookups from `O(N log N)` to `O(N)`. Ensure to add comments explaining the performance benefit as required by Bolt's boundaries.
