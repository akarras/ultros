
## 2024-05-19 - Precomputing Lowercase Labels in Select Combobox
**Learning:** In Leptos, computing `.to_lowercase()` directly inside a `Memo` that triggers on user input keystrokes causes unnecessary String allocations on the main thread for every item in the list, creating a micro-stutter for large dropdowns.
**Action:** When creating searchable dropdowns or filterable lists, pre-compute the lowercased search keys once in a separate reactive primitive (like a `Memo` watching the item list), rather than inside the search input's filter loop.
