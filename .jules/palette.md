## 2026-01-30 - Accessibility in reusable input components
**Learning:** Reusable input components often lack `aria-invalid` states, relying only on visual cues like border colors. This makes error states invisible to screen reader users.
**Action:** Always add `aria-invalid` bound to the error state in custom input components, even if no explicit error message is displayed.
