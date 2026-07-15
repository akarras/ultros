## 2023-11-20 - Accessible Icon-Only Buttons
**Learning:** Found that some icon-only interactive elements like 'Delete Group' and 'Add Member' in the groups component lacked `aria-label`s, making them invisible to screen readers.
**Action:** Always add reactive `aria-label` properties to icon-only buttons to ensure they are fully accessible, especially when their function might change based on state (e.g. asking for confirmation).

## 2026-07-07 - Accessible textareas
**Learning:** In the MakePlaceImporter, the textarea was missing an ID and the associated label lacked a `for` attribute, which breaks form field accessibility for screen readers.
**Action:** Ensure all form controls, such as `<textarea>` and `<input>`, have unique IDs and are properly associated with their corresponding `<label>` elements using the `for` attribute.

## 2026-07-28 - Explicit aria-label for Image-only Menu Buttons
**Learning:** Found that interactive elements containing only images with alt text might still need an explicit `aria-label` if the image's alt text doesn't adequately describe the element's action (e.g. 'username' vs 'Account menu button').
**Action:** Always add an explicit `aria-label` to avatar dropdown buttons to standardize the action's description across login states.
## 2026-07-15 - Added ARIA label to resend button
**Learning:** Screen readers will struggle to identify multiple identical buttons (like "Resend") across rows in a data table unless they have unique labels with context.
**Action:** Always include row-specific context (like the item name) in the `aria-label` for buttons inside list/table rows to ensure accessibility.
