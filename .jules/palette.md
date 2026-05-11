## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2024-05-15 - Empty State for SearchBox
**Learning:** Virtual Scroller components without built-in empty states will simply show a blank box when there are no items to render. This happens silently and can be confusing to users who type a search query and see nothing happen.
**Action:** Always add an explicit `<Show>` or conditional render below the list or VirtualScroller to handle the `!loading && !query.is_empty() && results.is_empty()` state with a helpful message.
