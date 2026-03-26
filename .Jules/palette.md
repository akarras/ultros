## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2026-03-26 - Add ARIA label to AddRecipeToCurrentListModal close button
**Learning:** The AddRecipeToCurrentListModal close button had no screen reader text (it only had an X icon), making it difficult for visually impaired users to understand what the button did.
**Action:** Add `aria-label="Close modal"` to all modal close buttons and icon-only buttons during creation.
