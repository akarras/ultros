## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2026-05-04 - Added ARIA labels to list item row action buttons
**Learning:** Found that the action buttons in the list item row component (delete, edit, mark as acquired) lacked ARIA labels, making them inaccessible to screen readers since they only contained icons.
**Action:** Always verify that icon-only buttons have descriptive  attributes to ensure they are accessible. For buttons that toggle state (like edit/save), the aria-label should also dynamically reflect the current action.
## 2025-05-04 - Added ARIA labels to list item row action buttons
**Learning:** Found that the action buttons in the list item row component (delete, edit, mark as acquired) lacked ARIA labels, making them inaccessible to screen readers since they only contained icons.
**Action:** Always verify that icon-only buttons have descriptive `aria-label` attributes to ensure they are accessible. For buttons that toggle state (like edit/save), the aria-label should also dynamically reflect the current action.
