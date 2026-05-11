## 2025-02-13 - Optimize Retainer Lookup in `ultros-db`
**Learning:** Found a nested loop where we were searching linearly through a `HashMap` of retainers for every single listing being added to the database (`retainers.values().find(|r| r.id == l.retainer_id)`). This turns an $O(K)$ insertion step into $O(N \times K)$ where $N$ is the number of retainers in the map.
**Action:** Always prefer reverse lookup maps when repeatedly looking up items by a secondary key (like ID when the primary map key is Name). Building a `HashMap<i32, &retainer::Model>` beforehand makes the lookup $O(1)$.

## 2024-04-28 - Sorting Optimizations
**Learning:** Found multiple usages of `sort_by_key` across the frontend
**Action:** Replaced `sort_by_key` with `sort_unstable_by_key` which is faster and allocates less memory, as stable sorting is not required for these cases (e.g. price per unit).
