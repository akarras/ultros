## 2026-05-23 - Optimize WorldHelper Lookups
**Learning:** `WorldHelper` is a shared struct used in both backend and WASM frontend. It was performing O(N) lookups for World/Datacenter/Region resolution by ID using nested iterators. This is expensive when rendering tables (e.g. `ListingsTable`) where this lookup happens for every row.
**Action:** Replaced O(N) scan with O(1) `HashMap` lookups by building an index on initialization.
**Gotcha:** `WorldHelper` derives `Deserialize`. Adding `#[serde(skip)]` fields (the maps) causes them to be empty on deserialization, breaking the struct.
**Solution:** Implemented `Deserialize` manually to read the inner data and rebuild the indices via `From<WorldData>`.
