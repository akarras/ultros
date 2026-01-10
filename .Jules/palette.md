# Palette's Journal

## 2025-02-23 - Focus styles on dynamic alerts
**Learning:** Toast notifications often use semantic colors (success=green, error=red) for text/border. Standard "brand" focus rings can clash or look out of place.
**Action:** Use `focus:ring-current` to inherit the text color for the focus ring, ensuring the focus indicator matches the semantic context of the alert (e.g., green ring on success toast).
