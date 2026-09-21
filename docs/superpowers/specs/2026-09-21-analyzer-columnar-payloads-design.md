# Columnar wire shapes for the analyzer bulk endpoints

Date: 2026-09-21
Status: approved
Follows: `2026-09-21-cheapest-listings-payload-design.md` (PR #1576)

## 1. Problem

After #1576 the analyzer pages' HTML drops from 1.25 MB to ~80 KB, because
the only thing serialized into it was the root `CheapestPrices` map. What
remains is what the page fetches after hydration. Measured on prod
(Adamantoise / Aether, 2026-09-21):

| Endpoint | rows | raw | brotli | backing |
|---|---|---|---|---|
| `recentSales/{world}` | 24,531 items, 140,345 sales | 9.1 MB | 1,206 KB | `AnalyzerService` in-memory |
| `sale_stats/{dc}?window=7` | 19,932 | 4.4 MB | 662 KB | ClickHouse, cached `Bytes` |
| `listing_stats/{dc}` | 21,951 | 3.6 MB | 456 KB | ClickHouse, cached `Bytes` |

The flip finder loads `recentSales` plus 2–5 cheapest boards (~1.5 MB br);
the recipe analyzer loads cheapest + `sale_stats` + `listing_stats`
(~1.3 MB br). `recentSales` alone is eight times the payload #1576 removed.
All three are row-of-objects JSON, so most of the raw bytes are repeated
field names, and the raw size is what the wasm client has to parse.

Columnar JSON simulated on the real prod bodies:

| Encoding | raw | gzip | brotli |
|---|---|---|---|
| recentSales rows | 8,919 KB | 1,218 KB | 1,206 KB |
| recentSales columnar (unix-second timestamps) | 2,564 KB | 829 KB | 816 KB |
| sale_stats rows | 4,302 KB | 658 KB | 662 KB |
| sale_stats columnar | 1,460 KB | 428 KB | 380 KB |
| listing_stats rows | 3,524 KB | 454 KB | 456 KB |
| listing_stats columnar | 931 KB | 311 KB | 292 KB |

## 2. Scope

Add an opt-in `?format=columnar` struct-of-arrays shape to the three
endpoints and switch the site's fetchers to it. The legacy row shape stays
the default for outside consumers. No caller of the in-memory row types
changes; conversion happens at the fetch boundary, exactly as #1576 does
for `/api/v1/cheapest`.

Out of scope (recorded as follow-ups in §7): timestamps as age-from-now,
dropping near-constant columns, trimming the 6-sales-per-item buffer.

## 3. Wire shapes

All three live in `ultros-api-types` next to their row type, derive
`Clone, Debug, PartialEq, Serialize, Deserialize, Default`, and implement
`From<Rows> for Columnar` and `From<Columnar> for Rows`. Decoding zips the
columns, so a malformed payload with unequal column lengths degrades to
the rows every column has rather than panicking (same contract as
`CheapestListingsColumnar`). The server emits rows in `(item_id, hq)`
order where the source already provides it; decoding does not depend on
the order.

### 3.1 `RecentSalesColumnar`

```rust
pub struct RecentSalesColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    /// Number of flattened sales belonging to row `i`.
    pub count: Vec<u32>,
    pub price: Vec<i32>,
    /// Unix seconds. `NaiveDateTime` on the row side is second-resolution.
    pub sold_unix: Vec<i64>,
}
```

Row `i` owns the next `count[i]` entries of `price`/`sold_unix`, in the
same newest-first order the row shape carries today. Decoding walks the
flattened columns with a cursor; if the columns run out before `count`
says they should, the remaining sales for that row (and every later row)
are simply absent. `sold_unix` converts back with
`DateTime::from_timestamp(secs, 0).naive_utc()`; an out-of-range value
falls back to the epoch rather than failing the whole payload.

### 3.2 `BulkSaleStatsColumnar`

One `Vec` per `ItemSaleStats` field, same names: `item_id, hq, min_price,
median_price, avg_price, num_sold, last_sold_unix, units_sold, vwap,
gil_volume, sales_per_day, confidence`. `confidence` is
`Vec<ConfidenceBand>`; it already serializes as a short string and keeps
its `#[serde(default)]`-style tolerance by being a normal column.

### 3.3 `BulkListingStatsColumnar`

One `Vec` per flat `ItemListingStats` field: `item_id, hq, alive_count,
alive_units, distinct_retainers, oldest_reviewed_unix, median_age_secs,
floor_alive`, plus

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub window: Option<Vec<ListingWindowStats>>,
```

`ListingWindowStats` is a 15-field nested object with enums inside.
Exploding it into columns buys little and complicates the decoder, so on
windowed requests it rides as one parallel column of objects; on
current-only requests the key is absent. Decoding: `window[i]` attaches to
row `i` when the column is present; a short `window` column leaves the
tail rows' `window` as `None`.

## 4. Server

Each handler gains a `format: Option<String>` query field. Exactly
`"columnar"` selects the new shape; anything else (including absent) keeps
the legacy shape. The comparison is case-sensitive, as in #1576.

- `recent_sales` is not cached. The handler builds the same `Vec<SaleData>`
  it does today and, for columnar, converts it with
  `RecentSalesColumnar::from` before `Json`. Cache-Control stays
  `max-age=30`.
- `sale_stats` and `listing_stats` cache pre-serialized `Bytes` behind
  `stats_cache::CacheKey { selector, window_days }` with single-flight
  refresh and stale fallback. `CacheKey` gains `columnar: bool` so each
  shape has its own slot. The `load_*` functions take the flag and
  serialize either the row struct or its columnar conversion. Two shapes
  requested for the same scope cost two ClickHouse loads; once the site
  switches, the row slot is only ever warmed by outside consumers.
  `cached_response` and its headers are unchanged. The `sale_stats`
  `window` validation and the `listing_stats` `WINDOWS` check are
  unchanged and run before the cache lookup as today.

No route changes; the query parameter is additive.

## 5. Client

In `ultros-frontend-core/src/api.rs`:

- `get_recent_sales_for_world` fetches `?format=columnar` as
  `RecentSalesColumnar` and maps to `RecentSales`.
- `get_sale_stats` fetches `?window={days}&format=columnar` as
  `BulkSaleStatsColumnar` and maps to `BulkSaleStats`.
- `get_listing_stats` / `get_listing_stats_window` fetch with
  `format=columnar` (and `window={days}` for the latter) as
  `BulkListingStatsColumnar` and map to `BulkListingStats`.

Function signatures and return types are unchanged, so no analyzer, kit or
component code moves. The 503-on-cold-window behaviour of
`get_listing_stats_window` is unaffected because the status is checked
before deserialization.

## 6. Testing

`ultros-api-types`, per columnar type:

- round-trip rows → columnar → rows preserves order and every field;
- exact JSON shape for a one-row payload;
- empty round-trips both ways;
- mismatched column lengths truncate to the shortest;
- `RecentSalesColumnar` only: ragged `count` (0, 3, 1 sales) round-trips,
  and `count` claiming more sales than the flattened columns hold yields
  the sales that exist without panicking;
- `BulkListingStatsColumnar` only: windowed payload round-trips with the
  `window` column and a current-only payload serializes without the key.

`ultros` handlers:

- `format` matching accepts only `"columnar"` (shared helper or per-handler
  test, as in `cheapest_per_world`);
- `CacheKey` with `columnar: true` and `false` are distinct keys;
- the recent-sales conversion emits `count` and flattened columns in
  `item_map` order.

Manual: local serve, hit each endpoint with and without `format=columnar`,
confirm equal-length columns; load `/analyzer/...` and
`/recipe-analyzer/...`, confirm the network panel shows exactly the
columnar requests and the tables render identically. Run
`./check_ci.sh` before committing.

## 7. Follow-ups (not in this change)

- `recentSales.sold_unix` as minute-resolution age from a `now` field:
  ~816 → ~605 KB br, but changes the DTO's meaning.
- `sale_stats` ships `confidence: "unknown"` on every multi-world row and
  `avg_price ≈ vwap` almost everywhere; a columnar shape could omit
  constant columns.
- The flip finder already prefers the ClickHouse sales rate over the
  6-sale buffer; if `sale_stats` grew the percentile the flip finder
  needs, `recentSales` could shrink to 1–2 sales per item or go away.
- #1576's encoder comparison shows generic binary barely beats columnar
  JSON after brotli; only a hand-rolled delta+varint encoding wins, and it
  is not worth a new dependency and decoder for ~30%.
