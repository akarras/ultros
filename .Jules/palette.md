## 2025-01-06 - [Gil Party Accessibility]
**Learning:** Keyboard-triggered click events often report (0,0) coordinates, which can cause visual effects to appear in the wrong place (top-left corner).
**Action:** When handling click events that rely on coordinates, always check for (0,0) and provide a sensible fallback (e.g., center of screen or element) for keyboard users.
