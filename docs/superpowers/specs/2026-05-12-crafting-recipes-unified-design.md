# Crafting Recipes — Unified Cost, Shard Toggle, and On-Hand Accounting — Design

**Date:** 2026-05-12
**Status:** Awaiting user review
**Scope:** Treat crafting recipes as one product surface across the item page, recipe analyzer, and FC crafting analyzer. Unify the three divergent cost calculators into a single module, promote the existing "ignore crystals" toggle and `ListItem.acquired` field to first-class options that every craft-cost surface honors, and add an ephemeral on-hand input for users who aren't using a list. Cross-pollinate item page ↔ analyzers via shared URL/cookie state so the number a user sees is the same number wherever they look.

---

## Why

Today the app quotes a "crafting cost" in three places that don't agree with each other and don't share toggles:

- **Item page** uses [`calculate_crafting_cost`](../../ultros-frontend/ultros-app/src/components/related_items.rs:108) — a flat `(hq, lq)` pair, no subcrafts, no HQ-aware sourcing, no toggles.
- **Recipe analyzer** uses a *different* [`calculate_crafting_cost`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:81) — recursive with depth=2, HQ-aware, but ignores shards-vs-non-shards entirely.
- **FC crafting analyzer** has its own ingredient walk.

A user who opens an item page, sees "cost ~12,000g", clicks into the analyzer, and sees the same recipe at "cost ~19,000g" reasonably concludes the site is wrong. It isn't — but the toggles each surface honors are different.

Three concrete gaps drive this spec:

1. **No shard/crystal toggle anywhere a user reads a cost.** The codebase already detects crystals (`item.item_search_category == 59`) and exposes an `ignore_crystals` checkbox in [`add_recipe_to_current_list.rs:65`](../../ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs:65) — but only on the "add recipe to list" modal. Shards are a category players sit on a stockpile of, and their market prices are the noisiest line item in any recipe. Every cost-quoting surface should honor a single shards toggle.

2. **On-hand accounting exists but is invisible to the analyzers.** `ListItem.acquired` is already a persisted field ([`list_item.rs:15`](../../ultros-db/src/entity/list_item.rs:15)), `list_summary.rs` already subtracts it ([`list_summary.rs:57-58`](../../ultros-frontend/ultros-app/src/components/list/list_summary.rs:57)), and the list UI has a "Mark as acquired" button ([`list_item_row.rs:166`](../../ultros-frontend/ultros-app/src/components/list/list_item_row.rs:166)). None of that flows into the analyzer's profit math or the item page's cost preview. A user who has 200 Lightning Crystals and 50 Cermet Ingots stockpiled cannot tell the analyzer that.

3. **`calculate_crafting_cost` is duplicated and diverging.** Two implementations with different signatures and different sourcing strategies. Either drifts independently; consolidating them is a prerequisite for #1 and #2 to land consistently.

These are the three product questions Aaron raised, addressed as one shipping unit because the cost-function consolidation is the unlock for the other two.

## Non-goals

- Replacing or migrating the existing `lists` / `list_items` schema. We read from it, we don't change it.
- Server-side recipe simulation or a server `/crafting/cost` endpoint. All math stays client-side off the existing `CheapestListings` payload.
- Importing on-hand inventory from Allagantools / Teamcraft / in-game parsers. Designed-for via the `OnHand` trait, but the importer itself is a separate spec.
- Touching the "Recipe Lists" feature or `add_recipe_to_current_list.rs` UX. The crystal toggle there continues to live there; we mirror it elsewhere.
- New API endpoints. None are needed.
- Server-persisted on-hand independent of lists. The existing list mechanism covers the "save my stockpile" case; building a parallel surface is YAGNI.
- The recipe-list / shopping-list redesign — that's a separate product surface and a separate spec.

## Changes

### 1. New shared module: `components/crafting_cost.rs`

Create `ultros-frontend/ultros-app/src/components/crafting_cost.rs` as the single home for craft-cost math. Public surface:

```rust
pub struct CraftingCostOptions<'a> {
    pub require_hq: bool,
    pub max_subcraft_depth: u8,   // 0 disables subcrafts; matches existing depth=2 default
    pub shards: ShardsMode,
    pub on_hand: &'a OnHand,
}

pub enum ShardsMode { ExcludeShards, IncludeMarket }

pub trait OnHand {
    fn available(&self, item: ItemId) -> i32;
    fn consume(&self, item: ItemId, qty: i32);   // tracked-but-not-mutated by default
}

pub struct CostBreakdown {
    pub hq_cost: i32,
    pub lq_cost: i32,
    pub shard_cost: i32,           // what we excluded when shards=ExcludeShards
    pub on_hand_savings: i32,      // gil avoided by on-hand
    pub ingredient_lines: Vec<IngredientLine>,
    pub sub_crafts: Vec<SubcraftInfo>,
}

pub fn compute_cost(
    recipe: &Recipe,
    prices: &CheapestListingsMap,
    recipes_by_output: &HashMap<ItemId, Vec<&'static Recipe>>,
    opts: &CraftingCostOptions<'_>,
) -> CostBreakdown;
```

Implementations:

- `IngredientsIter` ([`related_items.rs:71`](../../ultros-frontend/ultros-app/src/components/related_items.rs:71)) moves to this module unchanged and stays the canonical ingredient walker.
- `recipes_by_output` matches the existing analyzer convention ([`recipe_analyzer.rs:195`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:195)). The item page passes an empty map and `max_subcraft_depth: 0` for a non-recursive cost.
- `compute_cost` returns a single `CostBreakdown` for the supplied `require_hq`. Surfaces that need both HQ and LQ (currently only the item page's recipe panel) call it twice — matching the current "sum twice with `prefer_hq` flag" pattern in [`related_items.rs:110`](../../ultros-frontend/ultros-app/src/components/related_items.rs:110). Subcraft recursion makes a true "both at once" pass non-trivial (a subcraft's optimal cost depends on `require_hq`), and the surfaces that care can afford the double walk.

After landing this module, delete the local `calculate_crafting_cost` in both [`related_items.rs:108`](../../ultros-frontend/ultros-app/src/components/related_items.rs:108) and [`recipe_analyzer.rs:81`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:81).

**Parity test (Tier-1 of test plan, see §6):** with `shards=IncludeMarket`, empty `OnHand`, `max_subcraft_depth=0`, the `(hq_cost, lq_cost)` returned by `compute_cost` must equal the pair returned by the current `related_items::calculate_crafting_cost` over a snapshot of the existing `CheapestListings` payload. This is the regression guard.

### 2. Shards toggle on every craft-cost surface

`is_shard(item) = item.item_search_category == 59` — matches the convention already used in [`add_recipe_to_current_list.rs:65`](../../ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs:65). When `shards = ExcludeShards`, `compute_cost` skips shard ingredients in the cost sum but still accumulates their would-be price into `CostBreakdown.shard_cost` so the UI can show "(shards excluded: 1,240g)".

Wiring:

- **Item page** ([`related_items.rs:760`](../../ultros-frontend/ultros-app/src/components/related_items.rs:760), the "Crafting Recipes" panel): new toggle row above the recipe cards — `[ ] Require HQ` `[ ] Include sub-crafts` `[x] Exclude shards` `[ ] Use on-hand`. Defaults come from the `craft_options` cookie (§4).
- **Recipe analyzer**: new URL query param `shards=exclude|include`, new filter card alongside the existing "Options" card ([`recipe_analyzer.rs:478`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:478)). Default `exclude`.
- **FC crafting analyzer**: same URL param and checkbox.

**Default = `ExcludeShards`.** Rationale: a stockpile is the norm for any crafter past level 50, and shard market spreads (1–80g per shard depending on world) dominate the noise floor in `compute_cost`'s output. The toggle is one click away if a user wants the strict number.

The existing `ignore_crystals` checkbox in the "add recipe to list" modal continues to use its local state — that flow is about list contents, not cost numbers, and the two should remain independent.

### 3. On-hand accounting

Two `OnHand` implementations behind one trait:

**3a. Ephemeral / LocalStorage** (`LocalOnHand`):
- Storage key: `ultros.craft.on_hand.v1`. JSON-encoded `HashMap<i32, i32>` (item id → qty).
- New component `components/on_hand_input.rs` exposes:
  - Inline `OnHandQuantity` widget — one number input per ingredient. Used both on the item page recipe cards (per-ingredient input under each line) and as an editable column on the analyzer's expanded-row ingredient breakdown.
  - `OnHandPanel` — collapsible global summary showing all tracked items, "Reset on-hand" button.
- Cleared by a single button. No server traffic.

**3b. List-backed** (`ListOnHand`):
- When the user has a list selected as their "active crafting list" (new cookie `active_craft_list: Option<i32>`), `available(item_id)` returns `list_item.acquired - already_consumed_in_this_compute_pass`.
- The active-list picker is a single dropdown in the existing list navigation. Selecting one shows a banner on the cost-quoting surfaces: "On-hand pulled from list: «name»". The banner has a dismiss/disable affordance that falls back to `LocalOnHand` for the session.
- Read-only from the analyzer's perspective: nothing in this spec writes to `acquired`. The user already has UI for that on the list page ([`list_item_row.rs:166`](../../ultros-frontend/ultros-app/src/components/list/list_item_row.rs:166)).

Math in `compute_cost`:

```
needed_total = recipe_amount * craft_count
needed_after_on_hand = max(0, needed_total - on_hand.available(item))
on_hand_savings += min(needed_total, on_hand.available(item)) * unit_price
ingredient_cost = needed_after_on_hand * unit_price
```

For the multi-craft case (e.g. "I want 50 of this craftable"), the on-hand qty is consumed in proportion across the entire batch — the user types "I have 100 lightning shards"; if 50 crafts need 200 shards total, the cost is for 100 shards.

`OnHandSavings` is exposed in `CostBreakdown` and rendered as a green chip on the recipe card / analyzer row: "Saved 3,200g from on-hand".

**Source-of-truth precedence:** if a list is active, `ListOnHand` wins; otherwise `LocalOnHand`. A small inline label on each surface says which source is active.

### 4. Cross-pollination: shared `craft_options` state

A new global state module `ultros-frontend/ultros-app/src/global_state/craft_options.rs` owns:

```rust
pub struct CraftOptions {
    pub require_hq: bool,
    pub include_subcrafts: bool,
    pub exclude_shards: bool,
    pub use_on_hand: bool,
    pub active_craft_list: Option<i32>,
}
```

Persisted to a cookie (`CRAFT_OPTIONS`), same pattern as the existing `CrafterLevels` cookie ([`recipe_analyzer.rs:215`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:215)).

Precedence:
1. URL query params on the analyzer routes override the cookie when present (existing pattern: `query_signal::<bool>("subcrafts")` etc.).
2. The item page reads the cookie directly (it has no URL params to spare; the item page URL identifies the item, not the cost-view state).
3. Toggling a checkbox on the item page writes the cookie. Toggling on the analyzer writes both the URL param and the cookie.

**Item page → analyzer link.** Each recipe card on the item page gets an "Open in analyzer" button that constructs `?job=<derived from recipe.craft_type>&require-hq=<current>&subcrafts=<current>&shards=<current>` — so the user lands on the analyzer at the same toggle state. The button derives `craft_type` from the recipe using the existing mapping at [`recipe_analyzer.rs:260-271`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:260).

**Analyzer → item page link.** Existing `<A>` links to `/item/...` already work; they get a `?craft-options=cookie` query (or no extra param — the cookie carries state). No behavioral change required since the cookie is the source of truth on the item page.

### 5. Item page recipe panel — interactive cost

Today the "Crafting Recipes" panel ([`related_items.rs:760`](../../ultros-frontend/ultros-app/src/components/related_items.rs:760)) is read-only summary mode. After this work:

- One toggle row at the top of the panel: `[ ] Require HQ` `[ ] Include sub-crafts` `[x] Exclude shards` `[ ] Use on-hand`. Compact, single line.
- Each recipe card consumes `CostBreakdown` and shows:
  - `Cost (HQ): 12,400g  Cost (LQ): 11,200g`
  - Conditional chip: `Excluded shards: 1,240g` (when `shards=ExcludeShards`)
  - Conditional chip: `Saved from on-hand: 3,200g` (when on-hand applied)
- The existing "Est. Profit" computation ([`related_items.rs:225-299`](../../ultros-frontend/ultros-app/src/components/related_items.rs:225)) collapses: it currently re-walks ingredients with a duplicate inline closure. Replace that closure with the same `compute_cost` call — single source of truth, profit chips become consistent with the cost line.
- Each card gains an "On-hand" disclosure: clicking opens the inline per-ingredient `OnHandQuantity` widgets for that recipe's ingredients only. Saves on visual clutter; users who don't care never see it.

The "Add to list" button on each card already exists ([`AddRecipeToList`](../../ultros-frontend/ultros-app/src/components/related_items.rs:207)) — leave it.

### 6. Tests

New `cargo test` cases live in `crafting_cost.rs`:

1. **Parity guard.** With `shards=IncludeMarket`, empty `LocalOnHand`, `max_subcraft_depth=0`, `require_hq=false`, `compute_cost` matches a snapshot of `related_items::calculate_crafting_cost`. (Snapshot generated from current behavior pre-deletion, committed as a fixture under `ultros-frontend/ultros-app/src/components/crafting_cost/fixtures.rs`.)
2. **Shard exclusion math.** With `shards=ExcludeShards`, the cost reduces by exactly the sum of shard ingredient prices, and `CostBreakdown.shard_cost` equals that delta.
3. **On-hand clamp.** `on_hand=999` for an ingredient where `needed=10` produces `needed_after_on_hand=0`, not negative, and `on_hand_savings = 10 * unit_price` not `999 * unit_price`.
4. **On-hand precedence.** `ListOnHand` (when active) wins over `LocalOnHand`.
5. **Subcraft recursion termination.** A pathological recipe graph (item A's recipe needs item B; item B's recipe needs item A — synthetic fixture) terminates at `max_subcraft_depth` and produces a finite cost. (Today's code uses depth=2; this just verifies the bound holds when other options are layered in.)
6. **HQ fallback.** With `require_hq=true` and an item where no HQ listing exists, the cost falls back to LQ. (Matches existing [`recipe_analyzer.rs:102-106`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:102).)

No backend tests required — no schema or endpoint changes.

### 7. UI deletions / collapses

- Delete `calculate_crafting_cost` from [`related_items.rs:108`](../../ultros-frontend/ultros-app/src/components/related_items.rs:108).
- Delete `calculate_crafting_cost` from [`recipe_analyzer.rs:81`](../../ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:81).
- Collapse the duplicate inline `sum_for` closure at [`related_items.rs:235-255`](../../ultros-frontend/ultros-app/src/components/related_items.rs:235) into a `CostBreakdown` consumer.
- Move `IngredientsIter` to `crafting_cost.rs` and update the one external user ([`add_recipe_to_current_list.rs:5`](../../ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs:5)) — keep the re-export from `related_items.rs` for the transition or update the import directly.

## Files touched

**New:**
- `ultros-frontend/ultros-app/src/components/crafting_cost.rs`
- `ultros-frontend/ultros-app/src/components/crafting_cost/fixtures.rs` (test data)
- `ultros-frontend/ultros-app/src/components/on_hand_input.rs`
- `ultros-frontend/ultros-app/src/global_state/craft_options.rs`

**Edited:**
- `ultros-frontend/ultros-app/src/components/related_items.rs` — remove local cost fn; add toggle row + on-hand disclosure to "Crafting Recipes" panel; rewire `RecipePriceEstimate` and the inline profit closure to consume `CostBreakdown`.
- `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` — remove local cost fn; add `shards` + `on-hand` URL params and filter cards; consume `CraftOptions` cookie defaults.
- `ultros-frontend/ultros-app/src/routes/fc_crafting_analyzer.rs` — same toggles, same flow.
- `ultros-frontend/ultros-app/src/components/mod.rs` — export new modules.
- `ultros-frontend/ultros-app/src/global_state/mod.rs` — export `craft_options`.
- `ultros-frontend/ultros-app/src/components/add_recipe_to_current_list.rs` — re-point `IngredientsIter` import to the new home.
- i18n string tables — add 4 new toggle labels and 2 banner phrases.

## Roadmap / appendix (not in this spec)

- **Tier 2: Teamcraft / Allagantools paste-importer for on-hand.** Writes into `LocalOnHand` (or into `ListItem.acquired` if a list is active). Separate spec.
- **Tier 3: Per-row on-hand savings column on the analyzer table.** Visible only when on-hand is active. Low lift once the breakdown is in `CostBreakdown` but the analyzer's table layout needs its own design pass.
- **Tier 4: "Plan a craft batch" surface.** Given a target item and a desired craft count, compute the full shopping list with on-hand applied and a one-click "Save as list" flow. Reuses everything in this spec; needs new UI.
