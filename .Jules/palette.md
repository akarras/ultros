## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2026-01-26 - Icon-only buttons accessibility
**Learning:** Multiple icon-only buttons in modals and lists (e.g., Delete, Edit/Save, Close) were missing `aria-label` attributes, relying only on visual icons which is insufficient for screen readers.
**Action:** Enforce `aria-label` on all `<button>` elements that only contain `<Icon>` components during creation or refactor. Use reactive closures (`move || if condition { "A" } else { "B" }`) for buttons that change state.
