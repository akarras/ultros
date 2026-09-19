# Undercut Columns Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two follow-window "Listings" columns on every analyzer grid, "Undercuts/day (Nd)" and "Undercut % (Nd)", fed by the windowed listing snapshot, plus the per-window listing slot in the market kit that later #1343 history columns reuse.

**Architecture:** The ClickHouse snapshot reducer (`listing_history::window_items`) runs one extra bounded SQL aggregate per item batch that pairs same-listing price drops and returns a count and median per `(item, hq)`; `window()` then folds a scope-wide coverage span into a per-day rate. Three serde-defaulted fields ride on `ListingWindowStats`. The frontend market kit gains per-window listing slots fetched with `?window=N` only when a windowed listing column is wanted, with a retry ladder for the 503 a cold scope returns while the worker publishes. Two new `ListingWindowKind` columns plug into the existing id/label/picker/value plumbing.

**Tech Stack:** Rust nightly (workspace pin), ClickHouse (`clickhouse` crate, window functions), Leptos 0.8 + `leptos-i18n`, Puppeteer e2e in `integration/`.

Spec: `docs/superpowers/specs/2026-09-18-undercut-columns-design.md`.

## Global Constraints

- Run `./check_ci.sh` (fmt-check + clippy `-D warnings`) before every commit that touches Rust. Log it to a unique file inside the worktree, foreground, 600000 ms timeout, never a Monitor: `./check_ci.sh > ./ci-undercut.log 2>&1; echo "REAL_EXIT=$?"; tail -30 ./ci-undercut.log`. Never `#[allow]` a clippy lint to silence it.
- Windows shell: before the first cargo command export `PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`, `OPENSSL_RUST_USE_NASM=0`, `CARGO_PROFILE_DEV_DEBUG=0`.
- Every user-facing string goes through `leptos-i18n`: keys in **all seven** locale files `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json`, real translations, `snake_case`, `market_` prefix.
- Column ids are a bookmark contract: exactly `market-undercuts` and `market-undercut-pct`, no `-N` suffixed variants.
- Undercut definition (verbatim from the spec): an `updated` row with `prev_price > price_per_unit`, or a `removed` then `added` pair on one `(item_id, hq, world_id, listing_id)` within 600 s where the add is cheaper and falls inside `[from, to)`. Exclude `source = 'snapshot'`, empty `listing_id`, same-price pairs, raises, pairs more than 600 s apart. Drop = `(prev_price - price_per_unit) / prev_price`.
- Rate denominator: scope-wide `span_secs = clamp(last - max(first, from), 86400, days * 86400)` over every key's `listing_coverage`; `None` when no key has an observation.
- Cells render `—` for `Missing`/`Unavailable`; never `0` for an absent row.
- Work only inside this worktree: `C:\Users\chw11\code\ultros\.claude\worktrees\ultros-listing-analyzer-columns-6ca829`, branch `claude/ultros-listing-analyzer-columns-6ca829`. Verify with `git rev-parse --show-toplevel` before the first git command.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

### Task 1: Wire fields on `ListingWindowStats`

**Files:**
- Modify: `ultros-api-types/src/listing_stats.rs:113-128` (the `ListingWindowStats` struct) and its `tests` module (lines 42-83).

**Interfaces:**
- Produces: `ListingWindowStats { undercuts: u64, undercuts_per_day: Option<f64>, undercut_median: Option<f64>, .. }`, all `#[serde(default)]`. Task 2 fills them; Task 5 reads them.

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests` in `ultros-api-types/src/listing_stats.rs` (after `bulk_payload_round_trips`):

```rust
    #[test]
    fn window_without_undercut_fields_still_deserializes() {
        // A generation stored before the deploy, or an older server. Every
        // other field is present because the reducer always writes them.
        let old = r#"{"window_days":7,"from":0,"to":604800,"additions":1,"removals":2,
            "listing_coverage":{"first_observed_unix":null,"last_observed_unix":null,"observed_span_secs":0,"continuity_verified":false},
            "floor_min":null,"floor_max":null,"floor_known_secs":0,"floor_empty_secs":0,"floor_unknown_secs":604800,
            "matches":{"matched":0,"ambiguous":0,"repriced":0,"unmatched":0,"sales_without_receipt":0,
              "receipt_coverage":{"first_observed_unix":null,"last_observed_unix":null,"observed_span_secs":0,"continuity_verified":false},
              "received_sales":0,"settled_through_unix":0,"pending":0,"median_time_to_sell_secs":null,"age_origin":"last_review_time"},
            "stock_status":"unavailable","days_of_stock":null}"#;
        let window: ListingWindowStats = serde_json::from_str(old).unwrap();
        assert_eq!(window.undercuts, 0);
        assert_eq!(window.undercuts_per_day, None);
        assert_eq!(window.undercut_median, None);
    }

    #[test]
    fn undercut_fields_round_trip() {
        let window = ListingWindowStats {
            window_days: 7,
            undercuts: 3,
            undercuts_per_day: Some(0.5),
            undercut_median: Some(0.026),
            ..Default::default()
        };
        let json = serde_json::to_string(&window).unwrap();
        assert!(json.contains("\"undercuts\":3"));
        assert_eq!(serde_json::from_str::<ListingWindowStats>(&json).unwrap(), window);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-api-types listing_stats -- --nocapture`
Expected: compile error `no field 'undercuts' on type ListingWindowStats`.

- [ ] **Step 3: Add the fields**

In `ListingWindowStats`, after `pub days_of_stock: Option<f64>,`:

```rust
    /// Same-listing price drops observed in the window: an `updated` row
    /// whose price fell, or a removed-then-added pair on one listing id
    /// within 600 s where the add is cheaper. Raises, same-price pairs,
    /// the seed and id-less legacy listings are excluded.
    #[serde(default)]
    pub undercuts: u64,
    /// `undercuts` per day of scope-wide listing coverage (clamped to at
    /// least one day and at most the window). `None` only when the scope
    /// observed no listing events at all in the window.
    #[serde(default)]
    pub undercuts_per_day: Option<f64>,
    /// Median relative drop across those undercuts, in `0..1`. `None` when
    /// `undercuts == 0`.
    #[serde(default)]
    pub undercut_median: Option<f64>,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p ultros-api-types listing_stats`
Expected: all tests in the module PASS, including `old_wire_shape_still_deserializes`.

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/listing_stats.rs
git commit -m "feat(api-types): undercut fields on ListingWindowStats

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Reprice aggregate and per-day rate in the snapshot reducer

**Files:**
- Modify: `ultros-clickhouse/src/listing_history.rs` (`window` at lines 31-78, `add_missing_receipts` at 80-118, `window_items` at 153-268, `tests` at 560+).
- Test: `ultros-clickhouse/tests/listing_history_smoke.rs` (gated integration test, lines 30-121 hold the fixture).

**Interfaces:**
- Consumes: Task 1's fields.
- Produces: `pub fn set_undercut_rates(output: &mut BTreeMap<(i32, bool), ListingWindowStats>, from: i64, to: i64)`; `fn empty_window(days: u16, from: i64, to: i64) -> ListingWindowStats`; `fn reprice_sql(item_sql: &str, world_sql: &str, from: i64, to: i64) -> String`. The snapshot publisher (`listing_snapshots::refresh`) calls `window()` unchanged and therefore stores the new fields.

- [ ] **Step 1: Write the failing unit test for the rate fold**

Append inside `mod tests` in `ultros-clickhouse/src/listing_history.rs`:

```rust
    #[test]
    fn undercut_rate_uses_scope_coverage_with_a_day_floor_and_window_cap() {
        let key = |i: i32| (i, false);
        let with = |undercuts: u64, first: Option<i64>, last: Option<i64>| ListingWindowStats {
            undercuts,
            listing_coverage: HistoryCoverage {
                first_observed_unix: first,
                last_observed_unix: last,
                ..Default::default()
            },
            ..Default::default()
        };
        // No key observed anything: the rate stays unknown, never zero.
        let mut none = BTreeMap::from([(key(1), with(2, None, None))]);
        set_undercut_rates(&mut none, 0, 7 * 86400);
        assert_eq!(none[&key(1)].undercuts_per_day, None);
        // A five-minute scope span is floored to one day; a key without its
        // own observations still gets the scope's denominator.
        let mut short = BTreeMap::from([
            (key(1), with(4, Some(1000), Some(1300))),
            (key(2), with(0, None, None)),
        ]);
        set_undercut_rates(&mut short, 0, 7 * 86400);
        assert_eq!(short[&key(1)].undercuts_per_day, Some(4.0));
        assert_eq!(short[&key(2)].undercuts_per_day, Some(0.0));
        // Observations before `from` (the reducer reads from - 600) do not
        // stretch the span; a span longer than the window is capped.
        let from = 100 * 86400;
        let to = from + 30 * 86400;
        let mut long = BTreeMap::from([
            (key(1), with(30, Some(from - 600), Some(to - 1))),
            (key(2), with(15, Some(from - 40 * 86400), Some(from + 10 * 86400))),
        ]);
        set_undercut_rates(&mut long, from, to);
        let rate = long[&key(1)].undercuts_per_day.unwrap();
        assert!((rate - 1.0).abs() < 1e-3, "{rate}");
        assert!((long[&key(2)].undercuts_per_day.unwrap() - 0.5).abs() < 1e-3);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ultros-clickhouse --lib undercut_rate`
Expected: compile error `cannot find function set_undercut_rates`.

- [ ] **Step 3: Add `empty_window`, `set_undercut_rates`, `reprice_sql`, and the merge**

In `ultros-clickhouse/src/listing_history.rs`:

(a) Replace the `or_insert_with` closure body in `add_missing_receipts` with the shared helper. Change

```rust
    for row in missing {
        output
            .entry((row.item_id, row.hq != 0))
            .or_insert_with(|| ListingWindowStats {
                window_days: days,
                from,
                to,
                floor_unknown_secs: (to - from) as u64,
                matches: MatchedSalesStats {
                    settled_through_unix: to - 601,
                    ..Default::default()
                },
                ..Default::default()
            })
            .matches
            .sales_without_receipt = row.n;
    }
```

to

```rust
    for row in missing {
        output
            .entry((row.item_id, row.hq != 0))
            .or_insert_with(|| empty_window(days, from, to))
            .matches
            .sales_without_receipt = row.n;
    }
```

and add, directly above `add_missing_receipts`:

```rust
/// A key the scope knows about (a sale, a reprice) without any listing
/// observation of its own: the floor is unknown for the whole window.
fn empty_window(days: u16, from: i64, to: i64) -> ListingWindowStats {
    ListingWindowStats {
        window_days: days,
        from,
        to,
        floor_unknown_secs: (to - from) as u64,
        matches: MatchedSalesStats {
            settled_through_unix: to - 601,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// `undercuts` per day of scope-wide listing coverage. One denominator for
/// every key: an item's own span measures its activity, not ingestion
/// coverage (two undercuts three hours apart would read as 16/day), and a
/// 30/90-day window that history has not filled yet needs the real span.
/// Observations before `from` come from the reducer's `from - 600` read and
/// do not count; the span is floored to one day and capped at the window.
pub fn set_undercut_rates(
    output: &mut BTreeMap<(i32, bool), ListingWindowStats>,
    from: i64,
    to: i64,
) {
    let mut first: Option<i64> = None;
    let mut last: Option<i64> = None;
    for stats in output.values() {
        if let Some(f) = stats.listing_coverage.first_observed_unix {
            first = Some(first.map_or(f, |v| v.min(f)));
        }
        if let Some(l) = stats.listing_coverage.last_observed_unix {
            last = Some(last.map_or(l, |v| v.max(l)));
        }
    }
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    let span = (last - first.max(from)).clamp(86400, (to - from).max(86400));
    let days = span as f64 / 86400.0;
    for stats in output.values_mut() {
        stats.undercuts_per_day = Some(stats.undercuts as f64 / days);
    }
}

/// Same-listing price drops, paired in ClickHouse so no listing id string
/// reaches the Rust reducer. `DISTINCT` mirrors the events read: a retried
/// writer batch stores every row twice. The first row of a partition has no
/// predecessor; `lagInFrame` yields the type default (0) and the
/// `prev_removed = 1` test rejects it.
fn reprice_sql(item_sql: &str, world_sql: &str, from: i64, to: i64) -> String {
    format!(
        "SELECT item_id, hq, count() AS undercuts, quantileExact(0.5)(drop) AS undercut_median FROM (
        SELECT item_id, hq, (prev_price - price_per_unit) / prev_price AS drop
        FROM (SELECT DISTINCT item_id, hq, world_id, listing_id, event_time, price_per_unit, prev_price FROM listing_events
              WHERE kind = 'updated' AND source != 'snapshot' AND item_id IN ({item_sql}) AND world_id IN ({world_sql})
                AND event_time >= toDateTime({from}) AND event_time < toDateTime({to}))
        WHERE prev_price > price_per_unit
        UNION ALL
        SELECT item_id, hq, (prev_price - price_per_unit) / prev_price AS drop
        FROM (SELECT item_id, hq, kind, event_time, price_per_unit,
                     lagInFrame(kind = 'removed') OVER w AS prev_removed,
                     lagInFrame(price_per_unit) OVER w AS prev_price,
                     lagInFrame(event_time) OVER w AS prev_time
              FROM (SELECT DISTINCT item_id, hq, world_id, listing_id, kind, event_time, price_per_unit FROM listing_events
                    WHERE kind IN ('removed', 'added') AND source != 'snapshot' AND listing_id != ''
                      AND item_id IN ({item_sql}) AND world_id IN ({world_sql})
                      AND event_time >= toDateTime({}) AND event_time < toDateTime({to}))
              WINDOW w AS (PARTITION BY item_id, hq, world_id, listing_id ORDER BY event_time ROWS BETWEEN 1 PRECEDING AND CURRENT ROW))
        WHERE kind = 'added' AND prev_removed = 1 AND event_time >= toDateTime({from})
          AND dateDiff('second', prev_time, event_time) <= 600 AND prev_price > price_per_unit
    ) GROUP BY item_id, hq{LIMITS}",
        from - 600
    )
}

#[derive(Row, Deserialize)]
struct RepriceRow {
    item_id: i32,
    hq: u8,
    undercuts: u64,
    undercut_median: f64,
}
```

(b) In `window_items`, directly after the `receipts` read (the line ending `.fetch_all::<SaleReceiptRow>().await?;`), add:

```rust
    let reprices = ch
        .client()
        .query(&reprice_sql(&item_sql, &world_sql, from, to))
        .fetch_all::<RepriceRow>()
        .await?;
```

and directly before `add_missing_receipts(ch, &mut output, ...)` at the end of the function, add:

```rust
    // A reprice always has its add/update event in the same read, so the key
    // exists; merge defensively anyway.
    for row in reprices {
        let stats = output
            .entry((row.item_id, row.hq != 0))
            .or_insert_with(|| empty_window(days, from, to));
        stats.undercuts = row.undercuts;
        stats.undercut_median = (row.undercuts > 0).then_some(row.undercut_median);
    }
```

(c) In `window()`, change the tail

```rust
    let mut output = BTreeMap::new();
    while let Some(batch) = pending.try_next().await? {
        output.extend(batch);
    }
    Ok(output)
```

to

```rust
    let mut output = BTreeMap::new();
    while let Some(batch) = pending.try_next().await? {
        output.extend(batch);
    }
    set_undercut_rates(&mut output, from, to);
    Ok(output)
```

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p ultros-clickhouse --lib listing_history`
Expected: PASS, including the three pre-existing matcher tests.

- [ ] **Step 5: Extend the gated smoke test**

In `ultros-clickhouse/tests/listing_history_smoke.rs`, after the `INSERT INTO listing_events SELECT * FROM listing_events WHERE item_id = {item}` duplicate insert (the block ending at line ~62), add a second item covering every reprice shape:

```rust
    // Undercut shapes on a separate key. Only three count: the in-place
    // update, the same-id pair 30 s apart, and the pair whose removal
    // precedes `from` by less than 600 s. VALUES order: event_time, kind,
    // source, item_id, hq, world_id, listing_id, pg_listing_id, retainer_id,
    // price_per_unit, quantity, prev_price, prev_quantity, reviewed_at.
    let reprice_item = item + 10;
    ch.client()
        .query(&format!(
            "INSERT INTO listing_events VALUES
        ({removal},'removed','websocket',{reprice_item},0,1,'pair-a',21,21,100,1,0,0,{removal}),
        ({},'added','websocket',{reprice_item},0,1,'pair-a',21,21,80,1,0,0,{removal}),
        ({removal},'removed','websocket',{reprice_item},0,1,'pair-up',22,22,100,1,0,0,{removal}),
        ({},'added','websocket',{reprice_item},0,1,'pair-up',22,22,120,1,0,0,{removal}),
        ({},'removed','websocket',{reprice_item},0,1,'pair-slow',23,23,100,1,0,0,{removal}),
        ({removal},'added','websocket',{reprice_item},0,1,'pair-slow',23,23,50,1,0,0,{removal}),
        ({removal},'removed','websocket',{reprice_item},0,1,'',24,24,100,1,0,0,{removal}),
        ({},'added','websocket',{reprice_item},0,1,'',24,24,50,1,0,0,{removal}),
        ({removal},'updated','snapshot',{reprice_item},0,1,'snap',25,25,50,1,100,1,{removal}),
        ({removal},'updated','websocket',{reprice_item},0,1,'up',26,26,90,1,100,1,{removal}),
        ({},'removed','websocket',{reprice_item},0,1,'pair-before',27,27,100,1,0,0,{removal}),
        ({},'added','websocket',{reprice_item},0,1,'pair-before',27,27,60,1,0,0,{removal})",
            removal + 30,
            removal + 30,
            removal - 3000,
            removal + 30,
            from - 100,
            from + 100
        ))
        .execute()
        .await
        .unwrap();
    // The same acknowledged-late retry as above: every row twice.
    ch.client()
        .query(&format!(
            "INSERT INTO listing_events SELECT * FROM listing_events WHERE item_id = {reprice_item}"
        ))
        .execute()
        .await
        .unwrap();
```

Then, after the existing assertions on `stats` (right after `assert!(!stats.listing_coverage.continuity_verified);`), add:

```rust
    // The fixture's own 60 -> 50 update is the key's only undercut; the
    // retainer-heuristic `repriced` pair uses two listing ids and does not count.
    assert_eq!(stats.undercuts, 1);
    assert!((stats.undercut_median.unwrap() - 1.0 / 6.0).abs() < 1e-9);
    // Scope span: first in-window observation at `from`, last at `to - 100`,
    // floored to one day.
    assert!((stats.undercuts_per_day.unwrap() - 1.0).abs() < 1e-9);
    let reprices = &rows[&(reprice_item, false)];
    assert_eq!(reprices.undercuts, 3, "update, 30 s pair, pre-window removal pair");
    assert!((reprices.undercut_median.unwrap() - 0.2).abs() < 1e-9);
    assert!((reprices.undercuts_per_day.unwrap() - 3.0).abs() < 1e-9);
```

- [ ] **Step 6: Run the smoke test against a throwaway ClickHouse**

The test only runs with `ULTROS_CH_INTEGRATION=1`, a loopback `CLICKHOUSE_URL` and a `CLICKHOUSE_DATABASE` starting with `ultros_t12_`. Start a scratch container (or reuse one on `127.0.0.1:53135` if it is already up; check with `docker ps`):

```bash
docker run -d --name ultros-ch-scratch -p 53135:8123 clickhouse/clickhouse-server:latest
```

Then:

```bash
CLICKHOUSE_URL=http://127.0.0.1:53135 CLICKHOUSE_DATABASE=ultros_t12_undercut CLICKHOUSE_USER=default CLICKHOUSE_PASSWORD= ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_history_smoke -- --nocapture
```

Expected: PASS. `ClickHouseClient::from_env` reads exactly `CLICKHOUSE_URL`, `CLICKHOUSE_DATABASE`, `CLICKHOUSE_USER` (default `ultros`, hence the explicit `default`) and `CLICKHOUSE_PASSWORD`. Do not point this at the shared dev ClickHouse.

If `lagInFrame(kind = 'removed')` errors on the enum comparison inside a window function, use `lagInFrame(toUInt8(kind = 'removed'))` and keep `prev_removed = 1`.

- [ ] **Step 7: Run CI checks and commit**

```bash
./check_ci.sh > ./ci-undercut.log 2>&1; echo "REAL_EXIT=$?"; tail -30 ./ci-undercut.log
```
Expected: `REAL_EXIT=0`.

```bash
git add ultros-clickhouse/src/listing_history.rs ultros-clickhouse/tests/listing_history_smoke.rs
git commit -m "feat(clickhouse): same-listing undercut count, median drop and per-day rate in listing windows

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Client API function for the windowed body

**Files:**
- Modify: `ultros-frontend/ultros-frontend-core/src/api.rs:308-310`.

**Interfaces:**
- Produces: `pub async fn get_listing_stats_window(scope_name: &str, days: u16) -> AppResult<BulkListingStats>`. Task 5 calls it.

- [ ] **Step 1: Add the function**

Directly after `get_listing_stats`:

```rust
/// The alive set plus `window` history for `days` (1/7/30/90) from the
/// committed exact-scope snapshot. A cold scope/window pair is 503 until the
/// server's background worker publishes a generation; callers retry.
pub async fn get_listing_stats_window(
    scope_name: &str,
    days: u16,
) -> AppResult<BulkListingStats> {
    fetch_api(&format!("/api/v1/listing_stats/{scope_name}?window={days}")).await
}
```

- [ ] **Step 2: Check it compiles for both targets**

Run: `cargo check -p ultros-frontend-core`
Expected: clean (an unused-function warning is fine until Task 5 wires it; clippy `-D warnings` runs at the end of Task 5).

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-frontend-core/src/api.rs
git commit -m "feat(frontend-core): get_listing_stats_window client call

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Column definitions, labels, picker entries and locale keys

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/stat_columns.rs` (listing section lines 116-180, `shared_cols_in` at 180-190, `market_picker_options` at 296-330, tests at 395+).
- Modify: `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` (insert after the `"market_picker_group_listings"` line, line 336 in every file; line 337 is another key, so every inserted line ends with a comma).

**Interfaces:**
- Produces: `pub enum ListingWindowKind { UndercutsPerDay, UndercutMedian }`, `pub static LISTING_WINDOW_COLUMNS: [(ListingWindowKind, &str); 2]`, `pub fn listing_window_id(kind) -> &'static str`, `pub fn listing_window_wanted(needs: &HashSet<String>) -> bool`, `pub fn listing_window_label(kind, window: Window) -> String`, `pub fn listing_window_title(kind) -> String`. Task 5 uses all of them.

- [ ] **Step 1: Write the failing tests**

In `stat_columns.rs` `mod tests`, add after `listing_columns_are_window_free_unique_and_wanted_together`:

```rust
    #[test]
    fn listing_window_columns_are_follow_window_ids_wanted_together() {
        let ids: HashSet<_> = LISTING_WINDOW_COLUMNS.iter().map(|(_, id)| *id).collect();
        assert_eq!(ids.len(), 2);
        for (kind, id) in &LISTING_WINDOW_COLUMNS {
            assert_eq!(listing_window_id(*kind), *id);
            assert!(!STAT_COLUMNS.iter().any(|c| c.id == *id), "{id} collides");
            assert!(!FOLLOW_COLUMNS.iter().any(|(_, f)| f == id), "{id} collides");
            assert!(!LISTING_COLUMNS.iter().any(|(_, f)| f == id), "{id} collides");
            for window in Window::ALL {
                assert!(!id.ends_with(&format!("-{}", window.days())), "{id}");
            }
        }
        let needs: HashSet<String> = ["market-undercut-pct", "roi"].map(str::to_owned).into();
        assert!(listing_window_wanted(&needs));
        assert!(!listings_wanted(&needs), "history columns never want the alive set");
        assert!(required_windows(&needs, Window::D7, false).is_empty());
        let none: HashSet<String> = ["market-alive"].map(str::to_owned).into();
        assert!(!listing_window_wanted(&none));
        assert_eq!(
            shared_cols_in(Some("profit,market-undercuts,market-alive")),
            HashSet::from(["market-undercuts", "market-alive"])
        );
    }
```

And extend `labels_and_picker_match_the_legacy_seven_day_text`: change the `options.len()` assertion to

```rust
            assert_eq!(
                options.len(),
                STAT_COLUMNS.len()
                    + FOLLOW_COLUMNS.len()
                    + LISTING_COLUMNS.len()
                    + LISTING_WINDOW_COLUMNS.len()
            );
```

replace `assert_eq!(options.last().unwrap().id, "market-oldest-listing");` with

```rust
            assert_eq!(options.last().unwrap().id, "market-undercut-pct");
            let undercuts = options.iter().find(|o| o.id == "market-undercuts").unwrap();
            assert_eq!(undercuts.label, "Undercuts/day (7d)");
            assert_eq!(
                undercuts.group.as_ref().map(|g| g.label.as_str()),
                Some("Listings")
            );
            assert_eq!(
                undercuts.hint.as_deref(),
                Some("Same-listing price drops per day across the scope, both edit-in-place and remove-and-relist. Raises and new listings are not counted.")
            );
            assert_eq!(
                listing_window_label(ListingWindowKind::UndercutMedian, Window::D30),
                "Undercut % (30d)"
            );
            assert_eq!(
                listing_window_title(ListingWindowKind::UndercutMedian),
                "Median drop as a share of the previous price across those undercuts."
            );
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --lib stat_columns`
Expected: compile error `cannot find value LISTING_WINDOW_COLUMNS`.

- [ ] **Step 3: Add the locale keys to all seven files**

Insert after the `"market_picker_group_listings": ...,` line in each file (keep the existing line, add four lines after it, every line ending with a comma):

`en.json`:
```json
    "market_undercuts": "Undercuts/day",
    "market_undercut_pct": "Undercut %",
    "market_undercuts_title": "Same-listing price drops per day across the scope, both edit-in-place and remove-and-relist. Raises and new listings are not counted.",
    "market_undercut_pct_title": "Median drop as a share of the previous price across those undercuts.",
```

`fr.json`:
```json
    "market_undercuts": "Sous-cotations/jour",
    "market_undercut_pct": "Sous-cotation %",
    "market_undercuts_title": "Baisses de prix d'une même annonce par jour sur la zone, qu'elle soit modifiée sur place ou retirée puis remise en vente. Les hausses et les nouvelles annonces ne sont pas comptées.",
    "market_undercut_pct_title": "Baisse médiane en part du prix précédent sur ces sous-cotations.",
```

`de.json`:
```json
    "market_undercuts": "Unterbietungen/Tag",
    "market_undercut_pct": "Unterbietung %",
    "market_undercuts_title": "Preissenkungen desselben Angebots pro Tag im Bereich, ob direkt geändert oder entfernt und neu eingestellt. Erhöhungen und neue Angebote zählen nicht.",
    "market_undercut_pct_title": "Mediane Senkung als Anteil des vorherigen Preises über diese Unterbietungen.",
```

`ja.json`:
```json
    "market_undercuts": "値下げ/日",
    "market_undercut_pct": "値下げ率",
    "market_undercuts_title": "範囲内で同じ出品が1日に値下げされた回数。出品の修正と、取り下げて再出品した場合の両方を含みます。値上げと新規出品は数えません。",
    "market_undercut_pct_title": "これらの値下げにおける、以前の価格に対する値下げ幅の中央値。",
```

`cn.json`:
```json
    "market_undercuts": "压价次数/日",
    "market_undercut_pct": "压价幅度 %",
    "market_undercuts_title": "范围内同一挂单每日降价的次数，包括直接修改和撤下后重新上架。涨价和新挂单不计入。",
    "market_undercut_pct_title": "这些压价中，相对上一价格的降幅中位数。",
```

`ko.json`:
```json
    "market_undercuts": "가격 인하/일",
    "market_undercut_pct": "가격 인하 %",
    "market_undercuts_title": "범위 내 동일 매물의 하루 가격 인하 횟수로, 직접 수정과 내렸다가 다시 올린 경우를 모두 포함합니다. 인상과 신규 매물은 세지 않습니다.",
    "market_undercut_pct_title": "이러한 가격 인하에서 이전 가격 대비 인하폭의 중앙값입니다.",
```

`tc.json`:
```json
    "market_undercuts": "壓價次數/日",
    "market_undercut_pct": "壓價幅度 %",
    "market_undercuts_title": "範圍內同一掛單每日降價的次數，包括直接修改與下架後重新上架。漲價與新掛單不計入。",
    "market_undercut_pct_title": "這些壓價中，相對上一價格的降幅中位數。",
```

Use the Write/Edit tools, not a Bash heredoc, so the non-ASCII text survives. Afterwards verify every file still parses and every file has all four keys:

```bash
for f in en fr de ja cn ko tc; do python3 -c "import json,sys; d=json.load(open('ultros-frontend/ultros-i18n/locales/$f.json', encoding='utf-8')); print('$f', all(k in d for k in ['market_undercuts','market_undercut_pct','market_undercuts_title','market_undercut_pct_title']))"; done
```
Expected: seven lines ending in `True`.

- [ ] **Step 4: Add the kind, ids, labels and picker entries**

In `stat_columns.rs`, directly after `listing_title` (ends around line 178):

```rust
/// One statistic read from `ItemListingStats::window`: listing history over
/// the page window, so labels carry the window suffix like the
/// follow-window sale columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListingWindowKind {
    /// Same-listing price drops per day of scope-wide listing coverage.
    UndercutsPerDay,
    /// Median relative drop across those undercuts, shown as a percentage.
    UndercutMedian,
}

/// Follow-window ids only: one body per scope and selected window, never a
/// pinned `-N` variant, because every window body is a separate fetch.
pub static LISTING_WINDOW_COLUMNS: [(ListingWindowKind, &str); 2] = [
    (ListingWindowKind::UndercutsPerDay, "market-undercuts"),
    (ListingWindowKind::UndercutMedian, "market-undercut-pct"),
];

pub fn listing_window_id(kind: ListingWindowKind) -> &'static str {
    LISTING_WINDOW_COLUMNS
        .iter()
        .find(|(k, _)| *k == kind)
        .unwrap()
        .1
}

/// Whether any windowed listing column is in the grid's wanted set.
pub fn listing_window_wanted(needs: &HashSet<String>) -> bool {
    LISTING_WINDOW_COLUMNS
        .iter()
        .any(|(_, id)| needs.contains(*id))
}

pub fn listing_window_label(kind: ListingWindowKind, window: Window) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let name = match kind {
        ListingWindowKind::UndercutsPerDay => t_string!(i18n, market_undercuts),
        ListingWindowKind::UndercutMedian => t_string!(i18n, market_undercut_pct),
    }
    .to_string();
    with_window(name, window)
}

/// Hover text: what counts as an undercut, and what the percentage is of.
pub fn listing_window_title(kind: ListingWindowKind) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match kind {
        ListingWindowKind::UndercutsPerDay => t_string!(i18n, market_undercuts_title),
        ListingWindowKind::UndercutMedian => t_string!(i18n, market_undercut_pct_title),
    }
    .to_string()
}
```

`with_window` is defined further down the file (a private `fn with_window(name: String, window: Window) -> String`); Rust does not care about order.

In `shared_cols_in`, add one more chain link after the `LISTING_COLUMNS` one:

```rust
        .chain(LISTING_WINDOW_COLUMNS.iter().map(|(_, id)| *id))
```

In `market_picker_options`, after the `.chain(LISTING_COLUMNS.iter().map(...))` block and before `.collect()`:

```rust
        .chain(LISTING_WINDOW_COLUMNS.iter().map(|(kind, id)| ColumnOption {
            id,
            label: listing_window_label(*kind, window),
            group: Some(PickerHeading {
                label: market_picker_group_listings(),
                title: None,
            }),
            disabled: false,
            hint: Some(listing_window_title(*kind)),
        }))
```

Update the doc comment above `market_picker_options` to mention the history columns: change `columns under "Listings".` to `columns and the windowed listing history under "Listings".`

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-app --lib stat_columns`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/stat_columns.rs ultros-frontend/ultros-i18n/locales
git commit -m "feat(analyzer-kit): undercut column ids, labels, picker entries and locale keys

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Per-window listing slots, retry ladder, and the two metrics in the market kit

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/market.rs` (imports 1-50, `MarketData` 76-162, constructor 205-230, `fetch_listing_stats` 236-277, `MarketMetric` 455-520, `market_metrics` 540-560, `metric_title`/`metric_label` 566-593, `listing_value` 635-655, `market_value` 720-780, `display_value` 782-820, the `needs` effect 977-985, tests 1240+).

**Interfaces:**
- Consumes: Task 3's `get_listing_stats_window`; Task 4's `ListingWindowKind`, `LISTING_WINDOW_COLUMNS`, `listing_window_id`, `listing_window_label`, `listing_window_title`, `listing_window_wanted`; Task 1's fields.
- Produces: `MarketData::listing_window(self, window: Window) -> Option<ListingSlot>`, `MarketData::want_listing_window(self, window: Window)`, `MarketMetric::ListingWindow(ListingWindowKind)`, `fn listing_window_value(kind: ListingWindowKind, stats: Option<&ItemListingStats>) -> GridValue`.

- [ ] **Step 1: Write the failing tests**

In `market.rs` `mod tests`, after `listing_columns_distinguish_pending_failed_empty_and_absent_rows`, add:

```rust
    #[test]
    fn listing_window_columns_follow_the_selected_window_and_its_states() {
        use ultros_api_types::listing_stats::ListingWindowStats;
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let scope = RwSignal::new("Gilgamesh".to_owned());
            let selected = RwSignal::new(Window::D7);
            let mut market = use_market_data(scope.into());
            market.window.selected = Memo::new(move |_| selected.get());
            let sparks = RwSignal::new(MarketSparkStore::default());
            let scope_world = Memo::new(|_| None);
            let worlds = Arc::new(HashMap::new());
            let subject = MarketSubject::new(42, true, 7);
            let value = |kind| {
                market_value(
                    MarketMetric::ListingWindow(kind),
                    &subject,
                    market,
                    sparks,
                    scope_world,
                    &worlds,
                )
            };
            let with_window = |undercuts_per_day, undercut_median| ItemListingStats {
                window: Some(ListingWindowStats {
                    window_days: 7,
                    undercuts: 3,
                    undercuts_per_day,
                    undercut_median,
                    ..Default::default()
                }),
                ..row(42, true, 3)
            };
            let slot = |scope: &str, rows: Vec<ItemListingStats>, failed| {
                Some(ListingSlot {
                    scope: scope.into(),
                    index: Arc::new(listing_index(&rows)),
                    failed,
                    fetched_unix: 1_000_000,
                })
            };
            // Nothing wanted yet: the slot is empty and cells wait.
            assert!(!market.listing_windows_wanted[Window::D7.index()].get_untracked());
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Pending);
            market.want_listing_window(Window::D7);
            assert!(market.listing_windows_wanted[Window::D7.index()].get_untracked());
            assert!(!market.listing_windows_wanted[Window::D30.index()].get_untracked());
            // Another scope's body never answers; a failed body is unknown.
            market.listing_windows[Window::D7.index()]
                .set(slot("Cactuar", vec![with_window(Some(0.5), Some(0.25))], false));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Pending);
            market.listing_windows[Window::D7.index()].set(slot("Gilgamesh", Vec::new(), true));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Unavailable);
            assert_eq!(value(ListingWindowKind::UndercutMedian), GridValue::Unavailable);
            // An empty successful body, a row without history, and an
            // NQ row for an HQ subject are all missing, never zero.
            market.listing_windows[Window::D7.index()].set(slot("Gilgamesh", Vec::new(), false));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Missing);
            market.listing_windows[Window::D7.index()]
                .set(slot("Gilgamesh", vec![row(42, true, 3)], false));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Missing);
            market.listing_windows[Window::D7.index()].set(slot(
                "Gilgamesh",
                vec![ItemListingStats { hq: false, ..with_window(Some(0.5), Some(0.25)) }],
                false,
            ));
            assert_eq!(value(ListingWindowKind::UndercutMedian), GridValue::Missing);
            market.listing_windows[Window::D7.index()]
                .set(slot("Gilgamesh", vec![with_window(Some(0.5), Some(0.25))], false));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Number(0.5));
            assert_eq!(value(ListingWindowKind::UndercutMedian), GridValue::Number(25.0));
            // No undercuts: the rate is a real zero, the median is missing.
            market.listing_windows[Window::D7.index()]
                .set(slot("Gilgamesh", vec![with_window(Some(0.0), None)], false));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Number(0.0));
            assert_eq!(value(ListingWindowKind::UndercutMedian), GridValue::Missing);
            // The column reads the selected window's slot, not the seven-day one.
            selected.set(Window::D30);
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Pending);
            assert_eq!(
                metric_by_id("market-undercuts").unwrap().window(selected.get()),
                Some(Window::D30)
            );
            market.listing_windows[Window::D30.index()]
                .set(slot("Gilgamesh", vec![with_window(Some(1.5), Some(0.1))], false));
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Number(1.5));
            scope.set("Cactuar".into());
            assert_eq!(value(ListingWindowKind::UndercutsPerDay), GridValue::Pending);
        });
    }

    #[test]
    fn undercut_cells_format_as_rate_and_percent() {
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::UndercutsPerDay),
                GridValue::Number(0.29)
            ),
            "0.29"
        );
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::UndercutMedian),
                GridValue::Number(2.6)
            ),
            "2.6%"
        );
        assert_eq!(
            display_value(
                MarketMetric::ListingWindow(ListingWindowKind::UndercutMedian),
                GridValue::Missing
            ),
            "—"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ultros-app --lib analyzer_kit::market`
Expected: compile errors (`no variant ListingWindow`, `no field listing_windows`).

- [ ] **Step 3: Implement the slots, fetch, metric and values**

All in `market.rs`.

(a) Imports. Change the `api::{...}` import to
```rust
    api::{get_listing_stats, get_listing_stats_window, get_sale_stats, post_sparklines},
```
and the `stat_columns::{...}` import to
```rust
    stat_columns::{
        FOLLOW_COLUMNS, LISTING_COLUMNS, LISTING_WINDOW_COLUMNS, ListingKind, ListingWindowKind,
        STAT_COLUMNS, StatKind, Window, follow_id, listing_id, listing_label, listing_title,
        listing_window_id, listing_window_label, listing_window_title, listing_window_wanted,
        listings_wanted, market_picker_group, market_picker_group_listings, required_windows,
        stat_column, stat_label,
    },
```

(b) `MarketData` fields. After `listings_wanted: RwSignal<bool>,` add:
```rust
    /// Listing history per window, fetched with `?window=N` only when a
    /// windowed listing column wants the selected window. Each window body
    /// is a separate multi-megabyte fetch, so nothing pins a window.
    listing_windows: [RwSignal<ScopedListings>; Window::ALL.len()],
    listing_windows_wanted: [RwSignal<bool>; Window::ALL.len()],
```

(c) Methods. After `want_listings`, add:
```rust
    /// The windowed listing body for the present scope and `window`, once it
    /// has landed. `failed` is set only after the retry ladder is exhausted.
    pub fn listing_window(self, window: Window) -> Option<ListingSlot> {
        let scope = self.scope.get();
        self.listing_windows[window.index()]
            .with(|v| v.as_ref().filter(|slot| slot.scope == scope).cloned())
    }

    /// Ask for a window's listing history. Idempotent; never un-wants.
    pub fn want_listing_window(self, window: Window) {
        let flag = self.listing_windows_wanted[window.index()];
        if !flag.get_untracked() {
            flag.set(true);
        }
    }
```
In `track_all`, after `self.listings.with(|_| ());` add:
```rust
        for slot in self.listing_windows {
            slot.with(|_| ());
        }
```

(d) Constructor (`use_market_data_configured`). Add the two fields to the struct literal:
```rust
        listing_windows: std::array::from_fn(|_| RwSignal::new(None)),
        listing_windows_wanted: std::array::from_fn(|_| RwSignal::new(false)),
```
Replace the call `fetch_listing_stats(scope, market.listings, market.listings_wanted.into());` with:
```rust
    fetch_listing_stats(scope, market.listings, market.listings_wanted.into(), None);
    for window in Window::ALL {
        fetch_listing_stats(
            scope,
            market.listing_windows[window.index()],
            market.listing_windows_wanted[window.index()].into(),
            Some(window.days()),
        );
    }
```

(e) `fetch_listing_stats`. Change the signature to
```rust
fn fetch_listing_stats(
    scope: Signal<String>,
    output: RwSignal<ScopedListings>,
    wanted: Signal<bool>,
    days: Option<u16>,
) {
```
and replace the `spawn_local` body:
```rust
        leptos::task::spawn_local(async move {
            // A cold scope/window pair is 503 until the snapshot worker
            // publishes (about a minute for a world, longer for a DC), so a
            // windowed body waits and retries; the alive set never retries.
            const RETRY_MS: [u32; 3] = [15_000, 30_000, 60_000];
            let mut attempt = 0usize;
            let result = loop {
                let result = match days {
                    Some(days) => get_listing_stats_window(&name, days).await,
                    None => get_listing_stats(&name).await,
                };
                if result.is_ok() || days.is_none() || attempt == RETRY_MS.len() {
                    break result;
                }
                gloo_timers::future::TimeoutFuture::new(RETRY_MS[attempt]).await;
                attempt += 1;
                if scope.try_get_untracked().as_ref() != Some(&name)
                    || generation.try_get_value() != Some(epoch)
                {
                    return;
                }
            };
            let result = result.map(|body| listing_index(&body.stats));
            let failed = result.is_err();
            let index = result.unwrap_or_default();
            if scope.try_get_untracked().as_ref() != Some(&name)
                || generation.try_get_value() != Some(epoch)
            {
                return;
            }
            let _ = output.try_set(Some(ListingSlot {
                scope: name,
                index: Arc::new(index),
                failed,
                fetched_unix: chrono::Utc::now().timestamp(),
            }));
        });
```
Update the doc comment above the function: after the existing sentence add `A windowed body (` `days` `is` `Some` `) retries a failure three times before it settles as failed.`

(f) `MarketMetric`. Add a variant after `Listings(ListingKind)`:
```rust
    /// Listing history over the selected window; ids and labels come from
    /// `LISTING_WINDOW_COLUMNS`.
    ListingWindow(ListingWindowKind),
```
In `id()`: `Self::ListingWindow(kind) => listing_window_id(kind),`.
In `window()`: `Self::ListingWindow(_) => Some(selected),` (placed before the `_ => None` arm).
`text()` and `partial()` need no change.
In `market_metrics()`, after the `LISTING_COLUMNS` chain:
```rust
        .chain(
            LISTING_WINDOW_COLUMNS
                .iter()
                .map(|(kind, _)| MarketMetric::ListingWindow(*kind)),
        )
```

(g) Labels and titles. In `metric_title`: add `MarketMetric::ListingWindow(kind) => Some(listing_window_title(kind)),` before `_ => None`. In `metric_label`: add `MarketMetric::ListingWindow(kind) => return listing_window_label(kind, selected),` next to the `Listings` arm.

(h) Values. After `listing_value`, add:
```rust
/// A row absent from a successful body, or one the server synthesized for a
/// newly alive key without a snapshot row (`window` is `None`), has no
/// history to show. A zero rate is a real zero; a missing median means no
/// undercuts happened.
fn listing_window_value(kind: ListingWindowKind, stats: Option<&ItemListingStats>) -> GridValue {
    let Some(window) = stats.and_then(|s| s.window.as_ref()) else {
        return GridValue::Missing;
    };
    match kind {
        ListingWindowKind::UndercutsPerDay => number(window.undercuts_per_day),
        ListingWindowKind::UndercutMedian => number(window.undercut_median.map(|m| m * 100.0)),
    }
}
```
In `market_value`, add an arm after the `MarketMetric::Listings(kind) => ...` arm:
```rust
        MarketMetric::ListingWindow(kind) => {
            match market.listing_window(market.window.selected.get()) {
                None => GridValue::Pending,
                Some(slot) if slot.failed => GridValue::Unavailable,
                Some(slot) => {
                    listing_window_value(kind, slot.index.get(&(subject.item_id, subject.hq)))
                }
            }
        }
```

(i) Display. In `display_value`, add before the `{n:.2}` branch:
```rust
        GridValue::Number(n)
            if matches!(
                metric,
                MarketMetric::ListingWindow(ListingWindowKind::UndercutMedian)
            ) =>
        {
            format!("{n:.1}%")
        }
```
and extend the `{n:.2}` branch's pattern:
```rust
                MarketMetric::Stat(StatKind::SalesPerDay | StatKind::Cadence, _)
                    | MarketMetric::Follow(StatKind::SalesPerDay | StatKind::Cadence)
                    | MarketMetric::ListingWindow(ListingWindowKind::UndercutsPerDay)
```

(j) Wanted gate. In the `Effect::new(move |_| { needs.with(|n| { ... }) })` block in `MarketGrid` (the one calling `market.want(window)` and `market.want_listings()`), add after `if listings_wanted(n) { market.want_listings(); }`:
```rust
            if listing_window_wanted(n) {
                market.want_listing_window(market.window.selected.get());
            }
```
This effect already re-runs when `needs` changes; it must also re-run when the page window changes, and `market.window.selected.get()` inside the closure subscribes it.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-app --lib analyzer_kit`
Expected: PASS, including the pre-existing `listing_columns_distinguish_pending_failed_empty_and_absent_rows` and `stat_columns` tests.

- [ ] **Step 5: Run CI checks and commit**

```bash
./check_ci.sh > ./ci-undercut.log 2>&1; echo "REAL_EXIT=$?"; tail -30 ./ci-undercut.log
```
Expected: `REAL_EXIT=0`. A clippy complaint about the `ListingWindow` arm being unreachable or a `match` that should be `if let` must be fixed in code, not allowed.

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/market.rs
git commit -m "feat(analyzer-kit): windowed listing slots with a retry ladder; Undercuts/day and Undercut % columns

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: End-to-end probes

**Files:**
- Modify: `integration/market-window.cjs` (fixture interception lines 15-60, the current-listing block lines 200-235, the Cactuar failure block lines 275-290, the expected-failures list at the end).
- Modify: `integration/shared-analyzer-market-fixture.cjs:43-48`.
- Modify: `integration/shared-analyzer-data.cjs:382-385` and `457-464`.

**Interfaces:**
- Consumes: the running app from Task 5 (`PORT=8093 HOSTNAME=127.0.0.1` local serve, or the harness in `scripts/run_e2e.sh`), column ids `market-undercuts` / `market-undercut-pct`, heading text `Undercuts/day (30d)`, title text `price drops per day`.

- [ ] **Step 1: Teach the market-window fixture about windowed listing bodies**

In `integration/market-window.cjs`:

(a) Change `isExpectedFailure` so the windowed Cactuar 503 is expected once:
```js
      url.pathname === '/api/v1/listing_stats/Cactuar' && (!url.search || url.search === '?window=30')
```

(b) Next to `const listingHits = new Map();` add `const listingWindowHits = new Map();` and a body builder:
```js
  // Every field of ListingWindowStats is required by the wire except the
  // undercut trio; row 42 carries history, row 43 is a synthesized alive row.
  const windowBody = (days, now) => {
    const coverage = { first_observed_unix: now - days * 86400, last_observed_unix: now, observed_span_secs: days * 86400, continuity_verified: false };
    return { window_days: days, from: now - days * 86400, to: now, additions: 0, removals: 0, listing_coverage: coverage,
      floor_min: null, floor_max: null, floor_known_secs: 0, floor_empty_secs: 0, floor_unknown_secs: 0,
      matches: { matched: 0, ambiguous: 0, repriced: 0, unmatched: 0, sales_without_receipt: 0, receipt_coverage: coverage,
        received_sales: 0, settled_through_unix: now - 601, pending: 0, median_time_to_sell_secs: null, age_origin: 'last_review_time' },
      stock_status: 'unavailable', days_of_stock: null,
      undercuts: days, undercuts_per_day: days === 7 ? 0.29 : 1.5, undercut_median: 0.026 };
  };
```

(c) In the `listing_stats` interception, after `if (url.searchParams.has('window')) listingWindowed = true;` add:
```js
      const days = Number(url.searchParams.get('window'));
      if (days) {
        const key = `${scope}/${days}`;
        listingWindowHits.set(key, (listingWindowHits.get(key) || 0) + 1);
      }
```
and after the `body` literal add:
```js
      if (days) body.stats[0].window = windowBody(days, now);
```
Replace the final `return request.respond(scope === 'Cactuar' ? { status: 503 ... } : { status: 200 ... })` with:
```js
      // A cold Cactuar window is 503 once, then publishes; the alive set fails for good.
      const unavailable = scope === 'Cactuar' && (!days || listingWindowHits.get(`${scope}/${days}`) === 1);
      return request.respond(unavailable
        ? { status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'Listing statistics temporarily unavailable' }) }
        : { status: 200, contentType: 'application/json', body: JSON.stringify(body) }).catch(() => {});
```

- [ ] **Step 2: Add the windowed-column assertions**

In the same file, after the line `assert.equal(listingHits.get('Gilgamesh'), 1, 'hidden listing filters reuse the alive-set body');` and its following `await query({ gf: null, cols: 'market-sale-median,market-sale-median-7' }); await rows(3);`, insert (the page window is 30 here):

```js
    // Windowed listing columns fetch the selected window's body, only once wanted.
    assert.equal(listingWindowHits.size, 0, 'no windowed listing body before an undercut column is wanted');
    await query({ cols: 'market-sale-median,market-undercuts,market-undercut-pct' });
    await cell('market-undercuts', '1.50');
    await cell('market-undercut-pct', '2.6%');
    await cell('market-undercuts', '—'); // row 43: alive without a snapshot row; row 44 absent
    await heading('market-undercuts', 'Undercuts/day (30d)');
    assert.equal(listingWindowHits.get('Gilgamesh/30'), 1, 'both undercut columns share one request');
    assert.equal(listingHits.get('Gilgamesh'), 1, 'the windowed body does not refetch the alive set');
    assert.match(await page.$eval('[data-metric-sort="market-undercuts"]', el => el.parentElement.title), /price drops per day/);
    await Promise.all([
      page.waitForRequest(request => request.url().includes('listing_stats/Gilgamesh?window=7')),
      page.select('[data-market-window]', '7'),
    ]);
    await heading('market-undercuts', '(7d)');
    await cell('market-undercuts', '0.29');
    await page.click('[data-metric-sort="market-undercuts"]');
    await page.waitForFunction(() => new URL(location.href).searchParams.get('sort') === 'grid:market-undercuts');
    await first(42); // desc: 0.29, then the rows without history
    await page.select('[data-market-window]', '30');
    await heading('market-undercuts', '(30d)');
    await cell('market-undercuts', '1.50');
    assert.equal(listingWindowHits.get('Gilgamesh/30'), 1, 'returning to a window reuses its slot');
    await query({ cols: 'market-sale-median,market-sale-median-7', sort: 'grid:market-sale-median', dir: 'asc' });
    await rows(3);
```

Then, in the Cactuar failure block, after `assert.equal(listingHits.get('Gilgamesh'), 1, 'the old scope is not refetched');`, insert:

```js
    // A cold scope/window pair is 503 until the snapshot publishes: the cell
    // waits and the loader retries instead of settling on a failure.
    const windowed = request => request.url().includes('listing_stats/Cactuar?window=30');
    const firstTry = page.waitForRequest(windowed);
    await query({ cols: 'market-sale-median,market-undercuts' });
    await firstTry;
    await page.waitForRequest(windowed, { timeout: 25000 }); // the 15 s retry
    await cell('market-undercuts', '1.50');
    assert.equal(listingWindowHits.get('Cactuar/30'), 2);
    await query({ cols: 'market-sale-median,market-alive' });
```

Finally, extend the end-of-run expected-failure loop:
```js
    for (const endpoint of ['/api/v1/listing_stats/Cactuar', '/api/v1/listing_stats/Cactuar?window=30', '/api/v1/sale_stats/Cactuar?window=1', '/api/v1/sale_stats/Gilgamesh?window=1']) {
```
and add `windowed listing history` to the final `console.log('PASS market windows: ...')` sentence.

- [ ] **Step 3: Extend the shared analyzer probe**

In `integration/shared-analyzer-market-fixture.cjs`, replace the `listing_stats` branch with:
```js
    if (kind === 'listing_stats') {
      const days = Number(url.searchParams.get('window'));
      const now = Math.floor(Date.now() / 1000);
      const coverage = { first_observed_unix: now - 86400, last_observed_unix: now, observed_span_secs: 86400, continuity_verified: false };
      const window = days ? { window_days: days, from: now - days * 86400, to: now, additions: 0, removals: 0, listing_coverage: coverage,
        floor_min: null, floor_max: null, floor_known_secs: 0, floor_empty_secs: 0, floor_unknown_secs: 0,
        matches: { matched: 0, ambiguous: 0, repriced: 0, unmatched: 0, sales_without_receipt: 0, receipt_coverage: coverage,
          received_sales: 0, settled_through_unix: now - 601, pending: 0, median_time_to_sell_secs: null, age_origin: 'last_review_time' },
        stock_status: 'unavailable', days_of_stock: null, undercuts: 4, undercuts_per_day: 0.5, undercut_median: 0.1 } : undefined;
      body = { stats: ids.flatMap(item_id => [false, true].map(hq => ({
        item_id, hq, alive_count: hq ? 2 : 5, alive_units: hq ? 4 : 25, distinct_retainers: hq ? 2 : 3,
        oldest_reviewed_unix: now - 7200, median_age_secs: 1800, floor_alive: hq ? 1200 : 600, window,
      }))) };
    }
```

In `integration/shared-analyzer-data.cjs`, extend the `shared` list:
```js
        'market-alive', 'market-listing-age', 'market-undercuts', 'market-undercut-pct'];
```
and after the `market-listing-age` cell wait (`.some(cell => cell.textContent.trim() === '30m')`), add:
```js
          await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell[data-column="market-undercuts"]')]
            .some(cell => cell.textContent.trim() === '0.50'), { timeout: 90000 });
          await page.waitForFunction(() => [...document.querySelectorAll('.virtual-grid-cell[data-column="market-undercut-pct"]')]
            .some(cell => cell.textContent.trim() === '10.0%'), { timeout: 90000 });
```

- [ ] **Step 4: Build and serve the branch, run both probes**

Build the app (first build of the vendored OpenSSL takes ~10 minutes; foreground, 600000 ms timeout, re-run on timeout):
```bash
cargo leptos build 2>&1 | tail -5
```
Serve on a free port beside anything else running (the binary honours `PORT`; `METRICS_PORT` must differ from another server's):
```bash
PORT=8093 METRICS_PORT=9193 HOSTNAME=127.0.0.1 LEPTOS_SITE_ROOT=target/site ./target/debug/ultros.exe
```
(run in the background, note the PID, and check the log for `listening` before probing). Then:
```bash
BASE_URL=http://127.0.0.1:8093 node integration/market-window.cjs
```
Expected: `PASS market windows: ... windowed listing history ...`.
```bash
BASE_URL=http://127.0.0.1:8093 ANALYZER_TOOLS=flip-finder node integration/shared-analyzer-data.cjs
```
Expected: the flip-finder route passes including the two new cell waits. Stop the server afterwards (kill by PID; a running server locks `ultros.exe`).

If the market-window probe fails only on the 25 s retry wait, confirm the browser tab is fronted (a hidden pane throttles timers) before changing any code.

- [ ] **Step 5: Commit**

```bash
git add integration/market-window.cjs integration/shared-analyzer-market-fixture.cjs integration/shared-analyzer-data.cjs
git commit -m "test(e2e): windowed listing bodies, undercut columns and the cold-scope retry

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Changelog, final CI, pull request

**Files:**
- Create: `ultros-changelog/changes/2026-09-18-undercut-columns.json` (one JSON file per change; `build.rs` validates it and `cargo test -p ultros-changelog` exercises the crate).

- [ ] **Step 1: Add the changelog entry**

Create `ultros-changelog/changes/2026-09-18-undercut-columns.json`:
```json
{
  "category": "features",
  "importance": "medium",
  "title": "See how often listings get undercut",
  "blurb": "Every analyzer's column picker gains two Listings columns: Undercuts/day counts how often sellers drop the price of an existing listing over the selected window, and Undercut % shows the typical size of those drops."
}
```
Run: `cargo test -p ultros-changelog`
Expected: PASS (the build script rejects a malformed entry at compile time).

- [ ] **Step 2: Full CI check**

```bash
./check_ci.sh > ./ci-undercut.log 2>&1; echo "REAL_EXIT=$?"; tail -30 ./ci-undercut.log
```
Expected: `REAL_EXIT=0`. Also `cargo test -p ultros-api-types -p ultros-clickhouse --lib` and `cargo test -p ultros-app --lib analyzer_kit` green.

- [ ] **Step 3: Commit and push**

```bash
git add ultros-changelog/changes/2026-09-18-undercut-columns.json
git commit -m "docs(changelog): undercut columns

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
git push -u origin claude/ultros-listing-analyzer-columns-6ca829
```

- [ ] **Step 4: Open the PR**

```bash
gh pr create --base main --title "Undercuts/day and Undercut % columns on every analyzer grid (#1377)" --body-file - <<'EOF'
Two follow-window columns under the shared "Listings" picker group, fed by the windowed listing snapshot, plus the per-window listing slot the remaining #1343 history columns will reuse.

**Definition.** An undercut is a same-listing price drop: an `updated` event with a lower price, or a removed-then-added pair on the same Universalis listing id within ten minutes. Seed rows, id-less legacy listings, same-price pairs and raises are excluded. Prod (last 7 days): 810k updates, 96% of them drops, median 2.6%; ~260k remove-and-relist drops per day.

**Backend.** One bounded SQL aggregate per item batch in the snapshot reducer; ClickHouse does the pairing. Three serde-defaulted fields on `ListingWindowStats`: `undercuts`, `undercuts_per_day` (over scope-wide coverage, floored to a day, capped at the window, so 30/90d windows read correctly while history fills), `undercut_median`.

**Frontend.** Per-window listing slots in the market kit, fetched with `?window=N` only when a column is wanted, with a 15/30/60 s retry for the 503 a cold scope returns while the worker publishes. `market-undercuts` and `market-undercut-pct`, off by default, sortable and filterable like the alive columns. Seven locales.

**Tests.** Wire round-trip; rate-fold unit test; gated ClickHouse smoke case covering every reprice shape; market-kit state tests; `market-window.cjs` and the shared analyzer probe extended.

Spec: `docs/superpowers/specs/2026-09-18-undercut-columns-design.md`. Part of #1377 and #1343 (WS-J2).

Not in this PR: the item page, floor-drop metrics, pinned per-window variants, the other #1343 history columns, slimming the ~17 MB windowed payload.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
```

- [ ] **Step 5: Verify CI on the PR**

Use the `ccd_pr` tools (bind the PR, read status) rather than polling `gh`. Report the outcome; do not merge without the user.
