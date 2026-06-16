
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

## 2025-05-22 - Resolve N+1 Queries in Retainer Listings Lookup
**Learning:** In `get_retainer_listings_for_discord_user`, the code used `futures::future::join_all` to issue concurrent `.find_related()` queries for retainers and their listings. This still results in N+1 database queries (or $1 + N \times 2$) because it sends independent SELECTs per iteration, even if concurrently.
**Action:** Always combine `find_also_related` (to fetch parent and 1:1/N:1 child in a single query) and SeaORM's `load_many` (to fetch all 1:N relations for a collection of models in one batched query). This reduces the query pattern to exactly 2 queries regardless of the collection size, drastically cutting database roundtrips.
## 2026-05-29 - Optimize Leptos <For> rendering in conditional blocks
**Learning:** In Leptos, using `<For>` components inside conditionally rendered blocks (like `match` arms for `Resource` updates) that re-create the entire view adds unnecessary keyed reconciliation overhead. Furthermore, if the block captures an owned `Vec`, providing it to `each=move || vec.clone()` causes unnecessary cloning.
**Action:** Use `vec.into_iter().map(...).collect_view()` instead of `<For>` when the entire list is recreated on update. This avoids diffing overhead and unnecessary array cloning.
## 2025-05-29 - Avoid `Memo::new` for trivial signal gets and equality checks
**Learning:** Found a component (`Clipboard`) using `Memo::new` to wrap a simple `clipboard_text()` getter, as well as an equality check `clipboard_text() == last_copied_text()` and a boolean branch to pick an icon. Creating reactive `Memo` nodes carries an overhead (tracking dependencies, memory allocation, equality checking). For purely O(1) instantaneous operations, this overhead is much larger than the operation being "memoized".
**Action:** Replace `Memo::new` and `Signal::derive` with regular closures (`move || ...`) for trivial operations like getter calls, comparisons, and conditional returns. Save `Memo` for when the computation actually takes longer than the reactive system overhead.

## 2025-05-30 - Avoid Memo::new for trivial map lookups and struct accesses in Leptos components
**Learning:** Found multiple components (`RelatedItems`, `VirtualScroller`, `ItemExplorer`) where `Memo::new` was used for completely O(1) operations. In `RelatedItems` we wrapped a HashMap `.get()`. `Memo` creates a new reactive node, tracks dependencies, allocates memory for the value, and does equality checks on every update. For extremely cheap O(1) operations, the reactive system overhead is significantly higher than the operation itself.
**Action:** Replace `Memo::new` with `Signal::derive` (or raw `move ||`) for cheap map lookups, struct field accesses, and trivial mathematical operations in Leptos. Save `Memo` for when the operation actually involves heavy allocations, sorting, iterating large collections, or expensive formatting.

## 2025-06-10 - Replace Memo::new with Signal::derive for O(1) unwrap_or_default calls
**Learning:** Found multiple instances where `Memo::new` was used for completely O(1) operations like `.unwrap_or_default()` and `.is_empty()` after `with()` on signals. Creating a `Memo` adds reactivity overhead including tracking dependencies, allocating memory, and checking equality. This is inefficient for trivial operations.
**Action:** Always prefer `Signal::derive` (or simple closures) for trivial instantaneous operations such as unwrap, `is_empty`, and `.clone()` lookups. Only use `Memo::new` for more expensive operations such as iterating over lists or computing sorts.

## 2025-06-25 - Pre-compute allocations inside Memos for UI lists
**Learning:** In the `Select` component, the `search_results` memo filtered items by checking `.to_lowercase().contains(...)` on every item, inside every keystroke update. This resulted in O(N) string allocations during a frequent reactive event (typing), which caused performance stutter on large dropdown lists (like world picker).
**Action:** When a UI component needs to filter or search a list, pre-compute the derived search keys (like lowercased strings) alongside the original items inside the outer `Memo` that tracks the data itself. This avoids re-allocating strings on every keystroke, reducing filtering overhead significantly.
## 2025-06-25 - Pre-compute allocations inside StoredValues for UI lists
**Learning:** In the `AddRecipeToCurrentListModal` component, the `search_results` memo filtered items by checking `.to_lowercase().contains(...)` on every item's name, inside every keystroke update. This resulted in O(N) string allocations during a frequent reactive event (typing).
**Action:** When a UI component needs to filter or search a list, pre-compute the derived search keys (like lowercased strings) alongside the original items inside the outer `StoredValue` that stores the static data itself. This avoids re-allocating strings on every keystroke, reducing filtering overhead significantly.
