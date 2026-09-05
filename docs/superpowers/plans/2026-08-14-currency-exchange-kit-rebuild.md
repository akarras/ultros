# Currency Exchange Kit Rebuild Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild `/currency-exchange/:id` on the shared flip-finder UI kit (`ToolHeader`, `ControlBar` + `FilterChip`, `?cols=` column model, `SortHeader`, `TableSkeleton`) per `docs/superpowers/specs/2026-08-14-currency-exchange-kit-rebuild-design.md` (issue #1128).

**Architecture:** All changes live in `ultros-frontend/ultros-app/src/routes/currency_exchange.rs` plus the seven locale JSON files and one CSS class in `style/tailwind.css`. The data pipeline (`compute_prices`, `shop_items`, `is_in_range`, the stale cutoff, no-home-world banner) is untouched. The column model and sort enum are module-local copies of the analyzer's conventions (#1080 extraction is out of scope).

**Tech Stack:** Rust / Leptos 0.8 (SSR + hydrate), leptos-i18n, Tailwind.

## Global Constraints

- Run `./check_ci.sh > /tmp/ce_ci.log 2>&1; echo "REAL_EXIT=$?"` before every commit (check the echoed exit, never a pipe's). `cargo fmt --all` to fix formatting.
- Every user-facing string goes through `t!`/`t_string!`; every new key must be added to **all seven** locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) in `ultros-frontend/ultros-app/locales/`, with real translations (leptos-i18n fails to compile on a missing key).
- Filter query keys keep their exact current names (`price_per_item_min`, `price_per_item_max`, `number_received_min`, `number_received_max`, `total_profit_min`, `total_profit_max`, `hours_between_sales_min`, `hours_between_sales_max`) so deep links survive.
- Filter bindings use `crate::query_defaults::filter_query_signal` (replace: true, scroll: false), never plain `query_signal`, except for `?cols=` which uses `query_signal::<String>("cols")` exactly like the analyzer.
- Do not restructure the existing `Suspense` boundary or the `{move || ...}` closure inside it — only swap markup within (SSR suspense-registration is load-bearing; see memory notes).
- Tests in `ultros-app` that create signals must wrap in `leptos::reactive::owner::Owner`; the tests below are pure functions and need no owner.
- CI never runs `cargo test` — run `cargo test -p ultros-app currency_exchange` locally at each test step.
- Each task must leave clippy green (`-D warnings`): no dead code parked between tasks — deletions ride in the same task that obsoletes them.

---

### Task 1: New locale keys in all seven locales

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

**Interfaces:**
- Produces: i18n keys consumed by Tasks 2–3 via `t!`/`t_string!`. Key names below are exact.

- [ ] **Step 1: Add the new keys to `en.json`**

Keep keys grouped with the existing `currency_exchange_*` block. English values:

```json
{
  "currency_exchange_tool_summary": "Find items you can buy with a currency and resell on the market board for gil.",
  "currency_exchange_tool_context": "Prices assume you sell on your home world, using its cheapest current listing or most recent sale.",
  "currency_exchange_tool_help": "Enter how much of the currency you have, then sort or filter the table to find the most profitable exchange. Trades with no sale in the last 60 days are hidden.",
  "currency_exchange_trade_count": "{{ count }} trades",
  "currency_exchange_no_filters_hint": "No filters — showing every trade",
  "currency_exchange_no_matches": "No trades match your filters.",
  "currency_exchange_filter_price_min_label": "Minimum price per item",
  "currency_exchange_filter_price_max_label": "Maximum price per item",
  "currency_exchange_filter_qty_min_label": "Minimum quantity received",
  "currency_exchange_filter_qty_max_label": "Maximum quantity received",
  "currency_exchange_filter_profit_min_label": "Minimum total profit",
  "currency_exchange_filter_profit_max_label": "Maximum total profit",
  "currency_exchange_filter_hours_min_label": "Minimum hours between sales",
  "currency_exchange_filter_hours_max_label": "Maximum hours between sales"
}
```

- [ ] **Step 2: Re-value the eight existing chip keys to comparison shape (en)**

The keys already exist; only the values change:

```json
{
  "currency_exchange_chip_price_min": "Price ≥",
  "currency_exchange_chip_price_max": "Price ≤",
  "currency_exchange_chip_qty_min": "Qty ≥",
  "currency_exchange_chip_qty_max": "Qty ≤",
  "currency_exchange_chip_profit_min": "Profit ≥",
  "currency_exchange_chip_profit_max": "Profit ≤",
  "currency_exchange_chip_hours_min": "Hours/sale ≥",
  "currency_exchange_chip_hours_max": "Hours/sale ≤"
}
```

- [ ] **Step 3: Mirror into the other six locales with real translations**

Translate each value for `fr`, `de`, `ja`, `cn`, `ko`, `tc` (e.g. fr `"currency_exchange_trade_count": "{{ count }} échanges"`, de `"{{ count }} Tauschgeschäfte"`, ja `"{{ count }}件の取引"`). The ≥/≤ comparison chips keep the math symbols in every locale; translate only the word (de `"Gewinn ≥"`, ja `"利益 ≥"`). Follow the tone of each locale's existing `analyzer_filter_*_label` entries.

- [ ] **Step 4: Verify the workspace still compiles (missing-key check runs at compile time)**

Run: `cargo check -p ultros-app`
Expected: success, no missing-key warnings for the new keys.

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/locales
git commit -m "i18n(currency-exchange): keys for the kit rebuild (#1128)"
```

---

### Task 2: Column model + sort enum, wired into the table

Replaces the raw header/`QueryButton`/`FilterModal` table with the kit's column order, `?cols=` gating, `SortHeader` sorting, and `TableSkeleton`. Deletes `column_visibility`, `FilterModal`, the `SortableVec`/`FieldLabels` derives, and the `?sorted-by` param in the same task so clippy stays green.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/currency_exchange.rs`

**Interfaces:**
- Consumes: `crate::components::sort_header::{SortColumn, SortDir, SortHeader}`, `crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton}`.
- Produces (used by Tasks 3–4):
  - `const COL_PRICE_PER_ITEM/COL_SHOPS/COL_COST/COL_HOURS: &str` with values `"price_per_item"`, `"shops"`, `"cost"`, `"hours_between_sales"`; `ALL_OPTIONAL_COLS`, `DEFAULT_VISIBLE_COLS` (both = all four, in that order)
  - `fn parse_visible_cols(Option<&str>) -> HashSet<&'static str>`; `fn serialize_visible_cols(&HashSet<&'static str>) -> String`
  - `enum SortMode { Profit, PricePerItem, QtyReceived, HoursBetweenSales }` with `Display`/`FromStr` tokens `profit`/`price`/`qty`/`hours`, `SortColumn::fallback() = Profit`, `default_dir(HoursBetweenSales) = Asc`
  - `fn sort_trades(&mut [CurrencyTrade], SortMode, SortDir)`
  - In the view: `visible_cols: Memo<HashSet<&'static str>>` and `list_scroll: NodeRef<leptos::html::Div>` on the table's `overflow-x-auto` wrapper.

- [ ] **Step 1: Write the failing unit tests** (replace the two obsolete layout tests; keep the two locale/category tests untouched)

```rust
#[test]
fn cols_param_round_trips() {
    let all: std::collections::HashSet<_> = ALL_OPTIONAL_COLS.iter().copied().collect();
    assert_eq!(parse_visible_cols(None), all, "absent ?cols= means defaults, and all four default on");
    assert_eq!(parse_visible_cols(Some("")), Default::default(), "explicit empty set is respected");
    let mut some = std::collections::HashSet::new();
    some.insert(COL_SHOPS);
    some.insert(COL_HOURS);
    assert_eq!(parse_visible_cols(Some(&serialize_visible_cols(&some))), some);
    assert_eq!(parse_visible_cols(Some("shops,bogus,hours_between_sales")), some, "unknown tokens are dropped");
}

#[test]
fn sort_tokens_round_trip_and_hours_defaults_ascending() {
    use crate::components::sort_header::{SortColumn, SortDir};
    for mode in [SortMode::Profit, SortMode::PricePerItem, SortMode::QtyReceived, SortMode::HoursBetweenSales] {
        assert_eq!(mode.to_string().parse::<SortMode>(), Ok(mode));
    }
    assert_eq!(SortMode::fallback(), SortMode::Profit);
    assert_eq!(SortMode::HoursBetweenSales.default_dir(), SortDir::Asc, "descending hours puts the slowest sellers first");
    assert_eq!(SortMode::Profit.default_dir(), SortDir::Desc);
}

#[test]
fn sort_trades_orders_by_the_requested_column() {
    use crate::components::sort_header::SortDir;
    let trade = |profit: i64, hours: i16| CurrencyTrade {
        shop_names: ShopNames { shops: vec![] },
        cost_item: None,
        receive_item: None,
        price_per_item: 0,
        number_received: 0,
        total_profit: profit,
        hours_between_sales: hours,
    };
    let mut rows = vec![trade(10, 5), trade(30, 1), trade(20, 9)];
    sort_trades(&mut rows, SortMode::Profit, SortDir::Desc);
    assert_eq!(rows.iter().map(|t| t.total_profit).collect::<Vec<_>>(), [30, 20, 10]);
    sort_trades(&mut rows, SortMode::HoursBetweenSales, SortDir::Asc);
    assert_eq!(rows.iter().map(|t| t.hours_between_sales).collect::<Vec<_>>(), [1, 5, 9]);
}
```

Delete `the_smallest_layout_keeps_the_columns_that_carry_the_answer` (its subject, `column_visibility`, is deleted this task). Keep `filter_query_keys_cover_every_filterable_column` failing/ignored for now — Task 3 rewrites it against the filter table it introduces; if it won't compile once `field_labels()` is gone, replace its body in this task with the Task 3 version pinned against `FILTER_QUERY_KEYS` only:

```rust
#[test]
fn filter_query_keys_cover_every_filterable_column() {
    // Task 3 wires RANGE_FILTERS; until then pin the literal list so the
    // chip/clear-all contract can't drift silently.
    assert_eq!(
        FILTER_QUERY_KEYS,
        [
            "price_per_item_min", "price_per_item_max",
            "number_received_min", "number_received_max",
            "total_profit_min", "total_profit_max",
            "hours_between_sales_min", "hours_between_sales_max",
        ]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-app currency_exchange`
Expected: compile FAIL (`COL_SHOPS`, `SortMode`, `sort_trades` not defined).

- [ ] **Step 3: Implement the model**

Near the top of the file (replacing `column_visibility` and its doc comment):

```rust
/// Stable URL IDs for optional columns, in picker + `?cols=` order.
/// Required columns (item, qty received, profit) are not listed — they
/// always render, and lead the table so a phone's visible slice is the
/// answer, not the trivia.
const COL_PRICE_PER_ITEM: &str = "price_per_item";
const COL_SHOPS: &str = "shops";
const COL_COST: &str = "cost";
const COL_HOURS: &str = "hours_between_sales";

const ALL_OPTIONAL_COLS: &[&str] = &[COL_PRICE_PER_ITEM, COL_SHOPS, COL_COST, COL_HOURS];

/// All four default on; `?cols=` absent = this set, explicitly set (even
/// to "") = respected exactly — same contract as the flip finder.
const DEFAULT_VISIBLE_COLS: &[&str] = ALL_OPTIONAL_COLS;

fn parse_visible_cols(raw: Option<&str>) -> std::collections::HashSet<&'static str> {
    match raw {
        None => DEFAULT_VISIBLE_COLS.iter().copied().collect(),
        Some(s) => s
            .split(',')
            .filter_map(|tok| ALL_OPTIONAL_COLS.iter().find(|c| **c == tok).copied())
            .collect(),
    }
}

fn serialize_visible_cols(visible: &std::collections::HashSet<&'static str>) -> String {
    ALL_OPTIONAL_COLS
        .iter()
        .filter(|c| visible.contains(*c))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Profit,
    PricePerItem,
    QtyReceived,
    HoursBetweenSales,
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SortMode::Profit => "profit",
            SortMode::PricePerItem => "price",
            SortMode::QtyReceived => "qty",
            SortMode::HoursBetweenSales => "hours",
        })
    }
}

impl std::str::FromStr for SortMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "profit" => Ok(SortMode::Profit),
            "price" => Ok(SortMode::PricePerItem),
            "qty" => Ok(SortMode::QtyReceived),
            "hours" => Ok(SortMode::HoursBetweenSales),
            _ => Err(()),
        }
    }
}

impl SortColumn for SortMode {
    fn fallback() -> Self {
        SortMode::Profit
    }
    /// Hours-between-sales reads best-first ascending — descending would
    /// put the slowest sellers on top. Everything else is best-first
    /// descending, the kit default.
    fn default_dir(self) -> SortDir {
        match self {
            SortMode::HoursBetweenSales => SortDir::Asc,
            _ => SortDir::Desc,
        }
    }
}

fn sort_trades(rows: &mut [CurrencyTrade], mode: SortMode, dir: SortDir) {
    let key = |t: &CurrencyTrade| -> i64 {
        match mode {
            SortMode::Profit => t.total_profit,
            SortMode::PricePerItem => t.price_per_item as i64,
            SortMode::QtyReceived => t.number_received as i64,
            SortMode::HoursBetweenSales => t.hours_between_sales as i64,
        }
    };
    match dir {
        SortDir::Desc => rows.sort_by_key(|t| std::cmp::Reverse(key(t))),
        SortDir::Asc => rows.sort_by_key(key),
    }
}
```

Imports: add `use crate::components::sort_header::{SortColumn, SortDir, SortHeader};` and `use crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton};`. Drop `use field_iterator::{FieldLabels, SortableVec};` and remove both derives from `CurrencyTrade` (leave `#[derive(Clone)]`). Drop `use crate::Tooltip;`, `use crate::components::modal::Modal;`, `use crate::components::loading::Loading;` once their users go in step 4.

- [ ] **Step 4: Rewire the table in `ExchangeItemContent`**

Inside the component, replace the `sorted-by` signal with:

```rust
let (sort_param, _) = query_signal::<String>("sort");
let (dir_param, _) = query_signal::<String>("dir");
let sort_mode = Memo::new(move |_| sort_param().and_then(|s| s.parse::<SortMode>().ok()));
let sort_dir = Memo::new(move |_| dir_param().and_then(|s| s.parse::<SortDir>().ok()));
let (cols_param, set_cols_param) = query_signal::<String>("cols");
let visible_cols = Memo::new(move |_| parse_visible_cols(cols_param().as_deref()));
let list_scroll = NodeRef::<leptos::html::Div>::new();
```

(`set_cols_param` is consumed by Task 3's ControlBar; until then suppress the unused warning by destructuring `let (cols_param, _set_cols_param) = ...` and renaming in Task 3.)

Inside the existing `Suspense` closure, replace `sorted_and_filtered_rows`'s sort arm with:

```rust
let mode = sort_mode.get().unwrap_or_else(SortMode::fallback);
let dir = sort_dir.get().unwrap_or_else(|| mode.default_dir());
sort_trades(&mut p, mode, dir);
```

Replace the whole `<table>` block (headers and body). New column order: **item, qty, profit, then optional price / shops / cost / hours**, each optional one gated on `visible_cols`. Headers use `SortHeader` for the four sortable columns and plain `<th>` text for item/shops/cost:

```rust
view! {
    // Only the table scrolls sideways; the panel must not (overflow-y trap).
    <div class="overflow-x-auto" node_ref=list_scroll>
        <table class="w-full text-sm text-left">
            <thead class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                <tr class="border-b border-white/5">
                    <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                        {t!(i18n, currency_exchange_table_item)}
                    </th>
                    <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap text-right">
                        <div class="flex justify-end">
                            <SortHeader
                                mode=SortMode::QtyReceived
                                label=t_string!(i18n, currency_exchange_table_qty_recv).to_string()
                                sort_mode=sort_mode
                                sort_dir=sort_dir
                            />
                        </div>
                    </th>
                    <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap text-right">
                        <div class="flex justify-end">
                            <SortHeader
                                mode=SortMode::Profit
                                label=t_string!(i18n, currency_exchange_table_profit).to_string()
                                sort_mode=sort_mode
                                sort_dir=sort_dir
                            />
                        </div>
                    </th>
                    {move || visible_cols.get().contains(COL_PRICE_PER_ITEM).then(|| view! {
                        <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap text-right">
                            <div class="flex justify-end">
                                <SortHeader
                                    mode=SortMode::PricePerItem
                                    label=t_string!(i18n, currency_exchange_table_price_per_item).to_string()
                                    sort_mode=sort_mode
                                    sort_dir=sort_dir
                                />
                            </div>
                        </th>
                    })}
                    {move || visible_cols.get().contains(COL_SHOPS).then(|| view! {
                        <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                            {t!(i18n, currency_exchange_table_shops)}
                        </th>
                    })}
                    {move || visible_cols.get().contains(COL_COST).then(|| view! {
                        <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap">
                            {t!(i18n, currency_exchange_table_cost)}
                        </th>
                    })}
                    {move || visible_cols.get().contains(COL_HOURS).then(|| view! {
                        <th scope="col" class="px-3 py-2 font-bold whitespace-nowrap text-right">
                            <div class="flex justify-end">
                                <SortHeader
                                    mode=SortMode::HoursBetweenSales
                                    label=t_string!(i18n, currency_exchange_table_hours_per_sale).to_string()
                                    sort_mode=sort_mode
                                    sort_dir=sort_dir
                                />
                            </div>
                        </th>
                    })}
                </tr>
            </thead>
            <tbody class="divide-y divide-white/5">{sorted_and_filtered_rows}</tbody>
        </table>
    </div>
}
```

Row cells, same order and gating (`p` is the `CurrencyTrade`):

```rust
<tr class="hover:bg-white/5 transition-colors">
    <td class="px-3 py-2"><ItemAmount item_amount=p.receive_item /></td>
    <td class="px-3 py-2 text-right tabular-nums">{p.number_received}</td>
    <td class="px-3 py-2 text-right tabular-nums font-medium text-[color:var(--color-text)]">
        {p.total_profit}
    </td>
    {visible.contains(COL_PRICE_PER_ITEM).then(|| view! {
        <td class="px-3 py-2 text-right tabular-nums">{p.price_per_item}</td>
    })}
    {visible.contains(COL_SHOPS).then(|| view! {
        <td class="px-3 py-2 text-[color:var(--color-text-muted)]">
            <ShopNames shop_names=p.shop_names.clone() />
        </td>
    })}
    {visible.contains(COL_COST).then(|| view! {
        <td class="px-3 py-2"><ItemAmount item_amount=p.cost_item /></td>
    })}
    {visible.contains(COL_HOURS).then(|| view! {
        <td class="px-3 py-2 text-right tabular-nums text-[color:var(--color-text-muted)]">
            {p.hours_between_sales}
        </td>
    })}
</tr>
```

(`let visible = visible_cols.get();` once at the top of the row-mapping closure, not per cell. `ShopNames` needs `.clone()` because the gated closure is `FnOnce` inside a collected view — if the borrow checker allows the move, drop the clone.)

Replace the `Suspense` fallback `Loading` with a skeleton derived from the visible set:

```rust
fn skeleton_columns(visible: &std::collections::HashSet<&'static str>) -> Vec<SkeletonColumn> {
    let mut cols = vec![
        SkeletonColumn::new("flex-1 min-w-40", SkeletonCell::IconText),
        SkeletonColumn::new("w-20", SkeletonCell::Number),
        SkeletonColumn::new("w-24", SkeletonCell::Number),
    ];
    if visible.contains(COL_PRICE_PER_ITEM) {
        cols.push(SkeletonColumn::new("w-24", SkeletonCell::Number));
    }
    if visible.contains(COL_SHOPS) {
        cols.push(SkeletonColumn::new("w-40", SkeletonCell::Text));
    }
    if visible.contains(COL_COST) {
        cols.push(SkeletonColumn::new("w-40", SkeletonCell::IconText));
    }
    if visible.contains(COL_HOURS) {
        cols.push(SkeletonColumn::new("w-20", SkeletonCell::Number));
    }
    cols
}
```

```rust
<Suspense fallback=move || view! {
    <TableSkeleton columns=skeleton_columns(&visible_cols.get()) rows=10 />
}>
```

Delete in this same step: `FilterModal` (whole component), `column_visibility`, the old `<thead>` `QueryButton`/`Tooltip` wiring, the `labels`/`field_labels()` usage, the `info!(...)` debug log and its `use log::info;`, and `CurrencyTrade::sort_vec_by_label`'s caller. `QueryButton` stays imported only if the Task 3 chip row hasn't landed yet — the old chip row still uses it until Task 3; leave the old filter panel + chip row untouched in this task.

- [ ] **Step 5: Run tests, fmt, clippy**

Run: `cargo test -p ultros-app currency_exchange` → PASS.
Run: `./check_ci.sh > /tmp/ce_ci.log 2>&1; echo "REAL_EXIT=$?"` → `REAL_EXIT=0`.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/currency_exchange.rs
git commit -m "feat(currency-exchange): kit column model, SortHeader sorting, TableSkeleton (#1128)"
```

---

### Task 3: ToolHeader + ControlBar replace the bespoke filter surface

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/currency_exchange.rs`

**Interfaces:**
- Consumes: Task 2's `COL_*`, `serialize_visible_cols`, `set_cols_param`, `visible_cols`; `crate::components::control_bar::{ColumnOption, ControlBar, FilterOption}`; `crate::components::filter_chip::FilterChip`; `crate::components::tool_help::ToolHeader`; `crate::query_defaults::filter_query_signal`.
- Produces: `const RANGE_FILTERS: &[RangeFilter]` (the 8-entry filter table Task 5's test pins), the ControlBar wiring Task 4 hooks fades onto.

- [ ] **Step 1: Rewrite the filter-keys test against the filter table**

```rust
#[test]
fn filter_query_keys_cover_every_range_filter() {
    // FILTER_QUERY_KEYS drives "Clear all"; RANGE_FILTERS drives the chips
    // and the + Filter menu. A filter present in one but not the other is
    // either unclearable or unclear-all-able.
    let from_defs: Vec<&str> = RANGE_FILTERS.iter().map(|f| f.key).collect();
    assert_eq!(FILTER_QUERY_KEYS.to_vec(), from_defs);
}
```

- [ ] **Step 2: Run it to verify it fails** (`RANGE_FILTERS` undefined)

Run: `cargo test -p ultros-app currency_exchange`

- [ ] **Step 3: Implement the filter table and wiring**

```rust
/// One min/max half of a numeric column filter: everything the chip, the
/// `+ Filter` menu, and Clear-all need to agree on.
struct RangeFilter {
    /// Query key, kept verbatim from the old UI so deep links survive.
    key: &'static str,
    /// Spinner floor for the chip's inline input. `None` for profit —
    /// a negative profit floor is a legitimate filter.
    min: Option<&'static str>,
}

const RANGE_FILTERS: &[RangeFilter] = &[
    RangeFilter { key: "price_per_item_min", min: Some("0") },
    RangeFilter { key: "price_per_item_max", min: Some("0") },
    RangeFilter { key: "number_received_min", min: Some("0") },
    RangeFilter { key: "number_received_max", min: Some("0") },
    RangeFilter { key: "total_profit_min", min: None },
    RangeFilter { key: "total_profit_max", min: None },
    RangeFilter { key: "hours_between_sales_min", min: Some("0") },
    RangeFilter { key: "hours_between_sales_max", min: Some("0") },
];
```

In `ExchangeItemContent`, replace the `filters_open` signal, `active_filter_count`, the filter-toggle button, the `FilterRange` grid, and the hand-rolled chip row with:

```rust
use crate::query_defaults::filter_query_signal;

// One (getter, setter) per range filter, in RANGE_FILTERS order. The i32
// values themselves keep flowing through `is_in_range` via the query map —
// these signals exist for the chips.
let filter_signals: Vec<(Memo<Option<i32>>, SignalSetter<Option<i32>>)> = RANGE_FILTERS
    .iter()
    .map(|f| filter_query_signal::<i32>(f.key))
    .collect();
let filter_signals = StoredValue::new(filter_signals);

// A filter the user just added from the menu but hasn't committed yet —
// its chip mounts in edit state with an empty input.
let pending_filter: RwSignal<Option<&'static str>> = RwSignal::new(None);

let active_filters = Memo::new(move |_| {
    filter_signals.with_value(|sigs| {
        RANGE_FILTERS
            .iter()
            .zip(sigs)
            .filter(|(f, (get, _))| get.get().is_some() || pending_filter.get() == Some(f.key))
            .map(|(f, _)| f.key)
            .collect::<Vec<_>>()
    })
});
```

Label helpers (menu label = long/explanatory, chip label = comparison-shaped):

```rust
let menu_label = move |key: &str| -> String {
    match key {
        "price_per_item_min" => t_string!(i18n, currency_exchange_filter_price_min_label).to_string(),
        "price_per_item_max" => t_string!(i18n, currency_exchange_filter_price_max_label).to_string(),
        "number_received_min" => t_string!(i18n, currency_exchange_filter_qty_min_label).to_string(),
        "number_received_max" => t_string!(i18n, currency_exchange_filter_qty_max_label).to_string(),
        "total_profit_min" => t_string!(i18n, currency_exchange_filter_profit_min_label).to_string(),
        "total_profit_max" => t_string!(i18n, currency_exchange_filter_profit_max_label).to_string(),
        "hours_between_sales_min" => t_string!(i18n, currency_exchange_filter_hours_min_label).to_string(),
        "hours_between_sales_max" => t_string!(i18n, currency_exchange_filter_hours_max_label).to_string(),
        _ => String::new(),
    }
};
let chip_label = move |key: &str| -> String {
    match key {
        "price_per_item_min" => t_string!(i18n, currency_exchange_chip_price_min).to_string(),
        "price_per_item_max" => t_string!(i18n, currency_exchange_chip_price_max).to_string(),
        "number_received_min" => t_string!(i18n, currency_exchange_chip_qty_min).to_string(),
        "number_received_max" => t_string!(i18n, currency_exchange_chip_qty_max).to_string(),
        "total_profit_min" => t_string!(i18n, currency_exchange_chip_profit_min).to_string(),
        "total_profit_max" => t_string!(i18n, currency_exchange_chip_profit_max).to_string(),
        "hours_between_sales_min" => t_string!(i18n, currency_exchange_chip_hours_min).to_string(),
        "hours_between_sales_max" => t_string!(i18n, currency_exchange_chip_hours_max).to_string(),
        _ => String::new(),
    }
};
```

ControlBar inputs:

```rust
let filter_options = Memo::new(move |_| {
    let active = active_filters();
    RANGE_FILTERS
        .iter()
        .filter(|f| !active.contains(&f.key))
        .map(|f| FilterOption { id: f.key, label: menu_label(f.key) })
        .collect::<Vec<_>>()
});
let column_options = Memo::new(move |_| {
    vec![
        ColumnOption { id: COL_PRICE_PER_ITEM, label: t_string!(i18n, currency_exchange_table_price_per_item).to_string() },
        ColumnOption { id: COL_SHOPS, label: t_string!(i18n, currency_exchange_table_shops).to_string() },
        ColumnOption { id: COL_COST, label: t_string!(i18n, currency_exchange_table_cost).to_string() },
        ColumnOption { id: COL_HOURS, label: t_string!(i18n, currency_exchange_table_hours_per_sale).to_string() },
    ]
});
let toggle_column = Callback::new(move |col: &'static str| {
    let mut set = visible_cols.get_untracked();
    if set.contains(col) { set.remove(col); } else { set.insert(col); }
    set_cols_param.set(Some(serialize_visible_cols(&set)));
});
let reset_columns = Callback::new(move |_| set_cols_param.set(None));
let add_filter = Callback::new(move |key: &'static str| pending_filter.set(Some(key)));
let clear_all = Callback::new(move |_| {
    pending_filter.set(None);
    filter_signals.with_value(|sigs| {
        for (_, set) in sigs { set.set(None); }
    });
});
```

The result count needs the filtered row total. Hoist a `trade_count: RwSignal<usize>` written from inside the Suspense closure where `sorted_and_filtered_rows` already computes `count`, and read it in the bar's summary. (Writes to a signal that outlives the closure are safe; reads inside the bar re-render on change.)

The bar itself, rendered *above* the `Suspense` panel (it must not remount with the data):

```rust
<ControlBar
    summary=move || view! {
        <span class="text-sm font-semibold text-[color:var(--color-text)] whitespace-nowrap truncate">
            {move || t!(i18n, currency_exchange_trade_count, count = move || trade_count.get())}
        </span>
    }
    columns=Signal::derive(move || column_options())
    visible_columns=Signal::derive(move || visible_cols.get())
    on_toggle_column=toggle_column
    on_reset_columns=reset_columns
    available_filters=Signal::derive(move || filter_options())
    on_add_filter=add_filter
    on_clear_all=clear_all
    empty_label=Signal::derive(move || t_string!(i18n, currency_exchange_no_filters_hint).to_string())
    is_empty=Signal::derive(move || active_filters().is_empty())
>
    {move || {
        filter_signals.with_value(|sigs| {
            RANGE_FILTERS
                .iter()
                .zip(sigs.iter().copied())
                .filter(|(f, (get, _))| get.get().is_some() || pending_filter.get() == Some(f.key))
                .map(|(f, (get, set))| {
                    let key = f.key;
                    view! {
                        <FilterChip
                            label=chip_label(key)
                            value=Signal::derive(move || get.get().map(|v| v.to_string()))
                            numeric=true
                            min=f.min.map(|m| m.to_string())
                            start_editing=pending_filter.get_untracked() == Some(key)
                            on_commit=Callback::new(move |v: Option<String>| {
                                set.set(v.and_then(|v| v.parse::<i32>().ok()));
                                if pending_filter.get_untracked() == Some(key) {
                                    pending_filter.set(None);
                                }
                            })
                        />
                    }
                })
                .collect_view()
        })
    }}
</ControlBar>
```

- [ ] **Step 4: ToolHeader + quantity row + landing title move**

In the parent `CurrencyExchange` wrapper: delete the `<A>`-wrapped `<h3>` (keep the ad + `main-content` + `Outlet`). In `CurrencySelection` (landing), add the title back as its own heading above the search panel so the landing page keeps its name:

```rust
<h1 class="text-2xl font-bold text-[color:var(--brand-fg)]">
    {t!(i18n, currency_exchange_title)}
</h1>
```

In `ExchangeItemContent`, replace the old header panel (the `panel p-4` div holding the h2, quantity input, and filter button) with:

```rust
<ToolHeader
    title=format!("{} — {}", item_name(), t_string!(i18n, currency_exchange_title))
    summary=t_string!(i18n, currency_exchange_tool_summary).to_string()
    context=t_string!(i18n, currency_exchange_tool_context).to_string()
    help_href="/help"
    help_body=t_string!(i18n, currency_exchange_tool_help).to_string()
/>
<div class="flex flex-row justify-end items-center gap-3">
    <label for="currency-quantity" class="text-sm text-[color:var(--color-text-muted)]">
        {t!(i18n, currency_exchange_how_many)}
    </label>
    <input
        id="currency-quantity"
        class="input w-24"
        inputmode="numeric"
        prop:value=currency_quantity
        on:input=move |e| {
            if let Ok(p) = event_target_value(&e).parse() {
                set_currency_quantity.set(Some(p));
            }
        }
    />
</div>
```

(`help_href="/help"` — no per-tool help topic exists yet; a follow-up adds one.) Keep the "assuming sales on {world}" note and the no-home-world banner where they are.

- [ ] **Step 5: Empty state**

In the row-rendering path, after filtering: when the filtered set is empty and `!active_filters().is_empty()`, render instead of the table body's rows a full-width message row:

```rust
<tr>
    <td colspan="7" class="px-3 py-8 text-center text-[color:var(--color-text-muted)]">
        <div class="flex flex-col items-center gap-2">
            {t!(i18n, currency_exchange_no_matches)}
            <button class="btn-secondary" on:click=move |_| clear_all.run(())>
                {t!(i18n, analyzer_clear_all)}
            </button>
        </div>
    </td>
</tr>
```

- [ ] **Step 6: Delete the obsolete pieces**

`FilterRange` component, the `filters_open`/`active_filter_count` remnants, the old chip-row block and its `push_chip`, and now-unused imports (`QueryButton`, `ParseableInputBox`, `SignalSetter` if unused — note `SignalSetter` is still used by `filter_signals`' type). `FILTER_QUERY_KEYS` **stays** (Clear-all test + `is_in_range` contract).

- [ ] **Step 7: Tests, fmt, clippy**

Run: `cargo test -p ultros-app currency_exchange` → PASS.
Run: `./check_ci.sh > /tmp/ce_ci.log 2>&1; echo "REAL_EXIT=$?"` → `REAL_EXIT=0`.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/currency_exchange.rs
git commit -m "feat(currency-exchange): ToolHeader + shared ControlBar/FilterChip surface (#1128)"
```

---

### Task 4: Edge fades on the table scrollport

**Files:**
- Modify: `ultros-frontend/ultros-app/style/tailwind.css`
- Modify: `ultros-frontend/ultros-app/src/routes/currency_exchange.rs`

**Interfaces:**
- Consumes: Task 2's `list_scroll: NodeRef<leptos::html::Div>`.

- [ ] **Step 1: Add the CSS class**

Next to the existing `.filter-chip-row` fade rules in `style/tailwind.css` (find them by grepping `chip-fade`; mirror their mask technique — if they use `mask-image`, use the same property and vendor prefixes):

```css
/* Horizontal-scroll affordance: fades driven by --hfade-start/--hfade-end,
   set from scroll geometry in the component. Both 0 (no fade) until
   hydration measures the element. */
.hscroll-fade {
  --hfade-start: 0px;
  --hfade-end: 0px;
  mask-image: linear-gradient(
    to right,
    transparent 0,
    black var(--hfade-start),
    black calc(100% - var(--hfade-end)),
    transparent 100%
  );
}
```

- [ ] **Step 2: Wire the listeners**

Add `hscroll-fade` to the table wrapper's class (`<div class="overflow-x-auto hscroll-fade" node_ref=list_scroll>`). Then, in `ExchangeItemContent`, a hydrate-gated block modeled on the analyzer's chip fades (`analyzer.rs`, "Filter chip strip: edge fades" section — copy its listener-parking and cleanup shape exactly):

```rust
#[cfg(feature = "hydrate")]
{
    use web_sys::wasm_bindgen::JsCast;
    use web_sys::wasm_bindgen::closure::Closure;
    let fade_listeners = StoredValue::new_local(
        None::<(web_sys::HtmlDivElement, Closure<dyn FnMut()>, Closure<dyn FnMut()>)>,
    );
    on_cleanup(move || {
        fade_listeners.update_value(|slot| {
            if let Some((el, scroll_cb, resize_cb)) = slot.take() {
                let _ = el.remove_event_listener_with_callback(
                    "scroll", scroll_cb.as_ref().unchecked_ref());
                if let Some(win) = web_sys::window() {
                    let _ = win.remove_event_listener_with_callback(
                        "resize", resize_cb.as_ref().unchecked_ref());
                }
            }
        });
    });
    const FADE_PX: f64 = 24.0;
    let apply_fades = |el: &web_sys::HtmlDivElement| {
        let left = el.scroll_left();
        let right = (el.scroll_width() as f64 - el.client_width() as f64 - left as f64).max(0.0);
        let px = |on: bool| if on { format!("{FADE_PX}px") } else { "0px".to_string() };
        let style = web_sys::HtmlElement::style(el);
        let _ = style.set_property("--hfade-start", &px(left > 1));
        let _ = style.set_property("--hfade-end", &px(right > 1.0));
    };
    Effect::new(move |_| {
        // Tracked: toggling a column changes scrollWidth without a scroll
        // or resize event firing.
        let _ = visible_cols.get();
        let Some(el) = list_scroll.get() else { return };
        apply_fades(&el);
        if fade_listeners.with_value(|slot| slot.is_some()) {
            return;
        }
        let on_scroll = { let el = el.clone(); Closure::wrap(Box::new(move || apply_fades(&el)) as Box<dyn FnMut()>) };
        let on_resize = { let el = el.clone(); Closure::wrap(Box::new(move || apply_fades(&el)) as Box<dyn FnMut()>) };
        let _ = el.add_event_listener_with_callback("scroll", on_scroll.as_ref().unchecked_ref());
        if let Some(win) = web_sys::window() {
            let _ = win.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref());
        }
        fade_listeners.set_value(Some((el, on_scroll, on_resize)));
    });
}
```

(`apply_fades` must be a plain closure both `Closure`s can capture by clone — if borrow issues arise, make it a free `fn apply_table_fades(el: &web_sys::HtmlDivElement)`. Note `check_ci.sh` never lints `#[cfg(feature = "hydrate")]` code — compile it for real with `cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown` if the toolchain has the target installed; otherwise re-read the block carefully against the analyzer's, which is known-good.)

- [ ] **Step 3: fmt, clippy, wasm check**

Run: `./check_ci.sh > /tmp/ce_ci.log 2>&1; echo "REAL_EXIT=$?"` → `REAL_EXIT=0`.
Run: `cargo check -p ultros-app --features hydrate --target wasm32-unknown-unknown` (skip only if the target isn't installed, and say so in the PR).

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/style/tailwind.css ultros-frontend/ultros-app/src/routes/currency_exchange.rs
git commit -m "feat(currency-exchange): edge fades on the table scrollport (#1128)"
```

---

### Task 5: Locale cleanup, full verification, PR

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

- [ ] **Step 1: Remove now-unused keys from all seven locales**

Grep the crate for each key before deleting — remove only keys with zero `t!`/`t_string!` references. Candidates: `currency_exchange_edit_filter`, `currency_exchange_filter_tooltip`, `currency_exchange_filters`, `currency_exchange_min`, `currency_exchange_max`, `currency_exchange_min_field_aria`, `currency_exchange_max_field_aria`, `currency_exchange_full_results`, and the four `currency_exchange_*_title` filter-panel labels. Any key still referenced stays.

- [ ] **Step 2: Full test + CI run**

Run: `cargo test -p ultros-app currency_exchange` → all PASS.
Run: `./check_ci.sh > /tmp/ce_ci.log 2>&1; echo "REAL_EXIT=$?"` → `REAL_EXIT=0`.

- [ ] **Step 3: Visual check at phone width**

Serve locally (or verify statically if a local build isn't feasible): at 375px the visible slice of `/currency-exchange/:id` must show item, qty, profit; scrolling right reveals the optional columns with a fade on the clipped edge; the control bar renders chips only for set filters. Note in the PR which verification was done.

- [ ] **Step 4: Commit, push, open PR**

```bash
git add ultros-frontend/ultros-app/locales
git commit -m "i18n(currency-exchange): drop keys orphaned by the kit rebuild (#1128)"
git push -u origin claude/currency-exchange-refactor-ui-0d3f07
```

PR: base `main`, title `feat(currency-exchange): rebuild on the shared UI kit (closes #1128)`, body summarizing the spec, linking `docs/superpowers/specs/2026-08-14-currency-exchange-kit-rebuild-design.md`, and noting the `/help` link stopgap. End the body with the standard generated-with footer.
