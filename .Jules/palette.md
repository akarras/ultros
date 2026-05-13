## 2024-05-18 - Missing label associations and ARIA labels in Endpoint Forms
**Learning:** Found that `EndpointCreateForm` inputs lacked standard `id` and `for` associations, and icon-only buttons like "Delete endpoint" lacked `aria-label`. Standard accessible form practices seem to be occasionally missed in Leptos `view!` macros.
**Action:** Next time, remember to apply `id` and `for` attributes to `input` and `label` elements when creating or editing forms to ensure screen readers announce them properly. Always ensure `aria-label` is present on interactive elements that only contain icons.
