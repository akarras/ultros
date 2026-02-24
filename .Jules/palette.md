## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2026-02-17 - Empty Search State Feedback
**Learning:** Virtualized lists (like `VirtualScroller`) often render nothing when empty, leaving users without confirmation that a search completed with zero results. This is confusing for all users and especially problematic for screen reader users who may think the application is broken or still loading.
**Action:** Always implement an explicit "No results found" state with `role="status"` whenever filtering or searching results, especially when the result list might be completely hidden when empty.
