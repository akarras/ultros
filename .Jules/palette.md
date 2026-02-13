## 2026-01-06 - Handling Click Events for Keyboard Accessibility
**Learning:** When converting `div` click handlers to `button` for accessibility, keyboard activation (Enter/Space) fires a `click` event with `client_x` and `client_y` as 0. This breaks logic depending on mouse coordinates (like spawning effects).
**Action:** Always check for (0,0) coordinates in click handlers and provide a fallback (e.g., center of element or screen) for keyboard users.
