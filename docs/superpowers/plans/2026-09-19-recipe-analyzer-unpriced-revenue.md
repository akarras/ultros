# Recipe analyzer unpriced revenue + evidence filters — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A sale-statistic revenue with no sale row in the window renders "—" and sorts last instead of borrowing another world's listing, and the filter bar gains "Sold in window" and "Last sold within" row filters.

**Architecture:** `RecipeProfitData` gets an explicit `Revenue` enum and an `Option<ProfitLine>` so "no revenue" is a value rather than a zero. The pricing pass (`price_rows`) keeps such rows, the sort partitions them last, and the grid's metric filters see `GridValue::Missing` for their profit/ROI/price. Both new filters are row filters the pricing pass applies natively (like the job filter), reading the sell-place statistics body at the page window; a new `RecipeNeeds::evidence` flag makes the fetch gate request that body when either filter is set.

**Tech Stack:** Rust (nightly-2026-08-20, let-chains allowed), Leptos 0.8, `leptos-i18n` (7 locales), `humantime` (already a dependency of `ultros-app`), the `ultros-calc` crate for `ProfitLine` / `RecipeNeeds`.

**Spec:** `docs/superpowers/specs/2026-09-19-recipe-analyzer-unpriced-revenue-design.md`. One deliberate deviation: the spec suggested implementing "Last sold within" as a `FilterAlias` over the Last sold column. That column is pinned to the seven-day context body, so at a 30- or 90-day window every row not sold in the last seven days would read "never" and the filter would be wrong. Both filters therefore read the window body inside `price_rows` instead (Task 3 and Task 4 explain the body choice).

## Global Constraints

- Before every commit run, from the repo root, `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"` where `SCRATCH` is this session's scratchpad directory, never `/tmp`. Both `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` must pass. Fix clippy findings in code; no `#[allow]`.
- Every new user-facing string goes through `leptos-i18n`: add the key to **all seven** locale files under `ultros-frontend/ultros-i18n/locales/` (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) with a real translation. Keys are `snake_case`.
- Range filters are inclusive (`<=` / `>=`), the repo convention.
- Run all commands from the worktree root `C:\Users\chw11\code\ultros\.claude\worktrees\gilgamesh-data-accuracy-2b6b03` (Git Bash path `/c/Users/chw11/code/ultros/.claude/worktrees/gilgamesh-data-accuracy-2b6b03`). Verify with `git rev-parse --show-toplevel` before the first commit. Never `cd` to the main checkout.
- Windows build env: prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH`, export `OPENSSL_RUST_USE_NASM=0` and `CARGO_PROFILE_DEV_DEBUG=0` in every shell that builds.
- Test commands: `cargo test -p ultros-app --lib -- recipe_analyzer` (the recipe analyzer tests; needs the LFS game-data packs, already present in this worktree) and `cargo test -p ultros-calc`. CI does not run tests, so these are the only place the tests run: run them.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Line numbers below are from the branch at commit `f7f67480` and shift as tasks land; anchor on the quoted code, not the number.

---

## File map

| File | Responsibility in this change |
|---|---|
| `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` | Row model (`Revenue`, `line`), `price_rows` (keep unpriced rows, evidence filters), sort partition, cells, filter controls, page/table wiring, tests |
| `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` | `CellValue::RoiBadge(Option<i32>)`, `CellValue::GilWithNote { amount: Option<i32> }`, `CellNote::NoSales`, their rendering |
| `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` | Query value and measure for the two changed cell shapes |
| `ultros-frontend/ultros-calc/src/needed.rs` | `RecipeNeeds::evidence` and its fetch gating |
| `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` | Four new keys |

---

### Task 1: `Revenue` enum, `Option<ProfitLine>`, and unpriced rows survive `price_rows`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (struct at ~112, `price_rows` ~2528–2795, cells ~1089–1177 and ~1305, sort ~2245–2275, custom render ~3704, query/measure closures ~4021 and ~4070, test helpers `row` ~7040 and `price_row` ~7473)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/cells.rs` (`CellValue` ~54, `CellNote` ~121, `render_cell` ~222 and ~275)
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/grid.rs` (`query_cell` ~480, `measure_cell` ~594)
- Modify: all seven locale files
- Test: `recipe_analyzer.rs` test module (starts at `#[cfg(test)]`, ~4958)

**Interfaces:**
- Produces on `RecipeProfitData`: `revenue: Revenue`, `line: Option<ProfitLine>`, methods `price() -> Option<i32>`, `fell_back() -> bool`, `profit() -> Option<i32>`, `roi() -> Option<i32>`, `tax() -> Option<i32>`. Removes fields `profit`, `return_on_investment`, `market_price`, `tax`, `revenue_fell_back`. Keeps `cost: i32`.
- Produces `enum Revenue { Priced { price: i32, fell_back: bool }, Unpriced }` (`Copy, Clone, Debug, PartialEq, Eq`).
- Produces test fixture `evidence_fixture(revenue: PriceSignal, output_history: Option<(i64, i64)>) -> Vec<RecipeProfitData>` (Tasks 3 and 4 extend its parameter list).
- Produces `CellValue::RoiBadge(Option<i32>)`, `CellValue::GilWithNote { amount: Option<i32>, note: CellNote }`, `CellNote::NoSales`.
- Produces i18n keys `analyzer_price_no_sales`, `analyzer_price_no_sales_title`.

- [ ] **Step 1: Write the failing tests**

In the test module of `recipe_analyzer.rs`, next to `quality_fixture_at` (~8170), add:

```rust
    /// The prod shape of 2026-09-18 (Gilgamesh, item 13047): the output
    /// has no listing and no sale on the sell world, and exactly one
    /// listing elsewhere on the buy scope, at a troll price. Returns the
    /// rows the pass kept (zero or one). `output_history` = the HQ
    /// output's `(last_sold_unix, num_sold)` on the sell world, priced at
    /// 50,000; `None` = never sold.
    fn evidence_fixture(
        revenue: PriceSignal,
        output_history: Option<(i64, i64)>,
    ) -> Vec<RecipeProfitData> {
        static RECIPE: Recipe = Recipe {
            key_id: xiv_gen::RecipeId(9999100),
            item_result: 9999101,
            amount_result: 1,
            ingredient: [9999102, 0, 0, 0, 0, 0, 0, 0],
            amount_ingredient: [1, 0, 0, 0, 0, 0, 0, 0],
            craft_type: 0,
            recipe_level_table: 0,
        };
        let listing = |item_id, hq, cheapest_price, world_id| CheapestListingItem {
            item_id,
            hq,
            cheapest_price,
            world_id,
        };
        // The buy scope (a datacenter): the ingredient at home, the output
        // only on another world.
        let buy = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![
                listing(9999102, false, 10, 1),
                listing(RECIPE.item_result, true, 24_999_999, 2),
            ],
        });
        // The sell world: the ingredient only.
        let sell = CheapestListingsMap::from(CheapestListings {
            cheapest_listings: vec![listing(9999102, false, 10, 1)],
        });
        let stat = |item_id, hq, price, last_sold_unix, num_sold| ItemSaleStats {
            item_id,
            hq,
            min_price: price,
            median_price: price,
            avg_price: price,
            vwap: price,
            units_sold: num_sold as u64,
            num_sold,
            last_sold_unix,
            ..Default::default()
        };
        let mut stats: StatsIndex = HashMap::new();
        stats.insert((9999102, false), stat(9999102, false, 10, 1_699_990_000, 7));
        if let Some((last_sold_unix, num_sold)) = output_history {
            stats.insert(
                (RECIPE.item_result, true),
                stat(RECIPE.item_result, true, 50_000, last_sold_unix, num_sold),
            );
        }
        let formula =
            ProfitFormula::recipe_from_query(None, Some(revenue), None).effective(false, true);
        let (rows, _) = price_rows(&PriceInputs {
            stats_failed: StatFailures::default(),
            recipes: &[&RECIPE],
            recipe_level_tables: &xiv_gen_db::data().recipe_level_tables,
            recipes_by_output: &HashMap::new(),
            buy_listings: &buy,
            sell_listings: Some(&sell),
            buy_stats: None,
            sell_stats: &stats,
            sell_window_stats: Some(&stats),
            revenue_listings: Some(&sell),
            revenue_stats: Some(&stats),
            raw_sales: &HashMap::new(),
            formula,
            levels: &CrafterLevels::default(),
            job_filter: None,
            use_subcrafts: false,
            require_hq: false,
            filter_outliers: false,
            shards: ShardsMode::ExcludeShards,
            on_hand: None,
            needs: &needed_signals(&formula, &SignalWants::default(), false),
            home_world_id: 1,
            dc_of: &|_| None,
        });
        rows
    }

    #[test]
    fn sale_stat_revenue_with_no_sale_row_is_unpriced_not_a_listing() {
        let rows = evidence_fixture(PriceSignal::SaleMedian, None);
        assert_eq!(rows.len(), 1, "the row is kept, not dropped");
        let r = &rows[0];
        assert_eq!(r.revenue, Revenue::Unpriced);
        assert_eq!(r.line, None);
        assert_eq!(r.price(), None);
        assert_eq!(r.profit(), None);
        assert_eq!(r.roi(), None);
        assert_eq!(r.tax(), None);
        assert!(!r.fell_back());
        assert_eq!(r.cost, 10, "the cost side still prices from the buy scope");
        assert_eq!(r.revenue_world_id, 0, "no listing world for a price that does not exist");
        assert_eq!(r.vwap_pct, None);
        let row: RecipeRow = Arc::new(r.clone());
        assert_eq!(
            cell_price(&row, &test_ctx()),
            CellValue::GilWithNote {
                amount: None,
                note: CellNote::NoSales
            }
        );
        assert_eq!(cell_roi(&row, &test_ctx()), CellValue::RoiBadge(None));
        assert_eq!(
            cell_tax(&row, &test_ctx()),
            CellValue::GilWithNote {
                amount: None,
                note: CellNote::NoSales
            }
        );
        assert_eq!(profit_query_value(r), GridValue::Missing);
    }

    #[test]
    fn a_sale_row_in_the_window_prices_the_row_from_the_statistic() {
        let rows = evidence_fixture(PriceSignal::SaleMedian, Some((1_699_000_000, 3)));
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(
            r.revenue,
            Revenue::Priced {
                price: 50_000,
                fell_back: false
            }
        );
        assert_eq!(r.line.map(|l| l.revenue), Some(50_000));
        assert_eq!(profit_query_value(r), GridValue::Number(f64::from(r.profit().unwrap())));
    }

    #[test]
    fn listing_revenue_keeps_the_buy_scope_fallback_and_its_tell() {
        let rows = evidence_fixture(PriceSignal::ListingMin, None);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(
            r.revenue,
            Revenue::Priced {
                price: 24_999_999,
                fell_back: true
            }
        );
        assert_eq!(r.line.map(|l| l.revenue), Some(24_999_999));
        assert_eq!(r.revenue_world_id, 2);
    }
```

`test_ctx`, `cell_price`, `cell_roi`, `cell_tax`, `CheapestListingItem`, `CheapestListings`, `ItemSaleStats`, `ProfitFormula`, `SignalWants`, `needed_signals`, `ShardsMode`, `CrafterLevels` are all already in scope in that module (used by `quality_fixture_at`). `GridValue` is imported at the top of the file.

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::sale_stat_revenue 2>&1 | tail -30`
Expected: compile errors naming `Revenue`, `line`, `price()`, `profit_query_value`, `CellNote::NoSales`.

- [ ] **Step 3: Add the i18n keys**

Add to every locale file, directly after the existing `analyzer_price_listing_fallback` line (`en.json` line ~702; search for the key in the others):

| locale | `analyzer_price_no_sales` | `analyzer_price_no_sales_title` |
|---|---|---|
| en | `no sales in window` | `No sales in the selected window on the sell market, so there is no sale price to profit against` |
| fr | `aucune vente sur la période` | `Aucune vente sur la période choisie sur le marché de vente : il n'y a donc aucun prix de vente pour calculer un profit` |
| de | `keine Verkäufe im Zeitraum` | `Keine Verkäufe im gewählten Zeitraum auf dem Verkaufsmarkt, also gibt es keinen Verkaufspreis für einen Gewinn` |
| ja | `期間内の販売なし` | `選択した期間内に販売市場での販売がないため、利益を計算する販売価格がありません` |
| cn | `窗口内无销售` | `所选时间窗口内该出售市场没有成交记录，因此没有可用于计算利润的成交价` |
| ko | `기간 내 판매 없음` | `선택한 기간 동안 판매 시장에 판매 기록이 없어 수익을 계산할 판매 가격이 없습니다` |
| tc | `窗口內無銷售` | `所選時間窗口內該出售市場沒有成交記錄，因此沒有可用於計算利潤的成交價` |

JSON: each line is `    "key": "value",` — keep the trailing comma pattern consistent with the neighbours.

- [ ] **Step 4: Change the cell shapes in `cells.rs` and `grid.rs`**

In `cells.rs`:

```rust
    RoiBadge(Option<i32>),
```
(was `RoiBadge(i32)`), and

```rust
    /// A gil amount with an always-present note sub-line (the Price slot's
    /// "listing" fallback tell). `None` renders the dash: a sale-statistic
    /// revenue with no sale row (`CellNote::NoSales`).
    GilWithNote {
        amount: Option<i32>,
        note: CellNote,
    },
```

Add to `CellNote`, after `ListingFallback`:

```rust
    /// The selected sale statistic has no sale row for the window on the
    /// sell place, and no listing stands in for it. The cell is a dash and
    /// this is its reason.
    NoSales,
```

In `render_cell`, replace the `RoiBadge` arm:

```rust
        CellValue::RoiBadge(roi) => view! {
            <div  class=class>
                {match roi {
                    Some(roi) => view! {
                        <span class=roi_badge_class(roi)>{format!("{roi}%")}</span>
                    }
                    .into_any(),
                    None => view! {
                        <span class="text-[color:var(--color-text-muted)]">"—"</span>
                    }
                    .into_any(),
                }}
            </div>
        }
        .into_any(),
```

In the `GilWithNote` arm add a `NoSales` match arm beside `ListingFallback`:

```rust
                CellNote::NoSales => (
                    t_string!(i18n, analyzer_price_no_sales).to_string(),
                    SUB_LINE.to_string(),
                ),
```

and change the arm's view from `<Gil amount=amount />` to `<GilOrDash amount=amount />` (`GilOrDash` lives in `ultros_ui::components::gil`, the same module `Gil` is imported from in that file; add it to that `use`). Give the dash cell its reason as a title: wrap the outer div as

```rust
            let title = matches!(note, CellNote::NoSales)
                .then(|| t_string!(i18n, analyzer_price_no_sales_title).to_string());
            view! {
                <div  class=class title=title>
                    <GilOrDash amount=amount />
                    <div class=note_class>{text}</div>
                </div>
            }
            .into_any()
```

In `grid.rs` `query_cell`, replace the first arm:

```rust
        CellValue::Gil(n)
        | CellValue::RoiBadge(Some(n))
        | CellValue::GilWithNote {
            amount: Some(n), ..
        } => GridValue::Number(n as f64),
        CellValue::RoiBadge(None) | CellValue::GilWithNote { amount: None, .. } => {
            GridValue::Missing
        }
```

In `grid.rs` `measure_cell`, replace the first two arms:

```rust
        CellValue::Gil(n) | CellValue::GilWithPct { amount: n, .. } => {
            (n.separate_with_commas(), 42.0)
        }
        CellValue::GilWithNote { amount, .. } => (
            amount.map(|n| n.separate_with_commas()).unwrap_or_default(),
            42.0,
        ),
        CellValue::RoiBadge(n) => (n.map(|n| format!("{n}%")).unwrap_or_default(), 30.0),
```

Fix any other `RoiBadge(` / `GilWithNote {` constructors the compiler reports in `cells.rs` tests (wrap the amount in `Some(..)`).

- [ ] **Step 5: Change the row model in `recipe_analyzer.rs`**

Add `ProfitLine` to the `crate::formula` import at the top of the file (the `use` that brings in `profit_line` and `ProfitFormula`, ~line 13–15).

Replace, in `struct RecipeProfitData`, the five fields `profit`, `return_on_investment`, `market_price`, `tax`, `revenue_fell_back` (keep `cost`) with:

```rust
    /// The selected revenue signal at the sell place, or nothing.
    revenue: Revenue,
    /// The ledger line under the selected formula; `None` exactly when
    /// `revenue` is `Unpriced`.
    line: Option<ProfitLine>,
```

Delete the old doc comment on `revenue_fell_back` and the `/// The market board's cut ...` comment on `tax`. Add, directly above the struct:

```rust
/// Where the row's revenue came from. `Unpriced` is a value the cells
/// render as "—", not a zero sentinel: a sale-statistic revenue with no
/// sale row for the window on the sell place. No listing stands in for
/// it — the 2026-09-18 Gilgamesh row was a 24,999,999 listing on another
/// world wearing a "sale median" heading.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Revenue {
    Priced {
        price: i32,
        /// `price` is not the selected signal on the sell world: the
        /// listing fell back to the buy scope.
        fell_back: bool,
    },
    Unpriced,
}

impl RecipeProfitData {
    fn price(&self) -> Option<i32> {
        match self.revenue {
            Revenue::Priced { price, .. } => Some(price),
            Revenue::Unpriced => None,
        }
    }
    fn fell_back(&self) -> bool {
        matches!(self.revenue, Revenue::Priced { fell_back: true, .. })
    }
    fn profit(&self) -> Option<i32> {
        self.line.map(|l| l.profit)
    }
    fn roi(&self) -> Option<i32> {
        self.line.map(|l| l.roi)
    }
    fn tax(&self) -> Option<i32> {
        self.line.map(|l| l.tax)
    }
}

/// The Profit column's query value: the number, or `Missing` for an
/// unpriced row so a threshold hides it rather than ranking it at zero.
fn profit_query_value(r: &RecipeProfitData) -> GridValue {
    r.profit()
        .map(|p| GridValue::Number(f64::from(p)))
        .unwrap_or(GridValue::Missing)
}
```

- [ ] **Step 6: Keep unpriced rows in `price_rows`**

Replace

```rust
        let market_price = revenue_summary.lowest_gil().unwrap_or(0);

        if market_price == 0 {
            continue;
        }
```

with

```rust
        let market_price = revenue_summary.lowest_gil().unwrap_or(0);
        // A sale-statistic revenue with no sale row for the window on the
        // sell place has NO revenue. The listing layers under the stat in
        // `revenue_view` are the cost side's fallback, not this side's:
        // absence of sales is the signal, and no listing (least of all
        // another world's) stands in for it.
        let revenue_signal = inp.formula.revenue_signal();
        let unpriced = revenue_signal.sale_stat().is_some()
            && rev_signal_at(
                inp.revenue_listings,
                inp.revenue_stats,
                recipe.item_result,
                revenue_signal,
            )
            .is_none();

        if market_price == 0 && !unpriced {
            continue;
        }
```

Replace

```rust
        let (line, dropped) = profit_line(market_price, cost_per_unit, &inp.formula);
        if dropped {
            continue;
        }
```

with

```rust
        let (line, dropped) = profit_line(market_price, cost_per_unit, &inp.formula);
        // An unpriced row has no net for the drop rule to compare against.
        if dropped && !unpriced {
            continue;
        }
```

Replace

```rust
        let revenue_fell_back = rev_alt[inp.formula.revenue_signal().index()] != Some(market_price);
```

with

```rust
        let revenue = if unpriced {
            Revenue::Unpriced
        } else {
            Revenue::Priced {
                price: market_price,
                fell_back: rev_alt[revenue_signal.index()] != Some(market_price),
            }
        };
```

In the `results.push(RecipeProfitData { .. })` literal: replace `profit: line.profit,`, `return_on_investment: line.roi,`, `market_price: line.revenue,`, `tax: line.tax,` and `revenue_fell_back,` with

```rust
            revenue,
            line: (!unpriced).then_some(line),
```

change `vwap_pct: sell_scope_is_world.then(|| vwap_pct(market_price, vwap)).flatten(),` to

```rust
            vwap_pct: (sell_scope_is_world && !unpriced)
                .then(|| vwap_pct(market_price, vwap))
                .flatten(),
```

and change `revenue_world_id,` to `revenue_world_id: if unpriced { 0 } else { revenue_world_id },`.

- [ ] **Step 7: Update every reader of the removed fields**

Cells (~1089–1177, ~1305):

```rust
fn cell_roi(r: &RecipeRow, _: &CellCtx) -> CellValue {
    CellValue::RoiBadge(r.roi())
}
/// Compare the expected price with the same-quality sell-world median.
fn cell_price(r: &RecipeRow, _: &CellCtx) -> CellValue {
    match r.price() {
        Some(price) => CellValue::GilWithNote {
            amount: Some(price),
            note: price_note(price, r.sell_median, r.fell_back()),
        },
        None => CellValue::GilWithNote {
            amount: None,
            note: CellNote::NoSales,
        },
    }
}
```

```rust
fn cell_tax(r: &RecipeRow, _: &CellCtx) -> CellValue {
    match r.tax() {
        Some(tax) => CellValue::Gil(tax),
        None => CellValue::GilWithNote {
            amount: None,
            note: CellNote::NoSales,
        },
    }
}
```

```rust
fn cell_profit_per_day(r: &RecipeRow, _: &CellCtx) -> CellValue {
    match r.profit() {
        Some(profit) => CellValue::Gil(profit_per_day_from_rate(profit, r.daily_sales)),
        None => CellValue::GilWithNote {
            amount: None,
            note: CellNote::NoSales,
        },
    }
}
```

Sort (`compare_recipes`): replace the `Roi`, `Profit`, `Price`, `Tax`, `ProfitPerDay` arms with

```rust
        SortMode::Roi => cmp_none_last(a.roi(), b.roi(), dir, i32::cmp),
        SortMode::Profit => cmp_none_last(a.profit(), b.profit(), dir, i32::cmp),
```
```rust
        SortMode::Price => cmp_none_last(a.price(), b.price(), dir, i32::cmp),
```
```rust
        SortMode::Tax => cmp_none_last(a.tax(), b.tax(), dir, i32::cmp),
```
```rust
        SortMode::ProfitPerDay => cmp_none_last(
            a.profit().map(|p| profit_per_day_from_rate(p, a.daily_sales)),
            b.profit().map(|p| profit_per_day_from_rate(p, b.daily_sales)),
            dir,
            i32::cmp,
        ),
```

Custom Profit cell (~3704), replace the whole `ColumnKind::Profit => { .. }` arm:

```rust
            ColumnKind::Profit => {
                let readout = {
                    let data = data.clone();
                    move || {
                        Some(match data.line {
                            Some(line) => t_string!(
                                i18n,
                                recipe_analyzer_profit_readout,
                                price = line.revenue.separate_with_commas(),
                                tax = line.tax.separate_with_commas(),
                                cost = line.cost.separate_with_commas(),
                                profit = line.profit.separate_with_commas()
                            )
                            .to_string(),
                            None => t_string!(i18n, analyzer_price_no_sales_title).to_string(),
                        })
                    }
                };
                let profit = data.profit();
                view! {
                    <div  class=class title=readout>
                        <GilOrDash amount=profit />
                    </div>
                }
                .into_any()
            }
```

Query value closure (~4021): `ColumnKind::Profit => profit_query_value(data),`.
Measure closure (~4070): `ColumnKind::Profit => (data.profit().map(|p| p.separate_with_commas()).unwrap_or_default(), 42.0),`.

Test helper `row` (~7040): replace `profit,`, `return_on_investment: roi,`, `market_price: 2,`, `tax: 0,`, `revenue_fell_back: false,` with

```rust
            revenue: Revenue::Priced {
                price: 2,
                fell_back: false,
            },
            line: Some(ProfitLine {
                revenue: 2,
                tax: 0,
                cost: 1,
                profit,
                roi,
            }),
```

Test helper `price_row` (~7473):

```rust
    fn price_row(key: i32, price: i32, median: Option<i32>, fell_back: bool) -> RecipeRow {
        let mut r = Arc::try_unwrap(row(key, 0, 0, 1.0, 1)).ok().unwrap();
        r.revenue = Revenue::Priced { price, fell_back };
        r.line = r.line.map(|l| ProfitLine { revenue: price, ..l });
        r.sell_median = median;
        Arc::new(r)
    }
```

Remaining test-module reads of the old fields are mechanical. Find the test module's first line (`grep -n '^#\[cfg(test)\]' ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs`, call it `T`) and run, from the worktree root in Git Bash, with `T` substituted:

```bash
sed -i -E "T,\$ s/\.market_price\b/.price().unwrap()/g; T,\$ s/\.return_on_investment\b/.roi().unwrap()/g; T,\$ s/\.revenue_fell_back\b/.fell_back()/g; T,\$ s/\.profit\b([^(_])/.profit().unwrap()\1/g; T,\$ s/\.tax\b([^(_])/.tax().unwrap()\1/g" ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
```

Then compile and fix what is left by hand (typically `price_note(r.market_price, ..)` call sites in tests become `price_note(r.price().unwrap(), r.sell_median, r.fell_back())`, and any test that compared `revenue_fell_back` on a fixture row). Every existing fixture row is priced, so `.unwrap()` is correct there.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer 2>&1 | tail -40`
Expected: all recipe analyzer tests pass, including the three new ones and `price_rows_matches_recorded_oracle_on_fixture` (its oracle values are unchanged: every fixture row is priced).

Also run: `cargo test -p ultros-app --lib -- analyzer_kit 2>&1 | tail -20` (cells/grid tests) — PASS.

- [ ] **Step 9: CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
```
Expected: `REAL_EXIT=0`. Then:

```bash
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-app/src/analyzer_kit/cells.rs ultros-frontend/ultros-app/src/analyzer_kit/grid.rs ultros-frontend/ultros-i18n/locales/
git commit -m "fix(recipe-analyzer): a sale-stat revenue with no sale row is unpriced, not another world's listing

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Unpriced rows sort last under every mode and direction

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (`sort_recipes` ~2801–2823; tests near `sort_recipes_is_pure_with_a_key_tiebreak` ~7082)

**Interfaces:**
- Consumes `RecipeProfitData::line` and `Revenue::Unpriced` from Task 1.
- Produces test helper `unpriced_row(key: i32, daily: f32) -> Arc<RecipeProfitData>`.

- [ ] **Step 1: Write the failing test**

After `sort_recipes_is_pure_with_a_key_tiebreak`:

```rust
    fn unpriced_row(key: i32, daily: f32) -> Arc<RecipeProfitData> {
        let mut r = Arc::try_unwrap(row(key, 0, 0, daily, 1)).ok().unwrap();
        r.revenue = Revenue::Unpriced;
        r.line = None;
        Arc::new(r)
    }

    /// An unpriced row has nothing to rank: it trails every priced row
    /// whatever the header points at, in both directions — even under
    /// Velocity, where its own daily rate would otherwise put it first.
    #[test]
    fn unpriced_rows_sort_last_under_every_mode_and_direction() {
        let keys: Vec<i32> = fixture_recipes()
            .iter()
            .take(3)
            .map(|r| r.key_id.0)
            .collect();
        let rows = vec![
            unpriced_row(keys[0], 9.0),
            row(keys[1], 100, 10, 1.0, 7),
            row(keys[2], 300, 30, 0.5, 8),
        ];
        let modes = [
            SortMode::Profit,
            SortMode::Roi,
            SortMode::Price,
            SortMode::Tax,
            SortMode::Velocity,
            SortMode::CostPerUnit,
            SortMode::ProfitPerDay,
            SortMode::LastSold,
        ];
        for (i, mode) in modes.into_iter().enumerate() {
            for dir in [SortDir::Asc, SortDir::Desc] {
                let out = sort_recipes(&rows, mode, dir, None);
                assert_eq!(
                    out.last().unwrap().1.recipe.key_id.0,
                    keys[0],
                    "mode #{i} {dir:?}: the unpriced row must be last"
                );
                assert!(out[..2].iter().all(|(_, r)| r.line.is_some()));
            }
        }
        // Priced rows still order among themselves.
        let out = sort_recipes(&rows, SortMode::Profit, SortDir::Desc, None);
        assert_eq!(out[0].1.profit(), Some(300));
        assert_eq!(out[1].1.profit(), Some(100));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::unpriced_rows_sort_last 2>&1 | tail -20`
Expected: FAIL on `Velocity` / `LastSold` / `CostPerUnit` (the unpriced row's 9.0 daily rate ranks it first under Velocity Desc).

- [ ] **Step 3: Partition in `sort_recipes`**

Replace the `kept.sort_by(..)` call with:

```rust
    kept.sort_by(|a, b| {
        // Unpriced rows (a sale-statistic revenue with no sale row) trail
        // every priced row whatever the mode and direction: there is no
        // figure to rank, and a fast-moving ingredient market is not one.
        a.line
            .is_none()
            .cmp(&b.line.is_none())
            .then_with(|| compare_recipes(mode, dir, a, b, stats_30))
            // Deterministic tiebreak: the input comes from a std HashMap, so
            // without it ties could order differently on the server and the
            // client and mismatch the SSR-rendered rows.
            .then_with(|| a.recipe.key_id.0.cmp(&b.recipe.key_id.0))
    });
```

(`false < true`, so priced rows come first.) Remove the old tiebreak comment that now sits above the closure body if it is duplicated.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "fix(recipe-analyzer): unpriced rows sort after every priced row in both directions

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: "Sold in window" toggle (`?sold=true`)

**Files:**
- Modify: `ultros-frontend/ultros-calc/src/needed.rs` (`RecipeNeeds` ~74–135, `needed_bodies` ~181, tests ~340 and ~468)
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (consts ~380–402, `recipe_filter_controls` ~459–525, `RecipeProfitData`, `PriceInputs` ~2333–2375, `price_rows` after the job filter ~2495, table signals ~3162, `PriceInputs` literal ~3515, page signals ~4248, `RecipeNeeds` literals ~4547 and ~4618, `column_filters` ~4056, registry test ~5054–5140)
- Modify: all seven locale files
- Test: `recipe_analyzer.rs` test module, `needed.rs` test module

**Interfaces:**
- Produces `RecipeNeeds::evidence: bool` (default `false`), OR-ed into `wants_sell_stats` in `needed_bodies`.
- Produces `const FILTER_SOLD: &str = "sold"`.
- Produces on `RecipeProfitData`: `sold_in_window: bool`, `window_last_sold_unix: Option<i64>` (Task 4 reads the second).
- Produces on `PriceInputs`: `sold_only: bool`.
- Extends fixture to `evidence_fixture(revenue, output_history, sold_only: bool)`.
- Produces i18n key `recipe_analyzer_filter_sold_label`.

**Body choice.** Both evidence filters read, per row, `inp.revenue_stats.or(inp.sell_window_stats).unwrap_or(inp.sell_stats)`: the sell-place body at the page window (what a sale revenue signal reads); the sell world's own window body when a wider scope's body was not fetched; the seven-day context body as the last resort. Setting either filter makes the page request the window body via `RecipeNeeds::evidence`, so in practice the first arm is the one that fires.

- [ ] **Step 1: Failing test in `needed.rs`**

In the `needed.rs` test module, extend `the_default_window_is_seven_days` with `assert!(!RecipeNeeds::default().evidence);` and add, after the test that ends with `needed_bodies(&listing, &needs(false, false))` (the `rev_gil` test, ~468–492):

```rust
    /// The evidence row filters (Sold in window, Last sold within) read
    /// the sell-place body at the page window, exactly as revenue-side
    /// gil does, so they request it the same way.
    #[test]
    fn evidence_filters_want_the_sell_place_body_at_the_window() {
        let listing = ProfitFormula::recipe_from_query(None, None, None);
        let ev = RecipeNeeds {
            evidence: true,
            ..at(30, needs(false, false))
        };
        let got = needed_bodies(&listing, &ev);
        assert!(got.contains(&BodyRole::SellWorldStats(30)));
        let wider = listing.with_sell_scope(SellScope(Scope::Datacenter));
        assert!(needed_bodies(&wider, &ev).contains(&BodyRole::SellScopeStats(30)));
        // At the default window and sell scope the context body already
        // covers it: nothing is added.
        let ev7 = RecipeNeeds {
            evidence: true,
            ..needs(false, false)
        };
        assert_eq!(
            needed_bodies(&listing, &ev7),
            needed_bodies(&listing, &needs(false, false))
        );
    }
```

(`ProfitFormula`, `SellScope`, `Scope`, `BodyRole` are already used by the neighbouring tests in that module; if `ProfitFormula::recipe_from_query` is not how the neighbouring test builds `listing`, copy that test's construction verbatim.)

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-calc -- evidence_filters 2>&1 | tail -10`
Expected: compile error, no field `evidence`.

- [ ] **Step 3: Implement in `needed.rs`**

Add to `RecipeNeeds` after `stats_30`:

```rust
    /// A row filter that reads the sell place's sale body at the window
    /// is set (Sold in window, Last sold within): the same body a sale
    /// revenue signal and revenue-side gil read.
    pub evidence: bool,
```

Add `evidence: false,` to `impl Default for RecipeNeeds`. In `needed_bodies` change

```rust
    let wants_sell_stats = formula.revenue_signal().sale_stat().is_some()
        || needs.rev_signals.iter().any(|s| s.sale_stat().is_some())
        || needs.rev_gil;
```
to
```rust
    let wants_sell_stats = formula.revenue_signal().sale_stat().is_some()
        || needs.rev_signals.iter().any(|s| s.sale_stat().is_some())
        || needs.rev_gil
        || needs.evidence;
```

Run: `cargo test -p ultros-calc 2>&1 | tail -10` — PASS.

- [ ] **Step 4: Failing test in `recipe_analyzer.rs`**

Change the fixture signature to `fn evidence_fixture(revenue: PriceSignal, output_history: Option<(i64, i64)>, sold_only: bool)` and add `sold_only,` to its `PriceInputs` literal. Update the three Task 1 callers to pass `false`. Add:

```rust
    #[test]
    fn sold_in_window_hides_rows_with_no_sale_on_the_sell_place() {
        // No output history: hidden under the toggle, kept (unpriced) without it.
        assert!(evidence_fixture(PriceSignal::SaleMedian, None, true).is_empty());
        let kept = evidence_fixture(PriceSignal::SaleMedian, None, false);
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].sold_in_window);
        assert_eq!(kept[0].window_last_sold_unix, None);
        // The toggle reads the sale body, not the price: a listing revenue
        // is hidden the same way.
        assert!(evidence_fixture(PriceSignal::ListingMin, None, true).is_empty());
        // A sale in the window passes and records when.
        let rows = evidence_fixture(PriceSignal::ListingMin, Some((1_699_000_000, 3)), true);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].sold_in_window);
        assert_eq!(rows[0].window_last_sold_unix, Some(1_699_000_000));
    }
```

Extend `filter_registry_keys_are_a_stable_url_contract`: in the `keys` assertion append `"sold"` after `"on-hand"`; in the `for key in [FILTER_SUBCRAFTS, ..]` toggle loop add `FILTER_SOLD`; in the final `assert_eq!([FILTER_PROFIT, ..], ["profit", ..])` pair append `FILTER_SOLD` / `"sold"`.

- [ ] **Step 5: Run to verify failure**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::sold_in_window 2>&1 | tail -10`
Expected: compile errors (`sold_only`, `sold_in_window`, `FILTER_SOLD`).

- [ ] **Step 6: Implement in `recipe_analyzer.rs`**

Constants (after `FILTER_USE_ON_HAND`):

```rust
/// Row filters over the sell place's sale evidence at the page window.
/// Applied by the pricing pass like the job filter, not by the grid: the
/// Last sold column is pinned to the seven-day context body, so a grid
/// metric over it would read "never" for everything at a wider window.
const FILTER_SOLD: &str = "sold";
```

`RecipeProfitData`, after `has_sell_stats`:

```rust
    /// The output sold at least once in the page window on the sell place
    /// (the body a sale revenue signal reads). Read by the Sold in window
    /// filter and reported so the pass and the bar agree.
    sold_in_window: bool,
    /// The newest sale of either quality in that same body; `None` = no
    /// sale in the window.
    window_last_sold_unix: Option<i64>,
```

`PriceInputs`, after `filter_outliers: bool,`:

```rust
    /// Sold in window: drop rows with no sale on the sell place in the
    /// page window.
    sold_only: bool,
```

`price_rows`, directly after the `job_filter` `continue` block:

```rust
        // The sale evidence both row filters read: the sell place's body
        // at the page window, the sell world's own window body when a
        // wider scope's was not fetched, the context body as a last
        // resort. Both qualities count — the filters ask "does this sell",
        // not "does this quality sell".
        let evidence: &StatsIndex = inp
            .revenue_stats
            .or(inp.sell_window_stats)
            .unwrap_or(inp.sell_stats);
        let evidence_rows = [
            evidence.get(&(recipe.item_result, false)),
            evidence.get(&(recipe.item_result, true)),
        ];
        let sold_in_window = evidence_rows.iter().flatten().any(|s| s.num_sold > 0);
        let window_last_sold_unix = evidence_rows
            .iter()
            .flatten()
            .map(|s| s.last_sold_unix)
            .filter(|t| *t > 0)
            .max();
        if inp.sold_only && !sold_in_window {
            continue;
        }
```

In the `results.push(RecipeProfitData { .. })` literal add `sold_in_window,` and `window_last_sold_unix,` after `has_sell_stats: sell_stat.is_some(),`.

Controls: in `recipe_filter_controls`, after the `FILTER_USE_ON_HAND` toggle:

```rust
        toggle_control(
            FILTER_SOLD,
            t_string!(i18n, recipe_analyzer_filter_sold_label).to_string(),
        ),
```

Table (~3162), after the `FILTER_USE_ON_HAND` signal: `let (sold_only, _) = filter_query_signal::<bool>(FILTER_SOLD);` and in the `PriceInputs` literal (~3515) after `filter_outliers: ..,`: `sold_only: sold_only().unwrap_or(false),`.

Page (`RecipeAnalyzer`, after the `filter_outliers` signal ~4252):

```rust
    // The evidence row filters read the sell place's window body; either
    // being set is what makes the fetch gate request it.
    let (sold_page, _) = filter_query_signal::<bool>(FILTER_SOLD);
    let evidence_wanted = Memo::new(move |_| sold_page.get().unwrap_or(false));
```

(Task 4 extends this memo.) In the two `RecipeNeeds { .. }` literals that set `rev_gil` (the `sell_window_source` memo ~4547 and the sell-scope memo ~4618) add `evidence: evidence_wanted.get(),`.

Header menu (~4056): change `ColumnKind::RevenueSlot => &[],` to `ColumnKind::RevenueSlot => &[FILTER_SOLD],`.

Test helpers: add `sold_in_window: false, window_last_sold_unix: None,` to the `row()` literal (~7040) and `sold_only: false,` to every other `PriceInputs` literal in the test module (`quality_fixture_at` and any `run`/oracle helper; the compiler lists them).

i18n `recipe_analyzer_filter_sold_label`, placed after `recipe_analyzer_filter_use_on_hand_label` in each file:

| locale | value |
|---|---|
| en | `Sold in window` |
| fr | `Vendu sur la période` |
| de | `Im Zeitraum verkauft` |
| ja | `期間内に販売あり` |
| cn | `窗口内有销售` |
| ko | `기간 내 판매됨` |
| tc | `窗口內有銷售` |

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer 2>&1 | tail -20` and `cargo test -p ultros-calc 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 8: CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-calc/src/needed.rs ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs ultros-frontend/ultros-i18n/locales/
git commit -m "feat(recipe-analyzer): Sold in window row filter

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: "Last sold within" filter (`?last-sold=90d`)

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs` (consts, `recipe_filter_controls`, `PriceInputs`, `price_rows`, table/page wiring, `column_filters`, registry test)
- Test: `recipe_analyzer.rs` test module

**Interfaces:**
- Consumes `window_last_sold_unix` and `evidence_wanted` from Task 3.
- Produces `const FILTER_LAST_SOLD: &str = "last-sold"`; on `PriceInputs`: `last_sold_within_secs: Option<u64>`, `now_unix: i64`; `fn hour_now_unix() -> i64`.
- Extends fixture to `evidence_fixture(revenue, output_history, sold_only, last_sold_within_secs: Option<u64>)` with `now_unix: 1_700_000_000`.
- Reuses existing i18n key `analyzer_last_sold_within` ("Last Sold Within", present in all locales) for the label. No new key.

- [ ] **Step 1: Failing test**

Change the fixture signature to add `last_sold_within_secs: Option<u64>` and set `last_sold_within_secs, now_unix: 1_700_000_000,` in its `PriceInputs` literal; update the Task 1 and Task 3 callers to pass `None`. Add:

```rust
    /// The recency filter reads the window body (Task 3's evidence), not
    /// the seven-day context, so at a 90-day window a sale 30 days ago
    /// passes and one 176 days ago does not.
    #[test]
    fn last_sold_within_is_inclusive_and_reads_the_window_body() {
        let now = 1_700_000_000_i64;
        let day = 86_400_i64;
        let within_90d = Some(90 * 86_400_u64);
        let at = |days_ago: i64| Some((now - days_ago * day, 2_i64));
        assert_eq!(
            evidence_fixture(PriceSignal::SaleMedian, at(30), false, within_90d).len(),
            1
        );
        // Exactly on the bound: kept (inclusive, the repo convention).
        assert_eq!(
            evidence_fixture(PriceSignal::SaleMedian, at(90), false, within_90d).len(),
            1
        );
        assert!(evidence_fixture(PriceSignal::SaleMedian, at(176), false, within_90d).is_empty());
        assert!(evidence_fixture(PriceSignal::SaleMedian, None, false, within_90d).is_empty());
        // No filter: everything kept.
        assert_eq!(evidence_fixture(PriceSignal::SaleMedian, None, false, None).len(), 1);
        // The URL value is a humantime duration, like the flip finder's.
        assert_eq!(
            humantime::parse_duration("90d").map(|d| d.as_secs()).ok(),
            within_90d
        );
    }
```

Extend `filter_registry_keys_are_a_stable_url_contract`: append `"last-sold"` to the `keys` list after `"sold"`, and append `FILTER_LAST_SOLD` / `"last-sold"` to the final const/string pair. Also assert the control is a free-text row filter:

```rust
            let within = controls
                .iter()
                .find(|c| c.key == FILTER_LAST_SOLD)
                .expect("last-sold");
            assert!(!within.numeric && within.options.is_empty(), "free text like the flip finder's");
            assert!(within.clear_with_filters);
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer::test::last_sold_within 2>&1 | tail -10`
Expected: compile errors.

- [ ] **Step 3: Implement**

Constant, after `FILTER_SOLD`:

```rust
/// Last sold within: a humantime duration (`90d`, `1d 12h`), the same
/// grammar the flip finder's `last-sold` key uses. Inclusive.
const FILTER_LAST_SOLD: &str = "last-sold";
```

`PriceInputs`, after `sold_only: bool,`:

```rust
    /// Last sold within: drop rows whose newest sale in the window body is
    /// older than this many seconds; `None` = no filter.
    last_sold_within_secs: Option<u64>,
    /// The clock the recency filter reads. See [`hour_now_unix`].
    now_unix: i64,
```

`price_rows`, directly after the `sold_only` `continue`:

```rust
        if let Some(within) = inp.last_sold_within_secs
            && !window_last_sold_unix
                .is_some_and(|t| inp.now_unix.saturating_sub(t) <= within as i64)
        {
            continue;
        }
```

Above `price_rows`:

```rust
/// The clock the recency filter reads, floored to the hour. The row set is
/// computed on the server and again on the client, and a different set
/// on the two sides is a hydration mismatch; flooring makes them agree
/// except across an hour boundary that falls inside the page load.
fn hour_now_unix() -> i64 {
    let now = chrono::Utc::now().timestamp();
    now - now.rem_euclid(3_600)
}

/// `?last-sold=` as seconds, `None` for absent, blank or unparsable.
fn last_sold_within_secs(raw: Option<&str>) -> Option<u64> {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .and_then(|v| humantime::parse_duration(v).ok())
        .map(|d| d.as_secs())
}
```

Controls, after the `FILTER_SOLD` toggle:

```rust
        ColumnFilter::new(
            FILTER_LAST_SOLD,
            t_string!(i18n, analyzer_last_sold_within).to_string(),
            false,
        ),
```

Table: after the `sold_only` signal, `let (last_sold_within, _) = filter_query_signal::<String>(FILTER_LAST_SOLD);` and in the `PriceInputs` literal after `sold_only: ..,`:

```rust
                last_sold_within_secs: last_sold_within_secs(last_sold_within().as_deref()),
                now_unix: hour_now_unix(),
```

Page: after `sold_page`, `let (last_sold_page, _) = filter_query_signal::<String>(FILTER_LAST_SOLD);` and change the memo to

```rust
    let evidence_wanted = Memo::new(move |_| {
        sold_page.get().unwrap_or(false)
            || last_sold_within_secs(last_sold_page.get().as_deref()).is_some()
    });
```

Header menu: add an arm `ColumnKind::LastSold => &[FILTER_LAST_SOLD],` next to `ColumnKind::RevenueSlot`.

Test helpers: add `last_sold_within_secs: None, now_unix: 1_700_000_000,` to every other `PriceInputs` literal in the test module.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib -- recipe_analyzer 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: CI check and commit**

```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
git add ultros-frontend/ultros-app/src/routes/recipe_analyzer.rs
git commit -m "feat(recipe-analyzer): Last sold within row filter over the window body

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Verify in the browser against prod data, then open the PR

**Files:** none modified (unless verification finds a bug, which goes back to the owning task).

- [ ] **Step 1: Build and serve the branch locally**

From Git Bash at the worktree root, with the Windows build env from Global Constraints:

```bash
cargo leptos build 2>&1 | tail -5
```
Then start the server on a free port with the main checkout's `.env` values for Postgres and ClickHouse (see `AGENTS.md`; the notes in memory say `PORT=8093 HOSTNAME=… METRICS_PORT=…` works beside another server):

```bash
PORT=8093 METRICS_PORT=9093 ./target/debug/ultros.exe
```

- [ ] **Step 2: Reproduce the original URL**

Open `http://127.0.0.1:8093/recipe-analyzer/Gilgamesh?revenue=sale-median&window=90&v=1` in the browser pane. Expected: Cauldronmaster's Jackboots is no longer at the top; it appears at the bottom of the list with "—" in Price, Profit and ROI and the "no sales in window" sub-line. Take a screenshot of the top rows and of the row itself (search the Item filter for "Cauldronmaster").

- [ ] **Step 3: Exercise the two filters**

- Add `&sold=true`: the Jackboots row disappears; the row count in the bar drops; "Clear all" removes it.
- Replace with `&last-sold=90d`: same disappearance; `&last-sold=1000d` brings it back (it sold 2026-03-26 on Gilgamesh, which is within 1000 days).
- Switch `revenue=listing-min` with `sold=true`: rows without a 90-day sale are hidden even though they have listings.

If the local ClickHouse has no `sales` data (memory: local `sales` won't attach), run the same three URLs against `https://ultros.app` only as a **negative control** for the old behaviour and rely on the unit tests for the new one; say so in the PR.

- [ ] **Step 4: Open the PR**

```bash
git push -u origin claude/gilgamesh-data-accuracy-2b6b03
gh pr create --title "fix(recipe-analyzer): no listing fallback for sale-stat revenue; Sold in window + Last sold within filters" --body-file "$SCRATCH/pr.md"
```

Write `$SCRATCH/pr.md` first with: the prod symptom (item 13047 on Gilgamesh, 24,999,999 Faerie listing under "sale median 90d"), the root cause (`SignalView` listing layers under a sale stat), the three behaviour changes, the deviation from the spec (native row filters over the window body instead of a Last sold column alias, and why), the screenshots from Steps 2–3, the test commands run, and the two follow-ups the spec lists as out of scope (listing-revenue buy-scope fallback; seeding/last-view). End the body with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

Do not merge; report the PR URL.

---

## Self-review

- **Spec coverage.** Decision 1 (no fallback, "—", kept, exempt from drop rule, sort last, thresholds hide, alt columns unchanged, cost side unchanged, listing revenue unchanged): Task 1 + Task 2. Decision 2 (`sold` toggle, `last-sold` duration, both clear with Clear all, not seeded, URL contract): Tasks 3 + 4, with the documented mechanism change. Decision 3 (`Revenue` enum, `Option<ProfitLine>`, `last_sold` source): Task 1 + Task 3 (`window_last_sold_unix` replaces the spec's `last_sold_unix: Option<i64>` because the existing `last_sold_unix: i64` field is the seven-day column's and stays). Testing section: fixture (Task 1), sort (Task 2), drop rule (Task 1 asserts the row is kept with positive cost), filters (Tasks 3, 4), thresholds (`profit_query_value` → `Missing`, Task 1), i18n (Tasks 1, 3; Task 4 reuses an existing key).
- **Placeholders.** None; every code step is complete.
- **Type consistency.** `evidence_fixture` grows one parameter per task in the order (revenue, output_history, sold_only, last_sold_within_secs) and each task updates earlier callers. `Revenue`, `line`, `price()/profit()/roi()/tax()/fell_back()`, `profit_query_value`, `sold_in_window`, `window_last_sold_unix`, `RecipeNeeds::evidence`, `FILTER_SOLD`, `FILTER_LAST_SOLD`, `hour_now_unix`, `last_sold_within_secs` are named identically wherever they appear.
