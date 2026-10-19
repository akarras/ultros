## 2024-05-20 - Add ARIA label to Auto-mark Purchases input
**Learning:** Found an input field relying solely on `placeholder` without an `aria-label` or explicit `<label>`, which can be problematic for some screen readers. Reusing the translation key for the placeholder makes an excellent label.
**Action:** Added `aria-label=t_string!(i18n, list_auto_mark_character_name_placeholder)` to improve accessibility for screen readers.
