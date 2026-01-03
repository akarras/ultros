## 2024-05-23 - Search Input Clear Button
**Learning:** Search inputs without a clear button force users to manually delete text, which is tedious. Adding a dedicated "X" button when text is present significantly improves usability and is a standard pattern users expect.
**Action:** Always include a conditional "Clear" button in search inputs that resets the value and maintains focus.

## 2025-02-28 - Filter Links and Accessibility
**Learning:** Filter toggles implemented as `<a>` tags (modifying query params) often lack semantic state indicators for screen readers. Simply adding an `.active` class is insufficient.
**Action:** Use `aria-current="true"` on filter links that represent the currently active view or state within a set.

## 2025-05-23 - Tooltip Keyboard Accessibility
**Learning:** Tooltips that are triggered only by `mouseenter` events are completely inaccessible to keyboard users. Wrapping the interactive element in a container that handles both hover and focus events ensures tooltips are visible to all users.
**Action:** Always implement tooltips with both `mouseenter`/`mouseleave` AND `focusin`/`focusout` handlers.
