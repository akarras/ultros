# ClickHouse Surface Integration — Design

**Date:** 2026-05-18
**Scope:** Trends page rebuild, Flip Finder analyzer enrichment, Top Opportunities card, Market Movers labeling.

## Problem

The recent ClickHouse rollups (`sales_hourly`, `world_kpi_5min`, `item_stats_window`, `item_quality_score`) power the new home-page command-center dashboard well, but three other surfaces are still on Pass-1 (in-memory) data even though the wire already carries — or could trivially carry — clean CH aggregates:

1. **Flip Finder (`/flip-finder/{world}`).** Built entirely from `get_recent_sales_for_world` + `get_cheapest_listings`. No confidence band, no laundering filter, no sparkline. As a result, obvious gil-trader laundering shows up at the top: e.g. *Copper Wristlets* listed at `3` gil with a `18,999,997` "profit" because one shill sale on Hyperion priced it at 19M. The ClickHouse `item_quality_score` table already labels rows like this `Unusable` / high `launder_suspicion_pct`, but the analyzer never consults it.
2. **Trends (`/trends/{world}`).** `analyzer_service.get_trends` bucketizes items into rising / falling / high-velocity using up to 6 in-memory samples per item. It then enriches each row with `deep_scan_batch(window_days = 30)` and drops `Unusable` — so confidence works — but `average_sale_price` and `sales_per_week` shown to the user are still the 6-sample numbers, and there is no sparkline, no percentile, no window selector. The three Pass-1 buckets duplicate the at-a-glance Market Movers strip we render right above the table.
3. **Top Opportunity card on the home page.** Single featured deal from `get_best_deals(world)` (hard-coded `min_profit=10000&filter_sale=Week`). Server-side this already runs through `deep_scan_batch` and drops `Unusable`, but the FE `ResaleStatsDto` has not been updated to mirror the Phase 2 fields, so the FE *can't see* `confidence_band` or `launder_suspicion` even though they are on the wire. The card is also stuck showing exactly one deal when several are usually available.

Adjacent papercut: **Market Movers' "Volume" tab** sorts by `unit_volume` (unit count) while *six inches away* the Market Pulse "Market Volume" tile shows `gil_volume`. Same word, different meaning, no tooltip.

## Goals

- Flip Finder never recommends an `Unusable` / high-laundering row by default; users can opt in to see them.
- Flip Finder shows per-row confidence, a 24h sparkline, and a real 30-day VWAP so users can sanity-check a row in one glance.
- Trends page is sourced from `item_stats_window` for whichever window the user picks (7d / 30d / 90d), with a sparkline per row, real `sales/window` velocity, and full sortable columns.
- Top Opportunities card shows 5 deals, not 1; each deal is junk-filtered using the same policy as the analyzer; FE DTO surfaces all Phase 2 fields.
- Market Movers Volume tab is labeled and tooltipped so it can't be confused with gil-volume.
- Cross-cutting junk-filter policy lives in one place (`ResaleQualityFilter`) and is applied consistently across the three surfaces.

## Non-goals

- New ClickHouse views or rollups. The four existing rollups already cover all use cases here; designing new views before we see what's missing is premature.
- Datacenter-level trends (server still returns `BadRequest` for non-World selectors on `/api/v1/trends`).
- Reworking the price-history chart on item-view (worth its own spec — percentile band overlay, window selector — but out of scope here).
- Recomputing or replacing the existing Pass-1 / two-pass sniper-clamp logic in the analyzer's `ProfitTable`. Sniper-clamp + IQR filter stay; we *layer* the junk-filter on top.
- Renaming the route from `/flip-finder/{world}` (kept for stable inbound links).
- Recipe / Crafter resale paths. They use a different code path (`get_best_resale`) which is enriched already; this work doesn't touch them.

## Cross-cutting: `ResaleQualityFilter`

A single helper on the server side that, given a list of `(item_id, hq, world_id)` tuples and a `DeepScan` map, returns a closure deciding whether each row should be shown.

```rust
// ultros/src/resale_quality_filter.rs (new file)
pub struct ResaleQualityFilter {
    /// "Suspicious" = Unusable confidence OR laundering > threshold.
    pub include_suspicious: bool,
    /// Threshold (0.0–1.0) above which a row is flagged as suspicious.
    /// Default 0.7 — same threshold the analyzer rollup uses to flag
    /// laundering, and high enough to never catch a legitimate hot item.
    pub launder_threshold: f32,
}

impl Default for ResaleQualityFilter {
    fn default() -> Self {
        Self { include_suspicious: false, launder_threshold: 0.7 }
    }
}

impl ResaleQualityFilter {
    /// Decide whether to keep `(item, hq, world)` given the deep-scan map.
    /// Rows with no DeepScan present are kept (Pass-1 data, no signal to
    /// reject from). The FE renders the `confidence_band` directly via
    /// `ConfidenceBadge`; this helper is hide/show-only.
    pub fn keep(&self, scan: Option<&DeepScan>) -> bool { ... }
}
```

**Policy (B from brainstorm):**

- `confidence_band == Unusable` → hide unless `include_suspicious`.
- `launder_suspicion_pct > 0.7` → hide unless `include_suspicious`.
- `confidence_band == Low | Medium | High` → keep. FE renders the chip from `confidence_band` directly.
- `confidence_band == Unknown` (no CH coverage) → keep. Don't penalize items that haven't been rolled up yet.

The toggle is per surface (`include_suspicious` is a function argument), not a global setting — Trends and Analyzer each get their own URL-query toggle (`show-suspicious=1`).

## Workstream 1: FE `ResaleStatsDto` parity (prerequisite)

`ultros-frontend/ultros-app/src/api.rs:91` is stale. Expand to mirror the server DTO so the FE can read all Phase 2 fields. This is non-breaking — the server already emits these fields and serde tolerates extras only one-way.

```rust
// ultros-frontend/ultros-app/src/api.rs
#[derive(Clone, Debug, serde::Deserialize, PartialEq)]
pub struct ResaleStatsDto {
    pub profit: i32,
    pub item_id: i32,
    pub hq: bool,
    pub sold_within: String,
    pub return_on_investment: f32,
    pub world_id: i32,
    #[serde(default)]
    pub confidence_band: ConfidenceBand,
    #[serde(default)]
    pub vwap_30d: i32,
    #[serde(default)]
    pub sample_size_30d: u32,
    #[serde(default)]
    pub launder_suspicion: f32,
}
```

`get_best_deals` becomes parameterized:

```rust
pub async fn get_best_deals(
    world_name: &str,
    min_profit: i32,         // default 10000
    filter_sale: &str,       // "Day" | "Week" | "Month", default "Week"
) -> AppResult<Vec<ResaleStatsDto>> { ... }
```

Existing call sites get explicit defaults.

## Workstream 2: Trends page rebuild

### New server endpoint shape

`/api/v1/trends/{world_name}?window=7|30|90&show_suspicious=0|1`

Backwards compatibility: missing `window` → `30` (current behavior). Missing `show_suspicious` → `0`.

`TrendsData` gains one field — the rows are no longer split into three pre-bucketed lists, they're a single flat list and the FE sorts. Old fields stay for one release as `#[deprecated]` to ease rollout, but the new field is what the new UI reads.

```rust
// ultros-api-types/src/trends.rs
pub struct TrendsData {
    pub items: Vec<TrendItem>,           // NEW — single flat list
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub high_velocity: Vec<TrendItem>,   // kept temporarily for compat
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rising_price: Vec<TrendItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub falling_price: Vec<TrendItem>,
}

pub struct TrendItem {
    pub item_id: i32,
    pub hq: bool,
    pub price: i32,                          // current cheapest on this world
    pub world_id: i32,

    // CH-backed window stats (replaces 6-sample averages)
    pub window_days: u16,                    // echoed back from query
    pub vwap: i32,                           // window VWAP
    pub sales_in_window: u32,                // cleaned sample size
    pub unit_volume_window: u64,             // units traded in window
    pub gil_volume_window: u64,              // gil traded in window
    pub sales_per_day: f32,                  // sales_in_window / window_days
    pub price_percentile: u8,                // where `price` falls in p10..p90
    pub pct_change_window: f32,              // (p50_now - p50_then) — Phase 2
    pub confidence_band: ConfidenceBand,
    pub launder_suspicion: f32,

    // 24h sparkline — embedded so the table renders in one round-trip
    pub sparkline_24h: Vec<u32>,
}
```

**Server implementation** (`analyzer_service::get_trends_v2`):

1. Pull the world's `CheapestListings` (already in RAM).
2. For each `(item_id, hq)` in cheapest, build the request tuple list.
3. One `deep_scan_batch(window_days, &requests)` → `Vec<DeepScan>`.
4. One `sparklines_batch(&requests, 24)` → `Vec<SparklineRow>`.
5. Join in-memory; build `TrendItem` from each `DeepScan` row (skip if no DeepScan — the item has no recent CH activity, so it doesn't belong in a "trends" view).
6. Apply `ResaleQualityFilter` per `show_suspicious`.
7. Return at most `LIMIT = 500` items, sorted server-side by `unit_volume_window DESC` as default.

The FE handles further filter / sort / pagination on the cached payload (Leptos query-signal–driven, no extra round trips).

`pct_change_window`: defined as `(price_now / vwap_window) - 1.0`. Cheap, useful as a sortable column. A "true" delta vs the window-start price would need a window-aware bucketed query; not worth the SQL today.

### Frontend rewrite

Replace `routes/trends.rs::Trends` and `TrendsTable`. New layout:

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Market Trends — Gilgamesh                                                │
│  [World ▼ Gilgamesh]    [Window: 7d · 30d · 90d]    [☐ Show suspicious]   │
│                                                                           │
│  [MarketHeat band — unchanged, shows 24h activity by category]            │
│  [MarketMovers strip — unchanged, 24h rising/falling/volume]              │
│                                                                           │
│  Detail table (sortable, filterable)                                      │
│  ┌─ HQ ─ Item ─────── Spark ─ Price ─ VWAP ─ Δ ─── Sales/Day ─ Vol ─ Q ┐ │
│  │     ⚪ Iron Ore     ╱╲╱─   240    220    +9.1%   18.4         12k  ◆◆ │
│  │     ⚪ Cloud Mica   ─╱╲─   8,150  7,990  +2.0%   3.6          840  ◆◆◆│
│  │     ⚪ Dark Matter ⚠ ╱──   12,000 9,990  +20.1%  0.8           24  ⚠  │ ← chip
│  └────────────────────────────────────────────────────────────────────────┘ │
│  [Showing 187 of 500 · sorted by Units traded ↓ · 30d window]            │
└──────────────────────────────────────────────────────────────────────────┘
```

Filter chips work the same way as the analyzer: every filter has a chip and a clear-all.

- **Window selector:** three pills. Drives a single `window` URL query param. Default 30d.
- **Show suspicious toggle:** drives `show_suspicious` URL param. Default unchecked.
- **Category filter:** dropdown bound to `category` URL param (uses `tracked_data().item_search_categorys` — same as analyzer).
- **Min sales filter:** numeric input bound to `min_sales` URL param. Filters by `sales_in_window`.
- **Min price filter:** numeric input bound to `min_price`. Filters by `price`.
- **Sort:** clickable column headers (`vwap`, `pct_change`, `sales_per_day`, `unit_volume_window`, `price_percentile`). Stored in `sort` URL param.

Table reuses `VirtualScroller` (already in this file). New `Sparkline` column slots before Price. `ConfidenceBadge` becomes the Quality column.

The legacy three pills (High Velocity / Rising / Falling) are removed from the page. The same intent is satisfied by sorting + the MarketMovers strip above the table.

## Workstream 3: Flip Finder enrichment

### Data flow change

`AnalyzerWorldView` gains a third resource: a `deep_scan_batch` call for the top N (e.g. 200) profitable rows after Pass-1. This is keyed on the world. The enriched data flows into a hashmap keyed `(item_id, hq, world_id)` and the row renderer looks each row up.

```rust
let pass1_data = build_profit_table(...);
let top_rows: Vec<(i32, u8, i32)> = pass1_data
    .iter()
    .map(|r| (r.item_id, r.hq as u8, r.cheapest_world_id))
    .take(200)
    .collect();
let deep_scan_resource = ArcResource::new(
    move || top_rows.clone(),
    |reqs| async move { get_resale_quality_batch(&reqs, 30).await }
);
```

This requires a new API endpoint that wraps `deep_scan_batch` for the FE:

`POST /api/v1/resale_quality` — body is `[(item_id, hq, world_id), ...]`, returns `Vec<ResaleQualityRow>` with `confidence_band`, `vwap_30d`, `sample_size_30d`, `launder_suspicion`, `sales_per_day_30d`. Cache for 60s. Soft-fail on CH unavailable (return empty list).

### New columns

| Col | Visibility | Source |
|---|---|---|
| HQ | always | unchanged |
| Item | always | unchanged |
| **Spark (24h)** | `md:` and up | new — `sparklines_batch` |
| Profit | always | unchanged |
| Profit/day | `lg:` (was always) | unchanged, moved breakpoint |
| ROI | always (default sort) | unchanged |
| Buy price | always | unchanged |
| **VWAP 30d** | `lg:` | new — DeepScan |
| **Sales/day** | `md:` | new — DeepScan (replaces "Avg sale time" column for accuracy) |
| **Quality** | always | new — `ConfidenceBadge` |
| World | `xl:` (was `lg:`) | unchanged |
| Datacenter | hidden by default behind More Filters | unchanged |
| Last sold | `md:` | unchanged |

"Avg sale time" stays accessible behind a column-toggle, but is no longer the default. `Sales/day` from CH is the honest velocity metric.

### Filters

Existing filter bar gains two entries:

- **Quality:** `Any · Medium+ · High only` pills, bound to `quality` URL param. Defaults to `Any`; selecting `Medium+` filters out `Low` and `Unknown` (i.e. items with no CH coverage).
- **Show suspicious:** toggle, bound to `show-suspicious`. Defaults off — `Unusable` and `launder > 0.7` rows are hidden. When on, those rows render with the `Unusable` chip and a subtle red row tint so the user sees what they're opting into.

Empty-state on first load: "No safe deals match — try enabling Show suspicious or relaxing filters." Distinguishes "no data" from "everything was filtered out by the junk filter."

The existing Pass-1 sniper-clamp / IQR / troll-listing guards stay. They protect the in-memory median from snipes; the new CH-backed quality filter protects against laundering — different threats, both still relevant.

## Workstream 4: Top Opportunities card

Rename `TopOpportunity` → `TopOpportunities` (component, file, all i18n keys with `top_opportunity_` → `top_opportunities_`, *plus* keep legacy keys aliased for one release to avoid breaking i18n compile if anything references them).

New layout in the home page:

```
┌─ TOP OPPORTUNITIES ─────────────────── View all in Flip Finder ─┐
│  🔥 Hempen Halfgloves    Buy 10  · Sell 120,000,008  · +1.2Bn  │   ← featured (existing card style)
│     Sample size 18 · 2 sold this week                            │
│  ────────────────────────────────────────────────────────────── │
│  🛡 Iron Ingot           Buy 240 · Sell 1,200       · +400%    │
│  🌿 Cloud Mica           Buy 8.2k · Sell 12k        · +46%     │
│  ⚒ Mythril Wristlets    Buy 12k · Sell 25k        · +108%    │
│  🪨 Raw Star Quartz      Buy 6k · Sell 14k         · +133%    │
└──────────────────────────────────────────────────────────────────┘
```

The first row keeps the prominent card style; rows 2–5 are compact (single line each). Each row links to `/item/{world}/{item_id}`.

**Junk filter applied**: `get_best_deals` already drops `Unusable`. The FE additionally drops `launder_suspicion > 0.7` after the DTO surfaces it (workstream 1). A "View all in Flip Finder" link routes to `/flip-finder/{world}?sort=roi&show-suspicious=0`.

**Empty state:** "No safe opportunities right now — try Flip Finder for more options."

## Workstream 5: Market Movers labeling

Three changes inside `market_movers.rs`:

1. Rename Volume tab label → "Units" (singular semantic match to the underlying `unit_volume`).
2. Add a `<Tooltip>` on each tab label with one sentence:
   - Rising: "Items whose price climbed the most in the last 24 hours."
   - Falling: "Items whose price dropped the most in the last 24 hours."
   - Units: "Items with the most units traded in the last 24 hours."
3. When the Units tab is active, the row's right-side metric (currently the `pct_change_24h` pill) renders the `volume_24h` count instead, and the sparkline color drops to neutral (sparkline color signals price direction, which is incidental on a volume-sorted list).

No DTO changes; pure FE / i18n.

## Architecture / files

```
ultros/
├── src/
│   ├── resale_quality_filter.rs          NEW — Keep enum + filter policy
│   ├── analyzer_service.rs               MOD — get_trends_v2, expose resale_quality batch
│   ├── web/
│   │   └── api/
│   │       ├── trends.rs                 MOD — accept ?window and ?show_suspicious
│   │       ├── best_deals.rs             MOD — apply ResaleQualityFilter at junk_threshold
│   │       └── resale_quality.rs         NEW — POST batch endpoint for Flip Finder
ultros-api-types/
├── src/
│   ├── trends.rs                         MOD — new TrendItem fields + items vec
│   └── resale_quality.rs                 NEW — DTO for the new endpoint
ultros-frontend/ultros-app/
├── locales/{en,fr,de,ja,cn,ko,tc}.json   MOD — new keys, see § Translation
├── src/
│   ├── api.rs                            MOD — expanded ResaleStatsDto, parameterized get_best_deals, get_resale_quality
│   ├── components/
│   │   ├── market_movers.rs              MOD — rename Volume → Units, tooltips, units-mode rendering
│   │   ├── top_opportunities.rs          RENAMED from top_opportunity.rs — multi-row card
│   │   └── confidence_badge.rs           (existing, reused)
│   └── routes/
│       ├── trends.rs                     REWRITE — new layout, CH-backed table
│       └── analyzer.rs                   MOD — new resource, new columns, Quality + show-suspicious filters
```

## Migration / rollout

- `TrendsData.items` is the new authoritative list; old buckets stay populated for one release to avoid breaking any external API consumer. Removed in the next release.
- `top_opportunity_*` i18n keys are kept and added-to with `top_opportunities_*` variants; the component reads only the new keys. The legacy keys get deleted after the component rename lands on main for a week.
- `/api/v1/best_deals` Phase-2 fields were already emitted; this PR is the first one to read them on the FE.

## Translation

New i18n keys (snake_case, all 7 locales, real translations not English stubs):

- `trends_window_label`, `trends_window_7d`, `trends_window_30d`, `trends_window_90d`
- `trends_show_suspicious`, `trends_show_suspicious_help`
- `trends_col_spark`, `trends_col_vwap`, `trends_col_pct_change`, `trends_col_sales_per_day`, `trends_col_units_window`, `trends_col_quality`, `trends_col_price`, `trends_col_percentile`
- `trends_min_sales_label`, `trends_min_price_label`, `trends_empty_filtered`, `trends_summary_sortby`
- `analyzer_col_spark`, `analyzer_col_vwap_30d`, `analyzer_col_sales_per_day`, `analyzer_col_quality`
- `analyzer_filter_quality_label`, `analyzer_filter_quality_any`, `analyzer_filter_quality_medium_plus`, `analyzer_filter_quality_high`, `analyzer_show_suspicious`, `analyzer_show_suspicious_help`, `analyzer_empty_all_filtered`
- `top_opportunities_title`, `top_opportunities_view_all`, `top_opportunities_empty`, `top_opportunities_buy`, `top_opportunities_sell`, `top_opportunities_profit`, `top_opportunities_roi`, `top_opportunities_sample_size`
- `market_movers_units` (replaces `market_movers_volume`), `market_movers_tab_rising_help`, `market_movers_tab_falling_help`, `market_movers_tab_units_help`

The old `market_movers_volume` and `top_opportunity_*` keys stay in the locale files until the next release (avoids `leptos-i18n` compile error if anything still references them at the moment of switchover).

## Testing

- **Server `ResaleQualityFilter`:** unit tests for each band × suspicious-toggle combination + threshold edge cases (exactly `0.7`).
- **`analyzer_service::get_trends_v2`:** golden test on a small fixture with a known-Unusable item, asserting it's excluded by default and included when `show_suspicious=true`.
- **`/api/v1/trends?window=7|30|90`:** smoke test each window returns 200 and the `items` field is populated when CH has data.
- **FE `ResaleStatsDto` deserialization:** add a round-trip serde test against the server DTO fixture, so future drift fails the test.
- **Manual / E2E** (no automation; run by hand):
  - `/trends/Gilgamesh` shows the new layout, window selector changes data, Show suspicious reveals previously hidden rows with chips.
  - `/flip-finder/Gilgamesh` no longer surfaces `Copper Wristlets 3 → 18.9M`. Toggle Show suspicious → it appears with the suspicious chip.
  - Home card shows 5 entries, the gil-trader row never makes it in, and `View all in Flip Finder` links with `show-suspicious=0`.
  - Market Movers Units tab tooltipped + renamed; switching tabs swaps the right-side metric.

## Open questions / risks

- **Cache invalidation on `/api/v1/trends?window=*`:** three separate cache keys. Acceptable — each refreshes independently on its own TTL (60s).
- **Sparkline payload size on trends:** 500 rows × 24 hourly floats ≈ 48 KB serialized. Acceptable for desktop, worth checking on mobile data; if it bites, we drop the per-row sparkline from trends and only render it in the analyzer (smaller N).
- **`pct_change_window` semantics:** defined as `price_now / vwap_window - 1`. A real point-in-time delta would need a windowed-bucket query — punted; we'll add it if users ask. The current definition is honest and labelable as "vs window average."
- **Spec scope:** four workstreams in one spec is on the upper edge of comfortable. They share `ResaleQualityFilter` and i18n batching, so splitting buys little. Implementation order below keeps each step landable independently.

## Implementation order

1. Cross-cutting infra: `ResaleQualityFilter` + i18n key add across 7 locales.
2. FE `ResaleStatsDto` parity + `get_best_deals` parameterization.
3. Market Movers labeling (smallest; lands first to ship the easy win).
4. Top Opportunities rebuild.
5. Trends server: new `TrendsData` shape + `get_trends_v2` + `?window` & `?show_suspicious` params.
6. Trends FE: rewrite `routes/trends.rs`.
7. Analyzer: new `/api/v1/resale_quality` endpoint + Flip Finder columns + filters.
8. `./check_ci.sh` + manual verification on dev server.
