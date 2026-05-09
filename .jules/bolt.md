## 2026-03-02 - [Iterate before Cloning for Large Vectors]
**Learning:** `data.clone().into_iter().take(6)` copies the entire vector before throwing away everything after the 6th element. `data.iter().take(6).cloned()` iterates first, then clones, saving an `O(N)` clone, but must be `.collect::<Vec<_>>()`ed in Leptos to avoid closure lifetime issues.
**Action:** Be mindful of the order of operations when taking a slice of a larger vector, and ensure `collect()` is used to break lifetimes.
