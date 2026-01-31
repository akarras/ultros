## 2024-05-22 - [Refactoring Reactive Memos in Leptos]
**Learning:** Extracting expensive operations (like `get_computed_style`) into a separate `Memo` that depends only on relevant signals (like theme) prevents unnecessary re-computation when other signals (like width/height) change. This is critical for performance during frequent updates like resizing.
**Action:** Always verify if a `Memo` or `Effect` depends on signals that change frequently and if expensive operations inside it can be isolated.
