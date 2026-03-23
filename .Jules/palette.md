## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.
## 2024-05-19 - Modal Forms Accessibility Pattern
**Learning:** Multiple custom modal forms, such as `AddRecipeToCurrentListModal` and `AddToListModal`, were missing `aria-label`s on icon-only close buttons, and their internal input fields (like search and quantity) lacked proper association with descriptive labels or `aria-label`s.
**Action:** Enforce `aria-label`s on all icon-only buttons (like `BsX`), and always associate `<label>` elements with their corresponding `<input>` using `id` and `for` attributes, or use `aria-label` on `<input>` fields that lack a visible label.
