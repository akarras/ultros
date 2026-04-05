## 2025-04-05 - Optimize eager cloning before take/filter
**Learning:** Found instances where large `Vec`s were completely cloned before an `.into_iter().take()` or `.filter()`, causing O(n) memory allocation and copy overhead when only a subset was needed.
**Action:** When filtering or taking from a slice/Vec that is owned by a closure and needs to be returned as an iterator, prefer `.iter().take().cloned().collect()` or `.iter().filter().cloned().collect()` instead of `.clone().into_iter().take()` to minimize allocation and copying overhead.
