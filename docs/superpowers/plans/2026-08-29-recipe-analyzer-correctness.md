# Recipe Analyzer Correctness (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the recipe analyzer's profit math true: per-unit costing for multi-yield recipes, NPC vendor prices as an ingredient cost floor, 5% market-board tax deducted from revenue, and revenue defaulting to the selected world's price.

**Architecture:** All changes live in the Leptos frontend crate (`ultros-frontend/ultros-app`). The shared cost engine (`components/crafting_cost.rs`) gains a vendor-price floor; the analyzer route (`routes/recipe_analyzer.rs`) gains two pure helpers (per-unit cost, net-after-tax) wired into its `computed_data` memo; `price_basis.rs` flips the `RevenueMetric` default. No backend/API changes.

**Tech Stack:** Rust, Leptos 0.8 (SSR+wasm), leptos-i18n, xiv-gen game data.

**Spec:** `docs/superpowers/specs/2026-08-29-recipe-analyzer-improvements-design.md` (Phase 1).

## Global Constraints

- Every new/changed user-facing string goes in ALL 7 locale files under `ultros-frontend/ultros-app/locales/` (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`), with real translations (this plan provides them).
- Run `./check_ci.sh` (fmt + clippy) before every commit; read its exit code via `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"` — never through a pipe.
- Windows builds need Strawberry Perl first on PATH (Git Bash: `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`).
- Unit tests: `cargo test -p ultros-app --lib` (the `ultros` bin's tests don't link on Windows; don't run workspace-wide test commands).
- Filter URL keys are a stable contract — this phase adds no new filter keys, so `ADDABLE_FILTERS` and its pinning test must NOT change.
- Work happens in this worktree (`.claude/worktrees/recipe-analyzer-improvements-c077a9`), branch `claude/recipe-analyzer-improvements-c077a9`, using the worktree's own `target/`.
- No `#[allow]` to silence clippy; fix the code.

---

### Task 1: Vendor price floor in the cost engine

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/crafting_cost.rs`
- Modify (call sites, mechanical): `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs:491`, `ultros-frontend/ultros-app/src/routes/item_view.rs` (near lines 980-995), `ultros-frontend/ultros-app/src/components/related_items.rs` (near lines 200-230 and 410-430)

**Interfaces:**
- Produces:
  - `pub enum PriceSource { Market, Vendor, Subcraft }` (Copy, Clone, Debug, PartialEq, Eq) in `crafting_cost.rs`
  - `IngredientLine` gains `pub source: PriceSource`
  - `CraftingCostOptions` gains `pub vendor_prices: Option<&'a HashMap<i32, i32>>` (item_id → NPC unit price)
  - `pub fn vendor_price_map() -> &'static HashMap<i32, i32>` in `crafting_cost.rs` — lazily built from game data, for production call sites
- Consumes: existing `compute_cost` / `compute_ingredient_cost` internals; `tracked_data()` from `crate::global_state::xiv_data`.

**Semantics to implement:**
- Vendor floor applies only when `!opts.require_hq` (vendor goods are NQ; never override an HQ ingredient preference).
- A vendor price of `<= 0` is ignored.
- Vendor price wins when the market has no listing (`market == 0`) or when `vendor < market`. This also fixes "no listing → free ingredient" for vendor-sold items.
- The subcraft comparison automatically competes against the vendor-floored `unit_cost` (no extra code needed beyond ordering: floor first, subcraft check after). When a subcraft wins, the line's `source` becomes `PriceSource::Subcraft`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crafting_cost.rs` (after the existing `compute_ingredient_cost` tests). Note these won't compile yet — that's the failure signal for struct-field additions:

```rust
fn vendor_map(entries: &[(i32, i32)]) -> HashMap<i32, i32> {
    entries.iter().copied().collect()
}

#[test]
fn vendor_price_undercuts_market() {
    let prices = one_listing(100, false, 50, 1);
    let vendors = vendor_map(&[(100, 20)]);
    let oh = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: 0,
        shards: ShardsMode::ExcludeShards,
        on_hand: &oh,
        vendor_prices: Some(&vendors),
    };
    let line = compute_ingredient_cost(ItemId(100), 3, &prices, &opts);
    assert_eq!(line.unit_price, 20);
    assert_eq!(line.source, PriceSource::Vendor);
}

#[test]
fn market_price_undercuts_vendor() {
    let prices = one_listing(100, false, 50, 1);
    let vendors = vendor_map(&[(100, 80)]);
    let oh = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: 0,
        shards: ShardsMode::ExcludeShards,
        on_hand: &oh,
        vendor_prices: Some(&vendors),
    };
    let line = compute_ingredient_cost(ItemId(100), 3, &prices, &opts);
    assert_eq!(line.unit_price, 50);
    assert_eq!(line.source, PriceSource::Market);
}

#[test]
fn vendor_price_used_when_no_listing() {
    let prices = one_listing(999, false, 50, 1); // listing for a different item
    let vendors = vendor_map(&[(100, 20)]);
    let oh = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: 0,
        shards: ShardsMode::ExcludeShards,
        on_hand: &oh,
        vendor_prices: Some(&vendors),
    };
    let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
    assert_eq!(line.unit_price, 20);
    assert_eq!(line.source, PriceSource::Vendor);
}

#[test]
fn zero_vendor_price_is_ignored() {
    let prices = one_listing(100, false, 50, 1);
    let vendors = vendor_map(&[(100, 0)]);
    let oh = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: 0,
        shards: ShardsMode::ExcludeShards,
        on_hand: &oh,
        vendor_prices: Some(&vendors),
    };
    let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
    assert_eq!(line.unit_price, 50);
    assert_eq!(line.source, PriceSource::Market);
}

#[test]
fn require_hq_ignores_vendor_floor() {
    // HQ listing at 100, vendor at 20: an HQ-required ingredient must not be
    // silently downgraded to the NQ vendor copy.
    let prices = one_listing(100, true, 100, 1);
    let vendors = vendor_map(&[(100, 20)]);
    let oh = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq: true,
        max_subcraft_depth: 0,
        shards: ShardsMode::ExcludeShards,
        on_hand: &oh,
        vendor_prices: Some(&vendors),
    };
    let line = compute_ingredient_cost(ItemId(100), 1, &prices, &opts);
    assert_eq!(line.unit_price, 100);
    assert_eq!(line.source, PriceSource::Market);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-app --lib crafting_cost`
Expected: COMPILE ERROR — `CraftingCostOptions` has no field `vendor_prices`, `PriceSource` not found.

- [ ] **Step 3: Implement**

In `crafting_cost.rs`:

3a. New enum (near `ShardsMode`):

```rust
/// Where an ingredient line's `unit_price` came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PriceSource {
    /// Cheapest matching market listing.
    Market,
    /// NPC gil-shop price (always NQ; never used when `require_hq`).
    Vendor,
    /// Crafting the ingredient via a sub-recipe was cheaper than buying.
    Subcraft,
}
```

3b. Add fields:

```rust
pub struct CraftingCostOptions<'a> {
    pub require_hq: bool,
    pub max_subcraft_depth: u8,
    pub shards: ShardsMode,
    pub on_hand: &'a dyn OnHand,
    /// item_id -> NPC gil-shop unit price. `None` disables the vendor floor.
    pub vendor_prices: Option<&'a HashMap<i32, i32>>,
}
```

`item_page_default` sets `vendor_prices: Some(vendor_price_map())`.

`IngredientLine` gains `pub source: PriceSource` (after `is_shard`).

3c. In `compute_ingredient_cost`, after computing the market `unit_price` (rename the existing binding to `market_price`):

```rust
let vendor = if opts.require_hq {
    None
} else {
    opts.vendor_prices
        .and_then(|m| m.get(&item_id.0).copied())
        .filter(|p| *p > 0)
};
let (unit_price, source) = match vendor {
    Some(v) if market_price == 0 || v < market_price => (v, PriceSource::Vendor),
    _ => (market_price, PriceSource::Market),
};
```

and set `source` on the returned `IngredientLine`.

3d. In `compute_cost_inner`, where a winning subcraft re-prices the line (`line.unit_price = unit_cost;` around line 231), also set `line.source = PriceSource::Subcraft;` — but only inside the `if sub_unit > 0 && sub_unit < unit_cost` winner path (guard: only when `!best_sub_crafts.is_empty()` after the loop, i.e. set it next to the existing re-price when a winner was promoted).

3e. The lazily built production map (in `crafting_cost.rs`, outside the test module):

```rust
use std::sync::OnceLock;

/// item_id -> NPC gil-shop unit price, for every gil-shop-sold item.
/// Built once per process from game data (same construction as the
/// vendor-resale page).
pub fn vendor_price_map() -> &'static HashMap<i32, i32> {
    static MAP: OnceLock<HashMap<i32, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = crate::global_state::xiv_data::tracked_data();
        let mut map = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if let Some(item) = data.items.get(&xiv_gen::ItemId(shop_item.item)) {
                    if item.price_mid > 0 {
                        map.insert(shop_item.item, item.price_mid as i32);
                    }
                }
            }
        }
        map
    })
}
```

(If `tracked_data()`'s return type or the `gil_shop_items` iteration differs, mirror exactly what `routes/vendor_resale.rs:282-293` does — it builds the same map.)

3f. Update every `CraftingCostOptions { ... }` literal:
- All test literals in `crafting_cost.rs`: add `vendor_prices: None,`
- `routes/recipe_analyzer.rs` (the `opts` literal near line 491): `vendor_prices: Some(vendor_price_map()),` (import `vendor_price_map` from the crafting_cost module).
- `routes/item_view.rs` and `components/related_items.rs` literals: `vendor_prices: Some(vendor_price_map()),` — the item page's crafting-cost display must agree with the analyzer.
- Any construction via `item_page_default` needs no change.

Let the compiler find every literal: `cargo check -p ultros-app` and fix each error.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-app --lib crafting_cost`
Expected: PASS (all new tests + all pre-existing crafting_cost tests, which still use `vendor_prices: None` and must be unaffected).

- [ ] **Step 5: check_ci and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -A ultros-frontend/ultros-app/src
git commit -m "feat(recipe-analyzer): price ingredients at the NPC vendor floor"
```

---

### Task 2: Per-unit costing for multi-yield recipes

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (computed_data memo near lines 497-516; cost cell near line 1143; tests module)
- Modify: all 7 files in `ultros-frontend/ultros-app/locales/` (new key `recipe_analyzer_yield_note`)

**Interfaces:**
- Produces: `fn per_unit_cost(craft_cost: i32, amount_result: i32) -> i32` (private to `recipe_analyzer.rs`; Task 3 composes with it).
- Consumes: `Recipe::amount_result` (already on the xiv-gen `Recipe`; the subcraft path in `crafting_cost.rs:216` already divides by it).

- [ ] **Step 1: Write the failing tests**

In `recipe_analyzer.rs`'s `mod test`:

```rust
/// A craft costs `craft_cost` and yields `amount_result` units; the table
/// prices everything per unit. Guards the degenerate `amount_result == 0`
/// rows some sheets carry.
#[test]
fn per_unit_cost_divides_by_yield() {
    assert_eq!(per_unit_cost(300, 3), 100);
    assert_eq!(per_unit_cost(300, 1), 300);
    assert_eq!(per_unit_cost(300, 0), 300);
    assert_eq!(per_unit_cost(100, 3), 33); // integer division, floor
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros-app --lib per_unit_cost`
Expected: COMPILE ERROR — `per_unit_cost` not found.

- [ ] **Step 3: Implement**

3a. Helper (near `craft_type_acronym`):

```rust
/// Cost of one unit of output: one craft costs `craft_cost` and yields
/// `amount_result` units. Yields of 0 (bad sheet rows) are treated as 1.
fn per_unit_cost(craft_cost: i32, amount_result: i32) -> i32 {
    craft_cost / amount_result.max(1)
}
```

3b. In `computed_data`, replace the stale TODO block:

```rust
// craft_cost is the cost of one execution of the recipe, which yields
// `amount_result` units; the market price is per unit, so compare per unit.
let cost_per_unit = per_unit_cost(craft_cost, recipe.amount_result);
```

(delete the old comment claiming result quantities aren't exposed).

3c. Yield note in the cost cell (inside the existing cost `role="cell"` div, after the `<Gil amount=data.cost />` and before the subcraft `Show`):

```rust
{(data.recipe.amount_result > 1)
    .then(|| view! {
        <div class="text-xs text-[color:var(--color-text-muted)]">
            {t!(i18n, recipe_analyzer_yield_note, n = move || data.recipe.amount_result)}
        </div>
    })}
```

3d. Locale key, all 7 files (alphabetical placement next to the other `recipe_analyzer_*` keys):

| file | value |
|---|---|
| en.json | `"recipe_analyzer_yield_note": "×{{ n }} per craft"` |
| fr.json | `"recipe_analyzer_yield_note": "×{{ n }} par synthèse"` |
| de.json | `"recipe_analyzer_yield_note": "×{{ n }} pro Herstellung"` |
| ja.json | `"recipe_analyzer_yield_note": "1回の製作で ×{{ n }}"` |
| cn.json | `"recipe_analyzer_yield_note": "每次制作 ×{{ n }}"` |
| ko.json | `"recipe_analyzer_yield_note": "제작 1회당 ×{{ n }}"` |
| tc.json | `"recipe_analyzer_yield_note": "每次製作 ×{{ n }}"` |

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-app --lib per_unit_cost`
Expected: PASS. Also run the full route tests: `cargo test -p ultros-app --lib recipe_analyzer` — all pre-existing tests still pass.

- [ ] **Step 5: check_ci and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -A ultros-frontend/ultros-app
git commit -m "fix(recipe-analyzer): cost multi-yield recipes per unit"
```

---

### Task 3: Deduct the 5% market-board tax from revenue

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (computed_data memo; tests module)
- Modify: all 7 locale files (`recipe_analyzer_calc_formula` and `recipe_analyzer_calc_details` values)

**Interfaces:**
- Produces: `fn net_after_tax(gross: i32) -> i32` and `const MARKET_TAX_PERCENT: i64 = 5;` (private to `recipe_analyzer.rs`).
- Consumes: `per_unit_cost` from Task 2.

**Semantics:** Profit and ROI (and the profitability cutoff) use net revenue; the **Price column keeps showing the gross listing price** (what you'd actually list at). `market_price == 0 → skip` stays as-is.

- [ ] **Step 1: Write the failing tests**

```rust
/// The market board takes 5% of every sale; profit must be computed on the
/// 95% the seller actually receives, rounded down.
#[test]
fn net_after_tax_takes_five_percent() {
    assert_eq!(net_after_tax(100), 95);
    assert_eq!(net_after_tax(1), 0); // floor, not round
    assert_eq!(net_after_tax(0), 0);
    assert_eq!(net_after_tax(1_999_999_999), 1_899_999_999); // no i32 overflow
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros-app --lib net_after_tax`
Expected: COMPILE ERROR — `net_after_tax` not found.

- [ ] **Step 3: Implement**

3a. Helper (next to `per_unit_cost`):

```rust
/// The market board's cut of every sale.
const MARKET_TAX_PERCENT: i64 = 5;

/// What the seller actually receives from a sale listed at `gross`.
fn net_after_tax(gross: i32) -> i32 {
    (gross as i64 * (100 - MARKET_TAX_PERCENT) / 100) as i32
}
```

3b. In `computed_data`, after `cost_per_unit` (Task 2) and the existing `market_price == 0` skip:

```rust
let net_revenue = net_after_tax(market_price);
if cost_per_unit >= net_revenue {
    continue;
}
let profit = net_revenue - cost_per_unit;
```

(replacing the old `if cost_per_unit >= market_price { continue; }` and `let profit = market_price - cost_per_unit;`; ROI keeps dividing by `cost_per_unit`). `RecipeProfitData::market_price` continues to hold the gross price for the Price column.

3c. Locale value updates — replace both keys in every file:

en.json:
```json
"recipe_analyzer_calc_formula": "profit = (market price − 5% tax) − cost per unit",
"recipe_analyzer_calc_details": "Ingredient cost uses the cheapest matching listings, or the NPC vendor price when that is cheaper. Recipes that produce multiple items are costed per unit. Revenue assumes selling on your selected world and deducts the 5% market board tax. Subcraft mode checks whether crafting intermediate ingredients is cheaper than buying them directly.",
```

fr.json:
```json
"recipe_analyzer_calc_formula": "profit = (prix du marché − 5 % de taxe) − coût par unité",
"recipe_analyzer_calc_details": "Le coût des ingrédients utilise les annonces les moins chères, ou le prix du vendeur PNJ s'il est inférieur. Les recettes produisant plusieurs objets sont calculées à l'unité. Le revenu suppose une vente sur votre monde sélectionné et déduit la taxe de 5 % du comptoir de vente. Le mode sous-synthèse vérifie s'il est moins cher de fabriquer les ingrédients intermédiaires que de les acheter.",
```

de.json:
```json
"recipe_analyzer_calc_formula": "Gewinn = (Marktpreis − 5 % Steuer) − Kosten pro Einheit",
"recipe_analyzer_calc_details": "Die Zutatenkosten verwenden die günstigsten Angebote oder den NPC-Händlerpreis, wenn dieser niedriger ist. Rezepte mit mehreren Ergebnissen werden pro Einheit berechnet. Der Erlös geht vom Verkauf auf deiner gewählten Welt aus und zieht die 5 % Marktbrett-Steuer ab. Der Zwischenprodukt-Modus prüft, ob das Herstellen von Zwischenmaterialien günstiger ist als der Kauf.",
```

ja.json:
```json
"recipe_analyzer_calc_formula": "利益 = (市場価格 − 税5%) − 1個あたりのコスト",
"recipe_analyzer_calc_details": "素材コストは最安の出品価格、またはそれより安い場合はNPCショップ価格を使用します。複数個できるレシピは1個あたりで計算します。収益は選択中のワールドでの販売を想定し、マーケットの税5%を差し引きます。中間素材モードでは、中間素材を製作する方が購入より安いかを確認します。",
```

cn.json:
```json
"recipe_analyzer_calc_formula": "利润 = (市场价格 − 5% 税) − 单件成本",
"recipe_analyzer_calc_details": "材料成本采用最便宜的在售列表，若NPC商店价格更低则采用商店价格。产出多件的配方按单件计算成本。收益假设在所选服务器出售，并扣除5%的市场税。半成品模式会检查制作中间材料是否比直接购买更便宜。",
```

ko.json:
```json
"recipe_analyzer_calc_formula": "이익 = (시장 가격 − 세금 5%) − 개당 비용",
"recipe_analyzer_calc_details": "재료 비용은 가장 저렴한 판매 목록을 사용하며, NPC 상점 가격이 더 싸면 상점 가격을 사용합니다. 여러 개가 만들어지는 제작법은 개당 비용으로 계산합니다. 수익은 선택한 서버에서의 판매를 가정하고 시장 세금 5%를 차감합니다. 중간 재료 모드는 중간 재료를 직접 제작하는 것이 구매보다 저렴한지 확인합니다.",
```

tc.json:
```json
"recipe_analyzer_calc_formula": "利潤 = (市場價格 − 5% 稅) − 單件成本",
"recipe_analyzer_calc_details": "材料成本採用最便宜的在售列表，若NPC商店價格更低則採用商店價格。產出多件的配方按單件計算成本。收益假設在所選伺服器出售，並扣除5%的市場稅。半成品模式會檢查製作中間材料是否比直接購買更便宜。",
```

(Note: the details text also describes Task 1's vendor floor and Task 4's world default — this task lands the copy once so the locales are touched a single time.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-app --lib recipe_analyzer`
Expected: PASS (new `net_after_tax` test + all existing).

- [ ] **Step 5: check_ci and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -A ultros-frontend/ultros-app
git commit -m "fix(recipe-analyzer): deduct the 5% market tax from profit"
```

---

### Task 4: Default revenue to the selected world's price

**Files:**
- Modify: `ultros-frontend/ultros-app/src/price_basis.rs` (enum default + doc comments + tests)

**Interfaces:**
- Produces: `RevenueMetric::default() == RevenueMetric::WorldMin`.
- Consumes: nothing new. `routes/recipe_analyzer.rs` needs **no code change**: the `world_min_world` memo, the world-listings resource, the `.or_else(scope-min)` fallback, and the chip's `filter(|m| *m != RevenueMetric::default())` all key off `RevenueMetric::default()` / `== WorldMin` and adapt automatically.

- [ ] **Step 1: Update the tests (they pin the old default)**

In `price_basis.rs` tests, replace `defaults_reproduce_historical_behavior`:

```rust
#[test]
fn defaults() {
    assert_eq!(CostBasis::default(), CostBasis::ListingMin);
    // Revenue defaults to the selected world's cheapest listing — you sell
    // on your own world, not wherever the region-wide minimum happens to be.
    assert_eq!(RevenueMetric::default(), RevenueMetric::WorldMin);
    assert_eq!(MarketScope::default(), MarketScope::Region);
    assert_eq!(CostBasis::default().sale_stat(), None);
    assert_eq!(RevenueMetric::default().sale_stat(), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros-app --lib price_basis`
Expected: FAIL — `RevenueMetric::default()` is still `ListingMin`.

- [ ] **Step 3: Implement**

Move the `#[default]` attribute in `RevenueMetric` from `ListingMin` to `WorldMin`, and swap the doc comments:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum RevenueMetric {
    /// Cheapest current listing in scope.
    ListingMin,
    SaleMedian,
    SaleMin,
    SaleAvg,
    /// Cheapest current listing on the analyzer's selected world — the
    /// price you'd actually list at. Falls back to the scope-wide listing
    /// when the world has none up.
    #[default]
    WorldMin,
}
```

Also update the module doc comment (line 10-11, "Defaults reproduce the analyzer's historical behavior exactly") to note the revenue default intentionally changed to WorldMin on 2026-08-29.

- [ ] **Step 4: Run tests + spot-check behavior**

Run: `cargo test -p ultros-app --lib price_basis` — PASS.
Run: `cargo test -p ultros-app --lib` — full lib suite PASS (nothing else pins the default).
Sanity: in `routes/recipe_analyzer.rs`, confirm (read, no edit) that with no `revenue=` query param, `world_min_world` now yields `Some(world)` and the table's `RevenueMetric::WorldMin` arm runs with the scope fallback.

- [ ] **Step 5: check_ci and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add ultros-frontend/ultros-app/src/price_basis.rs
git commit -m "feat(recipe-analyzer): default revenue to the selected world's price"
```

---

### Task 5: Changelog entry + full verification

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/changelog.rs` (new entry at the TOP of the entry list, matching the existing entry structure exactly)

- [ ] **Step 1: Add changelog entry**

Read the top of the existing entries in `changelog.rs` and add a new dated entry (2026-08-29, or merge date) at the top, following the established structure/format, with this content:

> Recipe Analyzer: profit math overhaul — multi-yield recipes are now costed per unit, NPC vendor prices floor ingredient costs, the 5% market board tax is deducted from profit, and revenue now defaults to your selected world's price instead of the region-wide minimum.

- [ ] **Step 2: Full test suite + CI check**

```bash
cargo test -p ultros-app --lib
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: tests PASS, `REAL_EXIT=0`. (If clippy is OOM-killed — exit 137 — re-run with `cargo clippy --all-targets -j 2 -- -D warnings`.)

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/changelog.rs
git commit -m "docs(changelog): recipe analyzer profit math overhaul"
```

---

## Verification (manual, after all tasks)

Optional but recommended before the PR: run the app locally (or rely on prod-vs-branch comparison at review time) and confirm on `/recipe-analyzer`:
1. A known multi-yield recipe (e.g. most CUL foods) now shows a plausible per-unit cost with the "×N per craft" note.
2. A recipe whose mats are vendor-sold (e.g. anything using cheap base ingredients like Alumen) shows a lower cost than before.
3. Profit ≈ 0.95 × price − cost for a spot-checked row.
4. With no `revenue=` in the URL, the numbers change when switching the selected world (world-default in effect).
