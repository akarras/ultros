# Ultros grid core

This crate owns typed metric filters, stable row sorting, partial-data query
results, saved-layout encoding, column placement, and visible row/column ranges.
It depends only on `serde` and `serde_json` and has no rendering-mode features.

The implementations and their existing tests move unchanged from the app's
`components::virtual_grid::{layout, metrics}` modules. The app re-exports those
paths and retains the rendered grid, reactive row sources, URL navigation,
storage, and browser measurements.

View edits can reuse the compiled grid algorithms. Changes to these algorithms
still rebuild their consumers, but their unit tests can run without compiling
Leptos or loading game data. Generic query code may still be instantiated in the
app; this extraction does not establish a percentage improvement in build time.

Validation:

```sh
cargo test --locked -p ultros-grid-core
cargo tree --locked -p ultros-grid-core
./check_ci.sh
cargo leptos build
```

The browser regression probes are `integration/virtual-grid.cjs` and
`integration/shared-analyzer-data.cjs`. Run them against a fresh build of the
same worktree using `BASE_URL`; their debug fixture routes need no market data.
