
## 2024-05-18 - Leptos `VirtualScroller` View Closure String Allocations
**Learning:** In Leptos, when building views inside iterative components like `VirtualScroller` or `For`, deep-cloning values like `String` just to satisfy the borrow checker for reactive closures (like `move || ...`) leads to heavy redundant allocation per row. `String` allocations are particularly expensive during rapid updates (e.g. searching/filtering).
**Action:** Always prefer wrapping heavy structs in `Arc` and bumping the refcount (`Arc::clone(&result)`) instead of cloning internal strings. This pattern drastically reduces allocations and GC overhead per row.
