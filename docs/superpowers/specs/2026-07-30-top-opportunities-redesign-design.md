# Top Opportunities card — honest ranking and a legible card

Date: 2026-07-30
Component: `ultros-frontend/ultros-app/src/components/top_opportunity.rs`
Backend: `ultros/src/analyzer_service.rs`, `ultros/src/web/api/best_deals.rs`

## Problem

The home-page Top Opportunities card is the first substantive thing a user with a
home world sees, and it currently advertises trades that cannot happen. Observed
on a 736px viewport:

| item | buy → sell | ROI | profit |
| --- | --- | --- | --- |
| Archeo Kingdom Partisan (rendered as "Arc") | 3,608 → 252,000,016 | 6,984,380% | 251,996,408 |
| Hard Leather Pot Helm | 1 → 133,333,336 | 13,333,334,016% | 133,333,335 |
| Hempen Coif | 5 → 42,000,000 | 839,999,872% | 41,999,995 |
| Occult Bracelet of Blood | 10,000,001 → 39,999,966 | +300% | 29,999,965 |
| Golden Beaver | 89,999,980 → 111,599,989 | +24% | 21,600,009 |

All five are gil transfers, not trades. FFXIV caps direct trades at 1,000,000 gil,
so a player moving currency between their own characters — or paying for something
out-of-band — lists a worthless item at an enormous price and buys it from
themselves. The market board is the only viable path for a single-account player,
which makes this a permanent, structural feature of the data rather than noise that
will wash out.

Three distinct faults produce the screenshot:

### Fault 1 — the sort key selects for laundering

`get_best_resale` (`analyzer_service.rs:1126`) computes:

```
est_sale_price = calculate_valuation(median_of_recent_sales, cheapest_home_listing)
profit         = est_sale_price - cheapest_regional_listing
```

then sorts by **raw absolute profit** descending (`analyzer_service.rs:1197`). The
single most extreme laundering trade on a world is, by construction, the row with
the largest absolute profit. The outlier is not leaking past the ranking; it *is*
what the ranking optimizes for.

The median makes it worse. `prices.select_nth_unstable(len / 2)`
(`analyzer_service.rs:1111`) picks the **upper** middle on even-length input, so a
two-sale laundering pair resolves to the higher of the two. The comment directly
above it asks "If even, pick the lower one to be conservative?" and then does the
opposite.

### Fault 2 — the deep-scan budget is spent on junk

`DEEP_SCAN_TOP_N = 200` (`analyzer_service.rs:1190`) selects which rows get
ClickHouse enrichment — by raw profit, i.e. by the compromised metric. On a busy
world the top 200 is dominated by laundering, so genuinely good deals never have
their quality data fetched. The quality filter sits downstream of the thing it is
meant to correct.

`ResaleQualityFilter` cannot rescue this. It drops `ConfidenceBand::Unusable` or
`launder_suspicion > 0.7`; an item with a handful of 30-day sales lands at `Low` or
`Unknown` and never crosses either bar.

### Fault 3 — the card's layout and hover styling

- **Truncation.** `sm:grid-cols-[auto_1fr_auto]` (`top_opportunity.rs:145`) puts
  the name (`1fr`, `truncate`) and the profit (`auto`, `shrink-0`, `text-3xl`
  mono) in the same row. A nine-digit number claims ~250px, collapsing the name
  column until "Archeo Kingdom Partisan" renders as "Arc".
- **Hover.** `style/tailwind.css:814` applies `hover:underline` plus a brand-tinted
  background to every `<a>` that is not a button. The featured deal is one large
  `<a>`, so hovering underlines all eleven text nodes simultaneously.
- **The route is missing.** `ResaleStats.world_id` is the *source* world — where
  the cheap listing is (`analyzer_service.rs:1199` is explicit). Both `FeaturedDeal`
  and `CompactDeal` receive it and neither renders it. The card describes a
  cross-world arbitrage without naming the other world.

## Goals

- The featured deal is one a player could actually execute.
- A reader who has never used Ultros can infer the mechanic from the card alone.
- Item names are never truncated by their own profit figure.
- The card and the Flip Finder it links to agree about the same item.

## Non-goals

- Changing the profit model itself (tax, cross-region rules, `calculate_valuation`'s
  listing-undercut behavior). Only the median tie-break changes.
- Fixing the ClickHouse ingest gap (see Finding 1) — treated as a constraint.
- Changing the Discord `/analyze` command's output.
- Server-side persistence of any kind.

## Findings that shape the design

### Finding 1 — ClickHouse covers ~7% of traded items

From `docs/superpowers/specs/2026-07-27-flip-finder-redesign-design.md`: a
stratified 150-item sample against `/api/v1/resale_quality/Gilgamesh` at
`window_days=30` returned 11 rows. Coverage is flat across price bands, so it is
not a price-correlated filter — the gap is upstream in the ClickHouse `sales`
ingest.

**Consequence:** `vwap_30d`, `sample_size_30d`, `confidence_band`, and
`launder_suspicion` cannot gate default behavior. Any design resting on them
silently no-ops on ~93% of rows. ClickHouse is a refinement layer.

An earlier draft of this design proposed a VWAP sanity cap
(`est_sale_price = min(est, vwap_30d × 1.5)`) and a sample-size gate as the primary
defense. Both are dropped for this reason.

### Finding 2 — `get_best_resale` now has only two consumers

The Flip Finder rewrite (#991) moved that page onto `get_cheapest_listings_live` +
`get_recent_sales_for_world`, computing its metrics client-side. Remaining callers:

- `ultros/src/web/api/best_deals.rs:90` — this card
- `ultros/src/discord/ffxiv/analyze.rs:66` — the Discord `/analyze` command

Changes here do not ripple across the app, and the "fix it once for every surface"
argument no longer applies. The Discord command is the compatibility surface to
respect.

### Finding 3 — velocity already defeats laundering, and it has 100% coverage

Flip Finder's shipped default query is `sort=profit-per-day`, velocity ≥ 0.2/day,
≥2 sales in buffer. Its spec states the mechanism plainly: a 2-gil item with a
fabricated 213M sale price has no real velocity, so the floor drops it. No launder
heuristic, no special case.

Velocity derives from the `RecentSales` 6-sale buffer, present on 100% of rows.
89.2% of Gilgamesh items carry a full 6-sale buffer.

### Finding 4 — a shared metrics vocabulary exists

`ultros-frontend/ultros-app/src/analysis.rs` now exports `velocity_per_day`,
`price_drift_pct`, `derived_confidence`, `get_sales_cadence`, `roi_badge_class`,
and `ROI_DISPLAY_CEILING = 100_000`, with `SalesCadenceBadge` in
`components/sales_cadence_badge.rs`. `real_price` implements a launder-resistant
estimate whose absolute anchor is a **vendor guard**: drop `qty == 1` sales priced
above 100× the item's `price_mid`.

The card must consume these rather than grow parallel definitions.

### Finding 5 — ROI was deliberately demoted

#991 moved ROI from required-and-default-sort to an off-by-default column, on the
grounds that it is the wrong default when retainer slots rather than capital are
the constraint. The card currently leads with ROI in the featured slot and shows it
on every compact row.

## Design

### 1. Eligibility — three layers, ordered by coverage

All three run in **pass 1**, before the `DEEP_SCAN_TOP_N` truncation at
`analyzer_service.rs:1192`. This is what fixes Fault 2: the deep-scan budget is
spent on rows that already qualify.

**Layer 1 — vendor anchor.** Reject any row whose `est_sale_price` exceeds
`100 × price_mid` for vendor-sold items. Reuses the multiple already established by
`real_price`. `xiv-gen-db` is already a dependency of the `ultros` crate
(`ultros/Cargo.toml:20`), so `price_mid` is available server-side.

This alone removes Hard Leather Pot Helm and Hempen Coif — vendor-sold starter
gear, where a 42M valuation is absurd against a ~50 gil vendor price. No velocity
or ClickHouse data required.

**Layer 2 — velocity floor.** Require `velocity_per_day >= 0.2` and
`buffer_sale_count >= 2`. Identical thresholds to Flip Finder's default query, so
the handoff link (below) opens a page with the same policy applied.

This removes Archeo Kingdom Partisan and Golden Beaver — neither is vendor-sold,
and neither has a real trade cadence.

**Layer 3 — ROI ceiling.** Reject `return_on_investment > 5000%`.

This covers the velocity floor's known blind spot: a burst of laundering sales
compressed into a short span produces a high derived velocity, because the buffer
holds the six *most recent* sales and the span shrinks accordingly. The ceiling is
set at 5000% rather than something tighter because a legitimate cheap-item flip can
genuinely clear 1000% — with `min_profit = 10_000`, a 715 → 10,715 gil flip is both
real and a 1400% return. 5000% still kills 6,984,380% and 13,333,334,016% by
several orders of magnitude.

**ClickHouse, where present (~7%),** refines but never gates: its `sales_per_day`
supersedes the derived rate and its `ConfidenceBand` supersedes the derived band,
matching the precedence Flip Finder uses.

### 2. Correctness fixes at the source

- **Conservative median.** `analyzer_service.rs:1111` becomes
  `prices.select_nth_unstable((len - 1) / 2)`. Odd-length behavior is unchanged
  (`len=3` → index 1 either way); even-length now picks the lower middle, matching
  the stated intent of the comment above it. `len=1` is index 0 under both.
- **ROI display clamp.** Wherever ROI is rendered, clamp at `ROI_DISPLAY_CEILING`.
  The card drops ROI entirely (§4), so this applies to the Discord command's
  formatting.

### 3. API changes

`ResaleOptions` gains:

```rust
pub(crate) min_velocity_per_day: Option<f32>,
pub(crate) min_buffer_sales: Option<u8>,
pub(crate) max_roi: Option<f32>,
```

All `None` by default, so `ultros/src/discord/ffxiv/analyze.rs` is unaffected. The
vendor anchor is unconditional — it rejects arithmetically impossible valuations,
not merely aggressive ones.

`BestDealsQuery` gains `min_velocity`, `min_buffer_sales`, and `max_roi` query
parameters threading to the above.

`ResaleStatsDto` gains:

```rust
pub(crate) velocity_per_day: Option<f32>,
pub(crate) buffer_sale_count: u8,
pub(crate) recent_price_low: i32,
pub(crate) recent_price_high: i32,
```

Serde tolerates unknown fields, so adding these is backward compatible.

Velocity is computed server-side from the same `SaleHistory` buffer that already
produces `sold_within`: `count / max(span_days, MIN_VELOCITY_SPAN_DAYS)` where
`span_days = (now - oldest_sale)`. This is the same quantity
`analysis::velocity_per_day` derives via `avg_sale_duration`, and the guard against
a zero-hour span (six listings cleared in one action) must be preserved. Computing
it server-side rather than in the card also keeps it out of hydration's way — no
client-side `now`.

`recent_price_low` / `recent_price_high` are the min and max of the same filtered
price buffer used for the median.

The card requests:

```
min_profit=10000  filter_sale=Week  limit=20  show_suspicious=0
min_velocity=0.2  min_buffer_sales=2  max_roi=5000
```

and keeps its existing frontend-side `launder_suspicion <= 0.7` defense-in-depth
pass.

### 4. Card anatomy

**Featured deal.** The name and the profit no longer share a row — this is the
structural fix for truncation. Vertical order:

1. Icon (`IconSize::Large`) + item name, name on its own line, two-line clamp
   rather than `truncate`.
2. Route line: `Buy on {source} → list on {home}`, resolved from the existing
   `LocalWorldData` context via `lookup_selector(AnySelector::World(id))`
   (`ultros-api-types/src/world_helper.rs:242`). No API change needed.
3. Divider.
4. Left: `PROFIT EACH` label, profit at `text-2xl` mono, `buy → sell` beneath as
   supporting detail.
   Right: `SalesCadenceBadge` (`compact=true`, reused as-is) over the recent-sales
   range.

The recent-sales range renders `last 6: {low}–{high}` from `recent_price_low/high`.
Where ClickHouse has a row, the same slot renders `30d avg {vwap}` instead. The
buffer form is the default because it has 100% coverage; per Finding 1 the CH form
cannot be.

**Compact rows.** Two lines, unchanged in structure:

- Line 1: item name · profit
- Line 2: `{source_world} · {velocity}/day` · `buy → sell`

ROI is removed from both the featured slot and the compact rows, per Finding 5.
This is what frees the space the route line and cadence badge occupy.

**Hover.** Add `.card-link` to the exclusion list on `style/tailwind.css:814`, and
apply it to both anchors. Hover becomes a background tint plus an underline on the
item name only.

**Header.** Drop the `🔥` emoji from the section title.

### 5. States

**Loading.** Skeleton must match the new anatomy or the card jumps on settle: a
taller featured block (icon + two text lines + divider + profit row) and taller
compact rows, which are now two lines. `LocalResource` does not execute on the
server, so SSR emits the skeleton and the first client render matches it.

**Empty.** Reachable in practice now that a velocity floor applies. Replaces the
current dead-end string:

> **Nothing worth flipping on {world} right now**
> Only items that actually sell show up here, so a quiet market means an empty card.
> → Browse everything in Flip Finder

The link targets `/flip-finder/{world}?sort=profit&vel=0` — floor removed — so the
empty state demonstrates that the filter exists, is deliberate, and is adjustable.

**Error.** Split from empty. `get_best_deals(...).await.ok()` currently collapses a
failed request into the same `None` as an empty result, rendering an outage as "the
market is quiet." Distinguish the `Err` arm and render "Couldn't load
opportunities."

**Partial.** Fewer than five eligible deals renders featured plus however many
compact rows exist. No padding rows.

**Handoff.** "View all in Flip Finder" links to
`/flip-finder/{world}?sort=profit&vel=0.2`. `SortMode` parses `"profit"`
(`analyzer.rs:348`) and the velocity floor round-trips as `?vel=`
(`analyzer.rs:759`), so Flip Finder opens ranked and filtered exactly as the card
was. Without this the card ranks by absolute profit while Flip Finder defaults to
profit-per-day, and the featured item can be nowhere near the top of the page it
hands off to.

### 6. i18n

New keys, added to **all seven** locale files with real translations per CLAUDE.md:

| key | English |
| --- | --- |
| `top_opportunities_route` | `Buy on {{source}} → list on {{home}}` |
| `top_opportunities_profit_each` | `Profit each` |
| `top_opportunities_recent_range` | `last 6: {{low}}–{{high}}` |
| `top_opportunities_vwap_30d` | `30d avg {{price}}` |
| `top_opportunities_empty_title` | `Nothing worth flipping on {{world}} right now` |
| `top_opportunities_empty_body` | `Only items that actually sell show up here, so a quiet market means an empty card.` |
| `top_opportunities_empty_cta` | `Browse everything in Flip Finder` |
| `top_opportunities_error` | `Couldn't load opportunities.` |

Removed from all seven locales: `top_opportunities_roi` (ROI no longer renders on
the card) and `top_opportunities_empty` (superseded by `_empty_title` +
`_empty_body`). Both deletions happen in the same change that removes their last
call site.

Retained: `top_opportunities_buy` / `top_opportunities_sell`. The visual design
drops their visible labels, but `12,800 → 21,450` is meaningless to a screen
reader, so they become the `aria-label` on that pair. `.jules/palette.md` records
three separate prior misses of exactly this kind.

## Testing

**Backend unit tests** (`analyzer_service.rs`):

- Conservative median: even-length input picks the lower middle; odd-length and
  single-element behavior unchanged.
- Vendor anchor: a valuation above 100× `price_mid` is rejected; at or below is
  kept; non-vendor items (`price_mid == 0`) are unaffected.
- Velocity floor and `min_buffer_sales` reject below threshold, keep at threshold.
- ROI ceiling rejects above 5000%, keeps at 5000%.
- All four gates run before truncation — assert a qualifying row ranked below 200
  by raw profit survives when 200+ non-qualifying rows outrank it.
- Zero-hour span does not divide by zero.

**Backend API tests** (`best_deals.rs`): new query parameters extract alongside the
existing ones, matching the existing `numeric_show_suspicious_is_accepted_with_other_params`
pattern.

**Frontend:** the featured card renders a full item name at 300px width without
truncation; the route line names the source world; empty and error states render
distinctly; ROI appears nowhere.

**CI:** `./check_ci.sh` before committing. Note that main moved the game data
submodule to 7.55 (`401c6c61`), so a fresh worktree may not have
`xiv-gen/ffxiv-datamining` populated and clippy will fail to compile `xiv-gen-db`.
Initialize with `--reference` against the main clone rather than `--depth=1`, which
leaves nested submodules empty. If submodule init is blocked, run
`cargo fmt --all -- --check` at minimum and note in the PR that clippy did not run.

## Residual risks

- **Burst laundering.** Six launder sales inside one hour yield a high derived
  velocity and pass Layer 2. Layer 3's ROI ceiling covers the common shape (near-zero
  buy price), and the vendor anchor covers vendor-sold items, but a non-vendor item
  bought at a plausible price and "sold" at a 20× plausible price within one burst
  would still pass all three. No 100%-coverage signal available today detects it.
  ClickHouse's `launder_suspicion` would, on the 7% of items it covers.
- **Threshold portability.** 0.2/day and ≥2 sales were validated against Gilgamesh,
  a high-population world. A low-population world may see the empty state more often
  than intended. The empty state is designed for this, but the thresholds should be
  re-checked against a small world before shipping.
- **Deep-scan ordering.** Gates now run pre-truncation, so the top 200 consists of
  qualifying rows — but it is still ordered by pass-1 profit, and ClickHouse
  refinement can still reorder within it. Only affects which rows get a `30d avg`
  instead of a `last 6` range, not eligibility.
