## 2024-05-23 - Accessibility in Modals with Leptos
**Learning:** Generic `Modal` components often miss `aria-labelledby` because the title is rendered by the parent component (the consumer of Modal).
**Action:** Add an optional `title_id` prop to the `Modal` component and apply `aria-labelledby` to the dialog container. When using `view!` macro in Leptos, ensure `title_id` (if it's a String) is cloned if used multiple times (e.g. in prop and child ID) to avoid ownership issues in the closure.
