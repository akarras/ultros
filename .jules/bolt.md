## 2024-06-25 - Avoid String formatting inside tight reactive render loops
**Learning:** Returning `&'static str` for frequently used CSS classes is much better than constructing them dynamically via `format!()` or other means, especially in virtual scrollers or tables. Also, double-check whether a reactive closure `move ||` is truly necessary when the value (`data.return_on_investment`) isn't actually a reactive signal but a plain struct field, as it avoids unnecessary cloning and allocations.
**Action:** Next time, favor returning `&'static str` literals when working with CSS classes and only use reactive closures when working directly with reactive signals or derived state.
## 2024-10-27 - Remove Reactive closures for static props
**Learning:** Leptos creates a reactive Effect for `move ||` closures even if the dependencies are static. For lists (like VirtualScroller), computing static styling outside the loop and passing it prevents unnecessary effect allocation per row.
**Action:** Hoist static class/style generation out of the `children={ move |(idx, item)|` closure to avoid per-row closures/format calls.
## 2024-07-16 - Replacing Vec clone/re-collect with retain in-place in Leptos
**Learning:** In a hot path like WebSocket event parsing (which occurs frequently), converting slices to new iterators using `.cloned().collect()` creates garbage memory. Replacing it with `retain` where possible combined with short circuit conditions like `seen.len() < MAX` directly truncates the collection naturally while filtering. Furthermore, replacing `Memo::new()` with `Signal::derive` in leptos is a deoptimization for operations that clone/allocate on the heap because `Signal::derive` re-executes on every read, thus discarding caching benefits.
**Action:** When seeing `Vec` reallocations with Iterators, look for in-place modifications using `make_contiguous`, `retain`, `drain`, or `truncate`. Also, do not blindly change `Memo` to `Signal::derive` if the inner function executes heap allocations.
## 2026-06-25 - Avoid heap allocations when parsing substrings
**Learning:** We can write a zero-allocation string search algorithm that performs identical substring matching without the need to allocate intermediate `String` instances with `format!()` in hot parsing paths.
**Action:** When working on parsing logic (e.g. FFXIV tags parsing), prefer manual string searches using `find` and `starts_with` rather than creating `String` using `format!()` for simple matching tasks.
## 2024-11-20 - Memoization Over-Allocation in `BuyingView`

**Learning:**
In `ultros-frontend/ultros-app/src/components/list/buying_view.rs`, a `Memo` creates a new array of grouped listings on every update. Previously, the code iterated over the input array using `items.clone()`, making an unnecessary copy of a large vector of items and their associated listings every time the memo ran (which could be frequently due to reactive signal updates like `excluded_datacenters`).

**Action:**
I removed the `clone()` on the `items` vector in the outer loop. Since we only need an iterative pass to calculate required listings, we can use `items.iter()` and then only `clone()` the individual inner `listings` vector (which needs to be cloned to be sorted). In addition, using `sort_unstable_by_key` instead of `sort_by_key` helps since stable sort allocates when it doesn't need to.
## 2024-11-20 - Filter before sorting to reduce O(N log N) work
**Learning:**
In `ultros-frontend/ultros-app/src/components/list/list_summary.rs`'s `get_cheapest_listing`, we were sorting a large array of `ActiveListing`s by price *before* filtering them by location and HQ constraints. Since sorting is `O(N log N)` and `N` can be very large when fetching all listings for an item across the region, filtering out ~90% of those listings *first* massively cuts down on the work required to sort them, yielding big CPU savings during hot render loops. Additionally, `sort_unstable_by_key` provides further wins over `sort_by_key` by avoiding unneeded allocations.

**Action:**
Always make sure to filter collections as small as possible *before* running expensive operations on them like `sort_by_key` or `sort_by`, especially when the filtering criteria are strict. Also, prefer `sort_unstable_by_key` when possible over `sort_by_key` to save allocations.
## 2024-11-20 - Finding Medians in O(N) instead of O(N log N)
**Learning:**
In `ultros-frontend/ultros-app/src/components/sale_history_table.rs`, we computed the median unit price and median stack size by completely sorting the arrays (`sort_unstable()`) and taking the middle element. Since finding a median is a classic selection problem, fully sorting the array does O(N log N) work when only O(N) is required. Rust's slice API provides `select_nth_unstable`, which rearranges the slice so that the element at the given index is the one that would be there if the slice were fully sorted, doing it in `O(N)` average time.
**Action:**
When computing medians or any k-th order statistic, always use `select_nth_unstable` (or `select_nth_unstable_by_key`) rather than fully sorting the collection to save unnecessary CPU cycles.
