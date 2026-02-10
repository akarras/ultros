# Bolt's Journal

## 2026-01-06 - Build Script Side Effects
**Learning:** Running `cargo test` in a repo with missing submodules (like `ffxiv-datamining`) can cause build scripts (like `xiv-gen/build.rs`) to fail silently or worse, generate empty configuration files (`extra.toml`), which then get staged.
**Action:** Always check `git status` carefully before committing, especially when working with repos that use code generation or submodules. Be wary of file changes in files you didn't explicitly edit.
