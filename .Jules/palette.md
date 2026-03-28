## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2025-01-20 - Icon-Only Button Accessibility
**Learning:** Icon-only action buttons (such as edit, delete, mark acquired, or close modal) are frequently missing `aria-label`s across various components, relying purely on visual cues which makes them inaccessible to screen readers.
**Action:** Always check for and enforce descriptive `aria-label`s on any button where the only child is an `<Icon />` component.
