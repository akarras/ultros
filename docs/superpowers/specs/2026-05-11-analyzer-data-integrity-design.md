# Analyzer Data Integrity & Guidance — Design

**Date:** 2026-05-11
**Status:** Awaiting user review
**Scope:** Tier 1 of a three-tier improvement plan for the Flip Finder analyzer. Fix the math that makes the default ROI sort surface ~71% 1-gil price-war rows, then re-tune presets so the landing experience guides users to realistic flips. Tier 2 (confidence column, per-row explainer) and Tier 3 (onboarding wizard, unmet-demand tab) are sketched as a roadmap appendix but out of scope for this spec.

---

## Why

I replayed the live `/api/v1/recentSales/Goblin` + `/api/v1/cheapest/{Goblin,North-America}` payloads through the current [analyzer.rs](../../ultros-frontend/ultros-app/src/routes/analyzer.rs) profit logic. Concrete findings:

- **Default ROI sort is dominated by sniper listings.** Of the top 500 rows by ROI: 357 (71%) have a buy price of 1 gil, 481 (96%) have a buy price ≤ 100 gil. A new user lands on Flip Finder and sees a wall of unscalable 99,999%-ROI rows.
- **HQ contamination of NQ estimates.** `compute_summary` folds HQ sale prices into the NQ row's `min_price` ([analyzer.rs:104-111](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:104)). 5,881 NQ rows have an estimated sell price < 50% of their own NQ sale average — the analyzer is *under-selling* the honest flips while the dishonest ones rank #1.
- **No troll-listing guard.** 4,969 region-vs-world price gaps are produced by 999,999,999-gil joke listings being treated as the world floor.
- **`estimated_sale_price = min(...)`** is the worst-case statistic. Median is the realistic seller price; current code uses `min` of the last six sales clamped against the world floor.
- **Velocity ignores time since the most-recent sale.** 83 of the top 500 by ROI haven't sold in 30+ days; 25 haven't sold in 90+. The "average sale duration" computes a cadence across the last 6 sales but never penalises the gap between now and the newest one.

These are arithmetic bugs and defaulting choices — no API changes, no DB changes, no new dependencies.

## Non-goals

- Adding new API endpoints. The `/api/v1/best_deals/{world}` endpoint exists but is not used by Flip Finder; we leave that alone.
- Changing the server-side sales window. The API still caps at six sales per item; we work within that constraint.
- Replacing the `filter_outliers_iqr_in_place` toggle. It stays as an opt-in pre-filter on `avg_price`; we add a separate sanity-clamp that applies unconditionally.
- Building the Confidence column, per-row explainer, onboarding wizard, or unmet-demand tab. Those are Tier 2/3, sketched in the appendix.
- Stack depth / alternate buy-worlds. Needs an API change; deferred.

## Changes

### 1. Drop HQ→NQ contamination

[`compute_summary`](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:97) currently accepts an `hq_data: Option<&SaleData>` and includes those prices in `min_price` when computing an NQ row. Remove the parameter; `min_price` is computed only from the row's own sales. The caller in [`ProfitTable::new`](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:218-222) is simplified accordingly.

Rationale: HQ and NQ are separate markets. Mixing them lets a single 1-gil HQ sale poison every NQ row for the same item.

### 2. Replace `min` with median for `estimated_sale_price`

In `SaleSummary`, add `median_price: i32`. Compute it as the median of the row's own sales (already only six values; a sort-and-pick is fine — no perf concern).

Change [`estimated_sale_price`](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:225-230) to:

```rust
let estimated_sale_price = match world_cheapest.get(&key) {
    Some((world_floor, _)) => summary.median_price.min(*world_floor),
    None => summary.median_price,
};
```

Rationale: a realistic seller prices at or near the recent median, not at the absolute floor. The min of the world's current listings still caps it (you can't sell above someone undercutting you), but the *historical* floor — which can be a single sniper — no longer drives the estimate.

Keep `min_price` in the struct; it's still useful for Tier 2 tooltips ("worst recent sale was X").

### 3. Troll-listing guard on the world floor

Add a sanity check after merging cross-region listings in [`ProfitTable::new`](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:174-201): if `world_cheapest_price > 50 * median_recent_sale`, drop that world-floor entry and treat the row as if there's no world floor (i.e. fall through to `summary.median_price`).

50× is generous enough to keep legitimate ultra-rare items (e.g. a desynth where the only recent sale was a lowball private trade) while filtering the joke listings I saw in the data — 999,999,999 against a 3,000-gil median.

Implementation note: this needs the row's recent sales in scope when consulting the world-floor map. The cleanest path is to fold the clamp into the same iterator over `sales.sales` that already builds each `ProfitData`, instead of pre-merging maps. That's a small refactor; the inner shape doesn't change.

### 4. Sniper-sale guard on `min_price` / `median_price`

When building `SaleSummary`, drop any individual sale whose `price_per_unit < 0.1 * (raw median of the six)` before computing the final stats. With only six sales, dropping one outlier still leaves five.

This is independent from the existing IQR outlier toggle. The IQR toggle is symmetric and opt-in; this clamp is asymmetric (low-side only), unconditional, and only fires on the obvious sniper cases.

### 5. Velocity that respects gap-since-last-sale

Change [`avg_sale_duration`](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:134-137) from "(oldest_sale - now) / len" to:

```rust
let newest = sales.first()?.sale_date;
let oldest = sales.last()?.sale_date;
let span_ms = (now - oldest).num_milliseconds();
let avg_sale_duration = Duration::milliseconds(span_ms / sales.len() as i64);
```

The numerator stays unchanged (the current code already takes `|last - now|`, equivalent), but we also expose `days_since_last_sale = now - newest` as a new field on `SaleSummary`. The Tier 1 visible change is one new column in the analyzer table: **"Last sold"** showing `2d ago`, `30d ago`, etc.

`profit_per_day` continues to use `avg_sale_duration` so existing URL filters keep working.

A follow-up filter card "Last sold within" (defaulting unset) plugs into `days_since_last_sale`. This is the single highest-leverage filter for separating live items from dead ones.

### 6. Preset re-tune

Current presets ([analyzer.rs:1190-1200](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:1190)):
- "300% return" → `?next-sale=7d&roi=300&profit=0&sort=profit&`
- "500% return" → `?next-sale=1M&roi=500&profit=200000&`
- "100k profit" → `?profit=100000`

All three let the 1-gil rows through because none of them sets a minimum buy price. New presets:

- **"Realistic flips" (new default landing CTA)** — `?max-price=&min-buy=5000&last-sold=7d&roi=30&sort=profit-per-day` — the answer to "what should I do with my next 30 minutes." Min buy 5k filters out the price-war noise; last-sold-within-7d filters out dead items; sort by ppd matches the actual decision.
- **"Big-ticket flips"** — `?min-buy=100000&last-sold=14d&roi=20&sort=profit`
- **"Volume flips"** — `?min-buy=1000&last-sold=3d&sort=profit-per-day`
- Keep the existing "100k profit" preset; it's a useful third archetype.

This needs one new URL query param + filter card: **Minimum buy price** (`min-buy`). The existing "Maximum Budget" (`max-price`) is the upper bound; the new param is the lower bound and is what actually solves the 1-gil problem.

### 7. New filter cards

Two new `FilterCard` entries in the existing grid, mirroring the patterns at [analyzer.rs:421-644](../../ultros-frontend/ultros-app/src/routes/analyzer.rs:421):

- **Minimum buy price** — query param `min-buy`, integer, default unset.
- **Last sold within** — query param `last-sold`, humantime duration (e.g. `7d`), default unset. Reuses the same `parse_duration` parsing as the existing `next-sale` predicted-time field.

Both get filter chips in the active-filters bar and "Clear all" wiring.

## File touch list

- [ultros-frontend/ultros-app/src/routes/analyzer.rs](../../ultros-frontend/ultros-app/src/routes/analyzer.rs) — all logic and UI changes land here.
- [ultros-frontend/ultros-app/locales/en.json](../../ultros-frontend/ultros-app/locales/en.json) and sibling locale files — strings for the two new filter cards, the new "Last sold" column, and the renamed presets. Default English content goes in; other locales fall back gracefully via the i18n setup.

No changes to `ultros-api-types`, `ultros-db`, the Axum routes, or any other crate.

## Testing

Logic changes are unit-testable inside `analyzer.rs`:

- `compute_summary` with HQ-only input, NQ-only input, and an NQ-with-HQ-sniper case → estimated stays anchored to NQ.
- `compute_summary` with a six-sample series that contains one obvious sniper (10% of median) → that sample is excluded from `min_price` and `median_price`.
- `ProfitTable::new` with a 999,999,999 troll listing in the world map → the row falls through to `median_price` instead of producing a $999M profit.
- `days_since_last_sale` math on synthetic timestamps.

UI changes are smoke-tested manually post-deploy on Goblin (Aether):

1. Land on `/flip-finder/Goblin` — verify the default preset shows no buy-price-of-1 rows.
2. Apply each new preset — verify the URL → filter wiring round-trips.
3. Sort by ROI explicitly — verify 1-gil rows are now gone *because of the troll/sniper clamp*, not just hidden by the preset.

A regression check on the existing E2E harness in `integration/` is enough; we are not adding new pages.

## Risk

Low. Every change is local to `analyzer.rs`, gated by either deterministic math or a query param that defaults to "unset" (preserves existing URLs). The only user-visible default change is the landing preset, which is opt-in via a CTA the user clicks — direct URLs and bookmarks keep working.

The 50× troll-listing threshold is the only "tunable magic number" in the design; if it turns out to filter legitimate rare-item rows on production data, it's a one-line change to raise.

## Tier 2 / Tier 3 roadmap (out of scope for this spec)

Sketched here so the next spec inherits a clear backlog, not designed:

**Tier 2 — Trust signals.**
- Confidence column folding sample size + days-since-last-sale + price coefficient-of-variation into low/med/high.
- Per-row explainer popover: "Estimated sell = median of last 6 sales (X). Your world floor: Y. Cheapest off-world: Z @ Worldname. Last sale: N days ago."
- Stack depth / alternate buy-worlds — needs an API addition to return top-N listings at the floor per item.

**Tier 3 — Guided flipper.**
- First-visit onboarding step: budget, time horizon, home world → writes URL filters. Plugs into the recently-shipped `/welcome` route.
- "Unmet demand" tab — items with recent sales but no current listings (276 on Goblin in today's snapshot).
- "Avoid my own retainers" filter using `/api/v1/user/retainer` for logged-in users.
