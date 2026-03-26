
## 2024-05-20 - [RangeInclusive<NaiveDateTime> Optimization]
**Learning:** Found unnecessary cloning of `RangeInclusive<NaiveDateTime>` during the `SalesWindow::try_new` and `find_date_range` logic. Although small, passing ranges by reference avoids copying.
**Action:** When filtering or processing dates with `RangeInclusive`, pass them by reference.
