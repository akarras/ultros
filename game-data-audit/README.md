# Game-data audit

A read-only CLI for measuring the existing game-data archive before changing
what the browser receives. It does not rebuild packs, modify schemas, change
caching, or introduce lazy routes.

```sh
# Inventory every table and column quickly. Input is an actual pack, not an
# LFS pointer. Select brotli for packs from 081f770 onward, zlib for older packs.
cargo run --release --locked -p game-data-audit -- \
  --pack data/xiv-db/en.rkyv --encoding brotli --columns \
  --output target/game-data-inventory.json

# Measure whole-pack candidate reductions with production compression settings.
cargo run --release --locked -p game-data-audit -- \
  --pack data/xiv-db/en.rkyv --encoding brotli --quality 11 \
  --experiment all --output target/game-data-experiments.json

# Focus the detailed column inventory on one table; the baseline/experiments
# still measure the complete pack.
cargo run --release --locked -p game-data-audit -- \
  --pack data/xiv-db/en.rkyv --encoding brotli --table items --columns

cargo test --locked -p game-data-audit
```

Quality 5 is the fast exploratory default. Quality 11 with a 24-bit window
matches the compression settings introduced in 081f770; it can take minutes.
Always compare candidates to the report's **reserialized baseline**, not to
the input file size. In particular, the zlib input option still produces
Brotli size measurements. The input must match this checkout's `xiv_gen::Data`
schema; validation rejects incompatible archives.

## What the numbers mean

- `table`: the existing HashMap serialized and compressed independently.
  `records` counts map entries (shop groups for maps of subrow vectors).
- `column_sample`: a sorted vector of `(map key, column value)` pairs. For a
  table with subrows, each subrow contributes one sample and keeps its order.
  These are useful for ranking payloads. They **include repeated keys** and
  do not preserve the original row layout, so they are not removal savings.
- `experiment`: changes an in-memory clone, then serializes and compresses the
  **entire** resulting `Data`. `saved_brotli_bytes` is a signed difference
  against the complete baseline. A negative number is a size increase, not
  an error. Independent deltas and independently compressed tables/columns
  must not be added together.

The JSON records the input SHA-256, schema source SHA-256, input encoding,
compression settings, and interpretation of every experiment. Keep those with
any claimed savings. Hashing the schema source is provenance, not a wire-format
compatibility version.

`build.rs` parses `xiv-gen/src/lib.rs` with `syn` and generates **typed field
accesses** for every Data table and every row column. This is an inventory of
what is stored, not a compiler analysis of which application code reads it.
New fields automatically join the inventory. Unsupported schema shapes fail
the build; the table-coverage test also compares the inventory with serde's
representation. No hand-maintained table list can silently become stale.

## Experiments and their limits

| Name | Measurement | Required before shipping a reduction |
| --- | --- | --- |
| `unused_fc_tables` | Empty the draft/type/category FC metadata tables | Verify consumers under SSR, hydrate, generation, and all relevant features; then remove the schema fields and regenerate all locales |
| `generation_columns` | Clear `TerritoryType.bg`; zero `ENpcResident.map`, `Map.offset_x`, `offset_y`, and `place_name_region` | Separate generator source types from client projection types; keep extraction inputs intact |
| `duplicate_shop_item` | Clear `SpecialShop.item` after verifying every row equals `item_receive_0` | Remove the redundant field and migrate any consumers; empty vectors still retain wire headers |
| `duplicate_leve_arrays` | Verify leve group/item arrays equal their scalar copies, then zero the arrays | Keep one representation and migrate consumers; the experiment retains fixed-width array slots |
| `item_icon_ids` | Zero `Item.icon` | Keep source icon IDs in extraction inputs; browser image URLs and server search already use item IDs |
| `item_descriptions` | Clear item descriptions | Supply descriptions to item pages/tooltips on demand |
| `referenced_npcs` | Retain names referenced by any shop index, leve issuers, or placement keys | Preserve arbitrary direct NPC pages through a server lookup or explicitly define supported NPC scope |
| `deferred_details` | Combine description and NPC experiments | Both requirements above; this is the measured combined delta |

Zeroing a number **does not remove its fixed-width field**. Clearing a string
or vector leaves its descriptor. These experiments establish payload costs;
actual schema removal changes layout and needs a fresh measurement. They do
not claim that the resulting clone is a functioning application database.

## How to establish that data is unused

Size and liveness are separate evidence:

1. Rank candidates with the column/table inventory. Search references once
   across frontend, server, shared helpers, generator, and tests. Use
   type-aware Find References for generic names such as `item`, `map`, or
   `name`; a text match cannot identify the receiver's type.
2. Classify each candidate as generator-only, server-only, used on initial
   render, used by a specialist feature, or candidate-unused. Follow helper
   methods: `SpecialShop::entries()` reads fields on behalf of its callers.
   Destructuring, struct construction, and macro-generated access also count.
3. For a proposed removal, remove the field from a **client projection type**
   on a disposable branch and run `cargo leptos build` plus `./check_ci.sh`.
   Keep raw CSV/extraction types separate so a generator reference does not
   force the field into the browser. The compiler is the authority for direct
   typed accesses, not search counts or the `dead_code` lint on public fields.
4. Check data semantics too: a field can influence generation or a serialized
   contract without a direct browser read. Validate retained references and
   perform direct-load and client-navigation tests for affected pages. A
   compiler pass alone does not establish SSR/hydration or lookup correctness.
5. Repack and measure all seven locales with identical settings, then run
   `scripts/run_e2e.sh` for affected flows. Runtime access traces can prioritize
   deferred loading; unvisited routes are not proof of unused data.

The first audit's reviewed consumers and measurements are recorded in
[AUDIT-2026-09-21.md](AUDIT-2026-09-21.md). No proposed deletion is applied by
this tool.
