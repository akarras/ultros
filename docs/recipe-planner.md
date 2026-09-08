# Recipe planner

`/recipe/:id` is a public batch planner linked from item recipe cards and Recipe
Analyzer rows. The analyzer continues to rank per-unit estimates; this page
calculates the spend for the actual quantity and whole market-board stacks.

## URL state

`quantity` is desired output (1–9999). `world` is the starting world;
`buy-scope=world|datacenter|region` defaults to datacenter. `route` selects
the shopping route: `home`, or the non-home world ids to visit, sorted and
comma-separated (`route=63,79`). Absent means the Best value card. A shared
route that no longer ranks is still computed and shown as a pinned card.
`visits=0|1|2|3|4` is the retired hop budget from older links; it is read as
"the best-ranked card with at most that many extra worlds" and dropped from
new share links. `unavailable=item:world,...` records "Not here" reports.
Datacenter/region item-page links resolve a starting world within that scope.
`require-hq` strictly filters HQ-capable ingredient purchases; `output-hq`
separately controls the finished-item comparison.

`craft=itemId:recipeId,...` chooses an explicit recipe for each intermediate;
absent entries are bought. `owned=itemId:quantity,...` records on-hand materials.
`shards-exclude=true` excludes crystals (ItemSearchCategory 58, via
`CRYSTAL_SEARCH_CATEGORY`) throughout the graph; when the key is absent the
page follows the shared craft-options cookie, which excludes them by default.
Sharing the URL shares these manually entered owned quantities as well as the
craft choices.
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
are marked approximate.

Route cards are the travel frontier. Each candidate plan has a travel shape
(datacenter hops and world hops beyond the worlds already on the itinerary)
and a distance, the gil-equivalent travel cost from the craft-options cookie
(`world_hop_gil`, default 2,000; `dc_hop_gil`, default 10,000; adjustable in
Planner settings). The engine keeps the best plan per shape, sorts shapes by
distance, and keeps only cards that strictly improve on the card to their
left (fewer missing units, or equal missing and less gil). "Stay home" (the
no-new-travel plan) is therefore always the first card, and because the full
scope is always evaluated and more worlds never cost more gil, the cheapest
plan found is always the last card, however many hops it takes. At most five
cards are shown; longer frontiers keep the first and last cards and the steps
with the largest marginal saving. The card `rank` `(missing, effective, cost,
worlds)` prefers is badged **Best value** and is the default selection; the
last card is badged **Cheapest**. The search evaluates every single-world
addition exhaustively, then a beam of five promising sets for larger routes,
plus whole-datacenter and full-scope seeds, so the UI labels routes
**best-found**, not globally optimal.
Complete supply ranks ahead of partial supply; partial totals remain visibly
incomplete. Travel is weighted in gil, not modeled as time or teleport fees,
and vendor stops are not counted. Price age comes from ingest timestamps, not
retainer listing timestamps.

Ticking an itinerary line locks that listing as a committed purchase: it stays
in the plan at the ticked price, counts against the need, and its world is
free to revisit, while everything else may re-plan around it. A tick is
client-only (not part of the shared link) and is dropped only when its item
stops being something to buy. "Not here" excludes that `(item, world)` pair,
unticks any lock on it, and re-plans; reports can be removed individually or
cleared.

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
