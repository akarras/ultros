## 2025-02-14 - Tooltip Accessibility Pattern
**Learning:** The `Tooltip` component wraps children but doesn't automatically associate the tooltip text with the trigger element (e.g., via `aria-describedby` or `title`). This means icon-only buttons inside tooltips remain inaccessible to screen readers unless `aria-label` is explicitly added to the button itself.
**Action:** When adding tooltips to icon-only buttons, ALWAYS add a descriptive `aria-label` to the button element. Consider enhancing `Tooltip` component in the future to handle this automatically if possible.
