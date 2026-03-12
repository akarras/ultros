## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2024-05-15 - [Add missing input labels in AddRecipeToListModal]
**Learning:** Found an accessibility issue pattern where inputs inside loops (`<For>`) or inputs linked to preceding labels lacked `for`/`id` pairings or `aria-label` attributes in Leptos views. When fixing this, `format!` should be evaluated outside of `move ||` closures unless reactivity is needed. If used multiple times in a macro, string `.clone()` should be utilized to avoid `use of moved value` build errors in `view!` macro.
**Action:** When adding IDs or ARIA attributes in Leptos `view!` macros, assign the formatted string to a variable outside the closure (using `.clone()` when used multiple times) rather than inlining it inside a reactive `move ||` closure unless it genuinely needs to be reactive.
