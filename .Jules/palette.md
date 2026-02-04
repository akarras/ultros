## 2024-05-23 - Select Component Accessibility
**Learning:** Reusable components like `Select` often miss basic ARIA roles because they are generic. Adding `role="combobox"`, `role="listbox"`, and `role="option"` significantly improves screen reader experience without changing the visual design.
**Action:** When creating or refactoring generic UI components, always check if they map to a standard ARIA pattern (like Combobox or Menu) and apply roles accordingly.
