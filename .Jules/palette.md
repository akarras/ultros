## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.


## 2024-05-18 - Missing Accessibility on Icon-only Buttons in Lists
**Learning:** Common list components using icon-only buttons (like delete, edit, save) often miss proper `aria-labels` and visual hints (tooltips), reducing accessibility and clarity.
**Action:** Always wrap icon-only action buttons in a `Tooltip` and provide a descriptive `aria-label` attribute (or a dynamically derived one when the functionality toggles) to ensure robust screen-reader and visual support.
