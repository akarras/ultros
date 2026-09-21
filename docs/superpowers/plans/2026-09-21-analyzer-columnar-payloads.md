# Analyzer Columnar Payloads Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in `?format=columnar` struct-of-arrays JSON shape to `/api/v1/recentSales`, `/api/v1/sale_stats` and `/api/v1/listing_stats`, and switch the site's fetchers to it, cutting the analyzer pages' post-hydration payload by roughly 40% on the wire and 3× raw.

**Architecture:** Each row DTO in `ultros-api-types` gets a `*Columnar` sibling with lossless `From` conversions both ways (mismatched column lengths truncate to the shortest). Server handlers branch on `format=columnar` exactly; the two ClickHouse handlers that cache pre-serialized bytes get a `columnar: bool` in their cache key. Client fetchers in `ultros-frontend-core/src/api.rs` request the columnar shape and convert at the boundary, so no consumer changes. This is the recipe PR #1576 used for `/api/v1/cheapest`.

**Tech Stack:** Rust, serde/serde_json, axum, chrono, Leptos (client fetch only).

Spec: `docs/superpowers/specs/2026-09-21-analyzer-columnar-payloads-design.md`

## Global Constraints

- Legacy row-of-objects shape stays the default for every endpoint; only the exact, case-sensitive string `columnar` selects the new shape.
- No consumer of `RecentSales`, `BulkSaleStats`, `BulkListingStats` changes; conversion happens only in `api.rs`.
- Existing `old_wire_shape_still_deserializes` tests in `sale_stats.rs` / `listing_stats.rs` must keep passing untouched.
- Cache-Control headers unchanged: `recentSales` `max-age=30`; stats endpoints via `cached_response` (`max-age=300, s-maxage=300, stale-while-revalidate=1800`).
- No new dependencies. No user-facing strings are introduced (no i18n work).
- Before committing: `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"` (fmt + clippy `-D warnings`). Use the session scratchpad for the log, not `/tmp`.
- Windows build env (Git Bash): `export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH" OPENSSL_RUST_USE_NASM=0 CARGO_PROFILE_DEV_DEBUG=0` before any `cargo` command that touches the `ultros` crate (vendored OpenSSL; `CARGO_PROFILE_DEV_DEBUG=0` keeps the bin-test rlib under 4 GiB and must be set from the first build).
- Commit messages end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

---

## File map

| File | Responsibility |
|---|---|
| `ultros-api-types/src/recent_sales.rs` | `RecentSalesColumnar` + conversions + tests |
| `ultros-api-types/src/sale_stats.rs` | `BulkSaleStatsColumnar` + conversions + tests |
| `ultros-api-types/src/listing_stats.rs` | `BulkListingStatsColumnar` + conversions + tests |
| `ultros/src/web/stats_cache.rs` | `CacheKey.columnar` flag |
| `ultros/src/web/api/sale_stats.rs` | `format` query param, columnar serialization in `load_sale_stats` |
| `ultros/src/web/api/listing_stats.rs` | `format` query param, columnar serialization in `load_listing_stats` |
| `ultros/src/web/api/recent_sales.rs` | `format` query param, columnar branch |
| `ultros-frontend/ultros-frontend-core/src/api.rs` | fetchers request `format=columnar`, convert at boundary |

---

### Task 1: `RecentSalesColumnar`

**Files:**
- Modify: `ultros-api-types/src/recent_sales.rs`

**Interfaces:**
- Produces: `pub struct RecentSalesColumnar { item_id: Vec<i32>, hq: Vec<bool>, count: Vec<u32>, price: Vec<i32>, sold_unix: Vec<i64> }`, `impl From<RecentSales> for RecentSalesColumnar`, `impl From<RecentSalesColumnar> for RecentSales`.

- [ ] **Step 1: Write the failing tests**

Append to `ultros-api-types/src/recent_sales.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn at(secs: i64) -> NaiveDateTime {
        DateTime::from_timestamp(secs, 0).unwrap().naive_utc()
    }

    fn sale(price: i32, secs: i64) -> Sales {
        Sales {
            price_per_unit: price,
            sale_date: at(secs),
        }
    }

    fn rows() -> RecentSales {
        RecentSales {
            sales: vec![
                SaleData {
                    item_id: 2,
                    hq: false,
                    sales: vec![],
                },
                SaleData {
                    item_id: 2,
                    hq: true,
                    sales: vec![sale(90, 1_700_000_300), sale(88, 1_700_000_200), sale(87, 1_700_000_100)],
                },
                SaleData {
                    item_id: 5,
                    hq: false,
                    sales: vec![sale(7, 1_699_999_000)],
                },
            ],
        }
    }

    #[test]
    fn columnar_round_trips_ragged_rows_in_order() {
        let columnar = RecentSalesColumnar::from(rows());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.count, vec![0, 3, 1]);
        assert_eq!(columnar.price, vec![90, 88, 87, 7]);
        assert_eq!(
            columnar.sold_unix,
            vec![1_700_000_300, 1_700_000_200, 1_700_000_100, 1_699_999_000]
        );
        assert_eq!(RecentSales::from(columnar), rows());
    }

    #[test]
    fn columnar_serializes_as_five_arrays() {
        let columnar = RecentSalesColumnar::from(RecentSales {
            sales: vec![SaleData {
                item_id: 5,
                hq: false,
                sales: vec![sale(7, 1_699_999_000)],
            }],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[5],"hq":[false],"count":[1],"price":[7],"sold_unix":[1699999000]}"#
        );
        let back: RecentSalesColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_empty_round_trips() {
        assert_eq!(RecentSales::from(RecentSalesColumnar::default()).sales, vec![]);
        assert_eq!(
            RecentSalesColumnar::from(RecentSales { sales: vec![] }),
            RecentSalesColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_key_columns_truncate_to_shortest() {
        let columnar = RecentSalesColumnar {
            item_id: vec![1, 2, 3],
            hq: vec![false, true],
            count: vec![1, 1, 1],
            price: vec![10, 20, 30],
            sold_unix: vec![100, 200, 300],
        };
        let rows = RecentSales::from(columnar);
        assert_eq!(rows.sales.len(), 2);
        assert_eq!(rows.sales[0].sales, vec![sale(10, 100)]);
        assert_eq!(rows.sales[1].sales, vec![sale(20, 200)]);
    }

    #[test]
    fn columnar_count_beyond_flattened_columns_yields_what_exists() {
        // `count` promises 3 + 2 sales but only 4 (price) / 3 (sold_unix) exist.
        let columnar = RecentSalesColumnar {
            item_id: vec![1, 2],
            hq: vec![false, false],
            count: vec![3, 2],
            price: vec![10, 11, 12, 20],
            sold_unix: vec![100, 101, 102],
        };
        let rows = RecentSales::from(columnar);
        assert_eq!(rows.sales.len(), 2);
        assert_eq!(rows.sales[0].sales, vec![sale(10, 100), sale(11, 101), sale(12, 102)]);
        assert_eq!(rows.sales[1].sales, vec![]);
    }

    #[test]
    fn out_of_range_timestamp_falls_back_to_epoch() {
        let columnar = RecentSalesColumnar {
            item_id: vec![1],
            hq: vec![false],
            count: vec![1],
            price: vec![10],
            sold_unix: vec![i64::MAX],
        };
        let rows = RecentSales::from(columnar);
        assert_eq!(rows.sales[0].sales, vec![sale(10, 0)]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-api-types recent_sales`
Expected: compile error, `RecentSalesColumnar` not found.

- [ ] **Step 3: Implement the type and conversions**

Insert after the `RecentSales` struct in `ultros-api-types/src/recent_sales.rs` (add `use chrono::DateTime;` at the top next to the `NaiveDateTime` import):

```rust
/// Struct-of-arrays wire form of [`RecentSales`] — what
/// `/api/v1/recentSales/{world}?format=columnar` returns. Row `i` is
/// `(item_id[i], hq[i])` and owns the next `count[i]` entries of the
/// flattened `price` / `sold_unix` columns, newest first, exactly as the
/// row shape orders them. Timestamps are unix seconds; the row shape's
/// `NaiveDateTime` is second-resolution so the conversion is lossless.
/// Rows are emitted in `(item_id, hq)` order, which is what makes the
/// columns compress well; decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecentSalesColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub count: Vec<u32>,
    pub price: Vec<i32>,
    pub sold_unix: Vec<i64>,
}

impl From<RecentSales> for RecentSalesColumnar {
    fn from(value: RecentSales) -> Self {
        let n = value.sales.len();
        let total: usize = value.sales.iter().map(|s| s.sales.len()).sum();
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            count: Vec::with_capacity(n),
            price: Vec::with_capacity(total),
            sold_unix: Vec::with_capacity(total),
        };
        for row in value.sales {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.count.push(row.sales.len() as u32);
            for sale in row.sales {
                out.price.push(sale.price_per_unit);
                out.sold_unix.push(sale.sale_date.and_utc().timestamp());
            }
        }
        out
    }
}

impl From<RecentSalesColumnar> for RecentSales {
    /// Zips the key columns and walks the flattened sale columns with a
    /// cursor. A malformed payload degrades rather than panicking: key
    /// columns of unequal length truncate to the shortest, and a `count`
    /// that outruns the flattened columns yields the sales that exist.
    fn from(value: RecentSalesColumnar) -> Self {
        let mut prices = value.price.into_iter();
        let mut dates = value.sold_unix.into_iter();
        let sales = value
            .item_id
            .into_iter()
            .zip(value.hq)
            .zip(value.count)
            .map(|((item_id, hq), count)| {
                let sales = (0..count)
                    .map_while(|_| {
                        Some(Sales {
                            price_per_unit: prices.next()?,
                            sale_date: unix_to_naive(dates.next()?),
                        })
                    })
                    .collect();
                SaleData { item_id, hq, sales }
            })
            .collect();
        Self { sales }
    }
}

/// Out-of-range seconds fall back to the epoch instead of failing the
/// whole payload.
fn unix_to_naive(secs: i64) -> NaiveDateTime {
    DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .naive_utc()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-api-types recent_sales`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/recent_sales.rs
git commit -m "feat(api-types): RecentSalesColumnar wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: `BulkSaleStatsColumnar`

**Files:**
- Modify: `ultros-api-types/src/sale_stats.rs`

**Interfaces:**
- Produces: `pub struct BulkSaleStatsColumnar` with one `Vec` per `ItemSaleStats` field, `impl From<BulkSaleStats> for BulkSaleStatsColumnar`, `impl From<BulkSaleStatsColumnar> for BulkSaleStats`.

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests` in `ultros-api-types/src/sale_stats.rs`:

```rust
    fn stat(item_id: i32, hq: bool, median_price: i32) -> ItemSaleStats {
        ItemSaleStats {
            item_id,
            hq,
            min_price: median_price - 1,
            median_price,
            avg_price: median_price + 1,
            num_sold: 14,
            last_sold_unix: 1_789_915_494,
            units_sold: 20,
            vwap: median_price + 2,
            gil_volume: 6_738,
            sales_per_day: 2.0,
            confidence: ConfidenceBand::High,
        }
    }

    #[test]
    fn columnar_round_trips_rows_in_order() {
        let rows = BulkSaleStats {
            stats: vec![stat(2, false, 100), stat(2, true, 300), stat(5, false, 7)],
        };
        let columnar = BulkSaleStatsColumnar::from(rows.clone());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.median_price, vec![100, 300, 7]);
        assert_eq!(columnar.confidence, vec![ConfidenceBand::High; 3]);
        assert_eq!(BulkSaleStats::from(columnar), rows);
    }

    #[test]
    fn columnar_serializes_as_twelve_arrays() {
        let columnar = BulkSaleStatsColumnar::from(BulkSaleStats {
            stats: vec![stat(5, true, 7)],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[5],"hq":[true],"min_price":[6],"median_price":[7],"avg_price":[8],"num_sold":[14],"last_sold_unix":[1789915494],"units_sold":[20],"vwap":[9],"gil_volume":[6738],"sales_per_day":[2.0],"confidence":["high"]}"#
        );
        let back: BulkSaleStatsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_empty_round_trips() {
        assert_eq!(BulkSaleStats::from(BulkSaleStatsColumnar::default()).stats, vec![]);
        assert_eq!(
            BulkSaleStatsColumnar::from(BulkSaleStats::default()),
            BulkSaleStatsColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_lengths_truncate_to_shortest() {
        let mut columnar = BulkSaleStatsColumnar::from(BulkSaleStats {
            stats: vec![stat(1, false, 10), stat(2, true, 20), stat(3, false, 30)],
        });
        columnar.vwap.pop();
        let rows = BulkSaleStats::from(columnar);
        assert_eq!(rows.stats, vec![stat(1, false, 10), stat(2, true, 20)]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-api-types sale_stats`
Expected: compile error, `BulkSaleStatsColumnar` not found.

- [ ] **Step 3: Implement the type and conversions**

Insert after the `BulkSaleStats` struct in `ultros-api-types/src/sale_stats.rs`:

```rust
/// Struct-of-arrays wire form of [`BulkSaleStats`] — what
/// `/api/v1/sale_stats/{scope}?format=columnar` returns. Row `i` is the
/// `i`th element of every column; the names match [`ItemSaleStats`]
/// field for field. Rows are emitted in `(item_id, hq)` order for
/// compressibility; decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct BulkSaleStatsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub min_price: Vec<i32>,
    pub median_price: Vec<i32>,
    pub avg_price: Vec<i32>,
    pub num_sold: Vec<i64>,
    pub last_sold_unix: Vec<i64>,
    pub units_sold: Vec<u64>,
    pub vwap: Vec<i32>,
    pub gil_volume: Vec<u64>,
    pub sales_per_day: Vec<f32>,
    pub confidence: Vec<ConfidenceBand>,
}

impl From<BulkSaleStats> for BulkSaleStatsColumnar {
    fn from(value: BulkSaleStats) -> Self {
        let n = value.stats.len();
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            min_price: Vec::with_capacity(n),
            median_price: Vec::with_capacity(n),
            avg_price: Vec::with_capacity(n),
            num_sold: Vec::with_capacity(n),
            last_sold_unix: Vec::with_capacity(n),
            units_sold: Vec::with_capacity(n),
            vwap: Vec::with_capacity(n),
            gil_volume: Vec::with_capacity(n),
            sales_per_day: Vec::with_capacity(n),
            confidence: Vec::with_capacity(n),
        };
        for row in value.stats {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.min_price.push(row.min_price);
            out.median_price.push(row.median_price);
            out.avg_price.push(row.avg_price);
            out.num_sold.push(row.num_sold);
            out.last_sold_unix.push(row.last_sold_unix);
            out.units_sold.push(row.units_sold);
            out.vwap.push(row.vwap);
            out.gil_volume.push(row.gil_volume);
            out.sales_per_day.push(row.sales_per_day);
            out.confidence.push(row.confidence);
        }
        out
    }
}

impl From<BulkSaleStatsColumnar> for BulkSaleStats {
    /// Zips the columns. A malformed payload with unequal column lengths
    /// degrades to the rows every column has rather than panicking.
    fn from(value: BulkSaleStatsColumnar) -> Self {
        let n = [
            value.item_id.len(),
            value.hq.len(),
            value.min_price.len(),
            value.median_price.len(),
            value.avg_price.len(),
            value.num_sold.len(),
            value.last_sold_unix.len(),
            value.units_sold.len(),
            value.vwap.len(),
            value.gil_volume.len(),
            value.sales_per_day.len(),
            value.confidence.len(),
        ]
        .into_iter()
        .min()
        .unwrap_or(0);
        let stats = (0..n)
            .map(|i| ItemSaleStats {
                item_id: value.item_id[i],
                hq: value.hq[i],
                min_price: value.min_price[i],
                median_price: value.median_price[i],
                avg_price: value.avg_price[i],
                num_sold: value.num_sold[i],
                last_sold_unix: value.last_sold_unix[i],
                units_sold: value.units_sold[i],
                vwap: value.vwap[i],
                gil_volume: value.gil_volume[i],
                sales_per_day: value.sales_per_day[i],
                confidence: value.confidence[i],
            })
            .collect();
        Self { stats }
    }
}
```

(Twelve columns make a nested `zip` unreadable; indexing after taking the minimum length is the same truncation contract.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-api-types sale_stats`
Expected: 5 passed (4 new + `old_wire_shape_still_deserializes`).

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/sale_stats.rs
git commit -m "feat(api-types): BulkSaleStatsColumnar wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: `BulkListingStatsColumnar`

**Files:**
- Modify: `ultros-api-types/src/listing_stats.rs`

**Interfaces:**
- Produces: `pub struct BulkListingStatsColumnar` with one `Vec` per flat `ItemListingStats` field plus `window: Option<Vec<ListingWindowStats>>`, `impl From<BulkListingStats> for BulkListingStatsColumnar`, `impl From<BulkListingStatsColumnar> for BulkListingStats`.

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests` in `ultros-api-types/src/listing_stats.rs`:

```rust
    fn listing(item_id: i32, hq: bool, floor_alive: i32) -> ItemListingStats {
        ItemListingStats {
            item_id,
            hq,
            alive_count: 3,
            alive_units: 12,
            distinct_retainers: 2,
            oldest_reviewed_unix: 1_700_000_000,
            median_age_secs: 86_400,
            floor_alive,
            window: None,
        }
    }

    fn window(days: u16) -> ListingWindowStats {
        ListingWindowStats {
            window_days: days,
            from: 1_700_000_000 - i64::from(days) * 86_400,
            to: 1_700_000_000,
            additions: 4,
            removals: 1,
            ..Default::default()
        }
    }

    #[test]
    fn columnar_round_trips_rows_in_order() {
        let rows = BulkListingStats {
            stats: vec![listing(2, false, 100), listing(2, true, 300), listing(5, false, 7)],
        };
        let columnar = BulkListingStatsColumnar::from(rows.clone());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.floor_alive, vec![100, 300, 7]);
        assert!(columnar.window.is_none());
        assert_eq!(BulkListingStats::from(columnar), rows);
    }

    #[test]
    fn columnar_current_only_serializes_as_eight_arrays_without_window() {
        let columnar = BulkListingStatsColumnar::from(BulkListingStats {
            stats: vec![listing(5, true, 7)],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[5],"hq":[true],"alive_count":[3],"alive_units":[12],"distinct_retainers":[2],"oldest_reviewed_unix":[1700000000],"median_age_secs":[86400],"floor_alive":[7]}"#
        );
        let back: BulkListingStatsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_windowed_round_trips_window_column() {
        let mut a = listing(2, false, 100);
        a.window = Some(window(7));
        let mut b = listing(5, false, 7);
        b.window = Some(window(30));
        let rows = BulkListingStats {
            stats: vec![a, b],
        };
        let columnar = BulkListingStatsColumnar::from(rows.clone());
        assert_eq!(
            columnar.window.as_ref().map(|w| w.len()),
            Some(2)
        );
        let json = serde_json::to_string(&columnar).unwrap();
        assert!(json.contains(r#""window":[{"#));
        let back: BulkListingStatsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(BulkListingStats::from(back), rows);
    }

    #[test]
    fn columnar_short_window_column_leaves_tail_rows_without_window() {
        let mut columnar = BulkListingStatsColumnar::from(BulkListingStats {
            stats: vec![listing(1, false, 10), listing(2, false, 20)],
        });
        columnar.window = Some(vec![window(7)]);
        let rows = BulkListingStats::from(columnar);
        assert_eq!(rows.stats[0].window, Some(window(7)));
        assert_eq!(rows.stats[1].window, None);
    }

    #[test]
    fn columnar_empty_round_trips() {
        assert_eq!(BulkListingStats::from(BulkListingStatsColumnar::default()).stats, vec![]);
        assert_eq!(
            BulkListingStatsColumnar::from(BulkListingStats::default()),
            BulkListingStatsColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_lengths_truncate_to_shortest() {
        let mut columnar = BulkListingStatsColumnar::from(BulkListingStats {
            stats: vec![listing(1, false, 10), listing(2, true, 20), listing(3, false, 30)],
        });
        columnar.median_age_secs.pop();
        let rows = BulkListingStats::from(columnar);
        assert_eq!(rows.stats, vec![listing(1, false, 10), listing(2, true, 20)]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-api-types listing_stats`
Expected: compile error, `BulkListingStatsColumnar` not found.

- [ ] **Step 3: Implement the type and conversions**

Insert after the `BulkListingStats` struct in `ultros-api-types/src/listing_stats.rs`:

```rust
/// Struct-of-arrays wire form of [`BulkListingStats`] — what
/// `/api/v1/listing_stats/{scope}?format=columnar` returns. Row `i` is
/// the `i`th element of every column; the names match
/// [`ItemListingStats`] field for field. The nested per-row
/// [`ListingWindowStats`] is not exploded: on windowed requests it rides
/// as one parallel column of objects, and on current-only requests the
/// key is absent. Rows are emitted in `(item_id, hq)` order for
/// compressibility; decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct BulkListingStatsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub alive_count: Vec<u32>,
    pub alive_units: Vec<u64>,
    pub distinct_retainers: Vec<u32>,
    pub oldest_reviewed_unix: Vec<i64>,
    pub median_age_secs: Vec<u32>,
    pub floor_alive: Vec<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Vec<ListingWindowStats>>,
}

impl From<BulkListingStats> for BulkListingStatsColumnar {
    /// The `window` column is present iff any row carries a window; rows
    /// without one contribute a `Default` placeholder so the column stays
    /// parallel. (The server fills every row on a windowed request.)
    fn from(value: BulkListingStats) -> Self {
        let n = value.stats.len();
        let windowed = value.stats.iter().any(|s| s.window.is_some());
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            alive_count: Vec::with_capacity(n),
            alive_units: Vec::with_capacity(n),
            distinct_retainers: Vec::with_capacity(n),
            oldest_reviewed_unix: Vec::with_capacity(n),
            median_age_secs: Vec::with_capacity(n),
            floor_alive: Vec::with_capacity(n),
            window: windowed.then(|| Vec::with_capacity(n)),
        };
        for row in value.stats {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.alive_count.push(row.alive_count);
            out.alive_units.push(row.alive_units);
            out.distinct_retainers.push(row.distinct_retainers);
            out.oldest_reviewed_unix.push(row.oldest_reviewed_unix);
            out.median_age_secs.push(row.median_age_secs);
            out.floor_alive.push(row.floor_alive);
            if let Some(window) = out.window.as_mut() {
                window.push(row.window.unwrap_or_default());
            }
        }
        out
    }
}

impl From<BulkListingStatsColumnar> for BulkListingStats {
    /// Zips the flat columns; unequal lengths truncate to the shortest
    /// rather than panicking. A `window` column shorter than the rows
    /// leaves the tail rows' `window` as `None`.
    fn from(value: BulkListingStatsColumnar) -> Self {
        let n = [
            value.item_id.len(),
            value.hq.len(),
            value.alive_count.len(),
            value.alive_units.len(),
            value.distinct_retainers.len(),
            value.oldest_reviewed_unix.len(),
            value.median_age_secs.len(),
            value.floor_alive.len(),
        ]
        .into_iter()
        .min()
        .unwrap_or(0);
        let mut windows = value.window.unwrap_or_default().into_iter();
        let stats = (0..n)
            .map(|i| ItemListingStats {
                item_id: value.item_id[i],
                hq: value.hq[i],
                alive_count: value.alive_count[i],
                alive_units: value.alive_units[i],
                distinct_retainers: value.distinct_retainers[i],
                oldest_reviewed_unix: value.oldest_reviewed_unix[i],
                median_age_secs: value.median_age_secs[i],
                floor_alive: value.floor_alive[i],
                window: windows.next(),
            })
            .collect();
        Self { stats }
    }
}
```

Note: `ListingWindowStats` must derive `Default` for `unwrap_or_default()`; check with `grep -n "derive" ultros-api-types/src/listing_stats.rs` around line 147 — `load_listing_stats` in `ultros` already uses `..Default::default()` on it, so it does.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-api-types listing_stats`
Expected: all pass (6 new + the existing ones in the module).

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/listing_stats.rs
git commit -m "feat(api-types): BulkListingStatsColumnar wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: `CacheKey.columnar`

**Files:**
- Modify: `ultros/src/web/stats_cache.rs:28-31` (struct), `:415-420` and `:561` (tests)
- Modify: `ultros/src/web/api/sale_stats.rs:62-65`
- Modify: `ultros/src/web/api/listing_stats.rs:68-71`

**Interfaces:**
- Produces: `CacheKey { selector: AnySelector, window_days: u16, columnar: bool }`.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `ultros/src/web/stats_cache.rs`:

```rust
    #[tokio::test]
    async fn columnar_and_row_shapes_have_separate_slots() {
        let cache = SaleStatsCache::with_config(
            4,
            1,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(1),
            64 * 1024 * 1024,
        );
        let rows = CacheKey {
            selector: AnySelector::World(1),
            window_days: 7,
            columnar: false,
        };
        let columnar = CacheKey {
            columnar: true,
            ..rows
        };
        cache
            .get_or_load(rows, || async { Ok(Bytes::from_static(b"rows")) })
            .await
            .unwrap();
        let value = cache
            .get_or_load(columnar, || async { Ok(Bytes::from_static(b"columnar")) })
            .await
            .unwrap();
        assert_eq!(value.disposition, CacheDisposition::Loaded);
        assert_eq!(&value.body[..], b"columnar");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros --bin ultros stats_cache`
Expected: compile error, no field `columnar`.

- [ ] **Step 3: Add the field and fix every construction site**

`ultros/src/web/stats_cache.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub selector: AnySelector,
    pub window_days: u16,
    /// Struct-of-arrays (`?format=columnar`) and row-of-objects bodies are
    /// cached in separate slots; each is a distinct serialization.
    pub columnar: bool,
}
```

In the same file's tests, the `key(id)` helper (line ~415) and the `CacheKey { ... }` at line ~561 gain `columnar: false,`.

`ultros/src/web/api/sale_stats.rs` line ~62 and `ultros/src/web/api/listing_stats.rs` line ~68: add `columnar: false,` to the `CacheKey { ... }` literal for now (Tasks 5 and 6 replace it with the parsed flag).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros --bin ultros stats_cache`
Expected: all pass, including the new one.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/web/stats_cache.rs ultros/src/web/api/sale_stats.rs ultros/src/web/api/listing_stats.rs
git commit -m "feat(stats-cache): separate cache slots per wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: `sale_stats` handler `?format=columnar`

**Files:**
- Modify: `ultros/src/web/api/sale_stats.rs`

**Interfaces:**
- Consumes: `BulkSaleStatsColumnar` (Task 2), `CacheKey.columnar` (Task 4).
- Produces: `pub(crate) fn is_columnar(format: Option<&str>) -> bool` in `ultros/src/web/api/mod.rs`, shared by Tasks 6 and 7.

- [ ] **Step 1: Write the failing tests**

Add a `mod tests` at the bottom of `ultros/src/web/api/mod.rs` (create the module if absent):

```rust
#[cfg(test)]
mod tests {
    use super::is_columnar;

    #[test]
    fn format_query_only_matches_columnar_exactly() {
        assert!(is_columnar(Some("columnar")));
        assert!(!is_columnar(Some("Columnar")));
        assert!(!is_columnar(Some("json")));
        assert!(!is_columnar(Some("")));
        assert!(!is_columnar(None));
    }
}
```

Add to `ultros/src/web/api/sale_stats.rs` (new `mod tests` at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> Vec<ItemSaleStats> {
        vec![
            ItemSaleStats {
                item_id: 2,
                hq: false,
                median_price: 100,
                ..Default::default()
            },
            ItemSaleStats {
                item_id: 5,
                hq: true,
                median_price: 7,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn serialize_body_rows_by_default_columnar_on_request() {
        let rows = serialize_body(stats(), false).unwrap();
        assert!(rows.starts_with(br#"{"stats":[{"item_id":2"#));
        let columnar = serialize_body(stats(), true).unwrap();
        assert!(columnar.starts_with(br#"{"item_id":[2,5],"hq":[false,true]"#));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros --bin ultros web::api`
Expected: compile errors, `is_columnar` and `serialize_body` not found.

- [ ] **Step 3: Implement**

In `ultros/src/web/api/mod.rs` add:

```rust
/// `?format=columnar` selects the struct-of-arrays wire shape the site
/// fetches. Anything else — including no `format` at all — keeps the
/// legacy row-of-objects shape for outside consumers of the public
/// endpoints. Exact, case-sensitive match.
pub(crate) fn is_columnar(format: Option<&str>) -> bool {
    format == Some("columnar")
}
```

In `ultros/src/web/api/sale_stats.rs`:

- `use ultros_api_types::sale_stats::{BulkSaleStats, BulkSaleStatsColumnar, ItemSaleStats};`
- `use super::is_columnar;`
- Extend the query struct:

```rust
#[derive(Debug, Deserialize)]
pub(crate) struct SaleStatsQuery {
    /// Trailing rollup window in days. Supported: 1, 7, 30, 90; defaults to 7.
    window: Option<u16>,
    /// `columnar` selects [`BulkSaleStatsColumnar`]; see [`is_columnar`].
    format: Option<String>,
}
```

- In `get_sale_stats`, after computing `window_days`:

```rust
    let columnar = is_columnar(query.format.as_deref());
```

and change the cache call to

```rust
            CacheKey {
                selector,
                window_days,
                columnar,
            },
            move || async move { load_sale_stats(&ch, world_ids, window_days, columnar).await },
```

- `load_sale_stats` gains `columnar: bool` and its tail becomes:

```rust
    let stats: Vec<ItemSaleStats> = rows
        .into_iter()
        .map(|r| ItemSaleStats { /* unchanged */ })
        .collect();

    serialize_body(stats, columnar)
}

/// Either wire shape, pre-serialized for the cache.
fn serialize_body(stats: Vec<ItemSaleStats>, columnar: bool) -> Result<Bytes, WebError> {
    let body = BulkSaleStats { stats };
    let json = if columnar {
        serde_json::to_vec(&BulkSaleStatsColumnar::from(body))
    } else {
        serde_json::to_vec(&body)
    };
    json.map(Bytes::from)
        .map_err(anyhow::Error::from)
        .map_err(Into::into)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros --bin ultros web::api`
Expected: both new tests pass.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/web/api/mod.rs ultros/src/web/api/sale_stats.rs
git commit -m "feat(sale-stats): ?format=columnar wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: `listing_stats` handler `?format=columnar`

**Files:**
- Modify: `ultros/src/web/api/listing_stats.rs`

**Interfaces:**
- Consumes: `BulkListingStatsColumnar` (Task 3), `CacheKey.columnar` (Task 4), `is_columnar` (Task 5).

- [ ] **Step 1: Write the failing test**

Add to the existing `mod tests` in `ultros/src/web/api/listing_stats.rs`:

```rust
    #[test]
    fn serialize_body_rows_by_default_columnar_on_request() {
        let stats = vec![to_wire(row(0, 40)), to_wire(row(1, 950))];
        let rows = serialize_body(stats.clone(), false).unwrap();
        assert!(rows.starts_with(br#"{"stats":[{"item_id":7,"hq":false"#));
        let columnar = serialize_body(stats, true).unwrap();
        assert!(columnar.starts_with(br#"{"item_id":[7,7],"hq":[false,true]"#));
        assert!(!columnar.ends_with(br#""window":null}"#));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros --bin ultros listing_stats`
Expected: compile error, `serialize_body` not found.

- [ ] **Step 3: Implement**

In `ultros/src/web/api/listing_stats.rs`:

- Import `BulkListingStatsColumnar` alongside `BulkListingStats`/`ItemListingStats`, and `use super::is_columnar;`.
- Extend the query struct:

```rust
#[derive(Debug, Deserialize)]
pub(crate) struct ListingStatsQuery {
    window: Option<u16>,
    /// `columnar` selects [`BulkListingStatsColumnar`]; see [`is_columnar`].
    format: Option<String>,
}
```

- In `get_listing_stats`, before the cache call: `let columnar = is_columnar(query.format.as_deref());` and

```rust
            CacheKey {
                selector,
                window_days: query.window.unwrap_or(NO_WINDOW),
                columnar,
            },
            move || async move { load_listing_stats(&ch, world_ids, query.window, columnar).await },
```

- `load_listing_stats` gains `columnar: bool`; replace its final `serde_json::to_vec(&BulkListingStats { stats })...` with `serialize_body(stats, columnar)` and add:

```rust
/// Either wire shape, pre-serialized for the cache.
fn serialize_body(stats: Vec<ItemListingStats>, columnar: bool) -> Result<Bytes, WebError> {
    let body = BulkListingStats { stats };
    let json = if columnar {
        serde_json::to_vec(&BulkListingStatsColumnar::from(body))
    } else {
        serde_json::to_vec(&body)
    };
    json.map(Bytes::from)
        .map_err(anyhow::Error::from)
        .map_err(Into::into)
}
```

(`let stats = stats.into_values().collect();` needs the type: `let stats: Vec<ItemListingStats> = ...`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros --bin ultros listing_stats`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/web/api/listing_stats.rs
git commit -m "feat(listing-stats): ?format=columnar wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: `recentSales` handler `?format=columnar`

**Files:**
- Modify: `ultros/src/web/api/recent_sales.rs`

**Interfaces:**
- Consumes: `RecentSalesColumnar` (Task 1), `is_columnar` (Task 5).

- [ ] **Step 1: Write the failing test**

The handler's `item_map` → `Vec<SaleData>` closure stays as is; the testable unit is the `body()` shape selector introduced in Step 3. Add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_rows_by_default_columnar_on_request() {
        let rows = vec![SaleData {
            item_id: 5,
            hq: false,
            sales: vec![Sales {
                price_per_unit: 7,
                sale_date: chrono::DateTime::from_timestamp(1_699_999_000, 0)
                    .unwrap()
                    .naive_utc(),
            }],
        }];
        let legacy = serde_json::to_string(&body(rows.clone(), false)).unwrap();
        assert!(legacy.starts_with(r#"{"sales":[{"item_id":5"#));
        let columnar = serde_json::to_string(&body(rows, true)).unwrap();
        assert_eq!(
            columnar,
            r#"{"item_id":[5],"hq":[false],"count":[1],"price":[7],"sold_unix":[1699999000]}"#
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros --bin ultros recent_sales`
Expected: compile error, `body` not found.

- [ ] **Step 3: Implement**

In `ultros/src/web/api/recent_sales.rs`:

```rust
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use ultros_api_types::recent_sales::{RecentSales, RecentSalesColumnar, SaleData, Sales};

use super::is_columnar;

#[derive(Debug, Deserialize)]
pub(crate) struct RecentSalesQuery {
    /// `columnar` selects [`RecentSalesColumnar`]; see [`is_columnar`].
    format: Option<String>,
}

/// The response body in either wire shape. Untagged so each variant
/// serializes as its own top-level object.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Body {
    Rows(RecentSales),
    Columnar(RecentSalesColumnar),
}

fn body(sales: Vec<SaleData>, columnar: bool) -> Body {
    let rows = RecentSales { sales };
    if columnar {
        Body::Columnar(RecentSalesColumnar::from(rows))
    } else {
        Body::Rows(rows)
    }
}

pub(crate) async fn recent_sales(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
    Query(query): Query<RecentSalesQuery>,
) -> Result<impl IntoResponse, WebError> {
    let sales: Vec<_> = analyzer
        .read_sale_history(
            &AnySelector::from(&world_cache.lookup_value_by_name(&world)?),
            |sales| { /* unchanged mapping to Vec<SaleData> */ },
        )
        .await?;
    let mut response: Response =
        Json(body(sales, is_columnar(query.format.as_deref()))).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(30)));
    Ok(response)
}
```

(If the untagged enum is awkward, two `Json(...).into_response()` branches assigned to `let mut response: Response = if columnar { ... } else { ... };` as in `cheapest_per_world.rs` is equally fine — keep `body()` for the test either way, returning `serde_json::Value` in that case.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ultros --bin ultros recent_sales`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/web/api/recent_sales.rs
git commit -m "feat(recent-sales): ?format=columnar wire shape

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 8: Client fetchers opt in

**Files:**
- Modify: `ultros-frontend/ultros-frontend-core/src/api.rs:296-322`

**Interfaces:**
- Consumes: all three `*Columnar` types.
- Produces: unchanged signatures for `get_sale_stats`, `get_listing_stats`, `get_listing_stats_window`, `get_recent_sales_for_world`.

- [ ] **Step 1: Update the fetchers**

Extend the `use ultros_api_types::{...}` block so `listing_stats`, `recent_sales` and `sale_stats` import their `*Columnar` type alongside the row type (check the exact existing import lines with `grep -n "listing_stats::\|recent_sales::\|sale_stats::" ultros-frontend/ultros-frontend-core/src/api.rs`). Then:

```rust
/// Bulk sale-history statistics (min/median/avg per item) for a world,
/// datacenter, or region — the recipe analyzer's selectable cost basis.
/// Fetches the columnar wire shape (roughly 40% fewer bytes after
/// compression, 3× less to parse) and converts at the boundary so callers
/// keep the row type.
pub async fn get_sale_stats(scope_name: &str, window_days: u16) -> AppResult<BulkSaleStats> {
    fetch_api::<BulkSaleStatsColumnar>(&format!(
        "/api/v1/sale_stats/{scope_name}?window={window_days}&format=columnar"
    ))
    .await
    .map(BulkSaleStats::from)
}

/// Current-listing statistics (alive count, units, sellers, ages) for every
/// item/quality pair in a world, datacenter, or region. An empty board is a
/// successful empty body, not an error; only transport failures are `Err`.
/// Columnar on the wire, rows at the boundary.
pub async fn get_listing_stats(scope_name: &str) -> AppResult<BulkListingStats> {
    fetch_api::<BulkListingStatsColumnar>(&format!(
        "/api/v1/listing_stats/{scope_name}?format=columnar"
    ))
    .await
    .map(BulkListingStats::from)
}

/// The alive set plus `window` history for `days` (1/7/30/90) from the
/// committed exact-scope snapshot. A cold scope/window pair is 503 until the
/// server's background worker publishes a generation; callers retry.
/// Columnar on the wire, rows at the boundary.
pub async fn get_listing_stats_window(scope_name: &str, days: u16) -> AppResult<BulkListingStats> {
    fetch_api::<BulkListingStatsColumnar>(&format!(
        "/api/v1/listing_stats/{scope_name}?window={days}&format=columnar"
    ))
    .await
    .map(BulkListingStats::from)
}

/// Recent sales (up to six per item/quality) for a world. Columnar on the
/// wire — this is the analyzer's largest fetch by far — rows at the boundary.
pub async fn get_recent_sales_for_world(region_name: &str) -> AppResult<RecentSales> {
    fetch_api::<RecentSalesColumnar>(&format!(
        "/api/v1/recentSales/{region_name}?format=columnar"
    ))
    .await
    .map(RecentSales::from)
}
```

- [ ] **Step 2: Build the frontend crates for both targets**

Run: `cargo check -p ultros-frontend-core --features ssr && cargo check -p ultros-frontend-core --features hydrate --target wasm32-unknown-unknown`
(Check `ultros-frontend/ultros-frontend-core/Cargo.toml` `[features]` for the exact feature names; `ultros-app` builds on top of it — `cargo check -p ultros-app --features ssr` as well.)
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-frontend-core/src/api.rs
git commit -m "perf(analyzer): fetch sale/listing stats and recent sales as columnar JSON

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 9: CI check and local verification

**Files:** none new.

- [ ] **Step 1: Full test run for touched crates**

Run:
```bash
cargo test -p ultros-api-types && cargo test -p ultros --bin ultros web::
```
Expected: all pass.

- [ ] **Step 2: `check_ci.sh`**

Run (Git Bash, with the Windows env exports from Global Constraints):
```bash
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
```
Expected: `REAL_EXIT=0`. If fmt fails, `cargo fmt --all` and re-run. If clippy exits 137, re-run `cargo clippy --all-targets -j 2 -- -D warnings`.

- [ ] **Step 3: Local serve smoke**

Start the app per `AGENTS.md` / memory recipe (`cargo leptos serve` or the release build on port 8080). Then:

```bash
for a in "recentSales/Adamantoise" "sale_stats/Aether?window=7" "listing_stats/Aether"; do
  sep=$([[ "$a" == *"?"* ]] && echo "&" || echo "?")
  echo "== $a"; curl -s "http://localhost:8080/api/v1/$a" | head -c 120; echo
  curl -s "http://localhost:8080/api/v1/$a${sep}format=columnar" | python -c "
import json,sys; d=json.load(sys.stdin); print({k:len(v) for k,v in d.items() if isinstance(v,list)})"
done
```
Expected: legacy bodies start with `{"stats":[{` / `{"sales":[{`; columnar bodies report equal lengths for the key columns (and `sum(count) == len(price) == len(sold_unix)` for sales). Local ClickHouse may 503/500 `sale_stats`/`listing_stats` (known local gap — see memory `reference_local_clickhouse_sales_broken_parts`); if so, verify only `recentSales` locally and rely on the unit tests for the stats shapes, and say so in the PR.

Open `http://localhost:8080/analyzer/North-America/Aether/Adamantoise` and `/recipe-analyzer/North-America/Aether/Adamantoise` in the browser pane: the network panel shows `?format=columnar` requests, tables render with prices, no new console errors.

- [ ] **Step 4: Measure and record**

Against prod (legacy) vs local (columnar) is apples-to-oranges; record local legacy vs local columnar raw sizes with `curl -s -o /dev/null -w "%{size_download}\n"` for each endpoint and put the table in the PR body alongside the prod simulation numbers from the spec.

- [ ] **Step 5: Open the PR**

```bash
git push -u origin claude/analyzer-page-size-d07937
gh pr create --title "perf(analyzer): columnar wire shapes for recentSales, sale_stats, listing_stats" --body-file "$SCRATCH/pr.md"
```
PR body: summary (three endpoints, opt-in, legacy default unchanged, cache slot per shape), the measurement table, test plan checklist, spec path, and note that it is independent of #1576 (different DTO files; adjacent but non-overlapping lines in `api.rs`). End with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
