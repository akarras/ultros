## 2025-01-27 - O(N) Clone in Virtual Scroller
**Learning:** Leptos `Signal<Vec<T>>` clones the entire vector on `get` and `set`. Using this inside a loop for virtual scrolling updates caused O(N) allocations per row update, leading to performance issues on resize/load. `StoredValue` allows in-place mutation `O(1)`.
**Action:** Inspect `Signal<Vec<T>>` in hot paths. If the signal is only used as a state container and not directly bound to UI in a way that requires whole-value reactivity, replace with `StoredValue` or `node_ref` based state.
