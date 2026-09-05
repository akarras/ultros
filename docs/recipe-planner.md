# Recipe planner

`/recipe/:id` is a public batch planner linked from item recipe cards and Recipe
Analyzer rows. The analyzer continues to rank per-unit estimates; this page
calculates the spend for the actual quantity and whole market-board stacks.

## URL state

`quantity` is desired output (1–9999). `world` is the starting world;
`buy-scope=world|datacenter|region` defaults to datacenter. `visits=0|1|2|3|4`
selects home, up to one/two/three additional worlds, or full scope. The default
is full scope. Datacenter/region item-page links resolve a starting world within
that scope. `require-hq` strictly filters HQ-capable ingredient purchases;
`output-hq` separately controls the finished-item comparison.

`craft=itemId:recipeId,...` chooses an explicit recipe for each intermediate;
absent entries are bought. `owned=itemId:quantity,...` records on-hand materials.
`shards-exclude=true` excludes crystals throughout the graph. Sharing the URL
shares these manually entered owned quantities as well as the craft choices.
All planning works without authentication. Saving adds only outstanding leaf
material quantities to an existing list, retaining ingredient quality choices.

## Calculation guarantees and limits

The dependency graph is topologically ordered. All parents contribute demand
before a shared intermediate is rounded to a whole number of crafts. Inventory
is applied once per item; intermediate surplus and purchased stack surplus are
reported without assuming resale proceeds. Craft choices are global per item.
Cycles, more than 128 materials, excessive depth and quantity overflow fail
explicitly. Zero supply is a shortage, not a zero-cost ingredient.

Purchases use a bounded 0/1 knapsack over complete listings. NPC gil prices can
fill remaining demand, assuming the player has vendor access. Large cases
(more than 10,000 required units or 200,000 quantity/listing combinations) use
the cheaper complete result of unit-price and stack-price greedy candidates and
are marked approximate. The route search examines every single-world option,
then retains a beam of four promising routes for two/three-world options. The
UI therefore labels comparisons **best-found**, not globally optimal. Complete
supply ranks ahead of partial supply; partial totals remain visibly incomplete.
Travel time, teleport fees, vendor stops and actual datacenter travel eligibility
are not modeled as market-world visits. Price age comes from ingest timestamps,
not retainer listing timestamps. A refresh replaces the market snapshot and
clears purchase ticks when the selected plan changes.

The deterministic engine is `ultros-app/src/recipe_planner.rs`; it has no UI or
network dependencies. API requests are client-side, keyed by scope and selected
leaf IDs, with at most four requests in flight. A quantity-only change reuses
the market snapshot. Reversing the dependency order yields crafting instructions.

## Validation

Run `./check_ci.sh`, `cargo leptos build`, and `./scripts/run_e2e.sh` using this
worktree's own server. The focused `npm --prefix integration run test:recipe-planner`
probe uses deterministic market fixtures, checks SSR/hydration and shared URLs,
exercises subcraft/owned/quantity changes, and captures desktop/mobile layouts.

For local Windows builds, use `cargo leptos build --bin-features test-auth`
and `LEPTOS_FEATURES=test-auth` for E2E; the default jemalloc feature does not
build with MSVC. Give parallel test servers their own `METRICS_PORT` as well
as their own web port. The recipe probe supplies market fixtures, while the
broader authenticated suite still requires healthy PostgreSQL and ClickHouse
services.
