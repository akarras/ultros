
## 2025-02-13 - Optimize Retainer Lookup in `ultros-db`
**Learning:** Found a nested loop where we were searching linearly through a `HashMap` of retainers for every single listing being added to the database (`retainers.values().find(|r| r.id == l.retainer_id)`). This turns an $O(K)$ insertion step into $O(N \times K)$ where $N$ is the number of retainers in the map.
**Action:** Always prefer reverse lookup maps when repeatedly looking up items by a secondary key (like ID when the primary map key is Name). Building a `HashMap<i32, &retainer::Model>` beforehand makes the lookup $O(1)$.

## 2025-01-28 - Precomputing strings in UI iterators
**Learning:** Leptos filters in UI inputs (like dropdown selections) are hot paths that shouldn't do per-keystroke allocations, especially for large game item databases.
**Action:** Always extract `to_lowercase()` or other string conversions into an upstream `Memo` cache instead of invoking them inside `filter_map` or `filter` on every input update.
