## 2024-05-23 - Accessibility in Leptos Input Components
**Learning:** Generic input components in Leptos (like `ParseableInputBox`) often lack `aria-*` attributes by default. Using `#[prop(optional)]` allows adding them without breaking existing call sites.
**Action:** Always check custom input wrappers for `aria-label`, `id`, and `aria-invalid` support.
