# Ultros calculations

This crate owns sales statistics and price estimates, outlier filtering,
profit formulas, lazy price lookups, analyzer data requirements, and recipe
batch and shopping-route planning. It depends only
on `chrono` and the API data types, with no Leptos or rendering-mode features.

Application view edits can reuse these compiled calculations. Calculation edits
still rebuild the crate and its consumers, but its unit tests can run without
compiling the app. The existing tests move with their implementations.

The app keeps its current module paths through re-exports. Duration formatting,
badge classes, and percentage colours remain in the app, alongside their tests.
Reactive sale-statistics state, fetching, game-data access, and analyzer pages
also remain in the app. The calculation crate describes which data an analyzer
needs and how to combine its price sources; the app owns fetching and signals.

Validation:

```sh
cargo test --locked -p ultros-calc
cargo tree --locked -p ultros-calc
./check_ci.sh
cargo leptos build
```

For cache verification, warm the browser build, then rebuild after an app-only
edit with `cargo leptos build --frontend-only --lib-cargo-args=-vv`.
`ultros-calc` should remain fresh while the app recompiles. Use the same machine,
features, and profiles for timing comparisons; crate separation alone does not
establish a percentage improvement in build time.
