## 2024-04-12 - [Leptos For Loop Keys]
**Learning:** Using computed values like timestamps as keys in Leptos `<For>` components causes unnecessary computation on every render cycle and risks key collision if times overlap.
**Action:** Always use unique, direct properties like database IDs as keys for list items to avoid reconciliation overhead.
