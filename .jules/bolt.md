# Bolt's Journal

## 2026-01-06 - Optimizing dumb-csv deserialization
**Learning:** Reusing `StringRecord` in `csv` crate deserialization loop can significantly reduce allocations and improve performance (observed ~20% speedup on 500k rows).
**Action:** Always check if `csv` reader loops are allocating per row (`rdr.records()`) and switch to `read_record` with a reused buffer when possible.
