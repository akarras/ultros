## 2026-01-28 - Generic Input Components Accessibility
**Learning:** The `ParseableInputBox` component was generic but lacked accessibility props (`aria-label`, `aria-invalid`), causing all usages (filters) to be inaccessible.
**Action:** When creating generic input wrappers, always include optional `aria_label` and bind error states to `aria-invalid`.
