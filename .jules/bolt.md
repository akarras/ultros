## 2024-04-28 - Initial entry
**Learning:** Starting analysis
**Action:** Profile the application
## 2024-04-28 - Virtual Scroller Analysis
**Learning:** Investigating leptos performance
**Action:** Found virtual scroller, checking if it handles rendering properly
## 2024-04-28 - Sorting Optimizations
**Learning:** Found multiple usages of  across the frontend
**Action:** Replaced  with  which is faster and allocates less memory, as stable sorting is not required for these cases (e.g. price per unit).
## 2024-04-28 - Sorting Optimizations
**Learning:** Found multiple usages of `sort_by_key` across the frontend
**Action:** Replaced `sort_by_key` with `sort_unstable_by_key` which is faster and allocates less memory, as stable sorting is not required for these cases (e.g. price per unit).
