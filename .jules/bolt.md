## 2024-05-06 - [SalesWindow Optimization]
**Learning:** Leptos `Memo::new` creates independent reactive nodes that allocate memory and perform equality checks. Using `Signal::derive` or bare closures is significantly lighter when pulling individual fields out of a single derived signal struct.
**Action:** Use `Signal::derive` or bare `move ||` closures instead of `Memo::new` for cheap derived field accesses from an existing reactive signal to prevent unnecessary reactivity overhead.

## 2024-05-06 - [Algorithmic Median Optimization]
**Learning:** Sorting an entire vector (`O(N log N)`) just to find the median is a performance bottleneck for large datasets.
**Action:** Use `select_nth_unstable(count / 2)` to find the median in `O(N)` time without fully sorting the vector.
