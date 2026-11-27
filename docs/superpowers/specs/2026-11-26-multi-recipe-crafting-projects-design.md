# Multi-Recipe Crafting Projects in Lists (`Build | Craft | Shop`) — design

Date: 2026-11-26
Status: Proposed
Affected crates & modules:
- `ultros-frontend/ultros-calc/src/recipe_planner.rs`, `ultros-frontend/ultros-calc/src/recipe_work.rs`
- `ultros-list-doc` (`document.rs`, `key.rs`, `snapshot.rs`, `validation.rs`, `undo.rs`)
- `ultros-db/src/list_doc.rs`, `ultros-db/src/guest_list_adoption.rs`, `ultros-lists/src/sync.rs`, `ultros-lists/src/activity.rs`
- `ultros-frontend/ultros-app/src/` (`list_doc/adapter.rs`, `list_doc/handle.rs`, `list_doc/guest.rs`, `components/list_workspace_shell.rs`, `components/list_craft.rs`, `components/list_shop.rs`, `components/cart/`, `routes/list_view_sync.rs`, `routes/guest_lists.rs`, `routes/job_set_detail.rs`, `routes/recipe_view.rs`, `routes/fc_crafting_analyzer.rs`)
- `ultros-frontend/ultros-ui-crafting/src/components/add_recipe_to_list.rs`
- `ultros-frontend/ultros-i18n/locales/*.json`
- `ultros-list-doc-ffi/src/lib.rs` and `plugins/dalamud/` (Phase 3)

---

## 1. Problem & Opportunity

Ultros already owns two strong capabilities that sit just short of connecting to each other:

1. **Single-recipe, ephemeral planning (`/recipe/:id`).** `ultros_calc::recipe_planner::expand` performs topological DAG expansion, rounds shared intermediates to craft-yield multiples after pooling parent demand, deducts `owned` materials once per item, routes whole-stack purchases across worlds/datacenters with a bounded 0/1 knapsack + travel-frontier search, includes NPC gil-shop pricing, and reverses topological order into a craft sequence. However, `expand()` accepts only a single `root: &Recipe`, and `SavePlan` in `ultros-frontend/ultros-app/src/routes/recipe_view.rs` requires authentication (omitting `LocalListTargets`) and flattens the result to leaf `ListItem` rows—discarding craft choices, subcraft hierarchy, and execution order.
2. **Flat shopping-only Lists 2.0 (`/list/:id`, `/list/device/:id`).** Lists 2.0 provides offline-first Loro CRDT sync (`ultros-list-doc/src/document.rs`), Discord-role FC group sharing, recurring restock, whole-stack travel routing (`components/list_shop.rs`), Document Picture-in-Picture companion mode, and live Dalamud sync with optional inventory auto-tracking (`plugins/dalamud/README.md`). Yet every recipe entry point dumps **1-level direct ingredients only** into flat `rows`:
   - `InlineRecipeAdd` in `ultros-frontend/ultros-app/src/routes/list_view_sync.rs`
   - `AddRecipeToListForm` on Recipe Analyzer in `ultros-frontend/ultros-ui-crafting/src/components/add_recipe_to_list.rs`
   - `AddSetToList` on Job Set Detail in `ultros-frontend/ultros-app/src/routes/job_set_detail.rs`

Because of this boundary, players building a multi-piece raid gear set, raid-night food + tincture batch, or multi-recipe profit batch still leave Ultros for external planners to consolidate precrafts and track crafting steps—even though Ultros already carries better live pricing, cross-world travel routing, CRDT collaboration, and an integrated Dalamud plugin. The Lists 2.0 design (`docs/superpowers/specs/2026-09-10-lists-2-product-design.md`, §Build) explicitly left recipe-contribution modeling as a future document-schema extension.

---

## 2. Settled Design Principles

1. **Converge, don't duplicate.** Multi-recipe expansion and Craft-vs-Buy optimization live in `ultros-calc::recipe_planner`, shared by `/recipe/:id`, Lists (`Build | Craft | Shop`), and server-side relational projection. No second solver or second shopping router is built.
2. **Zero regression for plain shopping lists.** Lists with no recipe targets remain on **schema v1** and keep the existing two-mode **`Build | Shop`** toggle with zero extra UI chrome, preserving compatibility with older Dalamud builds and existing list flows.
3. **Store intent and progress, derive needs.** Never store expanded intermediate/leaf quantities inside `rows.need`. Recipe targets (`targets`), per-intermediate craft/buy decisions and pre-existing stock (`craft_choices`), and cumulative progress counters (`rows.acquired`, `craft_choices.crafted`, `targets.completed`) are the CRDT state; total material requirements are derived deterministically from game recipes. That guarantees editing one target count, toggling an intermediate between Craft and Buy, or adding a second recipe automatically updates consolidated precrafts and leaf shopping rows without stale-quantity drift.
4. **Separate pre-shopping stock (`on_hand`) from active execution progress (`crafted`).** Pre-existing intermediates you already own before starting reduce downstream shopping demand; intermediates crafted *from* materials bought for this project must **not** shrink the Shop list's `total_needed`, or cumulative `rows.acquired` progress on raw mats would appear over-acquired / disappear mid-project.
5. **Additive multiplayer progress, LWW configuration.** Every progress field (`rows.acquired`, `craft_choices.crafted`, `targets.completed`) is a `LoroCounter`, so FC members or two devices checking off items simultaneously add deltas instead of overwriting each other. Target counts and Craft/Buy recipe choices are LWW fields.

---

## 3. CRDT Document Schema v2 (`ultros-list-doc`)

### 3.1 Lazy Schema Promotion

- `SCHEMA_VERSION` remains `1` for newly created lists and plain flat edits (`rows`, `meta.name`, `meta.scope`).
- `SCHEMA_VERSION_CRAFT: i64 = 2` is written to `meta.schema` lazily the first time a mutator touches `targets`, `craft_choices`, `meta.exclude_crystals`, or `meta.include_vendors`.
- Schema v1 documents continue to open, validate, and sync identically. Schema v2 validation permits all schema v1 documents plus the v2 containers below; schema > 2 continues to fail closed with `DocError::UnsupportedSchema`.
- Any edit to `rows` on a schema v2 document remains identical to v1 (`rows` holds manual flat items and cumulative leaf `acquired` counters).

### 3.2 Loro Container Layout

```text
root map "meta"
  schema:           i64     1 | 2
  name:             string
  scope:            string  "world:{id}" | "datacenter:{id}" | "region:{id}"
  exclude_crystals: bool    optional (absent => false)
  include_vendors:  bool    optional (absent => true)

root map "rows"
  key "{item_id}:{quality}"         quality ∈ { "any", "hq", "nq" }
  value LoroMap:
    item:     i64                   canonical item_id > 0
    quality:  string                "any" | "hq" | "nq"
    need:     i64                   >= 0 (0 = derived-leaf progress row only; > 0 = explicit flat item need)
    target:   i64                   optional price threshold (absent = none)
    acquired: LoroCounter           cumulative units acquired toward manual + derived leaf demand

root map "targets"                  [Schema v2]
  key "{recipe_id}:{quality}"       quality ∈ { "any", "hq", "nq" }
  value LoroMap:
    recipe:    i64                  canonical recipe_id > 0
    quality:   string               "any" | "hq" | "nq"
    count:     i64                  desired output units in 1..=9999
    completed: LoroCounter          units finished during this project run

root map "craft_choices"            [Schema v2]
  key "{item_id}"                   canonical item_id > 0
  value LoroMap:
    item:     i64                   canonical item_id > 0
    recipe:   i64                   0 = Buy intermediate; > 0 = craft using this RecipeId
    on_hand:  i64                   pre-existing stock owned before expanding sub-materials (>= 0)
    crafted:  LoroCounter           units crafted during this run from project materials
```

#### Design invariants on `rows`, `targets`, and `craft_choices`

1. **`rows.need == 0` progress-only rows:** When a player checks off a derived recipe leaf material that was not manually added as a flat item, the client records `acquired` on `rows["{item_id}:{quality}"]` with `need = 0`. In `Build` / `Shop` / relational projection, effective row demand is `manual_need + derived_leaf_need`. When `manual_need == 0 && derived_leaf_need == 0`, an orphaned `need == 0` row is hidden from Shop and garbage-collected on explicit target removal/cleanup (never silently deleting an un-zeroed `acquired` counter during transient edits so `Ctrl+Z` restores it cleanly).
2. **`targets.count` represents desired output units, not craft clicks:** Just like `/recipe/:id` (`output_demand` / `expand`), if a Culinary recipe yields 3× *Moqueca* per craft and `count = 30`, the project expands `ceil(30 / 3) = 10` crafts. If the user types craft clicks in `InlineRecipeAdd`, the UI converts `crafts * recipe.amount_result` to `count` and displays both (`"30 units (10 crafts)"`).
3. **Default intermediate policy when `craft_choices[item_id]` is absent:** Default is **Craft** using the item's lowest-`RecipeId` recipe (matching `/recipe/:id` when an intermediate has a single recipe, or deterministic canonical recipe when multiple crafters produce the same intermediate—e.g., *Glamour Prism* or *Alchemical Charcoal*—until the player switches recipe or clicks **Optimize Craft / Buy**). Storing absent entries implicitly keeps documents compact; `craft_choices[item_id]` is materialized only when the player toggles Craft/Buy, switches alternate recipe, enters `on_hand > 0`, or increments `crafted`.

### 3.3 Inventory & Consumption Accounting (Worked Semantics)

Separating **pre-existing stock (`on_hand`)** from **in-run crafting progress (`crafted`)** avoids the classic crafting-tracker bug where crafting a precraft hides the raw materials you already bought for it:

- **Top-down effective demand:**
  1. Sum raw demand across all `targets` (note: `targets.completed` tracks finished-goods milestones for the Craft checklist and does *not* retroactively erase raw materials already consumed to make them, unless the user explicitly adjusts `targets.count` or pre-existing `on_hand`).
  2. For each intermediate `item` where `recipe > 0` (Craft):
     - `effective_craft_units = (total_needed - on_hand).max(0)`
     - `crafts_required = ceil(effective_craft_units / yield_amount)`
     - Propagate `crafts_required * ingredient_amount` to child materials.
     - Remaining craft work shown in the **Craft** tab = `(effective_craft_units - crafted).max(0)` (clamped display progress `crafted.clamp(0, effective_craft_units)`).
  3. For each intermediate `item` where `recipe == 0` (Buy):
     - Does not expand children; adds `(total_needed - on_hand).max(0)` directly to leaf market demand (`Shop`).
  4. For every leaf item `(item_id, quality)`:
     - `effective_leaf_need = manual_need + derived_leaf_need`
     - `remaining_to_buy = (effective_leaf_need - acquired).max(0)`

**Worked examples (Target: 2× Sword, each needing 2× Ingot; each Ingot craft yields 1 and needs 3× Ore):**
- **Case A — Buy all mats, then craft:**
  - Initial: `Ore derived_leaf_need = 12`, `Ingot effective_craft_units = 4`, `Ore remaining_to_buy = 12`.
  - Player buys 12 Ore (`rows["ore:any"].acquired = 12`): `Ore remaining_to_buy = 0`, Craft tab shows Ingots **Ready** (`12 / 12 Ore acquired`).
  - Player crafts 2 Ingots (`craft_choices[ingot].crafted += 2`): `Ingots remaining = 2 / 4`, `Ore derived_leaf_need` stays `12` (`12 / 12` acquired—no weird negative/vanished row).
- **Case B — Already owned 2 Ingots in retainer before starting:**
  - Player sets `craft_choices[ingot].on_hand = 2`.
  - `Ingot effective_craft_units = 4 - 2 = 2`, so `Ore derived_leaf_need` immediately drops from `12` to `6`. Shop only buys 6 Ore.
- **Case C — `ResetAllAcquired` (recurring restock run):**
  - Resets `rows[k].acquired -> 0`, `craft_choices[i].crafted -> 0`, `craft_choices[i].on_hand -> 0`, and `targets[k].completed -> 0` in a single `ListUndo` group, keeping `targets.count`, `craft_choices.recipe`, `meta.exclude_crystals`, and `meta.include_vendors` intact for the next weekly raid/market restock batch.

### 3.4 Validation & Limits (`ultros-list-doc/src/validation.rs`)

Extend `validate(doc: &LoroDoc)` while preserving the 8 MiB snapshot limit, 32 MiB decode budget, 16 MiB block budget, and 100,000 retained operations limit (`ultros-list-doc/src/document.rs`, `plugins/dalamud/README.md`):

- Allowed root containers:
  - Schema `None` or `1`: `meta`, `rows` only (unchanged—rejects `targets` or `craft_choices` if `schema < 2`).
  - Schema `2`: `meta`, `rows`, and optional `targets`, `craft_choices` (`ContainerType::Map`). Unknown roots fail with `InvalidStructure`.
- `meta` fields in schema 2: `"schema" | "name" | "scope" | "exclude_crystals" | "include_vendors"`. Booleans validated as `LoroValue::Bool(_)`.
- `targets`:
  - Entry count `<= MAX_TARGETS` (`64`).
  - Canonical `TargetKey` (`"{recipe_id}:{quality}"` with `recipe_id > 0`, `recipe_id == recipe_id.to_string()`, `quality ∈ {"any", "hq", "nq"}`).
  - `recipe == i64::from(key.recipe_id)`, `quality == key.quality.as_str()`, `count ∈ 1..=9999`, `completed` is a finite integer `LoroCounter`.
- `craft_choices`:
  - Entry count `<= MAX_CRAFT_CHOICES` (`256`).
  - Canonical `ItemKey` (`"{item_id}"` with `item_id > 0` and canonical string formatting).
  - `item == i64::from(item_id)`, `recipe >= 0`, `on_hand ∈ 0..=1_000_000_000`, `crafted` is a finite integer `LoroCounter`.
- **Preflight & rollback safety:** All v2 mutations run through `ListDocument::preflight` so oversize snapshots or decode-budget violations fail cleanly before touching live observers or the undo stack.

### 3.5 Typed Rust API (`ultros-list-doc`)

Add to `ultros-list-doc/src/key.rs`, `snapshot.rs`, and `document.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetKey {
    pub recipe_id: i32,
    pub quality: Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetSnapshot {
    pub key: TargetKey,
    pub count: i64,
    pub completed: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CraftChoiceSnapshot {
    pub item_id: i32,
    /// `None` => Buy; `Some(recipe_id)` => Craft using `recipe_id`.
    pub recipe: Option<i32>,
    pub on_hand: i64,
    pub crafted: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CraftOptionsSnapshot {
    pub exclude_crystals: bool,
    pub include_vendors: bool,
}
```

`ListDocument` additions (every mutator returns `Result<_, DocError>` and auto-promotes `meta.schema` to `2` if currently `< 2`):
- `targets(&self) -> Vec<TargetSnapshot>`
- `craft_choices(&self) -> Vec<CraftChoiceSnapshot>`
- `craft_options(&self) -> CraftOptionsSnapshot`
- `add_target(&self, key: TargetKey, count: i64) -> Result<(), DocError>` (adds to `count` clamped to `9999`)
- `set_target_count(&self, key: TargetKey, count: i64) -> Result<(), DocError>`
- `add_target_completed(&self, key: TargetKey, delta: i64) -> Result<(), DocError>`
- `remove_target(&self, key: &TargetKey) -> Result<(), DocError>`
- `set_craft_choice(&self, item_id: i32, recipe: Option<i32>) -> Result<(), DocError>`
- `set_intermediate_on_hand(&self, item_id: i32, on_hand: i64) -> Result<(), DocError>`
- `add_intermediate_crafted(&self, item_id: i32, delta: i64) -> Result<(), DocError>`
- `set_craft_options(&self, options: CraftOptionsSnapshot) -> Result<(), DocError>`
- `collaborative_only_since(&self, version: &[u8]) -> Result<bool, DocError>` (replaces `row_only_since` in `ultros-db/src/list_doc.rs` permission check — see §4.2)

### 3.6 Multiplayer Conflict Resolution

| Concurrent Scenario | CRDT Outcome |
|---|---|
| Two FC members each craft 2× *Ra'Kaznar Ingot* | `LoroCounter` on `craft_choices[item].crafted` sums both increments (`+4`) |
| One member crafts on Dalamud while another buys ore on web | Independent containers (`craft_choices.crafted` vs `rows.acquired`); both progress bars advance |
| Peer A switches intermediate to Buy while Peer B switches to Craft | LWW on `craft_choices[item].recipe`; UI immediately reflects settled state; `crafted` counter is preserved if toggled back |
| Peer A increments `targets[k].count` while Peer B deletes target `k` | Loro map key delete vs update resolves deterministically by causal timestamp; undo restores target cleanly |
| Non-owner `Write` member adds first recipe target (schema 1 → 2) | Server authorizes `schema` promotion `1 -> 2` alongside `targets`/`craft_choices` writes while still blocking `meta.name` / `meta.scope` edits |

---

## 4. Server Persistence, Relational Projection & Permissions (`ultros-db`, `ultros-lists`)

### 4.1 Zero Database Migration Required

`list_doc.snapshot` in Postgres already persists opaque Loro snapshot bytes (`ultros-db/src/entity/list_doc.rs`). Schema v2 documents fit straight into existing `list_doc` rows, websocket relay (`ultros-lists/src/sync.rs`), and compaction (`COMPACT_AFTER_CHANGES`, `COMPACT_AFTER_BYTES`).

### 4.2 Permission Enforcement (`collaborative_only_since`)

Today `ultros-db/src/list_doc.rs::merge_loaded` checks:
```rust
if permission < ListPermission::Owner
    && (meta_after != meta_before || !doc.row_only_since(&version_before)?)
{
    return Err(ListError::MetaForbidden); // ListDocError::MetaForbidden
}
```
With schema v2, collaborators with `ListPermission::Write` must be able to:
- Edit `rows`, `targets`, and `craft_choices`.
- Promote `meta.schema` from `1` to `2` (strictly monotonic `1 -> 2`, never downgrading or exceeding `2`).
- Toggle `meta.exclude_crystals` / `meta.include_vendors`? **Design decision:** Keep `meta.name` and `meta.scope` strictly `ListPermission::Owner` (matching existing list settings semantics); allow `ListPermission::Write` to mutate `rows`, `targets`, `craft_choices`, `meta.schema` (`1 -> 2`), `meta.exclude_crystals`, and `meta.include_vendors` so a shared FC craft group can exclude crystals or use vendors without needing list ownership.

Replace `doc.row_only_since(&version_before)` with `doc.collaborative_only_since(&version_before, owner_meta_unchanged)` inspecting `export_json_updates` operations: only `"rows"`, `"targets"`, `"craft_choices"`, and `"meta"` keys in `{"schema", "exclude_crystals", "include_vendors"}` (with `schema` promoted `1 -> 2` only) are permitted when `permission < ListPermission::Owner`.

### 4.3 Relational `list_item` Projection Without Coupling `ultros-db` to `xiv-gen-db`

`ultros-db` deliberately avoids depending on `xiv-gen-db` so database crates compile fast without embedding game-data packs (`ultros-db/Cargo.toml`). At the same time, `ultros-calc` explicitly documents itself as game-data-agnostic (`"Keep this crate independent of Leptos, UI state, and game-data loading"` in `ultros-frontend/ultros-calc/src/lib.rs`).

To project schema v2 consolidated leaf requirements into `list_item` inside the existing Postgres transaction (`ultros-db/src/list_doc.rs::merge_loaded` and `ultros-db/src/guest_list_adoption.rs::make_list_online`):

1. `ultros-db` adds `ultros-calc = { path = "../ultros-frontend/ultros-calc" }` (minimal pure-Rust dependency: `chrono` + `ultros-api-types`).
2. `ultros-db` defines a lightweight callback/catalog trait:
   ```rust
   pub trait RecipeCatalog: Send + Sync {
       fn recipe(&self, recipe_id: i32) -> Option<ultros_calc::recipe_planner::Recipe>;
       fn recipes_for_output(&self, item_id: i32) -> Vec<ultros_calc::recipe_planner::Recipe>;
       fn is_crystal(&self, item_id: i32) -> bool;
   }
   ```
3. `ultros/src/utils.rs` / `ultros-lists` implements `RecipeCatalog` over `xiv_gen_db::data()` (`OnceLock`-backed static map) and passes `&Catalog` into `apply_list_update`, `edit_list_doc`, `apply_plugin_list_update`, and `make_list_online`.
4. Inside `merge_loaded`, when `doc.schema() == Some(2)` (or `!doc.targets().is_empty()`):
   - Expand `doc.targets()` using `ultros_calc::recipe_planner::expand_many` with `doc.craft_choices()` and `doc.craft_options()`.
   - Build `projected_rows_before` and `projected_rows_after` by combining `doc.rows()` manual `need` + expanded `derived_leaf_need` (assigning `quality = Quality::Any` unless a recipe leaf has an explicit quality override, carrying `acquired` and `target` from `doc.rows()`). Omit rows where `effective_need == 0 && acquired == 0`.
   - Pass `diff_rows(&projected_rows_before, &projected_rows_after)` to `project_rows(...)` and `ultros_lists::activity::entries_for(...)`.
5. **Result:** Existing list-price threshold alerts (`ultros-alerts`), Discord `/list` reads, and activity feeds automatically see the consolidated leaf materials and progress without schema migrations or duplicate alert infrastructure.
6. **Guest adoption (`ultros-db/src/guest_list_adoption.rs`):** `make_list_online` already preserves `original: Some(doc)` verbatim (so `targets` and `craft_choices` survive adoption untouched!). Update `prepare_online` to project `effective_rows(&doc, catalog)` into `AdoptGuestList.items` and raise the snapshot upload check from `512 * 1024` to `2 * 1024 * 1024` if needed (consistent with `ultros-list-doc` 8 MiB max, though typical v2 crafting projects are 3–25 KB).

---

## 5. Multi-Root Calculation Engine (`ultros-calc`)

### 5.1 Generalized Expansion (`ultros-frontend/ultros-calc/src/recipe_planner.rs`)

Generalize single-root `expand` into multi-root `expand_many`, with `expand` delegating to `expand_many` so `/recipe/:id` and Lists share one tested implementation:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct TargetDemand {
    pub recipe_id: i32,
    pub count: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct ExpandLimits {
    pub max_depth: usize, // default 12
    pub max_nodes: usize, // 128 for single-recipe, 256 for multi-recipe projects
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct Expansion {
    /// Topological parent-first order (reverse for crafting order).
    /// Includes both intermediates and consolidated leaf materials.
    pub materials: Vec<Material>,
    /// Per-node dependency attribution (`item_id -> Vec<(parent_or_target_item_id, amount)>`)
    /// so the UI can show "Used by: Blade (×2), Gauntlets (×2)".
    pub used_by: BTreeMap<i32, Vec<(i32, i64)>>,
    /// Any target recipe ID missing from the catalog (e.g. retired in a game patch).
    pub missing_recipes: Vec<i32>,
}

pub fn expand_many(
    targets: &[TargetDemand],
    recipes: &BTreeMap<i32, Recipe>,
    choices: &BTreeMap<i32, i32>, // 0 = Buy, >0 = RecipeId (absent => canonical default if craftable)
    owned: &BTreeMap<i32, i64>,
    excluded: &BTreeSet<i32>,
    limits: ExpandLimits,
) -> Result<Expansion, String>
```

#### Key algorithmic properties:
- **Multi-root Kahn / reverse-postorder topological sort:** Visit every root in `targets`, ensuring every parent's demand for a shared intermediate `X` is accumulated into `demands[X]` *before* `X` deducts `owned[X]` (`on_hand`) and rounds `ceil((needed - on_hand) / yield)` into `crafts`! That preserves `/recipe/:id`'s exact surplus-minimization guarantee across the whole project.
- **Root + Intermediate self-overlap:** If Target A is *Grade 2 Gemdraught* and Target B is an item that *uses* an intermediate also used by A, or Target B itself uses A as an ingredient, topological ordering naturally handles root-shares-intermediate and root-is-intermediate cases without cycle false-positives (cycles only trigger if an active DFS ancestor references itself via `choices`).
- **Missing recipes fail visibly, not silently:** If a game patch removes or renumbers a `recipe_id` stored in `targets`, `missing_recipes` flags that target cleanly in UI instead of failing the whole project calculation.

### 5.2 Craft-vs-Buy Auto-Optimizer (`optimize_craft_choices`)

Add a pure deterministic bottom-up DP optimizer to `recipe_planner.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub struct OptimizePlan {
    /// Recommended choice per craftable intermediate (`item_id -> Some(recipe_id) | None` for Buy).
    pub choices: BTreeMap<i32, Option<i32>>,
    /// Marginal gil cost of crafting vs buying each intermediate (negative = crafting saves gil).
    pub per_item_delta: BTreeMap<i32, i64>,
    pub total_savings_gil: i64,
}

pub fn optimize_craft_choices(
    targets: &[TargetDemand],
    recipes: &BTreeMap<i32, Recipe>,
    owned: &BTreeMap<i32, i64>,
    excluded: &BTreeSet<i32>,
    offers: &BTreeMap<i32, Vec<Offer>>,
    vendors: &BTreeMap<i32, i64>,
    allowed_worlds: &BTreeSet<i32>,
) -> Result<OptimizePlan, String>
```

- **How it works:**
  1. Walk all reachable intermediates in **bottom-up** topological order (leaves first).
  2. For each craftable intermediate `item` needing `rem = (needed - on_hand).max(0)`:
     - Evaluate `buy_cost = purchase(rem, scoped_offers, vendor_price)` (using partial-stack / whole-stack scoped offers; `i64::MAX` / `Incomplete` if unpriced or out of stock).
     - For every alternative `recipe` producing `item`, compute `craft_cost = sum(child_best_cost(child, crafts * qty))`.
     - Choose the cheapest **complete** option (`Buy` vs `Craft(recipe)`), breaking ties in favor of `Craft` when `craft_cost == buy_cost` or when `Buy` has missing supply (`missing > 0`).
  3. Because the walk is bottom-up, parent craft costs automatically reflect optimal Buy decisions on overpriced child intermediates!
- **One-click undoable application:** The UI previews `"Save 148,200 gil by buying 2 precrafts"` on the Craft toolbar; clicking applies all changed `craft_choices` in a single `ListUndo` group so `Ctrl+Z` reverts the entire optimization cleanly.

### 5.3 Web Worker Offloading (`ultros-calc/src/recipe_work.rs` & `ultros-app/src/recipe_work.rs`)

A full 11-piece crafted battle gear set can contain ~25–35 intermediates and ~60–90 leaf materials across up to 8 worlds. Running `compare_routes` + `optimize_craft_choices` inside the existing `ultros/static/recipe-worker-client.mjs` Web Worker (`recipe_work.rs`) keeps typing (`on_hand` edits, target quantity edits) completely free of main-thread stalls.

Extend `ultros_calc::recipe_work` with `ProjectRequest` / `ProjectOutput` (sharing `Planner::previous` frontier caching so toggling Craft tabs or job filters never re-runs the travel-frontier knapsack unless leaf remaining quantities or market offers actually change).

---

## 6. Workspace UI & Interaction Design (`ListWorkspaceShell`, `Build`, `Craft`, `Shop`)

### 6.1 Navigation & Mode Toggle (`ListWorkspaceModes` & URL Contract)

- **Plain flat lists (`targets.is_empty()`):** `ListWorkspaceModes` keeps the exact 2-option control **`Build | Shop`**. Zero visual change for non-crafting lists.
- **Crafting project lists (`!targets.is_empty()`):** `ListWorkspaceModes` switches to a 3-option segmented control (`role="tablist"`): **`Build | Craft | Shop`**.
- **URL state (`list_view_sync.rs`):**
  - Replace boolean `?buy=true` with `?mode=build|craft|shop` (default `build`; if `?mode=craft` is visited on an empty-targets list, render `build`).
  - Keep reading legacy `?buy=true` as an alias for `mode=shop` so existing bookmarks and shared links stay unbroken.
- **Shell status & summary integration (`components/cart/estimate.rs`):**
  - When targets exist, `ListEstimateSummary` displays:
    1. **Shop progress:** `Acquired: X / Y leaf units` (`list_view_units_acquired_progress`)
    2. **Craft progress:** `Crafted: C / T project steps`
    3. **Project economics:** `Craft materials: 1,420,800 gil · Save 890,400 gil vs buying finished` (linking to **Craft**).

### 6.2 `Build` Tab Enhancements

1. **`InlineListAdd` Recipes tab (`routes/list_view_sync.rs`):**
   - Current select has `"Ingredients"` | `"Finished item"`. Add `"Craft target (recommended)"` as the default option when selecting a recipe, while keeping `"Direct ingredients (flat)"` and `"Finished item"` available.
   - Displays output quantity, craft count (`crafts * amount_result`), and a quick hint: `"Tracks precrafts & crafting order in the Craft tab."`
2. **Craft Targets section (above `ListCart` when `!targets.is_empty()`):**
   - Lists each `TargetSnapshot` with item icon, crafter job badge (`BSM`, `WVR`, `CUL`, …), HQ/NQ/Any selector, editable target `count`, inline `"Completed: X / Y"` badge, `"Open in /recipe/:id"` link, and remove button (`×`, undoable).
   - Quick-toggles right below targets: `Exclude crystals & shards` (`meta.exclude_crystals`) and `Include NPC vendors` (`meta.include_vendors`).
3. **Additional Flat Items (`ListCart`):**
   - Continues rendering manual `rows` (`need > 0`) as today (e.g., *Dark Matter Grade 8*, materia, dyes) alongside an optional read-only collapsed pill `"Plus N consolidated recipe materials → View in Craft / Shop"` so `Build` never explodes into 90 read-only rows while still letting players see or set price alerts (`target_price`) on any leaf item.

### 6.3 `Craft` Tab (`ultros-frontend/ultros-app/src/components/list_craft.rs`)

Mount lazily like `ListShop` (`class:hidden` once mounted so filter state and scroll position survive switching between `Build`, `Craft`, and `Shop`):

1. **Craft Action Bar:**
   - **Auto-Optimize button:** `"Optimize Craft / Buy"` (with live badge when savings > 0, e.g. `"Save 148k gil"` — disabled with `"Optimal"` check when already optimal).
   - **Job Filter Chips:** `All` + only the crafter jobs present in the project (`CRP`, `BSM`, `ARM`, `GSM`, `LTW`, `WVR`, `ALC`, `CUL`) with item counts, so a player can craft all `BSM` steps at once before switching jobs in game.
   - **Hide Completed** toggle (`aria-pressed`).
   - **Cycle / Missing Recipe Banner** (only shown on error): highlights the cyclic intermediate with a 1-click `"Switch {item} to Buy"` fix button.

2. **Section 1 — Shared Precrafts (Bottom-Up Tiered Order):**
   - Grouped by topological craft depth (`Tier 1: Base Precrafts`, `Tier 2: Intermediate Assemblies`, etc.) so players always craft from bottom to top without switching jobs unnecessarily.
   - Each intermediate card/row shows:
     - **Identity:** Icon, Item Name + `Clipboard` copy button, Job pill (`BSM Lv.100`), Craft yield badge (`×2/craft`), and attribution popover/subline (`"Used by: Blade (×2), Gauntlets (×2)"`).
     - **Craft vs Buy Control:** `[ Craft · 18.4k | Buy · 12.1k ]` segmented switch showing live unit/subtotal cost from the list's market scope (plus NPC badge when vendor is cheapest, or alternate recipe selector if `recipes_for_output.len() > 1`). Choosing **Buy** collapses child materials out of this section and adds the remaining units straight into **Shop**.
     - **Pre-existing `On hand` input:** numeric input (`data-committed` protected against `Ctrl+Z` hijacking) that reduces `effective_craft_units` and leaf shopping demand immediately.
     - **Execution progress (`Crafted`):** `- / +` stepper + `Done` button incrementing `craft_choices[item].crafted` (`LoroCounter`), plus a **readiness pill** derived from leaf `rows.acquired`:
       - Green `Ready` when all child leaf materials (and child precrafts) are acquired/crafted.
       - Amber `Need 4× Ore` (links straight to **Shop** filtered/scrolled to missing mats).

3. **Section 2 — Final Crafts (`targets`):**
   - Same compact layout: Icon, Name, Quality badge (`HQ`/`NQ`/`Any`), Job pill, readiness badge (`Ready` vs `Missing 1 precraft`), `Crafted` stepper (`completed / count`) + `Done`.

4. **Section 3 — Crystals & Catalysts Strip (Collapsible `<details>`):**
   - Aggregates shard/crystal/cluster totals across all Craft-selected nodes when `!exclude_crystals`, showing `acquired / needed` and a 1-click toggle to exclude/include crystals.

### 6.4 `Shop` Tab (`ultros-frontend/ultros-app/src/components/list_shop.rs`)

No duplicate routing logic:
- Feed `build_shop_input(...)` with `effective_leaf_rows = manual_rows + expanded_leaf_materials` (including Buy-toggled intermediates and excluding crystals if `meta.exclude_crystals`), subtracting `rows.acquired` and `craft_choices.on_hand`.
- `ListShop`'s existing travel-frontier cards (`Stay home` / `Fewest stops` / `Cheapest` + marginal gil-savings labels), frozen active-trip itinerary, whole-stack 0/1 knapsack, `"Listing is gone"` replacement, `Bought` / `Undo` callbacks (`Edit::RecordPurchase` writes straight to `rows[k].acquired`), NPC vendor summary (honoring `meta.include_vendors`), and Document Picture-in-Picture companion all work immediately out of the box!
- Clicking `Bought` on a raw material in Shop (or PiP) immediately turns the dependent precraft badge in the **Craft** tab from `Need mats` → `Ready`.

---

## 7. Site-Wide Entry Points

1. **Job Set Detail (`/items/jobset/:jobset/set/:ilvl`, `routes/job_set_detail.rs`):**
   - Alongside existing `"Add set"` and `"Add materials"`, add **`"Craft full set in a List"`**.
   - Opens a modal supporting both **account lists** and **device lists (`LocalListTargets`)**, with an HQ toggle (`Quality::Hq` vs `Quality::Any`) and `"Create new project list"` quick option. Adds every craftable piece in the set as a `TargetKey` in one undo step (`Edit::AddTargets`).
2. **Recipe Analyzer (`/recipe-analyzer`, `ultros-ui-crafting/src/components/add_recipe_to_list.rs`):**
   - `AddRecipeToListForm` adds a mode switch (`"Add as craft target"` default vs `"Add direct ingredients"`), allowing gil-makers to queue multiple profitable recipes into one batch craft project (account or device list).
3. **Recipe Planner (`/recipe/:id`, `routes/recipe_view.rs::SavePlan`):**
   - Today `SavePlan` fails if logged out and flattens leaves via REST `bulk_add_item_to_list`.
   - Upgrade `SavePlan` to:
     - Support **both** signed-in lists (via `ListDocHandle` / server `edit_as_server` v2 endpoint) **and** anonymous device lists (`LocalListTargets`).
     - Offer **`"Save as Craft Project"`** (default): saves the root target + all non-default `craft=` intermediate choices + `owned=` `on_hand` values + `shards-exclude` preference, then offers `"Open Craft Workspace →"`. Keep `"Save shopping rows only"` as secondary option.
4. **FC Crafting Analyzer (`/fc-crafting-analyzer`, `routes/fc_crafting_analyzer.rs`) — Phase 4 follow-up:**
   - Submarine/airship builds use `CompanyCraftSequence` (multi-part, multi-phase turn-ins rather than standard `RecipeId`). Modeling `fc_targets` (`sequence_id`) cleanly extends this architecture in Phase 4 once standard `Recipe` projects are live.

---

## 8. Dalamud Plugin & FFI Integration (`ultros-list-doc-ffi`, Phase 3)

### 8.1 ABI 2 Extensions (`ultros-list-doc-ffi/src/lib.rs`)

Preserve ABI 2 safety invariants (`#![forbid(unsafe_code)]` in `ultros-list-doc`, checked integer handles, `128 MiB` linear memory limit, zero `unwrap`/`expect`/unchecked slicing/arithmetic side effects):
- Expose `targets`, `craft_choices`, and `craft_options` in the `Request::View` JSON response.
- Add `Edit` variants:
  - `AddTarget { recipe_id: i32, quality: String, count: i64 }`
  - `SetTargetCount { recipe_id: i32, quality: String, count: i64 }`
  - `AddTargetCompleted { recipe_id: i32, quality: String, delta: i64 }`
  - `RemoveTarget { recipe_id: i32, quality: String }`
  - `SetCraftChoice { item_id: i32, recipe_id: Option<i32> }`
  - `SetIntermediateOnHand { item_id: i32, on_hand: i64 }`
  - `AddIntermediateCrafted { item_id: i32, delta: i64 }`
  - `SetCraftOptions { exclude_crystals: bool, include_vendors: bool }`
- Note: `ultros-list-doc-ffi` stays **game-data-free** (does not embed `xiv-gen-db`); C# plugin code passes `Recipe` definitions or uses the C# `Lumine`/`Excel` game sheets already present in Dalamud to expand/render the **Craft** tab locally and sync CRDT deltas back via `ListDoc.Edit`.

### 8.2 Inventory Auto-Progress Routing

Extend the optional inventory watcher (`plugins/dalamud/README.md` §"Optional inventory progress") cleanly for v2 projects:
- Incoming positive net delta of `item_id` (matching quality):
  1. If `item_id` is an active **target output** with remaining `completed < count`, increment `targets[k].completed` first.
  2. Else if `item_id` is an active **Craft-selected intermediate** (`recipe.is_some()`) with remaining `crafted < effective_craft_units`, increment `craft_choices[item_id].crafted`.
  3. Else if `item_id` is a **leaf / Buy row** with remaining `acquired < effective_leaf_need`, increment `rows[k].acquired`.
- Because `crafted` does not subtract from leaf `total_needed`, crafting an intermediate in-game advances the **Craft** bar automatically without disrupting raw-material `acquired` counts!

### 8.3 Compatibility Gate

- Plain v1 lists continue syncing smoothly on existing Dalamud versions.
- When a connected Dalamud client below the v2-capable release opens a schema v2 list, the server/API returns an explicit readable compatibility notice (`426` / `unsupported_schema` message prompting plugin update) rather than corrupting v2 maps, controlled via `ULTROS_PLUGIN_MINIMUM_VERSION` once Phase 3 ships.

---

## 9. Performance, Bundle Size, Accessibility & i18n

- **WASM bundle budget:** Both `loro` (`ultros-list-doc`) and `ultros_calc::recipe_planner` (with `recipe-worker-client.mjs`) are already compiled into `ultros-client`. No new external crates are introduced; estimated growth is ~20–35 KB brotli (well within historical margins, verified via `./scripts/measure_wasm_size.sh`).
- **SSR & Hydration safety:**
  - `ListCraft` mounts client-side inside the same hydration-safe boundary as `ListShop` (`Show` / `class:hidden` after mount) so SSR HTML never diverges from client local storage or IndexedDB snapshots.
  - All `Vec` iterations use deterministic sorted order (`BTreeMap` / explicit keys)—no `HashMap`-ordered DOM.
- **Accessibility & Keyboard UX:**
  - `Build | Craft | Shop` uses `role="tablist"` with `aria-selected`, `aria-controls`, and keyboard focus preservation.
  - Numeric `On hand` / `Target count` inputs stamp `data-committed` and stop `Escape`/`Ctrl+Z` propagation while drafting, honoring the existing `list_doc::undo` guard and `editor_has_drafts()` check (`routes/guest_lists.rs`).
  - Action feedback (`"Optimized: saved 148,200 gil across 2 precrafts"`) announces via `role="status"` / `aria-live="polite"`.
- **i18n (`ultros-frontend/ultros-i18n/locales/*.json`):**
  - Add all `list_craft_*` keys across all 7 locales (`en`, `ja`, `de`, `fr`, `cn`, `ko`, `tc`) with real domain translations (not English fallbacks).

---

## 10. Delivery Plan & Verification Matrix

### Phase 1 — Shared Engine & Planner Quick Wins (No schema change)
1. Refactor `ultros-calc::recipe_planner`: introduce `TargetDemand`, `ExpandLimits`, `expand_many`, `optimize_craft_choices`, and delegate single-root `expand` to `expand_many`.
2. Add `ProjectRequest` / `ProjectOutput` to `ultros-calc::recipe_work` and worker boundary.
3. Upgrade `/recipe/:id` (`SavePlan`) to support `LocalListTargets` (anonymous guest lists) alongside signed-in lists.
- **Verify:** Unit & property tests in `ultros-calc` (shared intermediate rounding, `on_hand` vs `crafted` invariants, cycle detection, bottom-up DP optimality vs exhaustive search on small DAGs), `./check_ci.sh`, `cargo leptos build`.

### Phase 2 — Schema v2, Server Projection & `Build | Craft | Shop` Workspace
1. `ultros-list-doc`: add `TargetKey`, `TargetSnapshot`, `CraftChoiceSnapshot`, `CraftOptionsSnapshot`, `SCHEMA_VERSION_CRAFT = 2`, validation bounds, `collaborative_only_since`, Loro undo coverage.
2. `ultros-db` + `ultros-lists`: inject `RecipeCatalog`, project consolidated leaf demand into `list_item` inside `merge_loaded` & `make_list_online`, enforce `Write` vs `Owner` metadata permissions.
3. `ultros-app` + `ultros-ui-crafting`: `ListWorkspaceModes` 3-tab shell (`?mode=craft`), `ListCraft` component (`components/list_craft.rs`), `InlineRecipeAdd` default target mode, `AddSetToList` on `/items/jobset/:jobset/set/:ilvl`, `AddRecipeToListForm` target mode, `/recipe/:id` `"Save as Craft Project"`, and 7-locale i18n keys.
- **Verify:**
  - `ultros-list-doc` fuzz/property tests: schema v1 backwards compatibility, schema v1 → v2 lazy promotion, concurrent multiplayer convergence across random edit interleavings, undo/redo symmetry, snapshot byte & op limits.
  - `ultros-db` Postgres tests: `Write` permission allows target/craft edits while blocking `meta.name`/`meta.scope`; `list_item` projection stays in sync with targets & `on_hand`; guest `make_list_online` preserves targets & craft choices.
  - `./check_ci.sh`, `cargo leptos build --bin-features test-auth`, and new E2E Puppeteer suite `integration/list-craft-project.cjs` via `./scripts/run_e2e.sh` (SSR/hydration parity, desktop + mobile screenshots, Craft ↔ Shop reactive propagation, zero `console.error`).

### Phase 3 — Dalamud FFI & Plugin Craft Tab
1. Extend `ultros-list-doc-ffi` (`Edit` + `View` + `#![forbid(unsafe_code)]` + clippy strict lints) and verify `wasm32-wasip1` build.
2. Add **Craft** sub-tab & inventory routing priority (Targets → Craft intermediates → Leaf rows) in `plugins/dalamud`.
- **Verify:** `pwsh -File plugins/dalamud/build-plugin.ps1 -Configuration Release -StageOnly`, `Ultros.Lists.Smoke`, `node integration/dalamud-device-flow.cjs`.

### Phase 4 — FC Workshop Sequences (Follow-Up)
- Model `CompanyCraftSequence` phases as `fc_targets` in schema v2 (or v3) and wire `fc_crafting_analyzer.rs` `"Add project to List"`.

---

## 11. Non-Goals

- **Crafting macro / rotation simulator** (Teamcraft/Raphael territory; Ultros focuses on economics, sourcing, batch planning, and execution tracking).
- **Gathering timer / node-location maps** (handled via existing item sources and external links).
- **Manual drag-and-drop custom row ordering** (preserved from Lists 2.0: ordering stays deterministic by topological craft tier / job / sort order).
- **Replacing stateless `/recipe/:id` links** (single-recipe URLs remain shareable with zero sign-in or storage).
