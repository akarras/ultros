## 2024-05-22 - xiv-gen CSV Parsing Strategy
**Learning:** `xiv-gen` dynamically chooses between `dumb_csv` (custom deserializer) and `read_csv` (serde-based) depending on the number of fields (>100 uses `dumb_csv`). `read_csv` unconditionally reads the entire file to a string for error reporting, which is a major performance bottleneck for smaller tables.
**Action:** In future optimizations, target `read_csv` in `xiv-gen/src/csv_to_bincode.rs` to lazy-load the file content only on error.
