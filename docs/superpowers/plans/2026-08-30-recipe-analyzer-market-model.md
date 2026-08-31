# Recipe Analyzer Market Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the 2026-08-30 market-model spec: a permanent Market menu (buy scope / cost basis / revenue metric), widened ClickHouse sale stats with new columns, and world/DC columns + filters.

**Architecture:** Three stacked PRs. Phase 1 is frontend-only: a `BuyScope` enum replaces `MarketScope`, `RevenueMetric::WorldMin` is removed (revenue is always per-sell-world), and a Market popover button lands in ControlBar row 1. Phase 2 widens `bulk_sale_stats` (ClickHouse + API types + endpoint) and adds a Columns picker with stats-backed columns. Phase 3 adds cheapest-listing World/DC columns and filters.

**Tech Stack:** Rust, Leptos 0.7 (SSR+hydrate), leptos-i18n, ClickHouse via `clickhouse` crate, axum.

**Corrections to the spec discovered during planning (spec stands, details adjusted):**
- `daily_sales` is already sell-world scoped (`get_recent_sales_for_world(selected_world)`), so "Sales/day (sell world)" needs no data change — only ensuring the column is default-visible.
- `?world=` is the sell-world picker's param on this route, so Phase 3 filter keys are `listing-world` / `listing-dc` (not `world`/`datacenter`).
- Confidence bands are stored per `(item, hq, world)`; the widened endpoint returns them only when the scope resolves to a single world, `Unknown` otherwise.

**Branch/PR strategy:** Phase 1 on `claude/issue-1233-migration-103233` → PR to main. Phase 2 branches off Phase 1's branch, Phase 3 off Phase 2's; each PR body notes its base. **No PR says "closes #1233"** — reference it as "part of #1233" only.

**Pre-existing working-tree state:** the branch carries an uncommitted "PricingMenu" draft in `recipe_analyzer.rs` + locale files (a `recipe_analyzer_pricing_button` key). Task 1.3 reshapes it into the Market menu; rename the i18n key rather than adding a second one.

**Every commit:** run `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"` first and check the real exit code. Unit tests: `cargo test -p ultros-app` (frontend), `cargo test -p ultros-clickhouse` (CH, unit only), `cargo test -p ultros-api-types`.

---

## Phase 1 — Market menu + buy/sell model (frontend only)

### Task 1.1: `BuyScope` replaces `MarketScope`; `RevenueMetric` loses `WorldMin`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/price_basis.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (imports + all `MarketScope` references — done fully in Task 1.4; here only enough to keep compiling is NOT possible, so Tasks 1.1–1.4 form one commit)

- [ ] **Step 1: Write the failing tests** (replace the existing `defaults` and extend `url_values_round_trip` in `price_basis.rs`; add `override_listings` tests)

```rust
#[test]
fn url_values_round_trip() {
    // CostBasis loop unchanged...
    for metric in [
        RevenueMetric::ListingMin,
        RevenueMetric::SaleMedian,
        RevenueMetric::SaleMin,
        RevenueMetric::SaleAvg,
    ] {
        assert_eq!(metric.to_string().parse(), Ok(metric));
    }
    for scope in [BuyScope::World, BuyScope::Datacenter, BuyScope::Region] {
        assert_eq!(scope.to_string().parse(), Ok(scope));
    }
}

#[test]
fn defaults() {
    assert_eq!(CostBasis::default(), CostBasis::ListingMin);
    // Revenue is always evaluated on the sell world now; the default is its
    // cheapest current listing (the price you'd actually list at).
    assert_eq!(RevenueMetric::default(), RevenueMetric::ListingMin);
    // Buying defaults to the datacenter: a realistic "purchase zone" that
    // doesn't assume cross-region travel.
    assert_eq!(BuyScope::default(), BuyScope::Datacenter);
}

#[test]
fn world_min_token_no_longer_parses() {
    // `revenue=world-min` is handled by the page-level compat mapping
    // (Task 1.5), not by the enum.
    assert!("world-min".parse::<RevenueMetric>().is_err());
}

#[test]
fn override_listings_prefers_the_override_where_present() {
    let base = listings(&[(1, false, 100, 7), (2, false, 200, 7)]);
    let world = listings(&[(1, false, 150, 42)]);
    let merged = override_listings(&base, &world);
    // Item 1: the sell world's own (higher) listing wins — that is the
    // price you'd actually list at.
    assert_eq!(merged.find_matching_listings(1).lowest_gil(), Some(150));
    // Item 2: no listing on the sell world — fall back to the base map so
    // the row isn't dropped as unpriceable.
    assert_eq!(merged.find_matching_listings(2).lowest_gil(), Some(200));
}
```

- [ ] **Step 2: Implement in `price_basis.rs`**
  - Delete `MarketScope` (enum + FromStr + Display).
  - Add `BuyScope`:

```rust
/// Where ingredient prices are searched: the sell world only, its
/// datacenter, or the whole region. Also scopes the cost-basis sale stats.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BuyScope {
    World,
    #[default]
    Datacenter,
    Region,
}

impl FromStr for BuyScope {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "world" => Ok(BuyScope::World),
            "datacenter" => Ok(BuyScope::Datacenter),
            "region" => Ok(BuyScope::Region),
            _ => Err(()),
        }
    }
}

impl Display for BuyScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BuyScope::World => "world",
            BuyScope::Datacenter => "datacenter",
            BuyScope::Region => "region",
        })
    }
}
```

  - `RevenueMetric`: remove the `WorldMin` variant; `#[default]` moves to `ListingMin`; remove its `FromStr`/`Display`/`sale_stat` arms. Update the module doc comment (revenue is per-sell-world; `world-min` handled by URL compat).
  - Add `override_listings`:

```rust
/// Base map with every entry the override map carries replacing the base's.
/// Used for revenue: the sell world's own listing wins even when it is
/// higher than the buy-scope minimum (it is the price you would list at);
/// items with no sell-world listing keep the base price so rows aren't
/// dropped as unpriceable — same fallback the old `WorldMin` metric had.
pub fn override_listings(
    base: &CheapestListingsMap,
    over: &CheapestListingsMap,
) -> CheapestListingsMap {
    let mut map = base.map.clone();
    for (key, data) in &over.map {
        map.insert(*key, *data);
    }
    CheapestListingsMap { map }
}
```

  (If `CheapestListingData` is not `Copy`, clone it — check the type.)

- [ ] **Step 3: Run** `cargo test -p ultros-app price_basis` — the new tests pass; the crate will NOT fully compile until Task 1.4 removes the `MarketScope`/`WorldMin` references in `recipe_analyzer.rs`. Proceed; commit happens at the end of Task 1.5.

### Task 1.2: i18n keys for the Market menu (all 7 locales)

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

- [ ] **Step 1:** Rename the draft key `recipe_analyzer_pricing_button` → `recipe_analyzer_market_button` and set values; add the new keys next to `recipe_analyzer_cost_basis_label`:

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `recipe_analyzer_market_button` | Market | Marché | Markt | 市場設定 | 市场设置 | 시장 설정 | 市場設定 |
| `recipe_analyzer_buy_from_label` | Buy from | Acheter depuis | Einkaufen aus | 購入範囲 | 购买范围 | 구매 범위 | 購買範圍 |
| `recipe_analyzer_sell_world_label` | Sell on | Vendre sur | Verkaufen auf | 販売ワールド | 出售服务器 | 판매 서버 | 販售伺服器 |
| `buy_scope_home_world` | This world only | Ce monde uniquement | Nur diese Welt | このワールドのみ | 仅此服务器 | 이 서버만 | 僅此伺服器 |

  Reuse existing `region` / `datacenter` keys for the other two Buy-from options. The old `select_world_for_sales_data` key stays for other pages if referenced — check with `grep -rn select_world_for_sales_data ultros-frontend/ultros-app/src`; if only recipe_analyzer uses it, delete it from all locales.

- [ ] **Step 2:** `cargo check -p ultros-app` after Task 1.4 wires the keys (leptos-i18n fails the build on a key missing from any locale).

### Task 1.3: Market menu component

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`

- [ ] **Step 1:** Rework the draft `PricingMenu` (already in the working tree) into `MarketMenu`. Keep `PricingSelect` and the free option-list fns (`cost_basis_options`, `revenue_options`) as drafted; changes:
  - `revenue_options` no longer appends `world-min` — it becomes an alias of `cost_basis_options` (delete `revenue_options`, use `cost_basis_options` at both call sites; `price_basis_world_min` locale key is deleted from all 7 locales).
  - `scope_options` → `buy_scope_options`:

```rust
fn buy_scope_options(i18n: I18nContext<Locale, I18nKeys>) -> Vec<(&'static str, String)> {
    vec![
        ("world", t_string!(i18n, buy_scope_home_world).to_string()),
        ("datacenter", t_string!(i18n, datacenter).to_string()),
        ("region", t_string!(i18n, region).to_string()),
    ]
}
```

  - The menu (same button+popover shape as the draft, `MdiCashMultiple` icon, `recipe_analyzer_market_button` label) holds three `PricingSelect`s: Buy from (`FILTER_BUY_SCOPE`, `BuyScope`), Cost basis, Revenue metric. Each commits `parsed.filter(|v| *v != Default::default())` exactly as drafted.
  - Doc comment: pricing methodology gets a standing row-1 entry point (#1233); it is not a row filter.

- [ ] **Step 2:** Keep `<MarketMenu />` mounted in ControlBar `actions` next to `RealtimeStatus` (the draft already does this — just the rename).

### Task 1.4: Rewire the data flow (buy scope + per-world revenue)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`

- [ ] **Step 1: Constants + signals.** `FILTER_SCOPE`/"scope" → `FILTER_BUY_SCOPE`/"buy-scope". All `MarketScope` signal types → `BuyScope`. In the chip row, the scope chip becomes the buy-scope chip (`buy_scope_options(i18n)`, label `recipe_analyzer_buy_from_label`); revenue chip uses `cost_basis_options(i18n)`.

- [ ] **Step 2: Buy-scope name resolution** (in `RecipeAnalyzer`, replacing `price_scope_name`):

```rust
let (buy_scope, _) = filter_query_signal::<BuyScope>(FILTER_BUY_SCOPE);
// The name fed to ingredient-pricing fetches. World scope needs a selected
// world; before one resolves (first paint without a cookie) fall back to
// the datacenter, then the region, so the resource always has a fetchable
// name.
let buy_scope_name = Memo::new(move |_| match buy_scope().unwrap_or_default() {
    BuyScope::World => selected_world
        .get()
        .map(|w| w.name)
        .or_else(|| datacenter.get())
        .unwrap_or_else(|| region.get()),
    BuyScope::Datacenter => datacenter().unwrap_or_else(|| region.get()),
    BuyScope::Region => region(),
});
```

  Note ordering: `selected_world` is defined *below* the old `price_scope_name` — move the `initial_world`/`selected_world` block above this memo.
  `global_cheapest_listings` and the (buy-side) `sale_stats` resources key off `buy_scope_name` unchanged otherwise. The buy-side stats laziness condition becomes cost-basis-only:

```rust
let buy_sale_stats_scope = Memo::new(move |_| {
    cost_basis()
        .unwrap_or_default()
        .sale_stat()
        .is_some()
        .then(|| buy_scope_name.get())
});
```

- [ ] **Step 3: Sell-world resources.** Replace `world_min_world`/`world_min_listings` with an unconditional sell-world listings fetch plus a lazy sell-world stats fetch:

```rust
// Revenue is always the sell world's price now, so its listings are always
// needed (the old fetch was gated on the world-min metric).
let sell_world_name = Memo::new(move |_| selected_world.get().map(|w| w.name));
let sell_world_listings =
    ArcResource::new(sell_world_name, move |world: Option<String>| async move {
        match world {
            Some(world) => get_cheapest_listings(&world).await.map(Some),
            None => Ok(None),
        }
    });
// Sale-stat revenue metrics read the sell world's history, not the buy
// scope's — fetched only while such a metric is selected.
let sell_stats_world = Memo::new(move |_| {
    revenue_metric()
        .unwrap_or_default()
        .sale_stat()
        .is_some()
        .then(|| sell_world_name.get())
        .flatten()
});
let sell_world_sale_stats =
    ArcResource::new(sell_stats_world, move |world: Option<String>| async move {
        match world {
            Some(name) => get_sale_stats(&name, SALE_STATS_WINDOW_DAYS).await.map(Some),
            None => Ok(None),
        }
    });
```

- [ ] **Step 4: Table props + revenue map.** `RecipeAnalyzerTable` props: `sale_stats` stays (buy side), add `sell_world_sale_stats: Option<BulkSaleStats>`, rename `world_listings` → `sell_world_listings`. `sale_stats_error` is true when *either* selected sale-stat fetch failed. In the table:

```rust
let sell_world_prices = sell_world_listings.map(|l| Arc::new(CheapestListingsMap::from(l)));
let sell_world_sale_stats = Arc::new(sell_world_sale_stats.unwrap_or_default());
// Revenue base: buy-scope listings with the sell world's own entries
// winning (see override_listings). Sale-stat metrics overlay the sell
// world's history on top.
let revenue_prices = {
    let prices = prices.clone();
    let sell_stats = sell_world_sale_stats.clone();
    let world = sell_world_prices.clone();
    Memo::new(move |_| {
        let base = match &world {
            Some(w) => Arc::new(override_listings(&prices, w)),
            None => prices.clone(),
        };
        match revenue_metric().unwrap_or_default().sale_stat() {
            None => base,
            Some(stat) => Arc::new(overlay_sale_stats(&base, &sell_stats, stat)),
        }
    })
};
```

  In `computed_data`: delete the `RevenueMetric::WorldMin` match — `market_price` is simply `revenue.find_matching_listings(recipe.item_result).lowest_gil().unwrap_or(0)`. Compute `cheapest_world_id` from the **raw buy-scope map** (capture `let raw_prices = prices.clone();` outside the memo) instead of the revenue summary, so it keeps meaning "where the scope-cheapest listing sits" for Phase 3:

```rust
let scope_summary = raw_prices.find_matching_listings(recipe.item_result);
let cheapest_world_id = scope_summary
    .lq.map(|d| d.world_id)
    .or(scope_summary.hq.map(|d| d.world_id))
    .unwrap_or(0);
```

  Update the Suspense `match` in `RecipeAnalyzer` to also await `sell_world_sale_stats` and pass the new props. Relabel the world picker: `select_world_for_sales_data` → `recipe_analyzer_sell_world_label`.

### Task 1.5: URL compat mapping + contract test + commit

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`

- [ ] **Step 1: Write the failing tests** (module-level pure fn + test):

```rust
/// Rewrite pre-market-model query params (#1206 era) to their successors.
/// Returns `None` when nothing needs rewriting (the common case — avoids a
/// navigate loop). `scope` carried region|datacenter; `revenue=world-min`
/// described what is now the default and simply drops.
fn migrate_legacy_params(pairs: &[(String, String)]) -> Option<Vec<(String, String)>> {
    let legacy = pairs
        .iter()
        .any(|(k, v)| k == "scope" || (k == "revenue" && v == "world-min"));
    if !legacy {
        return None;
    }
    Some(
        pairs
            .iter()
            .filter(|(k, v)| !(k == "revenue" && v == "world-min"))
            .map(|(k, v)| {
                if k == "scope" {
                    ("buy-scope".to_string(), v.clone())
                } else {
                    (k.clone(), v.clone())
                }
            })
            .collect(),
    )
}

#[test]
fn legacy_scope_param_becomes_buy_scope() {
    let out = migrate_legacy_params(&[
        ("world".into(), "Gilgamesh".into()),
        ("scope".into(), "datacenter".into()),
    ])
    .unwrap();
    assert_eq!(out, vec![
        ("world".to_string(), "Gilgamesh".to_string()),
        ("buy-scope".to_string(), "datacenter".to_string()),
    ]);
}

#[test]
fn legacy_world_min_revenue_drops() {
    let out = migrate_legacy_params(&[("revenue".into(), "world-min".into())]).unwrap();
    assert!(out.is_empty());
}

#[test]
fn modern_urls_are_left_alone() {
    assert_eq!(
        migrate_legacy_params(&[("buy-scope".into(), "region".into())]),
        None
    );
}
```

- [ ] **Step 2:** Wire it in `RecipeAnalyzer` (once, on mount — `Effect::new` with untracked query read, navigate `replace: true`):

```rust
Effect::new(move |_| {
    let pairs: Vec<(String, String)> =
        query.with_untracked(|q| q.clone().into_iter().collect());
    if let Some(migrated) = migrate_legacy_params(&pairs) {
        let qs = migrated
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        nav(
            &format!("?{qs}"),
            NavigateOptions { replace: true, scroll: false, ..Default::default() },
        );
    }
});
```

  (`nav` is already cloned for the world effect — make a second clone.)

- [ ] **Step 3:** Update `filter_registry_keys_are_a_stable_url_contract`: `ADDABLE_FILTERS` no longer carries the pricing ids; assert the trimmed list, and add a second assertion pinning the pricing param keys:

```rust
// Pricing params left the filter menu (#1233) but their URL keys are
// still a bookmark contract.
assert_eq!(FILTER_COST_BASIS, "cost-basis");
assert_eq!(FILTER_REVENUE, "revenue");
assert_eq!(FILTER_BUY_SCOPE, "buy-scope");
```

- [ ] **Step 4:** `cargo test -p ultros-app` → all pass. `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"` → 0.
- [ ] **Step 5:** Commit everything from Tasks 1.1–1.5:

```bash
git add -A
git commit -m "feat(recipe-analyzer): Market menu with buy scope and per-world revenue (#1233 part 1)"
```

### Task 1.6: E2E smoke + PR 1

- [ ] **Step 1:** `./scripts/run_e2e.sh` (SSR-sensitive UI change; watch for hydration mismatches per the no-local-storage-in-SSR history). Fix anything it surfaces.
- [ ] **Step 2:** Push and open the PR. Body: what/why, the default-behavior change (region → buy-DC / sell-home), the URL compat mapping, "part of #1233" — **not** "closes".

```bash
git push -u origin claude/issue-1233-migration-103233
gh pr create --repo akarras/ultros --title "Recipe Analyzer: Market menu — buy scope + per-world revenue" --body-file /tmp/pr1.md
```

---

## Phase 2 — Widened sale stats + new columns

Branch: `git checkout -b claude/recipe-analyzer-stats-columns` off Phase 1's branch.

### Task 2.1: Widen `ItemSaleStats` (api types)

**Files:**
- Modify: `ultros-api-types/src/sale_stats.rs`

- [ ] **Step 1: Failing test** (old-shape payload must still deserialize):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_wire_shape_still_deserializes() {
        let old = r#"{"item_id":1,"hq":false,"min_price":10,"median_price":20,"avg_price":21,"num_sold":5}"#;
        let row: ItemSaleStats = serde_json::from_str(old).unwrap();
        assert_eq!(row.last_sold_unix, 0);
        assert_eq!(row.units_sold, 0);
        assert_eq!(row.vwap, 0);
        assert_eq!(row.sales_per_day, 0.0);
        assert_eq!(row.confidence, ConfidenceBand::Unknown);
    }
}
```

- [ ] **Step 2:** Add serde-defaulted fields to `ItemSaleStats` (import `crate::trends::ConfidenceBand`):

```rust
    /// Unix seconds of the newest sale in the window. 0 = unknown (old server).
    #[serde(default)]
    pub last_sold_unix: i64,
    /// Units traded in the window (sum of quantities).
    #[serde(default)]
    pub units_sold: u64,
    /// Volume-weighted average per-unit price over the window, rounded. 0 = unknown.
    #[serde(default)]
    pub vwap: i32,
    /// `num_sold / window_days`, precomputed server-side.
    #[serde(default)]
    pub sales_per_day: f32,
    /// Per-world confidence band; `Unknown` for multi-world scopes or old servers.
    #[serde(default)]
    pub confidence: ConfidenceBand,
```

  `f32` breaks `Eq`: change `ItemSaleStats` and `BulkSaleStats` derives from `Eq` to `PartialEq` only (grep for uses requiring `Eq` first — `price_basis.rs` tests only use `==`).
- [ ] **Step 3:** `cargo test -p ultros-api-types` → pass. Fix the `price_basis.rs` test fixture (`stats(...)` constructor) to fill the new fields with `..Default::default()`-style values (add `Default` derive to `ItemSaleStats` or spell the fields out).
- [ ] **Step 4:** Commit: `git commit -am "feat(api-types): widen ItemSaleStats with volume/vwap/last-sold/confidence"`

### Task 2.2: Widen the ClickHouse query + bulk confidence

**Files:**
- Modify: `ultros-clickhouse/src/queries.rs`
- Test: `ultros-clickhouse/tests/sale_stats_smoke.rs` (new, copy the harness shape from `price_density_smoke.rs`)

- [ ] **Step 1:** Check the `sales` schema for the quantity column name (`grep -n "quantity" ultros-clickhouse/src/*.rs` — the writer's `SaleRow` defines it). Use the exact name below.
- [ ] **Step 2:** Extend `BulkSaleStatsRow` + SQL:

```rust
pub struct BulkSaleStatsRow {
    // existing fields...
    pub last_sold_unix: i64,
    pub units_sold: u64,
    pub vwap: i32,
}
```

```sql
    toInt64(max(toUnixTimestamp(sold_date)))    AS last_sold_unix,
    toUInt64(sum(quantity))                     AS units_sold,
    toInt32(round(if(sum(quantity) = 0, 0,
        sum(price_per_item * quantity) / sum(quantity)))) AS vwap,
```

- [ ] **Step 3:** New query, single-world only, keyed lookup — no unfiltered joins:

```rust
/// Per-(item, hq) confidence bands for ONE world. The band is a stored
/// per-world judgement (see `aggregate_item_stats_variants` for why it
/// cannot be recomputed across worlds), so multi-world scopes don't call
/// this and report `Unknown`.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct BulkConfidenceRow {
    pub item_id: i32,
    pub hq: u8,
    pub confidence_band_raw: String,
}

pub async fn bulk_confidence(
    ch: &ClickHouseClient,
    world_id: i32,
) -> Result<Vec<BulkConfidenceRow>, ClickHouseError> {
    let sql = format!(
        "SELECT item_id, hq, toString(confidence_band) AS confidence_band_raw
         FROM item_quality_score FINAL
         WHERE world_id = {world_id}"
    );
    Ok(ch.client().query(&sql).fetch_all().await?)
}
```

  Add `impl BulkConfidenceRow { pub fn confidence_band(&self) -> ConfidenceBand }` reusing the same match as `DeepScan::confidence_band` (extract that match into a free fn `parse_confidence_band(&str)` used by both).
- [ ] **Step 4: Smoke test** (`sale_stats_smoke.rs`, gated on `ULTROS_CH_INTEGRATION`): insert fixture sales for one item — 2 sales of (price 100 × qty 1) and (price 200 × qty 3) with distinct `sold_date`s — assert `vwap == 175` (weighted, not 150), `units_sold == 4`, `last_sold_unix` equals the newer timestamp; plus a `bulk_confidence` row round-trip. Run per the memory recipe (throwaway docker CH) if the env var is set; otherwise note in the PR that only the unit suite ran locally.
- [ ] **Step 5:** `cargo test -p ultros-clickhouse` → pass. Commit: `git commit -am "feat(clickhouse): widen bulk_sale_stats, add bulk_confidence"`

### Task 2.3: Endpoint wiring

**Files:**
- Modify: `ultros/src/web/api/sale_stats.rs`

- [ ] **Step 1:** After `bulk_sale_stats`, fetch confidence when the scope is one world, and map:

```rust
let confidence: HashMap<(i32, bool), ConfidenceBand> = match world_ids.as_slice() {
    [only] => ultros_clickhouse::queries::bulk_confidence(&ch, *only)
        .await
        .map_err(|e| ClickHouseQueryError::new("bulk_confidence", e))?
        .into_iter()
        .map(|r| ((r.item_id, r.hq != 0), r.confidence_band()))
        .collect(),
    _ => HashMap::new(),
};
```

  In the row mapping fill `last_sold_unix`, `units_sold`, `vwap`, `sales_per_day: r.num_sold as f32 / window_days as f32`, `confidence: confidence.get(&(r.item_id, r.hq != 0)).copied().unwrap_or_default()`. Doc-comment the single-world confidence rule at the top of the file.
- [ ] **Step 2:** `cargo check -p ultros` → pass. Commit: `git commit -am "feat(api): serve widened sale stats + per-world confidence"`

### Task 2.4: Columns picker + new columns in the recipe analyzer

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/control_bar.rs` (host the shared cols helpers)
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` (use the shared helpers)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`
- Modify: locale files (column headers)

- [ ] **Step 1:** Move `parse_visible_cols`/`serialize_visible_cols` from `analyzer.rs` into `control_bar.rs` as pub fns parameterized by the page's column-order slice + default set (signatures follow the existing fns; move their unit tests along). Flip finder switches to the shared fns — behavior identical, tests prove it.
- [ ] **Step 2:** Recipe analyzer column registry (`?cols=` namespace, distinct from filters):

```rust
const COL_LAST_SOLD: &str = "last-sold";
const COL_VOLUME: &str = "volume";
const COL_VWAP: &str = "vwap";        // renders VWAP and % vs VWAP together
const COL_TAX: &str = "tax";
const COL_CONFIDENCE: &str = "confidence";
const OPTIONAL_COLUMN_ORDER: &[&str] =
    &[COL_CONFIDENCE, COL_LAST_SOLD, COL_VOLUME, COL_VWAP, COL_TAX];
/// Default-visible optional columns (spec: Sales/day is already an
/// always-on column; Confidence joins it by default).
const DEFAULT_COLS: &[&str] = &[COL_CONFIDENCE];
```

  Wire `ControlBar`'s `columns`/`visible_columns`/`on_toggle_column`/`on_reset_columns` props exactly as `analyzer.rs:1247-1411` does.
- [ ] **Step 3:** Data: make the sell-world stats fetch (Task 1.4 Step 3) also fire when any stats-backed column is visible:

```rust
let sell_stats_world = Memo::new(move |_| {
    let stats_column_visible = visible_cols.with(|c| {
        [COL_LAST_SOLD, COL_VOLUME, COL_VWAP, COL_CONFIDENCE]
            .iter()
            .any(|id| c.contains(id))
    });
    (revenue_metric().unwrap_or_default().sale_stat().is_some() || stats_column_visible)
        .then(|| sell_world_name.get())
        .flatten()
});
```

  (`visible_cols` lives in the table component in Phase 2 — hoist the `cols` query signal into `RecipeAnalyzer` so both can read it, passing the memo down.)
  `RecipeProfitData` gains `last_sold_unix: i64`, `units_sold: u64`, `vwap: i32`, `vwap_pct: Option<f32>` (None when vwap==0), `tax: i32`, `confidence: ConfidenceBand`; fill from a `HashMap<(i32, bool), &ItemSaleStats>` built once from `sell_world_sale_stats` (key on `(item_result, hq)` — look up NQ first, HQ if require-HQ is on, matching how `market_price` resolves). `tax = market_price - net_revenue`. `vwap_pct = (market_price - vwap) / vwap * 100`.
- [ ] **Step 4:** Rendering: one header + cell per optional column, gated on `visible.contains(col)`, using `SortableHeaderCell`. New `SortMode` variants + tokens: `LastSold`/"last-sold" (Desc = most recent first — compare on `last_sold_unix`), `Volume`/"volume", `Vwap`/"vwap", `Tax`/"tax", `Confidence`/"confidence" (Desc = High first; derive an ordinal: Unknown=0, Unusable=1, Low=2, Medium=3, High=4). Extend `compare_recipes` and the `SortMode` FromStr/Display pairs. Cells: last sold via the relative-time formatter the flip finder's `COL_LAST_SOLD` uses (find it with `grep -n last_sold ultros-frontend/ultros-app/src/routes/analyzer.rs`); confidence via the existing `ConfidenceBadge` component; gil values via the `Gil` component.
- [ ] **Step 5:** i18n column headers in all 7 locales — reuse flip-finder keys where they exist (`analyzer_col_*` for last sold/tax/confidence — check with grep); add `recipe_analyzer_col_volume` ("Volume (7d)" / real translations) and `recipe_analyzer_col_vwap` ("VWAP (7d)") only if no reusable key exists.
- [ ] **Step 6:** Unit tests: `SortMode` round-trip for the new tokens; confidence ordinal ordering; `vwap_pct` math incl. vwap=0 → None. Run `cargo test -p ultros-app`, then `./check_ci.sh`, commit:

```bash
git commit -am "feat(recipe-analyzer): columns picker with stats-backed columns"
```

### Task 2.5: PR 2

- [ ] `./scripts/run_e2e.sh`; push branch; `gh pr create` with base = Phase 1's branch (retarget to main after PR 1 merges). Body notes the stacking, the single-world confidence rule, and "part of #1233" (no "closes").

---

## Phase 3 — World/DC columns + filters

Branch: `claude/recipe-analyzer-world-cols` off Phase 2's branch.

### Task 3.1: Columns + filters

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`
- Modify: locale files

- [ ] **Step 1:** Column registry additions: `COL_LISTING_WORLD = "listing-world"`, `COL_LISTING_DC = "listing-dc"` appended to `OPTIONAL_COLUMN_ORDER` (not in `DEFAULT_COLS`). Resolve names from `cheapest_world_id` the way the flip finder's world/DC cells do (`grep -n "COL_WORLD\|world_name" ultros-frontend/ultros-app/src/routes/analyzer.rs` for the lookup; it goes through `LocalWorldData`). `cheapest_world_id == 0` (stat-overlay rows) renders "—".
- [ ] **Step 2:** Filters: `FILTER_LISTING_WORLD = "listing-world"`, `FILTER_LISTING_DC = "listing-dc"` — `filter_query_signal::<String>`, registered in `ADDABLE_FILTERS` (menu labels via new i18n keys `recipe_analyzer_filter_listing_world_label` / `..._dc_label`, e.g. en "Cheapest listing world" / "Cheapest listing datacenter"), chips mirror the flip finder's world/DC chips (select-type chip listing the scope's worlds/DCs — reuse the flip finder's option-building code shape). Retain predicate in `computed_data`'s filter block:

```rust
if let Some(world) = listing_world_filter() {
    results.retain(|d| world_name_of(d.cheapest_world_id).as_deref() == Some(world.as_str()));
}
if let Some(dc) = listing_dc_filter() {
    results.retain(|d| dc_name_of(d.cheapest_world_id).as_deref() == Some(dc.as_str()));
}
```

  (`world_name_of`/`dc_name_of`: small helpers over `LocalWorldData`, built once outside the loop as a `HashMap<i32, (String, String)>`.)
- [ ] **Step 3:** Extend the URL-contract test with the two new filter ids. Unit-test the retain predicates with a hand-built `HashMap`.
- [ ] **Step 4:** All locale keys in 7 files; `cargo test -p ultros-app`; `./check_ci.sh`; commit `feat(recipe-analyzer): cheapest-listing world/DC columns and filters`; e2e; PR 3 (base = PR 2's branch, "part of #1233", no "closes").

---

## Self-review checklist (done at write time)

- Spec coverage: Market menu (1.3), buy scope (1.1/1.4), per-world revenue + world-min removal (1.1/1.4), compat mapping (1.5), contract test (1.5), CH widening (2.2), endpoint (2.3), columns + defaults (2.4), world/DC cols + filters (3.1), i18n each task, e2e after each phase, no "closes #1233" anywhere.
- Sales/day default-on: covered by the correction note — it is the existing always-on velocity column; Confidence is the only newly default-visible column.
- Type consistency: `BuyScope` tokens match `buy_scope_options`; `ItemSaleStats.confidence` type `ConfidenceBand` matches `bulk_confidence` mapping; `FILTER_BUY_SCOPE = "buy-scope"` matches `migrate_legacy_params` output.
