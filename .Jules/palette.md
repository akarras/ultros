## 2024-05-22 - [Accessibilty for Dynamic Input Lists]
**Learning:** When using iterating lists of inputs (like ingredients), standard `label for` doesn't scale well without unique ID generation. `aria-label` is a cleaner solution for list items where context (visual proximity to item name) implies meaning but screen readers need explicit association.
**Action:** Use `aria-label` with dynamic text (e.g. `format!("Quantity for {}", item_name)`) for inputs in repeating lists to ensure accessibility without complex ID management.
