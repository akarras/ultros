# Real Price metric — design

**Date:** 2026-06-07
**Status:** Approved (design), pending implementation plan
**Scope:** Item detail page only (`/item/{world}/{item_id}`)

## Problem

The item detail page features a **naive arithmetic mean** of the last ~200 raw sales as its
headline "Recent Average" ([`item_view.rs:307`](../../../ultros-frontend/ultros-app/src/routes/item_view.rs)).
A single money-laundering / gil-transfer sale destroys it: for item 9294 on Gilgamesh, 199 sales at
~16K plus one sale at 75,000,000 yields a displayed average of ~391K — ~24× the price the item
actually trades at. (Money laundering in FFXIV: a buyer transfers gil to a seller by purchasing a
deliberately overpriced — usually single-unit — listing.)

The page already computes a robust **median** right beside the average, but it's relegated to muted
subtext; the misleading mean is the headline.

## Goal

Replace the headline with a **Real Price**: an outlier-resistant estimate of what the item actually
trades for, computed entirely from the ~200 sales the page already loads. No new backend call, no
ClickHouse dependency (so it works for **every** item immediately, regardless of rollup coverage).

## Non-goals (deliberately out of scope)

- The analyzer / flip-finder — already filters via `analyze_sales` + IQR.
- The price-history chart — already has a user "Filter outliers" toggle.
- API / ClickHouse / `item_stats_window` changes; running the rollup backfill.
- Any server-side computation. This is a client-side, presentation-layer fix.

## Background: what already exists

- [`filter_outliers_iqr_in_place(&mut [i32]) -> &[i32]`](../../../ultros-frontend/ultros-app/src/math.rs)
  — an O(N), tested IQR inlier filter (Q1−1.5·IQR, Q3+1.5·IQR). Handles `<4` samples (returns
  input) and zero-IQR (returns all equal values, drops far outliers). **We reuse this primitive.**
- [`analyze_sales`](../../../ultros-frontend/ultros-app/src/analysis.rs) — the analyzer's filtered
  average, but it operates on the API `SaleData` shape (price + date, grouped by hq), **not** the
  item page's `CurrentlyShownItem.sales: Vec<SaleHistory>`. We do **not** call it directly; we reuse
  only the `filter_outliers_iqr_in_place` primitive.
- Vendor price is already available client-side on this page via
  `tracked_data().items.get(&ItemId(item_id))` (`price_mid` / `price_low`), used today for the
  vendor source callout ([`item_view.rs:380`](../../../ultros-frontend/ultros-app/src/routes/item_view.rs)).
- The backend ClickHouse pipeline (`ultros-clickhouse/src/rollups.rs`) already computes a
  launder-resistant cleaned VWAP + percentiles + `launder_suspicion_pct` + `confidence_band`, but
  the item page does not consume it and (per project notes) `item_stats_window` covers only ~7% of
  items because the backfill was never run in prod. This is why the client-side approach is chosen.

## Data available

`CurrentlyShownItem.sales: Vec<SaleHistory>`
([`ultros-api-types/src/sale_history.rs:6`](../../../ultros-api-types/src/sale_history.rs)). Each
`SaleHistory` carries `price_per_item: i32`, `quantity: i32`, `hq: bool`, `sold_date` — everything
the metric needs.

## The metric

### Per-quality algorithm

Given the recent sales for one quality bucket (NQ or HQ) as `(price, qty)` pairs and an optional
vendor unit price `V`:

1. **Vendor guard** (only when `V` is present and `> 0`): drop any sale with `qty == 1` and
   `price > 100 · V`. This is the backend's single strongest launder signal — an absolute anchor
   that catches outliers so large they would distort even the quartiles.
2. Let `n` = remaining sale count.
   - `n == 0` → **no estimate** for this quality (renders "No data").
   - `n < 4` → estimate = **median** of remaining prices. (IQR needs ≥4 samples; the median stays
     launder-robust at tiny `n` — e.g. `[16K, 16K, 75M]` → 16K, whereas the mean would be ~35M.)
   - `n ≥ 4` → estimate = **mean of `filter_outliers_iqr_in_place(remaining)`** (the robust
     filtered mean).
3. Emit `{ value, used, total, excluded = total − used }`, where `total` is the original count for
   the quality (before the vendor guard) and `used` is the count the final value was computed from.

### Quality handling

- Compute **NQ and HQ independently**. Never average the two together — bimodal markets (e.g. NQ
  16K, HQ 50K) would otherwise yield a meaningless ~33K.
- The **headline** is the quality with more recent sales ("primary market"); on an exact tie,
  prefer NQ.
- The other quality is shown as a secondary value **only if it has ≥ 4 sales** (enough to be more
  than noise).

### Worked example (item 9294)

199 NQ sales @ ~16K + 1 NQ sale @ 75M, qty 1. If vendor item: vendor guard drops the 75M. If not:
`n = 200 ≥ 4`, IQR upper fence sits just above 16K, the 75M is in the upper tail and is dropped.
Either way Real Price ≈ 16K, `used = 199`, `excluded = 1`.

## Architecture (isolation)

### Pure function — `ultros-frontend/ultros-app/src/analysis.rs`

A Leptos-free, unit-testable function alongside the existing `analyze_sales`/`SaleSummary`:

```rust
/// One quality's robust price estimate plus the sample accounting behind it.
pub struct RealPriceEstimate {
    pub value: i32,
    pub used: usize,
    pub total: usize,
    pub excluded: usize,
}

/// NQ / HQ estimates computed independently. `primary()` returns whichever
/// quality had more sales (the headline); `secondary()` the other, if it has ≥4 sales.
pub struct RealPriceBreakdown {
    pub nq: Option<RealPriceEstimate>,
    pub hq: Option<RealPriceEstimate>,
}

/// `samples`: (price_per_item, quantity, hq) for the page's recent sales.
/// `vendor_price`: unit vendor price if the item is vendor-sold, else None.
pub fn real_price(samples: &[(i32, i32, bool)], vendor_price: Option<i32>) -> RealPriceBreakdown;
```

`RealPriceBreakdown` exposes helpers `primary() -> Option<(bool /*is_hq*/, RealPriceEstimate)>` and
`secondary() -> Option<(bool, RealPriceEstimate)>` so the view stays declarative. Exact field/method
names may be refined during implementation; behavior above is fixed. The function reuses
`filter_outliers_iqr_in_place` and contains the median fallback.

### View wiring — `ultros-frontend/ultros-app/src/routes/item_view.rs`

- In `MarketStatsPanel`, replace the naive `avg_price` / `median_price` closures
  ([`item_view.rs:307-327`](../../../ultros-frontend/ultros-app/src/routes/item_view.rs)) with a
  single `real_price(...)` call. Build `samples` via
  `data.sales.iter().map(|s| (s.price_per_item, s.quantity, s.hq))`; read `vendor_price` from
  `tracked_data()` (mirroring the existing vendor-callout lookup).
- The raw arithmetic mean and median are retained **only** as a demoted muted line (transparency),
  computed inline as today.

## Display — the bottom-left stat card

Current card at [`item_view.rs:570-584`](../../../ultros-frontend/ultros-app/src/routes/item_view.rs)
("Recent Average" headline + median subtext). New layout (card stays the same size, one of the 2×2
grid):

- **Label:** "Real Price", with a small NQ/HQ tag indicating the headline quality.
- **Headline:** primary-quality `value` rendered with `<Gil>`.
- **Secondary value** (when present): the other quality's `value`, compact (e.g. "HQ ~b").
- **Basis line** (muted): `{{used}}/{{total}} sales` — surfaces a thin or heavily-filtered sample,
  a lightweight stand-in for the backend's `confidence_band`.
- **Demoted raw values** (muted, for transparency, per approval): reuse existing `recent_average`
  and `median_label` keys — "Recent Average M · Median N" — no longer the headline.
- Empty sales → "No data" (existing behavior preserved).

## i18n (CLAUDE.md: every key in all 7 locales — en, fr, de, ja, cn, ko, tc)

- **New keys:**
  - `real_price` → "Real Price"
  - `real_price_basis` → "{{used}}/{{total}} sales" (interpolation params `used`, `total`)
- **Reused (already present):** `recent_average`, `median_label`, `no_data`, `nq`, `hq`.

Provide a real translation per locale, not an English stub; flag any genuinely uncertain ones in the
PR for a native-speaker pass.

> Naming: "Real Price" is the user's term (the analyzer's median field is even documented as the
> "realistic seller estimate"). "Fair Price" / "Typical Price" are acceptable alternates if
> preferred — a label-only change.

## Edge cases & honest limitations

- **Sparse data** (`n < 4`): median, not mean. Empty: "No data".
- **Non-vendor items:** no vendor anchor; rely on IQR alone (fine for the common single-outlier case).
- **Known limitation:** IQR + vendor-anchor robustly handles the *common* case — a few outliers
  among many genuine sales (the 75M case is trivial). It does **not** fully handle an item where
  **> 25% of recent sales are laundering** *and* there is no vendor anchor, because the quartiles
  themselves shift. That pathological case is precisely what the backend ClickHouse pipeline
  (relative-to-median + MAD layers, `launder_suspicion_pct`) is built for. The documented future
  upgrade is to consume `item_stats_window` here once the backfill has been run — explicitly **not**
  built now to avoid over-engineering and the ~7% coverage cliff.

## Testing

Unit tests on the pure `real_price` (native `cargo test`, no Leptos/WASM):

1. **The headline case:** 199 × 16K + 1 × 75M (qty 1) → ≈16K; `excluded == 1`.
2. **Vendor guard:** qty-1 sale > 100× vendor dropped; same sale at qty 2 retained.
3. **Small sample:** `[16K, 16K, 75M]` (n=3) → median 16K (not the ~35M mean).
4. **All-equal:** every sale identical → that value; `excluded == 0`.
5. **HQ/NQ split:** mixed NQ≈16K + HQ≈50K → two estimates, no cross-averaging; primary = larger
   bucket.
6. **Secondary threshold:** secondary quality with < 4 sales is omitted from `secondary()`.
7. **Empty:** no sales → both `None`.

Existing `filter_outliers_iqr_in_place` and `analyze_sales` tests must continue to pass.

## Build / verification notes (worktree)

Per project notes, building/testing in a `.claude/worktrees/` worktree needs the game-data submodule
initialized and `CARGO_TARGET_DIR` pointed at the main repo's warm target. Before committing, run
`./check_ci.sh` (`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`); if
submodule init is blocked, at minimum run `cargo fmt --all -- --check` and note it.

## Files touched

| File | Change |
|---|---|
| `ultros-frontend/ultros-app/src/analysis.rs` | New `real_price` fn + `RealPriceEstimate`/`RealPriceBreakdown` types + unit tests |
| `ultros-frontend/ultros-app/src/routes/item_view.rs` | Wire `real_price` into `MarketStatsPanel`; restyle the bottom-left card; demote raw avg/median |
| `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json` | Add `real_price`, `real_price_basis` |
