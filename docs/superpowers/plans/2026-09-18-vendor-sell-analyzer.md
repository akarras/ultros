# Vendor Sell Analyzer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new analyzer page, Vendor Sell, that lists market-board listings priced below what an NPC vendor pays for the item, after the buyer-side 5% market tax.

**Architecture:** Pure math lives in `ultros-calc` (`purchase_tax_for`, `vendor_sell_line`). A new route file `ultros-frontend/ultros-app/src/routes/vendor_sell.rs` builds rows from the region's cheapest listings joined against `Item.price_low`, and renders them with the analyzer kit (`MarketGrid`, `ControlBar`, `register_filters`, `Calculation`) exactly like Venture Analyzer. Discovery surfaces (sidebar, home chip, help topic, sitemap, changelog) and i18n keys complete the feature.

**Tech Stack:** Rust, Leptos 0.8 (SSR + hydrate), leptos-i18n, `ultros-calc`, `xiv_gen` game data, Puppeteer e2e in `integration/`.

Spec: `docs/superpowers/specs/2026-09-18-vendor-sell-analyzer-design.md`.

## Global Constraints

- Run `./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"` before every commit (fmt + clippy `-D warnings` + `scripts/check_tests.sh`). On Windows/Git Bash prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH` and export `OPENSSL_RUST_USE_NASM=0`, `CARGO_PROFILE_DEV_DEBUG=0`.
- Every user-facing string goes through `t!(i18n, key)` / `t_string!(i18n, key)`. Every new key must exist in all seven locale files: `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json`, with real translations.
- Keys are `snake_case`, prefixed `vendor_sell_`.
- Tax: flat 5%, rounded **up**, integer math with `i64` intermediates.
- HQ listings are valued at the NQ `price_low`. Rows with `profit <= 0` are dropped.
- Route: `/vendor-sell` and `/vendor-sell/:world` (one optional-param route `vendor-sell/:world?`).
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Work only inside the worktree `C:\Users\chw11\code\ultros\.claude\worktrees\vendor-price-analyzer-cca1ad` on branch `claude/vendor-price-analyzer-cca1ad`. Verify with `git rev-parse --show-toplevel` before any git command.

## File map

| File | Responsibility |
|---|---|
| `ultros-frontend/ultros-calc/src/formula.rs` | `purchase_tax_for`, `VendorSellLine`, `vendor_sell_line` + tests |
| `ultros-frontend/ultros-app/src/routes/vendor_sell.rs` (new) | row model, `build_rows`, `vendor_prices_from_data`, `SortMode`, table + page components, tests |
| `ultros-frontend/ultros-app/src/routes/mod.rs` | `pub mod vendor_sell;` |
| `ultros-frontend/ultros-app/src/lib.rs` | glob import + `<Route>` |
| `ultros-frontend/ultros-app/src/analyzer_kit/filters.rs` | shared `category_id_token` (moved from vendor_resale.rs) |
| `ultros-frontend/ultros-app/src/routes/vendor_resale.rs` | use the shared `category_id_token` |
| `ultros-frontend/ultros-app/src/components/side_nav.rs` | sidebar entry |
| `ultros-frontend/ultros-app/src/routes/home_page.rs` | tool chip |
| `ultros-frontend/ultros-app/src/routes/help.rs` | help topic |
| `ultros/src/web/sitemap.rs` | tool URL + help slug |
| `ultros-frontend/ultros-i18n/locales/*.json` | `vendor_sell_*` keys |
| `ultros-changelog/changes/2026-09-18-vendor-sell-analyzer.json` | release note |
| `integration/analyzer-grids.cjs`, `integration/shared-analyzer-data.cjs` | e2e route lists |

---

### Task 1: Buyer-side tax and profit line in `ultros-calc`

**Files:**
- Modify: `ultros-frontend/ultros-calc/src/formula.rs` (after `sale_tax_for`, around line 378; tests in the existing `mod tests` at the bottom)

**Interfaces:**
- Produces:
  ```rust
  pub fn purchase_tax_for(listing: i32) -> i32;
  #[derive(Copy, Clone, Debug, PartialEq, Eq)]
  pub struct VendorSellLine { pub listing: i32, pub tax: i32, pub cost: i32, pub vendor_price: i32, pub profit: i32, pub margin: i32 }
  pub fn vendor_sell_line(listing: i32, vendor_price: i32) -> VendorSellLine;
  ```

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `ultros-frontend/ultros-calc/src/formula.rs`:

```rust
    #[test]
    fn purchase_tax_rounds_up_to_the_buyers_disadvantage() {
        assert_eq!(purchase_tax_for(0), 0);
        assert_eq!(purchase_tax_for(1), 1);
        assert_eq!(purchase_tax_for(19), 1);
        assert_eq!(purchase_tax_for(20), 1);
        assert_eq!(purchase_tax_for(21), 2);
        assert_eq!(purchase_tax_for(99), 5);
        assert_eq!(purchase_tax_for(100), 5);
        assert_eq!(purchase_tax_for(101), 6);
        // i64 intermediate: must not overflow or panic.
        assert_eq!(purchase_tax_for(i32::MAX), 107_374_183);
    }

    #[test]
    fn vendor_sell_line_worked_example() {
        let line = vendor_sell_line(101, 120);
        assert_eq!(
            line,
            VendorSellLine {
                listing: 101,
                tax: 6,
                cost: 107,
                vendor_price: 120,
                profit: 13,
                margin: 12,
            }
        );
    }

    #[test]
    fn vendor_sell_line_zero_and_negative_profit() {
        assert_eq!(vendor_sell_line(100, 105).profit, 0);
        assert_eq!(vendor_sell_line(100, 105).margin, 0);
        let loss = vendor_sell_line(1_000, 500);
        assert_eq!(loss.tax, 50);
        assert_eq!(loss.cost, 1_050);
        assert_eq!(loss.profit, -550);
        assert_eq!(loss.margin, -52);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-calc purchase_tax vendor_sell_line`
Expected: compile error, `purchase_tax_for` / `vendor_sell_line` not found.

- [ ] **Step 3: Implement**

Insert after `sale_tax_for` in `formula.rs`:

```rust
/// The market board's cut charged to the *buyer* at purchase, rounded up
/// so a 1-gil edge never flatters the buyer. The real rate is 3–5% depending
/// on the retainer's city; we assume the worst case.
pub fn purchase_tax_for(listing: i32) -> i32 {
    ((listing as i64 * MARKET_TAX_PERCENT).div_ceil(100)) as i32
}

/// One "buy off the board, sell to an NPC" row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VendorSellLine {
    pub listing: i32,
    pub tax: i32,
    /// `listing + tax`: what the buyer actually pays.
    pub cost: i32,
    pub vendor_price: i32,
    pub profit: i32,
    /// `profit / cost`, as a clamped percentage.
    pub margin: i32,
}

/// `profit = vendor_price − (listing + purchase tax)`.
pub fn vendor_sell_line(listing: i32, vendor_price: i32) -> VendorSellLine {
    let tax = purchase_tax_for(listing);
    let cost = listing.saturating_add(tax);
    let profit = vendor_price.saturating_sub(cost);
    VendorSellLine {
        listing,
        tax,
        cost,
        vendor_price,
        profit,
        margin: crate::analysis::return_on_investment(profit, cost),
    }
}
```

`i64::div_ceil` is stable. `return_on_investment` (in `analysis.rs:333`) returns 0 when cost `<= 0` and clamps at ±100,000.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-calc purchase_tax vendor_sell_line`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-calc/src/formula.rs
git commit -m "feat(calc): buyer-side purchase tax and vendor sell line

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Share `category_id_token` via the analyzer kit

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/filters.rs` (append)
- Modify: `ultros-frontend/ultros-app/src/routes/vendor_resale.rs:59-77` (delete the local fn) and its import block

**Interfaces:**
- Produces: `pub fn category_id_token(id: i32) -> &'static str` in `crate::analyzer_kit::filters`.

- [ ] **Step 1: Move the function**

Cut lines 59–77 of `vendor_resale.rs` (the doc comment and `fn category_id_token`) and paste them at the end of `analyzer_kit/filters.rs` as `pub fn category_id_token`. The body is unchanged:

```rust
/// Intern a category id as a `&'static str` token for
/// [`FilterChip`](ultros_ui::components::filter_chip::FilterChip)'s
/// `(&'static str, String)` options contract.
///
/// `item_search_categorys` is a small, fixed-size table read from the
/// process-lifetime game data (`xiv_gen_db::data()`), so the set of ids ever
/// asked for here is bounded — each one is leaked exactly once and cached,
/// never per-render, so this cannot grow unbounded over a long session.
pub fn category_id_token(id: i32) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<i32, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("category token cache poisoned");
    guard
        .entry(id)
        .or_insert_with(|| Box::leak(id.to_string().into_boxed_str()))
}
```

In `vendor_resale.rs` change the import

```rust
use crate::analyzer_kit::filters::{
    duration_value, price_control, register_filters, toggle_control,
};
```
to
```rust
use crate::analyzer_kit::filters::{
    category_id_token, duration_value, price_control, register_filters, toggle_control,
};
```

- [ ] **Step 2: Verify it compiles and the vendor resale tests still pass**

Run: `cargo test -p ultros-app vendor_resale`
Expected: all vendor_resale tests pass, no warnings about unused imports.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/filters.rs ultros-frontend/ultros-app/src/routes/vendor_resale.rs
git commit -m "refactor(analyzer-kit): share category_id_token

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: i18n keys for Vendor Sell

**Files:**
- Modify: all seven of `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json`
- Create (scratch, not committed): `$CLAUDE_SCRATCHPAD/add_vendor_sell_keys.py`

**Interfaces:**
- Produces the keys below, used verbatim by Tasks 4–6.

- [ ] **Step 1: Write the merge script**

Write this to `$CLAUDE_SCRATCHPAD/add_vendor_sell_keys.py`. It reads each locale as UTF-8, inserts keys that are missing, and writes back with 4-space indent and `ensure_ascii=False` (matching the existing files). Use the Write tool, not a bash heredoc.

```python
import json, sys
from pathlib import Path

ROOT = Path(sys.argv[1])
LOCALES = ROOT / "ultros-frontend/ultros-i18n/locales"

E = {
 "vendor_sell": {"en":"Vendor Sell","fr":"Vente au PNJ","de":"Händlerverkauf","ja":"NPC売却","cn":"NPC出售","ko":"NPC 판매","tc":"NPC出售"},
 "vendor_sell_desc": {"en":"Find market listings that sell to an NPC vendor for more than they cost","fr":"Trouvez des annonces qui se revendent à un PNJ plus cher qu'elles ne coûtent","de":"Finde Marktangebote, die ein NPC-Händler für mehr ankauft, als sie kosten","ja":"購入価格よりNPC売却価格が高い出品を探す","cn":"查找NPC收购价高于购买成本的市场出品","ko":"구매 비용보다 NPC 판매가가 높은 시장 매물 찾기","tc":"尋找NPC收購價高於購買成本的市場出品"},
 "vendor_sell_meta_title": {"en":"Vendor Sell - Ultros","fr":"Vente au PNJ - Ultros","de":"Händlerverkauf - Ultros","ja":"NPC売却 - Ultros","cn":"NPC出售 - Ultros","ko":"NPC 판매 - Ultros","tc":"NPC出售 - Ultros"},
 "vendor_sell_meta_desc": {"en":"Find market board listings priced below the NPC vendor sell-back price, after tax.","fr":"Trouvez des annonces du marché en dessous du prix de rachat PNJ, taxe comprise.","de":"Finde Marktbrett-Angebote unter dem NPC-Ankaufspreis, nach Steuern.","ja":"税込みでもNPC売却価格を下回るマーケット出品を探します。","cn":"查找税后仍低于NPC收购价的市场板出品。","ko":"세금 포함 후에도 NPC 판매가보다 싼 시장판 매물을 찾습니다.","tc":"尋找稅後仍低於NPC收購價的市場板出品。"},
 "vendor_sell_tool_summary": {"en":"Buy the listing, sell it to any NPC vendor, keep the difference.","fr":"Achetez l'annonce, revendez-la à n'importe quel PNJ, gardez la différence.","de":"Angebot kaufen, an einen beliebigen NPC-Händler verkaufen, Differenz behalten.","ja":"出品を購入し、任意のNPCに売却して差額を得ます。","cn":"买下出品，卖给任意NPC，赚取差价。","ko":"매물을 사서 아무 NPC에게 팔고 차액을 남기세요.","tc":"買下出品，賣給任意NPC，賺取差價。"},
 "vendor_sell_tool_context": {"en":"Scans the cheapest listing of every marketable item across the selected world's region. The 5% purchase tax is always included; your retainer city may charge less.","fr":"Analyse l'annonce la moins chère de chaque objet vendable dans la région du monde choisi. La taxe d'achat de 5 % est toujours incluse ; votre cité de servant peut prélever moins.","de":"Prüft das günstigste Angebot jedes handelbaren Gegenstands in der Region der gewählten Welt. Die 5 % Kaufsteuer ist immer enthalten; deine Gehilfen-Stadt verlangt womöglich weniger.","ja":"選択したワールドの地域全体で、取引可能な各アイテムの最安出品を調べます。購入税5%は常に含まれます。リテイナーの都市によってはより低い場合があります。","cn":"扫描所选服务器所在区域内每件可交易物品的最低出品。始终计入5%购买税；你的雇员所在城市可能更低。","ko":"선택한 월드 지역 전체에서 거래 가능한 모든 아이템의 최저 매물을 검사합니다. 5% 구매세는 항상 포함되며, 집사 도시에 따라 더 낮을 수 있습니다.","tc":"掃描所選伺服器所在區域內每件可交易物品的最低出品。始終計入5%購買稅；你的雇員所在城市可能更低。"},
 "vendor_sell_tool_help": {"en":"Listings come and go quickly. Travel to the listing's world, buy, then sell to any vendor. Profit is per unit; the listing's stack size is not shown.","fr":"Les annonces vont et viennent vite. Rendez-vous sur le monde de l'annonce, achetez, puis revendez à n'importe quel PNJ. Le profit est par unité ; la taille de la pile n'est pas affichée.","de":"Angebote kommen und gehen schnell. Reise zur Welt des Angebots, kaufe und verkaufe an einen beliebigen Händler. Der Gewinn gilt pro Stück; die Stapelgröße wird nicht angezeigt.","ja":"出品はすぐに入れ替わります。出品ワールドへ移動して購入し、任意のNPCに売却してください。利益は1個あたりで、スタック数は表示されません。","cn":"出品变动很快。前往出品所在服务器购买，然后卖给任意NPC。利润按单件计算，不显示堆叠数量。","ko":"매물은 빠르게 바뀝니다. 매물 월드로 이동해 구매한 뒤 아무 NPC에게 파세요. 이익은 개당이며 묶음 수량은 표시되지 않습니다.","tc":"出品變動很快。前往出品所在伺服器購買，然後賣給任意NPC。利潤按單件計算，不顯示堆疊數量。"},
 "vendor_sell_calc_title": {"en":"How profit is estimated","fr":"Comment le profit est estimé","de":"So wird der Gewinn geschätzt","ja":"利益の計算方法","cn":"利润计算方式","ko":"이익 계산 방식","tc":"利潤計算方式"},
 "vendor_sell_calc_formula": {"en":"profit = vendor price - listing price - 5% purchase tax","fr":"profit = prix PNJ - prix de l'annonce - taxe d'achat de 5 %","de":"Gewinn = Händlerpreis - Angebotspreis - 5 % Kaufsteuer","ja":"利益 = NPC売却価格 - 出品価格 - 購入税5%","cn":"利润 = NPC收购价 - 出品价 - 5%购买税","ko":"이익 = NPC 판매가 - 매물 가격 - 5% 구매세","tc":"利潤 = NPC收購價 - 出品價 - 5%購買稅"},
 "vendor_sell_calc_details": {"en":"Tax is rounded up. HQ listings are valued at the NQ vendor price, so HQ rows never overstate. Only rows with a positive profit are shown.","fr":"La taxe est arrondie au supérieur. Les annonces HQ sont valorisées au prix PNJ NQ, donc jamais surestimées. Seules les lignes à profit positif sont affichées.","de":"Die Steuer wird aufgerundet. HQ-Angebote werden zum NQ-Händlerpreis bewertet und daher nie überschätzt. Nur Zeilen mit positivem Gewinn werden angezeigt.","ja":"税は切り上げです。HQ出品はNQのNPC売却価格で評価するため過大評価されません。利益がプラスの行のみ表示します。","cn":"税额向上取整。HQ出品按NQ收购价估值，不会高估。仅显示利润为正的行。","ko":"세금은 올림 처리됩니다. HQ 매물은 NQ 판매가로 평가하므로 과대평가되지 않습니다. 이익이 양수인 행만 표시합니다.","tc":"稅額向上取整。HQ出品按NQ收購價估值，不會高估。僅顯示利潤為正的行。"},
 "vendor_sell_assumption_tax": {"en":"Flat 5% tax, rounded up","fr":"Taxe fixe de 5 %, arrondie au supérieur","de":"Pauschal 5 % Steuer, aufgerundet","ja":"税率5%固定・切り上げ","cn":"固定5%税，向上取整","ko":"고정 5% 세금, 올림","tc":"固定5%稅，向上取整"},
 "vendor_sell_assumption_hq": {"en":"HQ valued at NQ vendor price","fr":"HQ valorisé au prix PNJ NQ","de":"HQ zum NQ-Händlerpreis bewertet","ja":"HQはNQ売却価格で評価","cn":"HQ按NQ收购价估值","ko":"HQ는 NQ 판매가로 평가","tc":"HQ按NQ收購價估值"},
 "vendor_sell_assumption_per_unit": {"en":"One unit per row; listing may be gone","fr":"Une unité par ligne ; l'annonce peut disparaître","de":"Ein Stück pro Zeile; Angebot kann weg sein","ja":"1行=1個・出品は消える可能性あり","cn":"每行一件；出品可能已售出","ko":"행당 1개, 매물이 사라질 수 있음","tc":"每行一件；出品可能已售出"},
 "vendor_sell_col_hq": {"en":"HQ","fr":"HQ","de":"HQ","ja":"HQ","cn":"HQ","ko":"HQ","tc":"HQ"},
 "vendor_sell_col_item": {"en":"Item","fr":"Objet","de":"Gegenstand","ja":"アイテム","cn":"物品","ko":"아이템","tc":"物品"},
 "vendor_sell_col_world": {"en":"World","fr":"Monde","de":"Welt","ja":"ワールド","cn":"服务器","ko":"월드","tc":"伺服器"},
 "vendor_sell_col_listing": {"en":"Listing","fr":"Annonce","de":"Angebot","ja":"出品価格","cn":"出品价","ko":"매물 가격","tc":"出品價"},
 "vendor_sell_col_tax": {"en":"Tax","fr":"Taxe","de":"Steuer","ja":"税","cn":"税","ko":"세금","tc":"稅"},
 "vendor_sell_col_vendor_price": {"en":"Vendor price","fr":"Prix PNJ","de":"Händlerpreis","ja":"NPC売却価格","cn":"NPC收购价","ko":"NPC 판매가","tc":"NPC收購價"},
 "vendor_sell_col_profit": {"en":"Profit","fr":"Profit","de":"Gewinn","ja":"利益","cn":"利润","ko":"이익","tc":"利潤"},
 "vendor_sell_col_margin": {"en":"Margin","fr":"Marge","de":"Marge","ja":"利益率","cn":"利润率","ko":"마진","tc":"利潤率"},
 "vendor_sell_filter_profit_min_label": {"en":"Profit (Min)","fr":"Profit (min)","de":"Gewinn (min.)","ja":"利益（最小）","cn":"利润（最低）","ko":"이익 (최소)","tc":"利潤（最低）"},
 "vendor_sell_filter_category_label": {"en":"Category","fr":"Catégorie","de":"Kategorie","ja":"カテゴリ","cn":"分类","ko":"분류","tc":"分類"},
 "vendor_sell_result_count": {"en":"{{ n }} listings","fr":"{{ n }} annonces","de":"{{ n }} Angebote","ja":"{{ n }} 件の出品","cn":"{{ n }} 条出品","ko":"매물 {{ n }}개","tc":"{{ n }} 筆出品"},
 "vendor_sell_no_filters_hint": {"en":"No filters — showing every profitable listing","fr":"Aucun filtre — toutes les annonces rentables","de":"Keine Filter — alle profitablen Angebote","ja":"フィルターなし — 利益の出る全出品を表示","cn":"无筛选 — 显示所有可获利出品","ko":"필터 없음 — 모든 수익 매물 표시","tc":"無篩選 — 顯示所有可獲利出品"},
 "vendor_sell_error_listings": {"en":"Error loading listings: ","fr":"Erreur de chargement des annonces : ","de":"Fehler beim Laden der Angebote: ","ja":"出品の読み込みエラー: ","cn":"加载出品出错：","ko":"매물 불러오기 오류: ","tc":"載入出品出錯："},
 "vendor_sell_preset_best_profit": {"en":"Best profit","fr":"Meilleur profit","de":"Bester Gewinn","ja":"最高利益","cn":"最高利润","ko":"최고 이익","tc":"最高利潤"},
 "vendor_sell_preset_best_margin": {"en":"Best margin","fr":"Meilleure marge","de":"Beste Marge","ja":"最高利益率","cn":"最高利润率","ko":"최고 마진","tc":"最高利潤率"},
}

for loc in ["en","fr","de","ja","cn","ko","tc"]:
    p = LOCALES / f"{loc}.json"
    data = json.loads(p.read_text(encoding="utf-8"))
    added = 0
    for key, vals in E.items():
        if key not in data:
            data[key] = vals[loc]
            added += 1
    p.write_text(json.dumps(data, ensure_ascii=False, indent=4) + "\n", encoding="utf-8")
    print(loc, "added", added)
```

- [ ] **Step 2: Run it**

Run from the worktree root: `python "$CLAUDE_SCRATCHPAD/add_vendor_sell_keys.py" .`
Expected: seven lines, each `added 28`.

- [ ] **Step 3: Verify the diff is keys only**

Run: `git diff --stat ultros-frontend/ultros-i18n/locales/ && git diff ultros-frontend/ultros-i18n/locales/en.json | grep '^[-+]' | grep -v '^[-+][-+]' | grep -vc vendor_sell`
Expected: seven files changed; the final count is `0` or `1` (the `1` is the previous last line gaining a trailing comma). If the count is larger, the file was reformatted; inspect `git diff` and fix the script's indent/`ensure_ascii` to match before continuing.

- [ ] **Step 4: Confirm the i18n crate builds**

Run: `cargo check -p ultros-i18n`
Expected: success, no "missing key" warnings mentioning `vendor_sell`.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-i18n/locales/
git commit -m "i18n: vendor sell analyzer keys in all locales

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Row model, `build_rows` and sorting (pure logic, TDD)

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/vendor_sell.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs` (add `pub mod vendor_sell;` in alphabetical order after `vendor_resale`)

**Interfaces:**
- Consumes: `ultros_calc::formula::vendor_sell_line` (Task 1), `ultros_api_types::cheapest_listings::{CheapestListings, CheapestListingItem}`.
- Produces (all `pub(crate)` or private, used by Task 5 in the same file):
  ```rust
  pub(crate) struct VendorSellRow { item_id: i32, hq: bool, world_id: i32, listing: i32, tax: i32, cost: i32, vendor_price: i32, profit: i32, margin: i32 }
  fn build_rows(listings: &CheapestListings, vendor_prices: &HashMap<i32, i32>) -> Vec<Arc<VendorSellRow>>
  fn vendor_prices_from_data() -> HashMap<i32, i32>
  enum SortMode { Profit, Margin, Listing, Tax, VendorPrice, World }   // impl FromStr, Display, SortColumn
  fn compare_rows(mode: SortMode, a: &VendorSellRow, b: &VendorSellRow) -> Ordering
  fn sort_rows(rows: &mut [Arc<VendorSellRow>], mode: SortMode, dir: SortDir)
  ```

- [ ] **Step 1: Create the file with the failing tests**

Write `ultros-frontend/ultros-app/src/routes/vendor_sell.rs`:

```rust
//! Vendor Sell: market listings priced below the NPC vendor sell-back price.
//!
//! Spec: docs/superpowers/specs/2026-09-18-vendor-sell-analyzer-design.md

use crate::components::sort_header::{SortColumn, SortDir};
use crate::global_state::xiv_data::tracked_data;
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
use ultros_api_types::cheapest_listings::CheapestListings;
use ultros_calc::formula::vendor_sell_line;
use xiv_gen::ItemId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VendorSellRow {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    pub listing: i32,
    pub tax: i32,
    pub cost: i32,
    pub vendor_price: i32,
    pub profit: i32,
    pub margin: i32,
}

/// `item_id -> price_low` for every item a player can both buy on the board
/// and sell to an NPC.
fn vendor_prices_from_data() -> HashMap<i32, i32> {
    tracked_data()
        .items
        .iter()
        .filter(|(_, item)| item.item_search_category != 0 && item.price_low > 0)
        .map(|(id, item)| (id.0, item.price_low as i32))
        .collect()
}

/// Join listings against vendor prices, drop anything that does not profit,
/// and return a deterministic (item id, then NQ before HQ) order.
fn build_rows(
    listings: &CheapestListings,
    vendor_prices: &HashMap<i32, i32>,
) -> Vec<Arc<VendorSellRow>> {
    let mut rows: Vec<Arc<VendorSellRow>> = listings
        .cheapest_listings
        .iter()
        .filter_map(|listing| {
            let vendor_price = *vendor_prices.get(&listing.item_id)?;
            let line = vendor_sell_line(listing.cheapest_price, vendor_price);
            (line.profit > 0).then(|| {
                Arc::new(VendorSellRow {
                    item_id: listing.item_id,
                    hq: listing.hq,
                    world_id: listing.world_id,
                    listing: line.listing,
                    tax: line.tax,
                    cost: line.cost,
                    vendor_price: line.vendor_price,
                    profit: line.profit,
                    margin: line.margin,
                })
            })
        })
        .collect();
    rows.sort_by_key(|row| (row.item_id, row.hq));
    rows
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    Margin,
    Listing,
    Tax,
    VendorPrice,
    World,
}

impl std::str::FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "margin" => Ok(SortMode::Margin),
            "listing" => Ok(SortMode::Listing),
            "tax" => Ok(SortMode::Tax),
            "vendor-price" => Ok(SortMode::VendorPrice),
            "world" => Ok(SortMode::World),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortMode::Profit => "profit",
            SortMode::Margin => "margin",
            SortMode::Listing => "listing",
            SortMode::Tax => "tax",
            SortMode::VendorPrice => "vendor-price",
            SortMode::World => "world",
        })
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }

    /// Money columns read best-first descending; the cost-like columns
    /// (listing, tax) and world read ascending.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::Profit | SortMode::Margin | SortMode::VendorPrice => SortDir::Desc,
            SortMode::Listing | SortMode::Tax | SortMode::World => SortDir::Asc,
        }
    }
}

fn compare_rows(mode: SortMode, a: &VendorSellRow, b: &VendorSellRow) -> Ordering {
    match mode {
        SortMode::Profit => a.profit.cmp(&b.profit),
        SortMode::Margin => a.margin.cmp(&b.margin),
        SortMode::Listing => a.listing.cmp(&b.listing),
        SortMode::Tax => a.tax.cmp(&b.tax),
        SortMode::VendorPrice => a.vendor_price.cmp(&b.vendor_price),
        SortMode::World => a.world_id.cmp(&b.world_id),
    }
    .then_with(|| a.item_id.cmp(&b.item_id))
    .then_with(|| a.hq.cmp(&b.hq))
}

fn sort_rows(rows: &mut [Arc<VendorSellRow>], mode: SortMode, dir: SortDir) {
    rows.sort_by(|a, b| {
        let order = compare_rows(mode, a, b);
        if dir == SortDir::Asc {
            order
        } else {
            order.reverse()
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use ultros_api_types::cheapest_listings::CheapestListingItem;

    fn listing(item_id: i32, hq: bool, cheapest_price: i32, world_id: i32) -> CheapestListingItem {
        CheapestListingItem {
            item_id,
            hq,
            cheapest_price,
            world_id,
        }
    }

    fn listings(items: Vec<CheapestListingItem>) -> CheapestListings {
        CheapestListings {
            cheapest_listings: items,
        }
    }

    #[test]
    fn items_without_a_vendor_price_never_become_rows() {
        // item 2 is absent from the catalog: unmarketable or price_low == 0.
        let rows = build_rows(
            &listings(vec![listing(1, false, 10, 7), listing(2, false, 10, 7)]),
            &[(1, 100)].into_iter().collect(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 1);
    }

    #[test]
    fn unprofitable_rows_are_dropped() {
        // 100 + 5 tax = 105 cost. Vendor 105 → profit 0 → dropped.
        let rows = build_rows(
            &listings(vec![listing(1, false, 100, 7), listing(2, false, 100, 7)]),
            &[(1, 105), (2, 106)].into_iter().collect(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 2);
        assert_eq!(rows[0].profit, 1);
    }

    #[test]
    fn hq_listing_is_valued_at_nq_price_and_keeps_its_flag_and_world() {
        let rows = build_rows(
            &listings(vec![listing(1, true, 101, 42), listing(1, false, 200, 43)]),
            &[(1, 120)].into_iter().collect(),
        );
        // NQ row at 200 costs 210 > 120 → dropped; HQ row survives.
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.hq);
        assert_eq!(row.world_id, 42);
        assert_eq!(row.listing, 101);
        assert_eq!(row.tax, 6);
        assert_eq!(row.cost, 107);
        assert_eq!(row.vendor_price, 120);
        assert_eq!(row.profit, 13);
        assert_eq!(row.margin, 12);
    }

    #[test]
    fn build_rows_order_is_item_then_nq_before_hq() {
        let rows = build_rows(
            &listings(vec![
                listing(5, true, 1, 1),
                listing(3, false, 1, 1),
                listing(5, false, 1, 1),
            ]),
            &[(3, 100), (5, 100)].into_iter().collect(),
        );
        let keys: Vec<_> = rows.iter().map(|r| (r.item_id, r.hq)).collect();
        assert_eq!(keys, vec![(3, false), (5, false), (5, true)]);
    }

    #[test]
    fn default_sort_is_profit_descending_and_asc_reverses() {
        let mut rows = build_rows(
            &listings(vec![
                listing(1, false, 10, 1),
                listing(2, false, 10, 1),
                listing(3, false, 10, 1),
            ]),
            &[(1, 50), (2, 500), (3, 20)].into_iter().collect(),
        );
        let mode = SortMode::fallback();
        assert_eq!(mode, SortMode::Profit);
        assert_eq!(mode.default_dir(), SortDir::Desc);
        sort_rows(&mut rows, mode, mode.default_dir());
        let ids: Vec<_> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![2, 1, 3]);
        sort_rows(&mut rows, mode, SortDir::Asc);
        let ids: Vec<_> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![3, 1, 2]);
    }

    #[test]
    fn sort_tokens_round_trip() {
        for token in ["profit", "margin", "listing", "tax", "vendor-price", "world"] {
            assert_eq!(SortMode::from_str(token).unwrap().to_string(), token);
        }
        assert!(SortMode::from_str("roi").is_err());
    }
}
```

Then add `pub mod vendor_sell;` to `routes/mod.rs` after `pub mod vendor_resale;`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p ultros-app vendor_sell`
Expected: 6 passed. (If `SortColumn` requires anything beyond `fallback`/`default_dir`, open `ultros-frontend/ultros-app/src/analyzer_kit/columns.rs:380-400` and mirror the venture analyzer's impl.) There will be dead-code warnings for the unused fns; that is expected until Task 5 and must be gone before the final `check_ci.sh`.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/vendor_sell.rs ultros-frontend/ultros-app/src/routes/mod.rs
git commit -m "feat(vendor-sell): row model, build_rows and sorting

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Page and table components, route registration

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/vendor_sell.rs` (append components above `mod tests`, extend imports)
- Modify: `ultros-frontend/ultros-app/src/lib.rs:75` (glob import) and `:638` (route)

**Interfaces:**
- Consumes: Task 4 fns; i18n keys from Task 3; `category_id_token` from Task 2.
- Produces: `pub fn VendorSell() -> impl IntoView` (the route view).

- [ ] **Step 1: Extend the imports**

Replace the import block at the top of `vendor_sell.rs` with:

```rust
use super::world_nav::use_analyzer_world;
use crate::analyzer_kit::calculation::{Calculation, CalculationStrip, CalculationTerm};
use crate::analyzer_kit::filters::{category_id_token, register_filters};
use crate::analyzer_kit::market::{MarketGrid, MarketSubject, use_market_data};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::term_badge::TermRole;
use crate::components::virtual_grid::metrics::{FilterOp, GridMetric, GridValue};
use crate::components::virtual_grid::registry::FilterAlias;
use crate::components::virtual_grid::saved_views::{
    GridPresetView, GridSavedViews, provide_grid_saved_views,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::query_defaults::{filter_query_signal, query_signal};
use crate::ws::realtime::use_realtime;
use crate::{
    analysis::roi_badge_class,
    api::get_cheapest_listings,
    components::{
        add_to_list::AddToList,
        clipboard::*,
        control_bar::ControlBar,
        gil::*,
        item_icon::*,
        realtime_status::RealtimeStatus,
        skeleton::BoxSkeleton,
        sort_header::{SortColumn, SortDir, SortableHeaderCell},
        tool_help::*,
        virtual_grid::{ColumnFilter, GridColumn},
        world_picker::WorldOnlyPicker,
    },
    global_state::{LocalWorldData, region_for_world::use_region_for_world},
};
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use std::{cmp::Ordering, collections::HashMap, sync::Arc};
use thousands::Separable;
use ultros_api_types::cheapest_listings::CheapestListings;
use ultros_calc::formula::vendor_sell_line;
use xiv_gen::ItemId;
```

If `crate::analysis::roi_badge_class` does not resolve, use the path Vendor Resale uses (`use crate::analysis::{..., roi_badge_class};` at `vendor_resale.rs:1`).

- [ ] **Step 2: Add filter constants, presets and the world-name lookup**

Insert after `sort_rows`:

```rust
// --- Filter registry -------------------------------------------------------
// Each id is the `filter_query_signal` key it drives, so the list doubles as
// the URL contract.
const FILTER_PROFIT: &str = "profit";
const FILTER_CATEGORY: &str = "category";

/// Queries only: labels live in [`vendor_sell_presets`] because `t_string!`
/// needs a literal key. Every key used here is pinned by a test below.
const PRESET_QUERIES: [&str; 2] = ["?sort=profit", "?sort=margin"];

fn vendor_sell_presets(i18n: I18nContext<Locale, I18nKeys>) -> Vec<GridPresetView> {
    [
        t_string!(i18n, vendor_sell_preset_best_profit).to_string(),
        t_string!(i18n, vendor_sell_preset_best_margin).to_string(),
    ]
    .into_iter()
    .zip(PRESET_QUERIES)
    .map(|(label, query)| GridPresetView {
        label,
        query: query.to_string(),
    })
    .collect()
}

#[cfg(test)]
const LEGACY_PRESET_FILTER_KEYS: &[&str] = &[FILTER_PROFIT, FILTER_CATEGORY];

/// `world_id -> world name` for the World column.
fn world_names() -> Arc<HashMap<i32, String>> {
    Arc::new(
        use_context::<LocalWorldData>()
            .and_then(|v| v.0.ok())
            .map(|helper| {
                helper
                    .get_inner_data()
                    .regions
                    .iter()
                    .flat_map(|r| r.datacenters.iter())
                    .flat_map(|dc| dc.worlds.iter())
                    .map(|w| (w.id, w.name.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    )
}
```

The `regions -> datacenters -> worlds` walk copies `analyzer_kit/market.rs:863-880`; open it and match the field names exactly if they differ.

- [ ] **Step 3: Add the table component**

Insert after `world_names`:

```rust
type Row = (usize, Arc<VendorSellRow>);

fn vendor_sell_metrics(worlds: Arc<HashMap<i32, String>>) -> Vec<GridMetric<Row>> {
    let items = &tracked_data().items;
    vec![
        GridMetric::text("item", move |(_, row): &Row| {
            GridValue::Text(
                items
                    .get(&ItemId(row.item_id))
                    .map(|item| item.name.clone())
                    .unwrap_or_default(),
            )
        }),
        GridMetric::text("hq", |(_, row): &Row| {
            GridValue::Text(if row.hq { "HQ" } else { "NQ" }.into())
        }),
        GridMetric::text("world", move |(_, row): &Row| {
            GridValue::Text(worlds.get(&row.world_id).cloned().unwrap_or_default())
        }),
        GridMetric::number("listing", |(_, row): &Row| GridValue::Number(row.listing as f64)),
        GridMetric::number("tax", |(_, row): &Row| GridValue::Number(row.tax as f64)),
        GridMetric::number("vendor-price", |(_, row): &Row| {
            GridValue::Number(row.vendor_price as f64)
        }),
        GridMetric::number("profit", |(_, row): &Row| GridValue::Number(row.profit as f64)),
        GridMetric::number("margin", |(_, row): &Row| GridValue::Number(row.margin as f64)),
    ]
}

#[component]
fn VendorSellTable(listings: CheapestListings, region: Signal<String>) -> impl IntoView {
    let i18n = use_i18n();
    let realtime = use_realtime();
    let rt_status = realtime.clone();
    let realtime_status = Signal::derive(move || {
        rt_status
            .as_ref()
            .map(|r| r.status.get())
            .unwrap_or_else(|| "offline".to_string())
    });
    let rt_update = realtime;
    let last_update = Signal::derive(move || rt_update.as_ref().and_then(|r| r.last_update.get()));
    let market = use_market_data(region);
    let worlds = world_names();
    let items = &tracked_data().items;

    let all_rows = build_rows(&listings, &vendor_prices_from_data());

    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (sort_dir, _set_sort_dir) = query_signal::<SortDir>("dir");
    let (category_filter, _set_category_filter) = filter_query_signal::<i32>(FILTER_CATEGORY);

    let sorted_rows = Memo::new(move |_| {
        let mut rows: Vec<Arc<VendorSellRow>> = all_rows
            .iter()
            .filter(|row| {
                category_filter()
                    .map(|cat_id| {
                        items
                            .get(&ItemId(row.item_id))
                            .map(|item| item.item_search_category == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        let mode = sort_mode().unwrap_or_else(SortMode::fallback);
        let dir = sort_dir().unwrap_or_else(|| mode.default_dir());
        sort_rows(&mut rows, mode, dir);
        rows.into_iter().enumerate().collect::<Vec<Row>>()
    });

    let category_options = move || {
        let mut categories = tracked_data()
            .item_search_categorys
            .iter()
            .filter(|(_, cat)| !cat.name.is_empty())
            .map(|(id, cat)| (id.0, cat.name.clone()))
            .collect::<Vec<_>>();
        categories.sort_by(|a, b| a.1.cmp(&b.1));
        categories
            .into_iter()
            .map(|(id, name)| (category_id_token(id), name))
            .collect::<Vec<_>>()
    };

    let filter_label = move |id: &str| -> String {
        match id {
            FILTER_PROFIT => t_string!(i18n, vendor_sell_filter_profit_min_label).to_string(),
            FILTER_CATEGORY => t_string!(i18n, vendor_sell_filter_category_label).to_string(),
            _ => String::new(),
        }
    };

    let filters = register_filters(
        vec![FilterAlias::integer(FILTER_PROFIT, "profit", FilterOp::Gte)],
        Signal::derive(move || {
            vec![
                ColumnFilter::new(FILTER_PROFIT, filter_label(FILTER_PROFIT), true),
                {
                    let mut f =
                        ColumnFilter::new(FILTER_CATEGORY, filter_label(FILTER_CATEGORY), false);
                    f.options = category_options();
                    f
                },
            ]
        }),
    );

    let presets = Signal::derive(move || vendor_sell_presets(i18n));

    let calculation = Calculation::provide(
        filters,
        vec![
            CalculationTerm::fixed(
                TermRole::Result,
                t_string!(i18n, vendor_sell_col_profit).to_string(),
                Some("profit"),
            ),
            CalculationTerm::fixed(
                TermRole::Revenue,
                t_string!(i18n, vendor_sell_col_vendor_price).to_string(),
                Some("vendor-price"),
            ),
            CalculationTerm::fixed(
                TermRole::Cost,
                t_string!(i18n, vendor_sell_col_listing).to_string(),
                Some("listing"),
            ),
            CalculationTerm::fixed(
                TermRole::Tax,
                t_string!(i18n, vendor_sell_col_tax).to_string(),
                Some("tax"),
            ),
        ],
        None,
    );

    let sorted = move |m: SortMode| {
        (
            sort_mode.get().unwrap_or_else(SortMode::fallback) == m,
            sort_dir.get().unwrap_or_else(|| m.default_dir()) == SortDir::Asc,
        )
    };
    let hq_badge_class = "px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]";

    view! {
        <div class="flex flex-col gap-6">
            <div class="flex flex-wrap items-start gap-3">
                <CalculationStrip calculation window=market.window />
            </div>

            <ControlBar sticky=false
                summary=move || {
                    view! {
                        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
                            {move || t!(i18n, vendor_sell_result_count, n = move || filters.row_count())}
                        </span>
                    }
                    .into_any()
                }
                actions=move || {
                    view! {
                        <RealtimeStatus status=realtime_status last_update=last_update />
                        <GridSavedViews id="vendor-sell-grid" presets=presets />
                    }
                    .into_any()
                }
                empty_label=Signal::derive(move || {
                    t_string!(i18n, vendor_sell_no_filters_hint).to_string()
                })
            />

            <div>
                <MarketGrid show_saved_views=false id="vendor-sell-grid"
                    label=t_string!(i18n, vendor_sell_col_item).to_string()
                    row_height=40.0
                    market
                    metrics=vendor_sell_metrics(worlds.clone())
                    subject=Arc::new(move |(_, row): &Row| {
                        let mut subject = MarketSubject::new(row.item_id, row.hq, row.world_id);
                        subject.listing_price = Some(row.listing);
                        subject
                    })
                    columns=Signal::derive(move || {
                        let (profit_on, profit_asc) = sorted(SortMode::Profit);
                        let (margin_on, margin_asc) = sorted(SortMode::Margin);
                        let (listing_on, listing_asc) = sorted(SortMode::Listing);
                        let (tax_on, tax_asc) = sorted(SortMode::Tax);
                        let (vendor_on, vendor_asc) = sorted(SortMode::VendorPrice);
                        let (world_on, world_asc) = sorted(SortMode::World);
                        vec![
                            GridColumn::new("hq", t_string!(i18n, vendor_sell_col_hq).to_string(), 60.0, true, true),
                            GridColumn::new("item", t_string!(i18n, vendor_sell_col_item).to_string(), 320.0, false, true),
                            GridColumn::new("world", t_string!(i18n, vendor_sell_col_world).to_string(), 130.0, false, true).sorted(world_on, world_asc),
                            GridColumn::new("listing", t_string!(i18n, vendor_sell_col_listing).to_string(), 120.0, true, true).sorted(listing_on, listing_asc),
                            GridColumn::new("tax", t_string!(i18n, vendor_sell_col_tax).to_string(), 90.0, true, true).sorted(tax_on, tax_asc),
                            GridColumn::new("vendor-price", t_string!(i18n, vendor_sell_col_vendor_price).to_string(), 130.0, true, true).sorted(vendor_on, vendor_asc),
                            {
                                let mut col = GridColumn::new("profit", t_string!(i18n, vendor_sell_col_profit).to_string(), 130.0, true, true).sorted(profit_on, profit_asc);
                                col.filters.push(ColumnFilter::new(FILTER_PROFIT, filter_label(FILTER_PROFIT), true));
                                col
                            },
                            GridColumn::new("margin", t_string!(i18n, vendor_sell_col_margin).to_string(), 100.0, true, true).sorted(margin_on, margin_asc),
                        ]
                    })
                    header=move |id| {
                        let cell = |mode: SortMode, label: String| view! {
                            <SortableHeaderCell embedded=true mode label class="w-full min-w-0" sort_mode sort_dir />
                        }.into_any();
                        match id {
                            "hq" => view! { <div class="text-center w-full min-w-0">{t!(i18n, vendor_sell_col_hq)}</div> }.into_any(),
                            "item" => view! { <div class="w-full min-w-0">{t!(i18n, vendor_sell_col_item)}</div> }.into_any(),
                            "world" => cell(SortMode::World, t_string!(i18n, vendor_sell_col_world).to_string()),
                            "listing" => cell(SortMode::Listing, t_string!(i18n, vendor_sell_col_listing).to_string()),
                            "tax" => cell(SortMode::Tax, t_string!(i18n, vendor_sell_col_tax).to_string()),
                            "vendor-price" => cell(SortMode::VendorPrice, t_string!(i18n, vendor_sell_col_vendor_price).to_string()),
                            "profit" => cell(SortMode::Profit, t_string!(i18n, vendor_sell_col_profit).to_string()),
                            "margin" => cell(SortMode::Margin, t_string!(i18n, vendor_sell_col_margin).to_string()),
                            _ => ().into_any(),
                        }
                    }
                    each=sorted_rows
                    key=move |(_, row): &Row| (row.item_id, row.hq)
                    measure=move |(_, row): &Row, id| {
                        match id {
                            "hq" => ("HQ".to_string(), 42.0),
                            "item" => (items.get(&ItemId(row.item_id)).map(|i| i.name.as_str()).unwrap_or_default().to_string(), 110.0),
                            "world" => (worlds.get(&row.world_id).cloned().unwrap_or_default(), 42.0),
                            "listing" => (row.listing.separate_with_commas(), 42.0),
                            "tax" => (row.tax.separate_with_commas(), 42.0),
                            "vendor-price" => (row.vendor_price.separate_with_commas(), 42.0),
                            "profit" => (row.profit.separate_with_commas(), 42.0),
                            "margin" => (format!("{}%", row.margin), 30.0),
                            _ => (String::new(), 0.0),
                        }
                    }
                    view=move |(index, row): Row, id| {
                        let item_id = row.item_id;
                        let item = items.get(&ItemId(item_id)).map(|i| i.name.as_str()).unwrap_or_default();
                        let icon_loading = if index < 20 { "eager" } else { "" };
                        let world_name = worlds.get(&row.world_id).cloned().unwrap_or_default();
                        match id {
                            "hq" => view! {
                                <div class="flex items-center justify-center w-full min-w-0">
                                    {row.hq.then(|| view! { <span class=hq_badge_class>{t!(i18n, vendor_sell_col_hq)}</span> })}
                                </div>
                            }.into_any(),
                            "item" => view! {
                                <div class="flex flex-row items-center gap-2 w-full min-w-0">
                                    <a class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                       href=format!("/item/{}/{item_id}", world_name)>
                                        <div class="shrink-0"><ItemIcon item_id icon_size=IconSize::Small loading=icon_loading /></div>
                                        {item}
                                    </a>
                                    <AddToList item_id />
                                    <Clipboard clipboard_text=item.to_string() />
                                </div>
                            }.into_any(),
                            "world" => view! { <div class="truncate w-full min-w-0">{world_name}</div> }.into_any(),
                            "listing" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.listing /></div> }.into_any(),
                            "tax" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.tax /></div> }.into_any(),
                            "vendor-price" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.vendor_price /></div> }.into_any(),
                            "profit" => view! { <div class="text-right flex items-center justify-end w-full min-w-0"><Gil amount=row.profit /></div> }.into_any(),
                            "margin" => view! {
                                <div class="text-right flex items-center justify-end w-full min-w-0">
                                    <span class=roi_badge_class(row.margin)>{format!("{}%", row.margin)}</span>
                                </div>
                            }.into_any(),
                            _ => ().into_any(),
                        }
                    }
                />
            </div>
        </div>
    }
}
```

Notes for the implementer:
- `MarketGrid`'s `key` closure must return something `Hash + Eq`; `(i32, bool)` is fine. If the grid's `K` bound needs `Clone + 'static`, that holds too.
- If `SortableHeaderCell` needs `label` as `String` prop and `mode` typed, the `cell` closure above already passes both; if the closure fails to type-check because `mode` is generic, inline the six `view! { <SortableHeaderCell ... /> }` blocks like `venture_analyzer.rs:470-505` does.
- `Calculation::provide(filters, terms, primary_input: Option<&'static str>)`: pass `None` since there is no selectable price basis. Check `analyzer_kit/calculation.rs:62` for the exact third parameter type.
- If `market.window` is unused elsewhere the `CalculationStrip` still needs it; keep it.
- If the `ItemIcon` `loading` prop does not exist, drop `loading=icon_loading` and the `icon_loading` binding.

- [ ] **Step 4: Add the page component**

Insert after `VendorSellTable`:

```rust
#[component]
pub fn VendorSell() -> impl IntoView {
    provide_grid_saved_views("vendor-sell-grid");
    let i18n = use_i18n();
    let (selected_world, set_selected_world) = use_analyzer_world("/vendor-sell");
    let region = use_region_for_world(move || selected_world.get().map(|world| world.name));

    let listings = ArcResource::new(region, move |region: String| async move {
        get_cheapest_listings(&region).await
    });

    view! {
        <div class="flex flex-col gap-4 h-full">
            <MetaTitle title=move || t_string!(i18n, vendor_sell_meta_title).to_string() />
            <MetaDescription text=move || t_string!(i18n, vendor_sell_meta_desc).to_string() />

            <div class="flex flex-col gap-4">
                <ToolHeader
                    title=t_string!(i18n, vendor_sell).to_string()
                    summary=t_string!(i18n, vendor_sell_tool_summary).to_string()
                    context=t_string!(i18n, vendor_sell_tool_context).to_string()
                    help_href="/help/vendor-sell"
                    help_body=t_string!(i18n, vendor_sell_tool_help).to_string()
                    calculation=ToolCalculation::new(
                        t_string!(i18n, vendor_sell_calc_title).to_string(),
                        t_string!(i18n, vendor_sell_calc_formula).to_string(),
                        t_string!(i18n, vendor_sell_calc_details).to_string(),
                    )
                    assumptions=vec![
                        t_string!(i18n, vendor_sell_assumption_tax).to_string(),
                        t_string!(i18n, vendor_sell_assumption_hq).to_string(),
                        t_string!(i18n, vendor_sell_assumption_per_unit).to_string(),
                    ]
                >
                    <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, world)}</label>
                    <div data-testid="analyzer-world-picker">
                        <WorldOnlyPicker
                            current_world=selected_world.into()
                            set_current_world=set_selected_world
                        />
                    </div>
                    <span class="text-sm text-[color:var(--color-text-muted)]" data-testid="analyzer-market-scope">
                        {t!(i18n, market_scope)} ": " {move || region.get()}
                    </span>
                </ToolHeader>
                <Suspense fallback=move || view! { <BoxSkeleton /> }>
                    {move || {
                        match listings.get() {
                            Some(Ok(listings)) => view! {
                                <VendorSellTable listings region=region.into() />
                            }.into_any(),
                            Some(Err(e)) => view! {
                                <div class="text-red-400">
                                    {t!(i18n, vendor_sell_error_listings)} {e.to_string()}
                                </div>
                            }.into_any(),
                            None => view! { <BoxSkeleton /> }.into_any(),
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}
```

- [ ] **Step 5: Add the preset tests**

Append inside `mod tests` in `vendor_sell.rs`:

```rust
    #[test]
    fn every_preset_query_is_a_clean_query_string() {
        for query in PRESET_QUERIES {
            assert!(query.starts_with('?'), "{query}");
            assert!(!query.ends_with('&'), "{query}");
            assert!(!query.contains("&&"), "{query}");
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                assert!(!key.is_empty(), "{query}");
                assert!(!value.is_empty(), "{query}");
            }
        }
    }

    #[test]
    fn preset_queries_only_use_keys_this_page_still_reads() {
        for query in PRESET_QUERIES {
            for pair in query.trim_start_matches('?').split('&') {
                let (key, value) = pair.split_once('=').expect("key=value");
                match key {
                    "sort" => assert!(SortMode::from_str(value).is_ok(), "{query}"),
                    "dir" => assert!(SortDir::from_str(value).is_ok(), "{query}"),
                    other => assert!(LEGACY_PRESET_FILTER_KEYS.contains(&other), "{query}"),
                }
            }
        }
    }
```

- [ ] **Step 6: Register the route**

In `ultros-frontend/ultros-app/src/lib.rs`, in the glob-import list near line 75 add `vendor_sell::*,` after `vendor_resale::*,`. In the `<Routes>` block, after the `venture-analyzer/:world?` route (line 638) add:

```rust
                        <Route path=path!("vendor-sell/:world?") view=VendorSell />
```

- [ ] **Step 7: Compile SSR and run the tests**

Run: `cargo test -p ultros-app vendor_sell`
Expected: 8 passed, no warnings. Fix any type errors against the exact signatures in `analyzer_kit/market.rs:836`, `analyzer_kit/calculation.rs:62`, `components/sort_header.rs` (find it with `grep -rn "pub fn SortableHeaderCell" ultros-frontend`).

- [ ] **Step 8: Compile the hydrate (wasm) target**

Run from the worktree root: `cargo leptos build 2>&1 | tail -20`
Expected: success. `#[component]` code that compiles under SSR can still fail under hydrate (e.g. `Send` bounds); fix here, not later.

- [ ] **Step 9: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/vendor_sell.rs ultros-frontend/ultros-app/src/lib.rs
git commit -m "feat(vendor-sell): page, grid and route

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Discovery surfaces — sidebar, home chip, help topic, sitemap, changelog

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/side_nav.rs:179-185`
- Modify: `ultros-frontend/ultros-app/src/routes/home_page.rs:280-282`
- Modify: `ultros-frontend/ultros-app/src/routes/help.rs:96-124`
- Modify: `ultros/src/web/sitemap.rs:79-83` and `:171`
- Create: `ultros-changelog/changes/2026-09-18-vendor-sell-analyzer.json`

- [ ] **Step 1: Sidebar**

In `side_nav.rs`, directly after the Vendor Resale `<SideNavItem>` (ends line 185) add:

```rust
                <SideNavItem
                    href=with_world("/vendor-sell/{world}", "/vendor-sell")
                    section="vendor-sell"
                    icon=i::FaCashRegisterSolid
                >
                    {t!(i18n, vendor_sell)}
                </SideNavItem>
```

- [ ] **Step 2: Home tool chip**

In `home_page.rs`, directly after the Vendor Resale `<ToolChip>` block (ends line 282) add:

```rust
                            <ToolChip href="/vendor-sell" label=t!(i18n, vendor_sell).into_any() description=t!(i18n, vendor_sell_desc).into_any()>
                                <Icon width="2em" height="2em" icon=i::FaCashRegisterSolid />
                            </ToolChip>
```

- [ ] **Step 3: Help topic**

In `help.rs`, directly after the `vendor-resale` `HelpTopic { .. },` (ends line 124) add:

```rust
    HelpTopic {
        slug: "vendor-sell",
        title: "Vendor Sell",
        category: "Market analysis",
        summary: "Find market board listings that an NPC vendor will buy for more than they cost.",
        purpose: "Use this for zero-risk gil: buy the listing, walk to any vendor, sell it.",
        inputs: &[
            "Selected world's region",
            "Cheapest listing per item and quality",
            "NPC vendor sell-back price",
        ],
        assumptions: &[
            "A flat 5% purchase tax, rounded up; your retainer city may charge less.",
            "HQ listings are valued at the NQ vendor price.",
            "Each row is one unit; stack size is not shown.",
        ],
        results: &[
            "Profit is vendor price minus listing minus tax.",
            "Margin is profit over what you paid.",
            "Only profitable rows are listed.",
        ],
        next_actions: &[
            "Travel to the listing's world before it sells.",
            "Sort by margin for the best return on small budgets.",
        ],
        image: None,
    },
```

- [ ] **Step 4: Sitemap**

In `ultros/src/web/sitemap.rs`, after the `vendor-resale` tuple (ends line 83) add:

```rust
        (
            "https://ultros.app/vendor-sell",
            0.8,
            ChangeFrequency::Daily,
        ),
```

and in `HELP_SLUGS` add `"vendor-sell",` after `"vendor-resale",`.

- [ ] **Step 5: Changelog**

Create `ultros-changelog/changes/2026-09-18-vendor-sell-analyzer.json`:

```json
{
  "category": "features",
  "importance": "medium",
  "title": "Vendor Sell: listings an NPC will pay more for",
  "blurb": "A new analyzer scans your region's cheapest listings for items priced below what an NPC vendor pays, after the 5% purchase tax. Buy the listing, sell it to any vendor, keep the difference. Sort by profit or margin and filter by category.",
  "link": "/vendor-sell"
}
```

If the changelog build script rejects `link` (check `ultros-changelog/build.rs` for the accepted fields), drop that field.

- [ ] **Step 6: Build and check the sitemap test**

Run: `cargo test -p ultros sitemap && cargo check -p ultros-app`
Expected: pass. If a sitemap test asserts the help-slug list against `HELP_TOPICS` count, it now passes only because both were updated.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/components/side_nav.rs ultros-frontend/ultros-app/src/routes/home_page.rs ultros-frontend/ultros-app/src/routes/help.rs ultros/src/web/sitemap.rs ultros-changelog/changes/2026-09-18-vendor-sell-analyzer.json
git commit -m "feat(vendor-sell): sidebar, home chip, help topic, sitemap, changelog

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: E2E coverage and CI gate

**Files:**
- Modify: `integration/analyzer-grids.cjs:9-17`
- Modify: `integration/shared-analyzer-data.cjs:372-380`

- [ ] **Step 1: Add the route to both e2e lists**

`analyzer-grids.cjs`, after the `vendor-resale` line:

```js
  ['vendor-sell',`/vendor-sell/${WORLD}`,'profit','profit'],
```

`shared-analyzer-data.cjs`, after the `vendor-resale` line:

```js
        ['vendor-sell', `/vendor-sell/${world}`],
```

- [ ] **Step 2: Run the JS unit tests**

Run: `node --test integration/*.test.cjs`
Expected: pass (these are fixture tests; the grid scripts are not executed here but must still parse).

- [ ] **Step 3: Run the full CI gate**

Run from Git Bash in the worktree root, with the Windows env from Global Constraints:

```bash
./check_ci.sh > "$CLAUDE_SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$CLAUDE_SCRATCHPAD/ci.log"
```

Expected: `REAL_EXIT=0`. If fmt fails, `cargo fmt --all` and re-run. If clippy fails, fix the code (no `#[allow]`). Exit 137 is an OOM kill, not a lint failure: re-run clippy with `-j 2`.

- [ ] **Step 4: Manual smoke in the browser**

Start the app (`PORT=8093 HOSTNAME=127.0.0.1 ./target/debug/ultros.exe` after `cargo leptos build`, or reuse a running server) and open `/vendor-sell/Gilgamesh` in the built-in browser. Confirm: the grid renders rows, profit sorts descending by default, clicking the Margin header re-sorts, the category chip filters, an HQ badge appears on an HQ row, and the sidebar highlights Vendor Sell. Take one screenshot for the PR.

- [ ] **Step 5: Commit**

```bash
git add integration/analyzer-grids.cjs integration/shared-analyzer-data.cjs
git commit -m "test(e2e): cover vendor-sell in analyzer grid smokes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Open the pull request

- [ ] **Step 1: Push and open the PR**

```bash
git push -u origin claude/vendor-price-analyzer-cca1ad
```

Then `gh pr create --base main --title "feat: Vendor Sell analyzer (market listings below NPC sell-back price)"` with a body that lists: what the page does, the tax rule (5% rounded up, buyer-side), the HQ rule, the i18n keys added (with a note that non-English strings were machine-translated and welcome native review), the screenshot from Task 7, and ends with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

- [ ] **Step 2: Watch CI**

Use the desktop app's PR tools (`mcp__ccd_pr__bind_pr`, then `get_status`) rather than polling `gh`. Fix anything red on the branch and push again.
