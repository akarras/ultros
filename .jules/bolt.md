## 2024-05-22 - xiv-gen build script issue
**Learning:** `xiv-gen/build.rs` overwrites `extra.toml` based on `ffxiv-datamining/csv` content. If the CSVs are in subdirectories (which they seem to be), the build script fails to find them and empties `extra.toml`.
**Action:** Be careful when running `cargo check` in this repo as it might destructively modify `xiv-gen/extra.toml`. Always check `git status` and restore `extra.toml` if unintended changes occur.
