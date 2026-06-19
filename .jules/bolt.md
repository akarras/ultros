## 2024-06-25 - Avoid String formatting inside tight reactive render loops
**Learning:** Returning `&'static str` for frequently used CSS classes is much better than constructing them dynamically via `format!()` or other means, especially in virtual scrollers or tables. Also, double-check whether a reactive closure `move ||` is truly necessary when the value (`data.return_on_investment`) isn't actually a reactive signal but a plain struct field, as it avoids unnecessary cloning and allocations.
**Action:** Next time, favor returning `&'static str` literals when working with CSS classes and only use reactive closures when working directly with reactive signals or derived state.
## 2024-10-27 - Remove Reactive closures for static props
**Learning:** Leptos creates a reactive Effect for `move ||` closures even if the dependencies are static. For lists (like VirtualScroller), computing static styling outside the loop and passing it prevents unnecessary effect allocation per row.
**Action:** Hoist static class/style generation out of the `children={ move |(idx, item)|` closure to avoid per-row closures/format calls.
