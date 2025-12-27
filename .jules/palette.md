## 2024-05-23 - Search Input Clear Button
**Learning:** Search inputs without a clear button force users to manually delete text, which is tedious. Adding a dedicated "X" button when text is present significantly improves usability and is a standard pattern users expect.
**Action:** Always include a conditional "Clear" button in search inputs that resets the value and maintains focus.

## 2025-02-28 - Filter Links and Accessibility
**Learning:** Filter toggles implemented as `<a>` tags (modifying query params) often lack semantic state indicators for screen readers. Simply adding an `.active` class is insufficient.
**Action:** Use `aria-current="true"` on filter links that represent the currently active view or state within a set.

## 2025-05-22 - Tooltip Focus Accessibility
**Learning:** Tooltips that only trigger on `mouseenter`/`mouseleave` are inaccessible to keyboard users. When a tooltip wraps an interactive element (like a button), it must also respond to `focusin` and `focusout` events to appear when the user tabs to the element.
**Action:** Always add `on:focusin` and `on:focusout` handlers to tooltip wrapper components to ensure they are accessible via keyboard navigation.
