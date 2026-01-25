## 2025-01-24 - Canvas Charts Accessibility
**Learning:** Charts rendered via `<canvas>` (using plotters) in this app lack default accessibility attributes, making them invisible to screen readers.
**Action:** Always check `<canvas>` elements for `role="img"` and `aria-label` or fallback content when reviewing chart components.
