## 2023-11-20 - Accessible Icon-Only Buttons
**Learning:** Found that some icon-only interactive elements like 'Delete Group' and 'Add Member' in the groups component lacked `aria-label`s, making them invisible to screen readers.
**Action:** Always add reactive `aria-label` properties to icon-only buttons to ensure they are fully accessible, especially when their function might change based on state (e.g. asking for confirmation).

## 2026-07-07 - Accessible textareas
**Learning:** In the MakePlaceImporter, the textarea was missing an ID and the associated label lacked a `for` attribute, which breaks form field accessibility for screen readers.
**Action:** Ensure all form controls, such as `<textarea>` and `<input>`, have unique IDs and are properly associated with their corresponding `<label>` elements using the `for` attribute.

## 2026-07-28 - Explicit aria-label for Image-only Menu Buttons
**Learning:** Found that interactive elements containing only images with alt text might still need an explicit `aria-label` if the image's alt text doesn't adequately describe the element's action (e.g. 'username' vs 'Account menu button').
**Action:** Always add an explicit `aria-label` to avatar dropdown buttons to standardize the action's description across login states.
## 2025-02-12 - Added ARIA label to resend button
**Learning:** Screen readers will struggle to identify multiple identical buttons (like "Resend") across rows in a data table unless they have unique labels with context.
**Action:** Always include row-specific context (like the item name) in the `aria-label` for buttons inside list/table rows to ensure accessibility.
## 2024-07-23 - Add explicit input associations
**Learning:** In Leptos, when defining `for` and `id` attributes that need dynamic values within `move ||` closures, ensure you do not inadvertently move an entire struct (like `group`) multiple times, which causes E0382. Instead, extract the required value (e.g. `let group_id = group.id;`) beforehand so it can be copied into the closures.
**Action:** Always extract and copy small values before using them inside Leptos closures to avoid ownership issues.
## 2026-08-12 - Confirm Action Aria-Live
**Learning:** Adding `aria-live="polite"` to a button that changes text to confirm an action (e.g. from "Clear All" to "Confirm Clear") is an easy accessibility win to notify screen reader users of the new state. However, putting `aria-live` directly on the button can sometimes be flaky across different screen readers, but it's an acceptable micro-UX improvement for an inline confirmation pattern.
**Action:** Use `aria-live` regions or visually hidden elements for more complex state changes, but inline `aria-live="polite"` on a changing button is a quick enhancement for simple confirm interactions.
