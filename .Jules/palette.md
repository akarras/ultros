## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2025-01-09 - Add ARIA Labels to List Item Row Buttons
**Learning:** Found multiple icon-only buttons in the `ListItemRow` component that were missing ARIA labels, making them inaccessible to screen readers.
**Action:** Added `aria-label` attributes to the parent `<button>` elements and `aria_hidden=true` to the inner `<Icon>` components to provide clear context without redundant reading.
