# Item Comparison ("Flip Verification") Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the item page URL carries `?compare-buy-from={BuyWorld}`, render a hero "Flip route" card that verifies the flip: cheapest buy-world listing vs the sell world's estimated sale price (same math as the flip-finder), profit after 5% tax. Flip-finder rows and a revived cross-world savings line link into it.

**Architecture:** Pure math extracted to `analysis.rs` (shared by analyzer + card so they can't drift). A new `routes/item_compare.rs` holds the testable verdict computation and a `FlipRouteCard` component that does its own `get_listings(item_id, buy_world)` fetch. The card mounts in `ListingsContent` above `DecisionHeader`. The dead `SavingsVerdict` banner is replaced by a `CheapestPrices`-driven savings line with a Compare action.

**Tech Stack:** Rust, Leptos 0.8 (SSR+hydrate), leptos-i18n, existing `/api/v1/listings/{world}/{item_id}` endpoint. No server changes.

**Spec:** `docs/superpowers/specs/2026-08-04-item-comparison-page-design.md`

## Global Constraints

- Before every commit: `./check_ci.sh` from repo root (fmt + clippy, `-D warnings`). Read its REAL exit code: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log`. Clippy exit 137 = OOM (re-run `-j 2`); exit 127 = MSYS perl shadowing Strawberry (prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to PATH).
- CI does NOT run `cargo test` — run `cargo test -p ultros-app` locally; green CI alone proves nothing about tests.
- Every user-facing string goes through `leptos-i18n`: add each key to ALL 7 locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) in `ultros-frontend/ultros-app/locales/` with real translations (exact strings are given in Task 4).
- No `HashMap` iteration order may reach the DOM (SSR/CSR determinism).
- Signal reads inside `Suspense`/`Transition` bodies use `with_or` / `get_or_default` (defined at top of `item_view.rs`), never bare `.with()`/`.get()`.
- URL param writes use `filter_query_signal` from `crate::query_defaults` (replace: true, scroll: false) — never plain `query_signal`.
- Tests that construct signals must run inside `reactive_graph::owner::Owner::new()` (see existing `item_view.rs` tests); pure-data tests don't need it.
- Cargo builds in this worktree: set a SHORT out-of-repo `CARGO_TARGET_DIR` if builds fail on path length; a shared target dir with another session serializes builds — wait, don't kill.

---

### Task 1: Shared flip math in `analysis.rs`

Move the analyzer's estimate math into `analysis.rs` so the card and the flip-finder share one implementation.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analysis.rs` (add functions + tests)
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (delete moved code, call shared fns)

**Interfaces:**
- Produces (in `crate::analysis`):
  - `pub fn median_in_place_i32(sorted: &mut [i32]) -> i32` (moved verbatim from `analyzer.rs:332`)
  - `pub fn sniper_clamp(prices: Vec<i32>) -> Vec<i32>` — drops prices below 10% of the raw median; returns the input set unchanged if the clamp would remove everything or input is empty
  - `pub fn is_troll_listing(price: i32, median: i32) -> bool` (moved verbatim from `analyzer.rs:444`, with `TROLL_MULTIPLE`)
  - `pub fn flip_estimated_sale_price(median_price: i32, world_floor: Option<i32>) -> i32` — troll-guards the floor, then `median.min(floor)`, falling back to median
  - `pub fn flip_profit(estimated_sale_price: i32, buy_price: i32, include_tax: bool) -> i32` — `(est * 0.95) as i32 - buy` when taxed, else `est - buy`

- [ ] **Step 1: Write failing tests in `analysis.rs`'s existing `#[cfg(test)]` module** (pure data, no `Owner` needed):

```rust
#[test]
fn sniper_clamp_drops_prices_below_ten_percent_of_median() {
    // raw median of [10, 1000, 1100, 1200, 1300] is 1100; floor = 110 → 10 dropped
    assert_eq!(
        sniper_clamp(vec![10, 1000, 1100, 1200, 1300]),
        vec![1000, 1100, 1200, 1300]
    );
}

#[test]
fn sniper_clamp_keeps_raw_set_when_clamp_would_empty_it() {
    // all equal → floor = 100 * 0.1 = 10, nothing dropped; and empty stays empty
    assert_eq!(sniper_clamp(vec![100]), vec![100]);
    assert_eq!(sniper_clamp(Vec::new()), Vec::<i32>::new());
}

#[test]
fn flip_estimate_caps_median_by_world_floor() {
    assert_eq!(flip_estimated_sale_price(1000, Some(800)), 800);
    assert_eq!(flip_estimated_sale_price(1000, Some(1200)), 1000);
    assert_eq!(flip_estimated_sale_price(1000, None), 1000);
}

#[test]
fn flip_estimate_ignores_troll_floor() {
    // floor 60_000 vs median 1_000 exceeds TROLL_MULTIPLE (50x) → ignored
    assert_eq!(flip_estimated_sale_price(1000, Some(60_000)), 1000);
}

#[test]
fn flip_profit_applies_five_percent_tax() {
    assert_eq!(flip_profit(1000, 500, true), 450); // 950 - 500
    assert_eq!(flip_profit(1000, 500, false), 500);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app sniper_clamp -- --nocapture` (and the flip_ tests)
Expected: FAIL to compile — functions not defined.

- [ ] **Step 3: Implement in `analysis.rs`**

Move `median_in_place_i32` (analyzer.rs:332-345), `SNIPER_FRACTION` (analyzer.rs:330), `TROLL_MULTIPLE` + `is_troll_listing` (analyzer.rs:440-446) into `analysis.rs`, made `pub`, doc comments carried along. Add:

```rust
/// Sniper-clamped price set: drops sales priced below `SNIPER_FRACTION` of the
/// raw median. If the clamp would remove everything, the raw set is kept.
/// Shared by the analyzer's `compute_summary` and the item-page flip card.
pub fn sniper_clamp(prices: Vec<i32>) -> Vec<i32> {
    if prices.is_empty() {
        return prices;
    }
    let mut raw = prices.clone();
    let raw_median = median_in_place_i32(&mut raw);
    let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;
    let clamped: Vec<i32> = prices.iter().copied().filter(|p| *p >= floor).collect();
    if clamped.is_empty() { prices } else { clamped }
}

/// Flip estimate shared by the flip-finder table and the item-page flip card:
/// median of recent sales, capped by the sell world's current floor. A floor
/// more than `TROLL_MULTIPLE`× the median is a troll listing and is ignored.
pub fn flip_estimated_sale_price(median_price: i32, world_floor: Option<i32>) -> i32 {
    match world_floor.filter(|floor| !is_troll_listing(*floor, median_price)) {
        Some(floor) => median_price.min(floor),
        None => median_price,
    }
}

/// Per-unit flip profit. The 5% market-board tax comes off the sale, not the buy.
pub fn flip_profit(estimated_sale_price: i32, buy_price: i32, include_tax: bool) -> i32 {
    let estimated = if include_tax {
        (estimated_sale_price as f32 * 0.95) as i32
    } else {
        estimated_sale_price
    };
    estimated - buy_price
}
```

- [ ] **Step 4: Refactor `analyzer.rs` to use the shared functions**

- Delete the moved items from `analyzer.rs`; import from `crate::analysis` (extend the existing `use crate::analysis::{...}` at line 1).
- `compute_summary` (analyzer.rs:347): replace its inline raw-median/clamp block (lines 365-374) with `let clamped = sniper_clamp(sales.iter().map(|s| s.price_per_unit).collect());` then `let mut clamped = clamped;` for the median call. min/max/avg logic unchanged.
- `ProfitTable::new` (analyzer.rs:504-507): replace the `world_floor` troll-filter + `match` with `let estimated_sale_price = flip_estimated_sale_price(summary.median_price, world_cheapest.get(&key).map(|(price, _)| *price));`. The *region*-floor troll guard that drops the whole row (line 490) stays as-is, now calling `analysis::is_troll_listing`.
- The include-tax row math (analyzer.rs:1390-1395): replace with `let profit = flip_profit(data.estimated_sale_price, data.cheapest_price, include_tax);` (the separate `estimated` binding disappears; nothing else reads it).

- [ ] **Step 5: Run the full ultros-app test suite**

Run: `cargo test -p ultros-app`
Expected: PASS, including the analyzer's existing `estimated_sale_price_uses_median_not_min` / troll tests (analyzer.rs:3390-3460) which now exercise the shared code. If any moved test broke, the refactor changed behavior — fix the refactor, not the test.

- [ ] **Step 6: check_ci + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -- ultros-frontend/ultros-app/src/analysis.rs ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "refactor(analysis): share flip estimate/profit math between analyzer and item view"
```

---

### Task 2: `compare-buy-from` URL helpers in `item_view_scope.rs`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view_scope.rs`

**Interfaces:**
- Produces:
  - `pub const COMPARE_BUY_FROM_PARAM: &str = "compare-buy-from";`
  - `pub fn compare_item_href(sell_world: &str, item_id: i32, buy_world: &str) -> String` — `/item/{sell}/{id}?compare-buy-from={buy}`, both names percent-encoded with the existing `COMPONENT_UNRESERVED` set; empty `buy_world` falls back to the plain item href.

- [ ] **Step 1: Write failing tests in the existing `tests` module**

```rust
#[test]
fn compare_href_carries_buy_world_param() {
    assert_eq!(
        compare_item_href("Gilgamesh", 40644, "Jenova"),
        "/item/Gilgamesh/40644?compare-buy-from=Jenova",
    );
}

#[test]
fn compare_href_encodes_hyphenated_names_stably() {
    // Hyphens survive both sides (hydration-mismatch guard, same as item_href).
    assert_eq!(
        compare_item_href("North-America", 1, "Ravana"),
        "/item/North-America/1?compare-buy-from=Ravana",
    );
}

#[test]
fn compare_href_without_buy_world_is_a_plain_item_link() {
    assert_eq!(compare_item_href("Gilgamesh", 40644, ""), "/item/Gilgamesh/40644");
}

#[test]
fn item_href_carries_compare_param_across_world_switches() {
    assert_eq!(
        item_href("Sargatanas", 40644, "compare-buy-from=Jenova"),
        "/item/Sargatanas/40644?compare-buy-from=Jenova",
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app item_view_scope`
Expected: FAIL to compile — `compare_item_href` not defined.

- [ ] **Step 3: Implement**

```rust
/// Query param naming the buy world for the item page's flip-verification card.
pub const COMPARE_BUY_FROM_PARAM: &str = "compare-buy-from";

/// Item URL that opens the flip-verification card: sell world in the path,
/// buy world in `?compare-buy-from=`. An unresolvable (empty) buy world
/// degrades to the plain item link.
pub fn compare_item_href(sell_world: &str, item_id: i32, buy_world: &str) -> String {
    if buy_world.is_empty() {
        return item_href(sell_world, item_id, "");
    }
    let escaped_buy = utf8_percent_encode(buy_world, COMPONENT_UNRESERVED).to_string();
    item_href(
        sell_world,
        item_id,
        &format!("{COMPARE_BUY_FROM_PARAM}={escaped_buy}"),
    )
}
```

Also extend `item_href`'s doc comment (line 26-27): mention `?compare-buy-from=` alongside `?exclude-worlds=` as a param that must survive world switches.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros-app item_view_scope`
Expected: PASS (all 7 tests in the module).

- [ ] **Step 5: check_ci + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -- ultros-frontend/ultros-app/src/routes/item_view_scope.rs
git commit -m "feat(item-view): compare-buy-from URL helpers"
```

---

### Task 3: Flip verdict computation (`routes/item_compare.rs`, logic only)

Pure, signal-free computation the card renders from. Component comes in Task 5.

**Files:**
- Create: `ultros-frontend/ultros-app/src/routes/item_compare.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/mod.rs` (add `pub mod item_compare;` — match the file's existing style)

**Interfaces:**
- Consumes: `crate::analysis::{flip_estimated_sale_price, flip_profit, median_in_place_i32, sniper_clamp}` (Task 1); `ultros_api_types::{ActiveListing, CurrentlyShownItem}`.
- Produces:
  - `pub(crate) struct FlipVerdict { pub hq: bool, pub buy_listing: ActiveListing, pub estimated_sale_price: i32, pub profit_per_unit: i32, pub stack_profit: i32 }`
  - `pub(crate) fn flip_verdict(buy: &CurrentlyShownItem, sell: &CurrentlyShownItem) -> Option<FlipVerdict>` — best-profit quality among NQ/HQ; `None` when no buy listing or no recent sell-side sale exists for any quality.

- [ ] **Step 1: Write the module with failing tests**

```rust
//! Flip-verification math for the item page's `?compare-buy-from=` card.
//!
//! Estimates use the exact flip-finder pipeline (`crate::analysis`):
//! sniper-clamped median of recent sales, capped by the sell world's
//! troll-guarded floor; profit is after the 5% market-board tax.

use crate::analysis::{flip_estimated_sale_price, flip_profit, median_in_place_i32, sniper_clamp};
use ultros_api_types::{ActiveListing, CurrentlyShownItem};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FlipVerdict {
    pub hq: bool,
    /// Cheapest buy-world listing for this quality.
    pub buy_listing: ActiveListing,
    pub estimated_sale_price: i32,
    /// After the 5% tax. Negative profits are kept — "this flip is dead" is
    /// exactly what the card exists to say.
    pub profit_per_unit: i32,
    /// `profit_per_unit * buy_listing.quantity`.
    pub stack_profit: i32,
}

fn cheapest_buy(buy: &CurrentlyShownItem, hq: bool) -> Option<&ActiveListing> {
    buy.listings
        .iter()
        .map(|(listing, _)| listing)
        .filter(|listing| listing.hq == hq && listing.price_per_unit > 0)
        .min_by_key(|listing| listing.price_per_unit)
}

fn sell_median(sell: &CurrentlyShownItem, hq: bool) -> i32 {
    let prices: Vec<i32> = sell
        .sales
        .iter()
        .filter(|sale| sale.hq == hq && sale.price_per_item > 0)
        .map(|sale| sale.price_per_item)
        .collect();
    let mut clamped = sniper_clamp(prices);
    median_in_place_i32(&mut clamped)
}

fn sell_floor(sell: &CurrentlyShownItem, hq: bool) -> Option<i32> {
    sell.listings
        .iter()
        .map(|(listing, _)| listing)
        .filter(|listing| listing.hq == hq && listing.price_per_unit > 0)
        .map(|listing| listing.price_per_unit)
        .min()
}

fn verdict_for_quality(
    buy: &CurrentlyShownItem,
    sell: &CurrentlyShownItem,
    hq: bool,
) -> Option<FlipVerdict> {
    let buy_listing = cheapest_buy(buy, hq)?.clone();
    let median = sell_median(sell, hq);
    if median == 0 {
        // No recent sales of this quality on the sell world — no estimate,
        // no verdict. Mirrors the flip-finder, whose rows come from sales.
        return None;
    }
    let estimated_sale_price = flip_estimated_sale_price(median, sell_floor(sell, hq));
    let profit_per_unit = flip_profit(estimated_sale_price, buy_listing.price_per_unit, true);
    let stack_profit = profit_per_unit.saturating_mul(buy_listing.quantity);
    Some(FlipVerdict {
        hq,
        buy_listing,
        estimated_sale_price,
        profit_per_unit,
        stack_profit,
    })
}

/// Best-profit verdict across NQ/HQ, or `None` when neither quality has both
/// a buy listing and at least one recent sell-world sale.
pub(crate) fn flip_verdict(
    buy: &CurrentlyShownItem,
    sell: &CurrentlyShownItem,
) -> Option<FlipVerdict> {
    [false, true]
        .into_iter()
        .filter_map(|hq| verdict_for_quality(buy, sell, hq))
        .max_by_key(|verdict| verdict.profit_per_unit)
}
```

Tests (same file; pure data, no `Owner` needed). Fixture helpers mirror the `listing()` fixture in `item_view.rs:1999`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ultros_api_types::{Retainer, SaleHistory};

    fn listing(id: i32, world_id: i32, price_per_unit: i32, quantity: i32, hq: bool)
        -> (ActiveListing, Arc<Retainer>) {
        (
            ActiveListing {
                id,
                world_id,
                item_id: 1,
                retainer_id: id,
                price_per_unit,
                quantity,
                hq,
                timestamp: chrono::Utc::now().naive_utc(),
            },
            Arc::new(Retainer {
                id,
                world_id,
                name: format!("Retainer {id}"),
                retainer_city_id: 1,
            }),
        )
    }

    fn sale(price_per_item: i32, hq: bool) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity: 1,
            price_per_item,
            buying_character_id: 0,
            hq,
            sold_item_id: 1,
            sold_date: chrono::Utc::now().naive_utc(),
            world_id: 2,
            buyer_name: None,
        }
    }

    fn shown(listings: Vec<(ActiveListing, Arc<Retainer>)>, sales: Vec<SaleHistory>)
        -> CurrentlyShownItem {
        CurrentlyShownItem { listings, sales, last_updated: Vec::new() }
    }

    #[test]
    fn verdict_uses_median_capped_by_sell_floor_and_taxes_profit() {
        let buy = shown(vec![listing(1, 1, 500, 3, false)], Vec::new());
        let sell = shown(
            vec![listing(2, 2, 900, 1, false)], // sell floor 900 < median 1000
            vec![sale(1000, false), sale(1000, false), sale(1200, false)],
        );
        let verdict = flip_verdict(&buy, &sell).unwrap();
        assert_eq!(verdict.estimated_sale_price, 900);
        // (900 * 0.95) as i32 - 500 = 855 - 500
        assert_eq!(verdict.profit_per_unit, 355);
        assert_eq!(verdict.stack_profit, 355 * 3);
    }

    #[test]
    fn verdict_none_without_buy_listings() {
        let buy = shown(Vec::new(), Vec::new());
        let sell = shown(Vec::new(), vec![sale(1000, false)]);
        assert!(flip_verdict(&buy, &sell).is_none());
    }

    #[test]
    fn verdict_none_without_recent_sell_sales() {
        let buy = shown(vec![listing(1, 1, 500, 1, false)], Vec::new());
        let sell = shown(vec![listing(2, 2, 900, 1, false)], Vec::new());
        assert!(flip_verdict(&buy, &sell).is_none());
    }

    #[test]
    fn verdict_picks_better_profit_quality() {
        let buy = shown(
            vec![listing(1, 1, 500, 1, false), listing(2, 1, 600, 1, true)],
            Vec::new(),
        );
        let sell = shown(
            Vec::new(),
            vec![sale(700, false), sale(2000, true)],
        );
        let verdict = flip_verdict(&buy, &sell).unwrap();
        assert!(verdict.hq); // (2000*0.95)-600 = 1300 beats (700*0.95)-500 = 165
    }

    #[test]
    fn verdict_keeps_negative_profit() {
        let buy = shown(vec![listing(1, 1, 5_000, 1, false)], Vec::new());
        let sell = shown(Vec::new(), vec![sale(1000, false)]);
        let verdict = flip_verdict(&buy, &sell).unwrap();
        assert!(verdict.profit_per_unit < 0);
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail, then pass**

Add `pub mod item_compare;` to `routes/mod.rs`. Run: `cargo test -p ultros-app item_compare`
Expected: compile, then PASS (the implementation is written alongside the tests here; if any test fails, fix the implementation).

- [ ] **Step 3: check_ci + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -- ultros-frontend/ultros-app/src/routes/item_compare.rs ultros-frontend/ultros-app/src/routes/mod.rs
git commit -m "feat(item-view): flip verdict computation for the compare card"
```

Note: clippy may flag `verdict_for_quality`'s argument order or dead_code until Task 5 wires the component — if `dead_code` fires, add the component in the same session (Task 5) before committing, or mark `pub(crate)` items used-by-tests with the component task following immediately. Do NOT `#[allow]`.

---

### Task 4: Locale keys (all 7 files)

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/en.json`, `fr.json`, `de.json`, `ja.json`, `cn.json`, `ko.json`, `tc.json`

No test cycle of its own — the compiler enforces key presence when Task 5 lands; this task exists so Task 5's diff stays reviewable. Keys (snake_case, `item_compare_` prefix), exact values:

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `item_compare_flip_route` | Flip route | Achat-revente | Flip-Route | 転売ルート | 倒卖路线 | 되팔이 경로 | 轉賣路線 |
| `item_compare_buy_on` | Buy on | Acheter sur | Kaufen auf | 購入先 | 购买于 | 구매 서버 | 購買於 |
| `item_compare_sell_on` | Sell on | Vendre sur | Verkaufen auf | 販売先 | 出售于 | 판매 서버 | 出售於 |
| `item_compare_est_sale_price` | Est. sale price | Prix de vente estimé | Gesch. Verkaufspreis | 推定販売価格 | 预估售价 | 예상 판매 가격 | 預估售價 |
| `item_compare_median_recent` | median of recent sales | médiane des ventes récentes | Median der letzten Verkäufe | 直近の取引価格の中央値 | 近期成交中位数 | 최근 판매 중앙값 | 近期成交中位數 |
| `item_compare_sales_per_day` | sales/day | ventes/jour | Verkäufe/Tag | 販売/日 | 笔/天 | 판매/일 | 筆/天 |
| `item_compare_profit_after_tax` | Profit after 5% tax | Profit après taxe de 5 % | Gewinn nach 5 % Steuer | 税5%控除後の利益 | 扣除5%税后利润 | 5% 세금 공제 후 이익 | 扣除5%稅後利潤 |
| `item_compare_per_unit` | per unit | par unité | pro Stück | 1個あたり | 每件 | 개당 | 每件 |
| `item_compare_stack_total` | for the stack | pour la pile | für den Stapel | スタック合計 | 整组合计 | 묶음 합계 | 整組合計 |
| `item_compare_no_listings` | No listings right now | Aucune offre pour le moment | Derzeit keine Angebote | 現在出品がありません | 当前没有在售商品 | 현재 판매 목록이 없습니다 | 目前沒有在售商品 |
| `item_compare_no_recent_sales` | No recent sales | Aucune vente récente | Keine aktuellen Verkäufe | 最近の取引がありません | 近期无成交 | 최근 판매 내역이 없습니다 | 近期無成交 |
| `item_compare_unavailable` | Couldn't load listings | Impossible de charger les offres | Angebote konnten nicht geladen werden | 出品情報を取得できませんでした | 无法加载在售商品 | 판매 목록을 불러오지 못했습니다 | 無法載入在售商品 |
| `item_compare_not_profitable` | Not currently profitable | Pas rentable actuellement | Derzeit nicht profitabel | 現在は利益が出ません | 当前无利可图 | 현재 수익이 나지 않습니다 | 目前無利可圖 |
| `item_compare_dismiss` | Dismiss comparison | Fermer la comparaison | Vergleich schließen | 比較を閉じる | 关闭对比 | 비교 닫기 | 關閉對比 |
| `item_compare_action` | Compare | Comparer | Vergleichen | 比較 | 对比 | 비교 | 對比 |

- [ ] **Step 1:** Add all 15 keys to each of the 7 files, keeping each file's existing alphabetical/nearby-key placement style (look at where `item_view_savings_*` sits and add the `item_compare_*` block adjacent).
- [ ] **Step 2:** `cargo check -p ultros-app` — leptos-i18n regenerates; expect no missing-key warnings.
- [ ] **Step 3:** Commit:

```bash
git add -- ultros-frontend/ultros-app/locales/
git commit -m "i18n: item_compare_* keys for the flip verification card"
```

(If executing tasks in one session, this commit may be folded into Task 5's.)

---

### Task 5: `FlipRouteCard` component + mount in `ListingsContent`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_compare.rs` (add the component)
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (mount at line ~1646, above `<DecisionHeader …>`)

**Interfaces:**
- Consumes: `flip_verdict` (Task 3), `COMPARE_BUY_FROM_PARAM` (Task 2), `crate::api::get_listings`, `crate::query_defaults::filter_query_signal`, `crate::freshness::derive_freshness_inputs`, `ultros_api_types::freshness::calculate_freshness_verdict`, components `Gil`, `WorldName`, `FreshnessBadge`, `BoxSkeleton`, `Icon`; `with_or`/`get_or_default` (make them `pub(crate)` in `item_view.rs` — `get_or_default` already is).
- Produces: `#[component] pub(crate) fn FlipRouteCard(item_id: Memo<i32>, world: Memo<String>, listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>) -> impl IntoView`

- [ ] **Step 1: Implement the component in `item_compare.rs`**

Behavior contract:

1. `let (compare_world, set_compare_world) = filter_query_signal::<String>(COMPARE_BUY_FROM_PARAM);`
2. Resolve the buy world: `Url::unescape` the raw value, `world_data.lookup_world_by_name(...)`, require `as_world()` (a real world, not a DC). Resolve the page scope the same way; require the page scope to be a world and the two ids to differ. All in a `Memo<Option<String>>` (`buy_world`, canonical name from `get_name()`). If it resolves to `None`, the component renders `()` — no error state.
3. Buy-side fetch: `Resource::new(move || (item_id(), buy_world.get()), …)` that returns `None` without fetching when `buy_world` is `None`, otherwise `get_listings(item_id, &name).await.map(Arc::new)` (same shape as `listing_resource`, item_view.rs:1529).
4. Render (all reads via `with_or`/`get_or_default`; wrap in `<Transition fallback=BoxSkeleton>`):
   - Card container: `class="flex flex-col gap-3 rounded-xl border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] p-3 sm:p-4 mb-4"`.
   - Header row: `<Icon icon=icondata::FaArrowRightArrowLeftSolid attr:class="text-sm shrink-0" />`, `<span class="font-semibold">{t!(i18n, item_compare_flip_route)}</span>`, then `<WorldName id=AnySelector::World(buy_id) /> " → " <WorldName id=AnySelector::World(sell_id) />`, and on the far right a dismiss button: `<button aria-label=t_string!(i18n, item_compare_dismiss) on:click=move |_| set_compare_world.set(None)><Icon icon=icondata::FaXmarkSolid /></button>`.
   - Body `class="grid grid-cols-1 sm:grid-cols-3 gap-3"`, three cells:
     - **Buy**: label `{t!(i18n, item_compare_buy_on)} <WorldName …buy…>`; when `flip_verdict` is `Some`: `<Gil amount=verdict.buy_listing.price_per_unit />` `" × "` quantity, HQ/NQ chip (same markup as the old banner's quality chip, item_view.rs:670-672), and a `FreshnessBadge` computed from the buy payload via `derive_freshness_inputs(&buy.last_updated, &buy.sales, 1, chrono::Utc::now().naive_utc())` + `calculate_freshness_verdict`. When the buy payload has no listings: `{t!(i18n, item_compare_no_listings)}`. When the fetch errored: `{t!(i18n, item_compare_unavailable)}`.
     - **Sell**: label `{t!(i18n, item_compare_sell_on)} <WorldName …sell…>`; `{t!(i18n, item_compare_est_sale_price)}: <Gil amount=verdict.estimated_sale_price />` with a muted `({t!(i18n, item_compare_median_recent)})` note, plus velocity from the SELL payload: `derive_freshness_inputs(&sell.last_updated, &sell.sales, 1, now).scope_sales_per_day` rendered as `format!("~{:.1}", v)` + `{t!(i18n, item_compare_sales_per_day)}`. When no verdict because the sell side had no sales: `{t!(i18n, item_compare_no_recent_sales)}`.
     - **Verdict**: `{t!(i18n, item_compare_profit_after_tax)}`, `<Gil amount=verdict.profit_per_unit /> {t!(i18n, item_compare_per_unit)}`, and when `quantity > 1` a second line `<Gil amount=verdict.stack_profit /> {t!(i18n, item_compare_stack_total)}`. Negative profit: wrap the numbers in `class="text-red-300"` and add `{t!(i18n, item_compare_not_profitable)}`.
5. SSR determinism: everything derives from `Vec` iteration (listings/sales), never HashMap — `flip_verdict` already guarantees this. The card renders identically on SSR and hydration because both sides await the same resources through `Transition`.
6. Icons: verify `icondata::FaArrowRightArrowLeftSolid` / `FaXmarkSolid` exist (grep `icondata::` in the repo for the crate's naming: e.g. `icondata::FaGlobeSolid` at item_view.rs:664); substitute the nearest existing arrow/close icons if these names differ.

- [ ] **Step 2: Mount in `item_view.rs`**

At line ~1645 (inside `<div id="overview" class="scroll-mt-16">`, immediately before `<DecisionHeader …>`):

```rust
<crate::routes::item_compare::FlipRouteCard item_id world listing_resource />
```

(`ListingsContent` has all three in scope; `listing_resource` is `Copy`.) If `with_or` is needed from the new module, change its visibility in `item_view.rs` to `pub(crate)`.

- [ ] **Step 3: Compile + test**

Run: `cargo check -p ultros-app` then `cargo test -p ultros-app`
Expected: clean check; all tests pass.

- [ ] **Step 4: check_ci + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -- ultros-frontend/ultros-app/src/routes/item_compare.rs ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "feat(item-view): flip route comparison card behind ?compare-buy-from="
```

---

### Task 6: Replace the dead `SavingsVerdict` banner with a `CheapestPrices`-driven savings line

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (`DecisionHeader`, lines 80-110, 535-575, 585-702, tests 2050-2135)

**Interfaces:**
- Consumes: `CheapestPrices` context (`global_state/cheapest_prices.rs`), `PriceSummary`/`CheapestListingData` (`ultros_api_types::cheapest_listings`), `COMPARE_BUY_FROM_PARAM` + `filter_query_signal`, existing `MEANINGFUL_CROSS_WORLD_SAVINGS_GIL`, `format_savings_percent`, i18n keys `item_view_savings_cheapest_on` / `item_view_savings_save` / `item_compare_action`.
- Produces: `struct ZoneSavings { cheapest: CheapestListingData, hq: bool, savings: i32, savings_percent: f64 }` and `fn zone_savings(local_floor_nq: Option<i32>, local_floor_hq: Option<i32>, summary: &PriceSummary, current_world_id: i32) -> Option<ZoneSavings>`.

- [ ] **Step 1: Write failing tests for `zone_savings`** (pure data; replaces the `cheapest_savings_verdict` tests at item_view.rs:2053-2135):

```rust
fn zone_listing(price: i32, world_id: i32) -> CheapestListingData {
    CheapestListingData { price, world_id }
}

#[test]
fn zone_savings_reports_cheaper_other_world() {
    let summary = PriceSummary { lq: Some(zone_listing(3_000, 200)), hq: None };
    let result = zone_savings(Some(5_000), None, &summary, 100).unwrap();
    assert_eq!(result.savings, 2_000);
    assert!(!result.hq);
    assert_eq!(result.cheapest.world_id, 200);
}

#[test]
fn zone_savings_none_when_cheapest_is_current_world() {
    let summary = PriceSummary { lq: Some(zone_listing(3_000, 100)), hq: None };
    assert!(zone_savings(Some(5_000), None, &summary, 100).is_none());
}

#[test]
fn zone_savings_ignores_trivial_savings() {
    // Below MEANINGFUL_CROSS_WORLD_SAVINGS_GIL (1_000)
    let summary = PriceSummary { lq: Some(zone_listing(4_500, 200)), hq: None };
    assert!(zone_savings(Some(5_000), None, &summary, 100).is_none());
}

#[test]
fn zone_savings_picks_larger_quality_saving() {
    let summary = PriceSummary {
        lq: Some(zone_listing(3_000, 200)),   // saves 2_000
        hq: Some(zone_listing(10_000, 300)),  // saves 30_000
    };
    let result = zone_savings(Some(5_000), Some(40_000), &summary, 100).unwrap();
    assert!(result.hq);
    assert_eq!(result.savings, 30_000);
}

#[test]
fn zone_savings_none_without_local_floor() {
    // Nothing listed locally to compare against — no claim to make.
    let summary = PriceSummary { lq: Some(zone_listing(3_000, 200)), hq: None };
    assert!(zone_savings(None, None, &summary, 100).is_none());
}
```

- [ ] **Step 2: Run to verify failure**, then **implement**:

```rust
/// Cross-world savings hint derived from the zone-wide cheapest map.
///
/// Replaces the listings-payload `SavingsVerdict`: a world-scoped listings
/// request only contains that world (world_cache.rs `get_all_worlds_in`),
/// so the old cross-world comparison could never fire.
#[derive(Clone, Debug, PartialEq)]
struct ZoneSavings {
    cheapest: CheapestListingData,
    hq: bool,
    savings: i32,
    savings_percent: f64,
}

fn zone_savings_for_quality(
    local_floor: Option<i32>,
    zone_cheapest: Option<CheapestListingData>,
    hq: bool,
    current_world_id: i32,
) -> Option<ZoneSavings> {
    let local = local_floor?;
    let cheapest = zone_cheapest?;
    if cheapest.world_id == current_world_id || cheapest.price <= 0 || local <= 0 {
        return None;
    }
    let savings = local - cheapest.price;
    if savings < MEANINGFUL_CROSS_WORLD_SAVINGS_GIL {
        return None;
    }
    Some(ZoneSavings {
        cheapest,
        hq,
        savings,
        savings_percent: (savings as f64 / local as f64) * 100.0,
    })
}

fn zone_savings(
    local_floor_nq: Option<i32>,
    local_floor_hq: Option<i32>,
    summary: &PriceSummary,
    current_world_id: i32,
) -> Option<ZoneSavings> {
    [
        zone_savings_for_quality(local_floor_nq, summary.lq, false, current_world_id),
        zone_savings_for_quality(local_floor_hq, summary.hq, true, current_world_id),
    ]
    .into_iter()
    .flatten()
    .max_by_key(|savings| savings.savings)
}
```

- [ ] **Step 3: Rewire `DecisionHeader`**

- Add prop `item_id: Memo<i32>` (caller at line 1646 passes it: `<DecisionHeader listing_resource filtered_listings world item_id />`).
- Delete `SavingsVerdict`, `SavingsVerdict::new`, `savings_verdict_for_quality`, `cheapest_savings_verdict` (lines 80-110, 535-567) and their tests (2053-2135). Keep `format_savings_percent` and its test.
- In the component: `let cheapest_prices = use_context::<CheapestPrices>();` plus the hydrated-flag idiom copied from `MarketStatsPanel` (lines 740-743) — the zone resource must read as unavailable during SSR and initial hydration or the shapes mismatch (same class as GlitchTip #5270; see the long comment at item_view.rs:715-739).
- Replace the `savings_verdict` computation (lines 613-614): local floors from `filtered_listings` (`min price_per_unit` per quality where `world_id == current_world_id`), zone summary from `cheapest_prices.read_listings.with(|r| … find_matching_listings(item_id()))` gated on `hydrated.get()`, then `zone_savings(…)`.
- Replace the banner markup's data sources (lines 651-690): `verdict.cheapest_listing.world_id` → `savings.cheapest.world_id`, `verdict.cheapest_listing.price_per_unit` → `savings.cheapest.price`, `verdict.cheapest_listing.hq` → `savings.hq`, `verdict.savings*` → `savings.savings*`. Keep the emerald pill styling. Change the wrapping `<a href="#listings">` into a `<button>` whose `on:click` runs `set_compare_world.set(Some(buy_name))` where `buy_name = world_data.lookup_selector(AnySelector::World(savings.cheapest.world_id)).map(|w| w.get_name().to_string())` (skip rendering the button when unresolvable), with `(compare_world, set_compare_world) = filter_query_signal::<String>(COMPARE_BUY_FROM_PARAM)` declared once at component top. Append a trailing `<span class="font-semibold underline">{t!(i18n, item_compare_action)}</span>` so the affordance is legible. Hide the line entirely when `compare_world` already matches that world (the card is open — don't advertise it twice).

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros-app`
Expected: PASS — new `zone_savings` tests green, removed tests gone, no other references to `SavingsVerdict` (grep to confirm).

- [ ] **Step 5: check_ci + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -- ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "feat(item-view): zone-driven savings line with Compare action, replacing dead SavingsVerdict"
```

---

### Task 7: Flip-finder rows link into compare mode

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (row view closure, lines 2635-2710)

**Interfaces:**
- Consumes: `compare_item_href` (Task 2); `AnalyzerTable`'s `world: Signal<String>` prop (line 1064 — the page's sell-world scope, NB the raw route param, so `Url::unescape` it).

- [ ] **Step 1: Rebind the shadowed `world` in the row closure**

In the `view=move |(index, data)|` closure (line 2635): the local `let world = worlds.lookup_selector(AnySelector::World(data.inner.cheapest_world_id))…` chain (lines 2647-2663) is the BUY world and currently shadows the table's sell-world prop. Before that chain, capture the outer prop: `let sell_world = world;` (Signals are `Copy`). Rename the chain's final bindings `world`/`datacenter` to `buy_world`/`buy_datacenter` and update every use inside this closure (the world/DC display cells and the line-2689 href).

- [ ] **Step 2: Point the item link at the sell world with the compare param**

Replace line 2689's `href=format!("/item/{}/{item_id}", world())` with:

```rust
href=move || {
    let sell = leptos_router::location::Url::unescape(&sell_world.get());
    crate::routes::item_view_scope::compare_item_href(&sell, item_id, &buy_world())
}
```

(`compare_item_href` already degrades to a plain item link when `buy_world` resolved to an empty string.)

- [ ] **Step 3: Compile, spot-check the href**

Run: `cargo test -p ultros-app item_view_scope` (href behavior is covered there) and `cargo check -p ultros-app`.
Expected: PASS / clean.

- [ ] **Step 4: check_ci + commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
git add -- ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(flip-finder): row links open the item page's flip comparison card"
```

---

### Task 8: Full verification

- [ ] **Step 1: Full gates**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
cargo test -p ultros-app
```

Expected: REAL_EXIT=0; all tests pass.

- [ ] **Step 2: Live check (best-effort on Windows)**

Run the app if practical (memory: `bin-features=[]` + jemalloc/MSVC wall; another session may own :8080 — verify the served build is YOURS by grepping the page for a string you added, e.g. `compare-buy-from`, before trusting anything). Then:

1. Open `/item/{SellWorld}/{item_id}?compare-buy-from={BuyWorld}` for a same-DC pair with market data — card renders with buy price, estimate, profit; numbers agree with the flip-finder row for the same item/worlds.
2. Dismiss (X) — param leaves the URL with no scroll jump and no extra history entry (back button leaves the page, not the card).
3. Switch sell world via the world buttons — param survives (item_href carry).
4. Bogus param (`?compare-buy-from=Nowhere`) and DC-scoped page (`/item/Aether/{id}?compare-buy-from=Jenova`) — no card, no crash, no hydration panic in console.
5. Flip-finder row click lands on the sell world's page with the card open.

If a local run is impractical, note in the PR that verification was tests + fmt/clippy only.

- [ ] **Step 3: Push and open the PR** (base `main`; include the spec + plan; note that `SavingsVerdict` was removed as dead code and why).
