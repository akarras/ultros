## 2026-01-15 - Table Accessibility Pattern
**Learning:** Multiple data tables (`ListingsTable`, `SaleHistoryTable`) were missing semantic `<thead>` wrappers and `scope` attributes, relying on browser auto-correction which is insufficient for accessibility.
**Action:** Enforce `<thead>` and `scope="col"`/`scope="row"` in all table components during creation or refactor.

## 2025-04-05 - Filter Chips Missing ARIA Labels
**Learning:** Icon-only close buttons inside filter chips (e.g., in `AnalyzerTable`) lacked `aria-label` attributes, making them unreadable to screen readers.
**Action:** Always ensure icon-only buttons have descriptive `aria-label`s, especially in dynamic lists of filters or tags.
