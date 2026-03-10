
## 2024-05-23 - [Accessibility] Dynamic ARIA Roles for Toasts
**Learning:**  is assertive and interrupts screen readers. Use  for non-critical notifications (success, info) to be polite.
**Action:** When implementing toast/notification systems, always distinguish between alert (error/warning) and status (info/success) roles.
## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

