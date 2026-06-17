## 2024-05-18 - Missing label associations and ARIA labels in Endpoint Forms
**Learning:** Found that `EndpointCreateForm` inputs lacked standard `id` and `for` associations, and icon-only buttons like "Delete endpoint" lacked `aria-label`. Standard accessible form practices seem to be occasionally missed in Leptos `view!` macros.
**Action:** Next time, remember to apply `id` and `for` attributes to `input` and `label` elements when creating or editing forms to ensure screen readers announce them properly. Always ensure `aria-label` is present on interactive elements that only contain icons.
## 2026-05-14 - Added aria-label to max_purchase_price filter button
**Learning:** Found that the button to remove the `max_purchase_price` filter chip in `analyzer.rs` was missing an `aria-label`, unlike the other filter removal buttons. This is an accessibility issue where screen readers wouldn't announce the purpose of the icon-only button.
**Action:** Always verify that dynamically generated icon-only buttons (like inside a loop or conditional rendering block) have appropriate `aria-label` attributes.
## 2026-05-21 - Prevent screen readers from reading decorative icons in buttons
**Learning:** Found that when buttons have an icon next to visible text, sometimes the icon isn't hidden with `aria_hidden=true`. While this isn't an error, adding `aria_hidden=true` to the icon (since the button already has visible text) makes the screen reader experience smoother by preventing it from reading the decorative icon.
**Action:** When inspecting buttons with both text and icons, consider adding `aria_hidden=true` to the icon component (if supported, e.g. the `Icon` component in this repo supports it) to avoid redundant or confusing announcements.
## 2026-06-17 - Toast Accessibility
**Learning:** Found that toast close buttons were missing keyboard focus indicators making them hard to use with keyboard navigation.
**Action:** Always add focus-visible ring styles (e.g., `focus-visible:ring-[var(--brand-ring)]`) to interactive elements like close buttons.
