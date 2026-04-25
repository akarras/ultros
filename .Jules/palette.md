## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2026-04-25 - ARIA Labels for Icon Buttons
**Learning:** Icon-only buttons for actions like Delete, Edit, and Mark as acquired were missing accessible names, making them unreadable to screen readers.
**Action:** Always include an `aria-label` attribute on `<button>` elements that only contain an `<Icon>`. If the button state changes (e.g. from Edit to Save), the `aria-label` should be dynamic (`aria-label=move || if state() { ... } else { ... }`).
