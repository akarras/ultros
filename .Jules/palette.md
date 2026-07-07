## 2023-11-20 - Accessible Icon-Only Buttons
**Learning:** Found that some icon-only interactive elements like 'Delete Group' and 'Add Member' in the groups component lacked `aria-label`s, making them invisible to screen readers.
**Action:** Always add reactive `aria-label` properties to icon-only buttons to ensure they are fully accessible, especially when their function might change based on state (e.g. asking for confirmation).

## 2026-07-07 - Accessible textareas
**Learning:** In the MakePlaceImporter, the textarea was missing an ID and the associated label lacked a `for` attribute, which breaks form field accessibility for screen readers.
**Action:** Ensure all form controls, such as `<textarea>` and `<input>`, have unique IDs and are properly associated with their corresponding `<label>` elements using the `for` attribute.
