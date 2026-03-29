## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2026-01-15 - ARIA Labels for Generic Select Components
**Learning:** Generic reusable UI components like `Select` (used in `WorldPicker`) often omit specific `aria-label`s on their inner inputs (`role="combobox"`), breaking accessibility because the wrapper components do not pass down the appropriate context.
**Action:** Always expose an `aria_label` prop on generic UI components (like `Select`) and pass context-specific labels from the parent components (e.g. `WorldPicker`) so that screen readers correctly announce the purpose of the input.