
## 2025-02-13 - Optimize Retainer Lookup in `ultros-db`
**Learning:** Found a nested loop where we were searching linearly through a `HashMap` of retainers for every single listing being added to the database (`retainers.values().find(|r| r.id == l.retainer_id)`). This turns an $O(K)$ insertion step into $O(N \times K)$ where $N$ is the number of retainers in the map.
**Action:** Always prefer reverse lookup maps when repeatedly looking up items by a secondary key (like ID when the primary map key is Name). Building a `HashMap<i32, &retainer::Model>` beforehand makes the lookup $O(1)$.

## 2024-05-18 - [Optimizing Leptos Reactivity & Arc Clones]
**Learning:** In Leptos, using a `move ||` closure for view arguments creates a dynamic node tracking reactivity. For static/immutable strings, computing it once (outside closure or immediately resolving inside `{ }` block without closure) avoids reactive tracking overhead. Also, when passing structs like `SearchResult` around, passing `Arc<SearchResult>` and cloning the `Arc` is O(1) and far cheaper than extracting and cloning its inner `String` components multiple times for callbacks.
**Action:** Always prefer cloning `Arc` instead of `String` within closures, and omit `move ||` for static derivations when rendering Leptos components.

## 2025-03-02 - Optimize Nested Iterator Chains to Reduce Allocations
**Learning:** In `get_retainer_undercut_items`, the previous implementation chained `.filter().collect::<Vec<_>>()` and then iterated again with `.iter().map().min()` to calculate `number_behind` and `price_to_beat`. This forced the allocation of a throwaway Vector for every single listing being checked just to compute its length and find a minimum value.
**Action:** Always compute aggregates directly within a single zero-allocation `for` loop pass over the original iterator/slice instead of relying on intermediate `Vec` allocations from `.collect()` during multi-step iterator chains.

## 2024-05-24 - [Discord List Resolution]
**Learning:** Found an N+1 query issue masquerading as `get_lists_for_user`. The Discord bot commands fetched ALL lists across all 3 relations just to do a `.into_iter().find(...)` in memory! This fetched unneeded list data entirely across the wire and instantiated unused memory.
**Action:** Always check the underlying DB functions behind fetching collections before using `.find(...)` on the collection in application code. Replace full collection retrievals with targeted queries if only one specific record is needed.

## 2025-03-02 - Avoid Unnecessary Memoization for Cheap Derivations in Leptos
**Learning:** Found a component (`WindowStats`) wrapping 9 extremely cheap operations (like accessing a struct field or `.round() as i32`) inside `Memo::new`. `Memo` creates a new reactive node, which carries overhead for tracking dependencies, allocating state, and checking equality. For cheap operations, this reactive overhead exceeds the computation cost itself.
**Action:** Always prefer simple closures (`move || ...`) for O(1) derived signals and cheap math. Only use `Memo::new` when the computation is actually expensive (e.g., sorting, iterating over large lists, or complex formatting) to avoid paying the cost on every update.
## 2026-05-19 - Optimize `find_set_for_job` in `ultros-app`\n**Learning:** In `find_set_for_job`, items were collected, mapped, string-sorted, and grouped across ALL item levels just to retrieve the specific group matching `target_ilvl`. This incurred an (N \log N)$ cost where $ was all items for a job.\n**Action:** Always filter iterator items by required predicates *before* running expensive data transformations, allocations, sorting, and grouping. Filtering earlier drops the expensive work from (N \log N)$ to (K \log K)$ where $ is the tiny subset of matching items.\n
