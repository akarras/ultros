## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2026-04-12 - Add aria-label to close button
**Learning:** Found an icon-only close button lacking an `aria-label` in a Leptos modal component (`AddRecipeToCurrentListModal`). This pattern (icon inside a button without screen reader text) is a common accessibility issue.
**Action:** Always ensure icon-only buttons have an `aria-label` or `title` to provide context for screen readers. Added `aria-label="Close modal"` to the specific button.
