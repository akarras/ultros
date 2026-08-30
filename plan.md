1. **Analyze `ultros-frontend/ultros-app/src/components/search_box.rs`**:
   - The `<button>` around line 405 (search box clear tooltip) lacks `type="button"`.
   - The `<button>` around line 443 (JOB_EXAMPLES) lacks `type="button"`.
   - Update these buttons to include `type="button"` to ensure proper semantic meaning and prevent accidental form submissions if placed inside forms.

2. **Check other components**:
   - Check `ultros-frontend/ultros-app/src/components/add_recipe_to_list.rs`, `<button>` at line 35 lacks `type="button"`.
   - Look for other icon-only buttons missing `aria-label` or `type="button"` and update appropriately based on context.

3. **Pre-commit Checks**:
   - Run verification via cargo commands (e.g. `cargo clippy`, `cargo fmt`).
