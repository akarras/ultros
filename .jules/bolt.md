## 2024-05-23 - [Build Script Side Effects]
**Learning:** `xiv-gen/build.rs` modifies `xiv-gen/extra.toml` based on available data. In a broken environment (missing submodules), this file gets overwritten with empty config, causing unexpected "Changes to be committed".
**Action:** Always check `git status` for unintended file changes after running `cargo check` or builds, especially when build scripts are involved. Revert strictly generated files if they shouldn't be committed.

## 2024-05-23 - [Leptos Key Optimization]
**Learning:** `SaleHistoryTable` used derived `timestamp()` as a key. This forces per-item conversion on every render.
**Action:** Use available unique IDs (primary keys) for `<For />` keys to avoid computation and ensure stability.
