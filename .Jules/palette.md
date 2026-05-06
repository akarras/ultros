## 2025-02-12 - Add ARIA label to AddRecipeToCurrentListModal Close Button
**Learning:** Icon-only close buttons in modals are common missing accessibility targets. In `AddRecipeToCurrentListModal` (`add_recipe_to_current_list.rs`), the close button (`btn-ghost`) had an icon but no textual label.
**Action:** Always verify icon-only action buttons (like modal closes or form clears) have an appropriate `aria-label` attribute (e.g., `aria-label="Close"`) to support screen readers, aligning with the `btn-ghost` styling pattern.
