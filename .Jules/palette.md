## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2026-04-22 - Added missing ARIA labels to icon buttons
**Learning:** Icon-only buttons using Leptos components often lack accessible names for screen readers. There are several instances in the app (like 'Refresh' in LiveSaleTicker, 'Delete/Edit' in ListItemRow) that only use an inner `<Icon />` component without any text or aria-label attributes.
**Action:** Always verify that buttons containing only icons have a descriptive `aria-label` added so they are accessible to keyboard and screen reader users.
