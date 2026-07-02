## 2023-11-20 - Accessible Icon-Only Buttons
**Learning:** Found that some icon-only interactive elements like 'Delete Group' and 'Add Member' in the groups component lacked `aria-label`s, making them invisible to screen readers.
**Action:** Always add reactive `aria-label` properties to icon-only buttons to ensure they are fully accessible, especially when their function might change based on state (e.g. asking for confirmation).
