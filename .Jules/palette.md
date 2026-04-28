## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2026-02-28 - ARIA labels for dynamic list item buttons
**Learning:** Leptos components with dynamic state (like toggleable edit modes on list items) benefit greatly from reactive closures for ARIA labels (`aria-label=move || if edit() { "Save edit" } else { "Edit item" }`), ensuring screen reader context stays perfectly in sync with the visual icon swap.
**Action:** Always use reactive closures for accessibility attributes on buttons whose icons or purposes change based on local component state.
