# Ultros crafting calculations

This crate owns ingredient costs, inventory consumption, recursive subcraft
selection, cost breakdowns, and travel-cost comparisons. Callers supply prices,
recipes, inventory, vendor prices, shard classification, and world mappings.
The calculation bodies and their existing tests move from `ultros-app`.

It depends on `ultros-calc` and the `item` and `recipe` types from `xiv-gen`.
It has no Leptos dependency, does not load game-data packs, and does not read
locale or application state. Keeping it separate also keeps `ultros-calc`
independent of the game-data generator.

The app retains its existing `crafting_cost` and `analyzer_kit::hop` import paths
through re-exports. The vendor-price cache stays in the app because it reads
locale-aware game data. Its convenience constructor stays there as the free
function `item_page_default`; its only caller was, and remains, its regression
test. Production callers continue supplying explicit `CraftingCostOptions`.

All 37 existing tests remain: 36 calculation tests here and the app-default test
in the app. Calculation tests can run without compiling application views or
loading game-data packs. Generic price lookups may still be instantiated by
callers; this separation alone does not establish a percentage speedup.

Validation:

```sh
cargo test --locked -p ultros-crafting
cargo tree --locked -p ultros-crafting
./check_ci.sh
cargo leptos build
```

The focused browser probes are `integration/recipe-planner.cjs` and
`integration/shared-analyzer-data.cjs`. Run them against a fresh build of the
same worktree via `BASE_URL`. Analyzer reloads also need the server's market
cache to be ready, even when client-side API fixtures are enabled.
