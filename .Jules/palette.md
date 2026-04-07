## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2026-04-07 - Icon-Only Button Accessibility
**Learning:** Some custom modal implementations (e.g., `AddRecipeToCurrentListModal`) were using icon-only close buttons lacking an `aria-label`.
**Action:** Consistently add `aria-label="Close"` to all icon-only close buttons. Avoid adding `aria_hidden=true` on leptos `<Icon>` components as it's not a supported prop and breaks compilation.
