## 2024-01-06 - Accessibility: Programmatic Validation State
**Learning:** Visual error states (like red borders) are insufficient for screen readers. Using `aria-invalid` tied to the validation logic ensures all users know when an input is rejected.
**Action:** Always pair visual validation cues with `aria-invalid` or `aria-errormessage` attributes.
