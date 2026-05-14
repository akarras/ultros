## 2024-05-18 - Missing label associations and ARIA labels in Endpoint Forms
**Learning:** Found that `EndpointCreateForm` inputs lacked standard `id` and `for` associations, and icon-only buttons like "Delete endpoint" lacked `aria-label`. Standard accessible form practices seem to be occasionally missed in Leptos `view!` macros.
**Action:** Next time, remember to apply `id` and `for` attributes to `input` and `label` elements when creating or editing forms to ensure screen readers announce them properly. Always ensure `aria-label` is present on interactive elements that only contain icons.
## 2026-05-14 - Added aria-label to max_purchase_price filter button
**Learning:** Found that the button to remove the `max_purchase_price` filter chip in `analyzer.rs` was missing an `aria-label`, unlike the other filter removal buttons. This is an accessibility issue where screen readers wouldn't announce the purpose of the icon-only button.
**Action:** Always verify that dynamically generated icon-only buttons (like inside a loop or conditional rendering block) have appropriate `aria-label` attributes.
