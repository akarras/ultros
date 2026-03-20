## 2025-03-20 - [Optimize slice cloning in tables]
**Learning:** Tables often slice large vectors using `.iter().take(n).cloned().collect()` to truncate items, which dynamically allocates and loops over clones.
**Action:** Replaced `.iter().take(n).cloned().collect()` with `.to_vec()` on a slice (e.g. `listings[..10.min(listings.len())].to_vec()`) to efficiently bulk copy memory.
