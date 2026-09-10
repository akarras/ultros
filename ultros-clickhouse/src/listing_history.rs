//! Windowed observations. No receipt is invented for backfill or historical sales.
use crate::{
    ClickHouseClient, ClickHouseError, floor_history,
    rows::{ListingEventKind, ListingEventRow, ListingEventSource, SaleReceiptRow},
};
use clickhouse::Row;
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use ultros_api_types::listing_stats::{HistoryCoverage, ListingWindowStats, MatchedSalesStats};

pub const WINDOWS: [u16; 4] = [1, 7, 30, 90];
const MATCH_SECS: i64 = 300;
const LIMITS: &str = " SETTINGS max_execution_time=10, max_result_rows=2000000, result_overflow_mode='throw', max_memory_usage=536870912";

pub fn coverage(times: impl Iterator<Item = i64>, from: i64, to: i64) -> HistoryCoverage {
    let mut result = HistoryCoverage::default();
    for time in times.filter(|t| *t < to) {
        result.first_observed_unix = Some(result.first_observed_unix.map_or(time, |v| v.min(time)));
        result.last_observed_unix = Some(result.last_observed_unix.map_or(time, |v| v.max(time)));
    }
    if let (Some(first), Some(last)) = (result.first_observed_unix, result.last_observed_unix) {
        result.observed_span_secs = (last - first.max(from)).max(0) as u64;
    }
    result
}

/// Discover item keys, then retain all worlds for each bounded item batch.
/// No per-world medians or floor extrema are merged: each item is reduced once
/// with its complete scope and matching context. Any failed batch fails the load.
pub async fn window(
    ch: &ClickHouseClient,
    worlds: &[i32],
    days: u16,
    to: i64,
) -> Result<BTreeMap<(i32, bool), ListingWindowStats>, ClickHouseError> {
    if !WINDOWS.contains(&days) || to < i64::from(days) * 86400 + 600 || to > i64::from(u32::MAX) {
        return Err(ClickHouseError::Backfill("invalid listing window".into()));
    }
    if worlds.is_empty() {
        return Ok(BTreeMap::new());
    }
    let from = to - i64::from(days) * 86400;
    let world_sql = worlds
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let items = ch.client().query(&format!("/* listing_history_items */ SELECT item_id,sum(event_rows) AS events,sum(receipt_rows) AS receipts,sum(floor_rows) AS floors FROM (
        SELECT item_id,count() AS event_rows,toUInt64(0) AS receipt_rows,toUInt64(0) AS floor_rows FROM listing_events WHERE world_id IN ({world_sql}) AND event_time >= toDateTime({}) AND event_time < toDateTime({to}) GROUP BY item_id
        UNION ALL
        SELECT item_id,toUInt64(0),count(),toUInt64(0) FROM sale_receipts WHERE world_id IN ({world_sql}) AND received_at >= toDateTime({}) AND received_at < toDateTime({to}) GROUP BY item_id
        UNION ALL
        SELECT item_id,toUInt64(0),toUInt64(0),countIf(event_time > toDateTime({from})) FROM floor_changes WHERE world_id IN ({world_sql}) AND event_time < toDateTime({to}) GROUP BY item_id
        ) GROUP BY item_id ORDER BY item_id{LIMITS}", from-600, from-600))
        .fetch_all::<ItemCounts>().await?;
    let batches = item_batches(items, worlds.len());
    // Two in-flight batches overlap SQL with local reduction without spawning
    // detached work. Dropping this stream cancels both on the cache deadline.
    let mut pending = futures::stream::iter(batches)
        .map(|items| async move { window_items(ch, worlds, days, to, &items).await })
        .buffer_unordered(2);
    let mut output = BTreeMap::new();
    while let Some(batch) = pending.try_next().await? {
        output.extend(batch);
    }
    // Sales without receipt evidence are explicitly counted, never guessed from
    // sold_date or ClickHouse inserted_at (which is also rewritten by backfill).
    #[derive(Row, Deserialize)]
    struct Missing {
        item_id: i32,
        hq: u8,
        n: u64,
    }
    let missing = ch.client().query(&format!("SELECT item_id, hq, count() AS n FROM sales FINAL
        WHERE world_id IN ({world_sql}) AND sold_date >= toDateTime({from}) AND sold_date < toDateTime({to})
        AND pg_id NOT IN (SELECT pg_id FROM sale_receipts WHERE world_id IN ({world_sql}) AND sold_at >= toDateTime({from}) AND sold_at < toDateTime({to}))
        GROUP BY item_id, hq{LIMITS}")).fetch_all::<Missing>().await?;
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
    Ok(output)
}

#[derive(Row, Deserialize)]
struct ItemCounts {
    item_id: i32,
    events: u64,
    receipts: u64,
    floors: u64,
}

fn item_batches(items: Vec<ItemCounts>, worlds: usize) -> Vec<Vec<i32>> {
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut rows: u64 = 0;
    for item in items {
        // A floor query also returns at most one pre-window baseline per world
        // and quality. Counts only plan batches; actual SQL limits still throw.
        let size = item
            .events
            .max(item.receipts)
            .max(item.floors.saturating_add(worlds as u64 * 2));
        if !batch.is_empty() && (batch.len() == 128 || rows.saturating_add(size) > 500_000) {
            batches.push(std::mem::take(&mut batch));
            rows = 0;
        }
        batch.push(item.item_id);
        rows = rows.saturating_add(size);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

async fn window_items(
    ch: &ClickHouseClient,
    worlds: &[i32],
    days: u16,
    to: i64,
    items: &[i32],
) -> Result<BTreeMap<(i32, bool), ListingWindowStats>, ClickHouseError> {
    let from = to - i64::from(days) * 86400;
    let world_sql = worlds
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let item_sql = items
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    // Ambiguous insert acknowledgements can make the writer retry a complete
    // batch. Deduplicate stored observations before the compact projection:
    // distinct listing identities can otherwise share every WindowEvent field.
    let events = ch.client().query(&format!("SELECT ?fields FROM (SELECT DISTINCT * FROM listing_events WHERE item_id IN ({item_sql}) AND world_id IN ({world_sql}) AND event_time >= toDateTime({}) AND event_time < toDateTime({to})){LIMITS}", from-600)).fetch_all::<WindowEvent>().await?;
    // Deduplicate receipts by stable PG identity, preserving the earliest actual
    // observation. An old sale replay cannot acquire a fresh matching timestamp.
    let receipts = ch.client().query(&format!("SELECT ?fields FROM sale_receipts WHERE item_id IN ({item_sql}) AND world_id IN ({world_sql}) AND received_at >= toDateTime({}) AND received_at < toDateTime({to}){LIMITS}", from-600)).fetch_all::<SaleReceiptRow>().await?;
    let mut unique = HashMap::<(i32, i32), SaleReceiptRow>::new();
    for (index, receipt) in receipts.into_iter().enumerate() {
        if index.is_multiple_of(4096) {
            tokio::task::yield_now().await;
        }
        unique
            .entry((receipt.world_id, receipt.pg_id))
            .and_modify(|old| {
                if receipt.received_at < old.received_at {
                    *old = receipt.clone();
                }
            })
            .or_insert(receipt);
    }
    let receipts = unique.into_values().collect::<Vec<_>>();
    let floors = floor_history::window_changes(ch, items, worlds, from, to).await?;
    let mut grouped_events: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut grouped_receipts: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut grouped_floors: BTreeMap<_, Vec<_>> = BTreeMap::new();
    // A bounded SQL result can still take seconds to process locally. Let the
    // enclosing StatsCache deadline and other requests run during large loads.
    for (index, event) in events.into_iter().enumerate() {
        if index.is_multiple_of(4096) {
            tokio::task::yield_now().await;
        }
        grouped_events
            .entry((event.item_id, event.hq != 0))
            .or_default()
            .push(event);
    }
    for (index, receipt) in receipts.into_iter().enumerate() {
        if index.is_multiple_of(4096) {
            tokio::task::yield_now().await;
        }
        grouped_receipts
            .entry((receipt.item_id, receipt.hq != 0))
            .or_default()
            .push(receipt);
    }
    for (index, floor) in floors.into_iter().enumerate() {
        if index.is_multiple_of(4096) {
            tokio::task::yield_now().await;
        }
        grouped_floors
            .entry((floor.item_id, floor.hq != 0))
            .or_default()
            .push(floor);
    }
    let keys = grouped_events
        .keys()
        .chain(grouped_receipts.keys())
        .chain(grouped_floors.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut output = BTreeMap::new();
    for key in keys {
        tokio::task::yield_now().await;
        let events = grouped_events.remove(&key).unwrap_or_default();
        let receipts = grouped_receipts.remove(&key).unwrap_or_default();
        let floors = grouped_floors.remove(&key).unwrap_or_default();
        let floor = floor_history::bounds(&floors, worlds, from, to);
        let relevant = |e: &&WindowEvent| {
            e.event_time.timestamp() >= from && e.source != ListingEventSource::Snapshot
        };
        output.insert(
            key,
            ListingWindowStats {
                window_days: days,
                from,
                to,
                additions: events
                    .iter()
                    .filter(relevant)
                    .filter(|e| e.kind == ListingEventKind::Added)
                    .count() as u64,
                removals: events
                    .iter()
                    .filter(relevant)
                    .filter(|e| e.kind == ListingEventKind::Removed)
                    .count() as u64,
                listing_coverage: coverage(
                    events.iter().map(|e| e.event_time.timestamp()),
                    from,
                    to,
                ),
                floor_min: floor.min,
                floor_max: floor.max,
                floor_known_secs: floor.known_secs,
                floor_empty_secs: floor.empty_secs,
                floor_unknown_secs: floor.unknown_secs,
                matches: match_observations(&events, &receipts, from, to),
                ..Default::default()
            },
        );
    }
    Ok(output)
}

/// Only fields used by turnover, coverage and conservative matching. The stored
/// event identity remains untouched; this read projection avoids unused strings.
#[derive(Row, Deserialize)]
struct WindowEvent {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    event_time: chrono::DateTime<chrono::Utc>,
    kind: ListingEventKind,
    source: ListingEventSource,
    item_id: i32,
    hq: u8,
    world_id: i32,
    retainer_id: i32,
    price_per_unit: u32,
    quantity: u16,
    prev_quantity: u16,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    reviewed_at: chrono::DateTime<chrono::Utc>,
}

impl From<&ListingEventRow> for WindowEvent {
    fn from(row: &ListingEventRow) -> Self {
        Self {
            event_time: row.event_time,
            kind: row.kind,
            source: row.source,
            item_id: row.item_id,
            hq: row.hq,
            world_id: row.world_id,
            retainer_id: row.retainer_id,
            price_per_unit: row.price_per_unit,
            quantity: row.quantity,
            prev_quantity: row.prev_quantity,
            reviewed_at: row.reviewed_at,
        }
    }
}

// Key includes quality and world even though callers group by item/quality.
type Key = (i32, i32, u8, u32, u16);
fn event_key(e: &WindowEvent) -> Key {
    (e.world_id, e.item_id, e.hq, e.price_per_unit, e.quantity)
}
fn sale_key(s: &SaleReceiptRow) -> Key {
    (s.world_id, s.item_id, s.hq, s.price_per_unit, s.quantity)
}

pub fn match_sales(
    events: &[ListingEventRow],
    receipts: &[SaleReceiptRow],
    from: i64,
    to: i64,
) -> MatchedSalesStats {
    let events = events.iter().map(WindowEvent::from).collect::<Vec<_>>();
    match_observations(&events, receipts, from, to)
}

fn match_observations(
    events: &[WindowEvent],
    receipts: &[SaleReceiptRow],
    from: i64,
    to: i64,
) -> MatchedSalesStats {
    let settled = to - 2 * MATCH_SECS - 1;
    let mut result = MatchedSalesStats {
        settled_through_unix: settled,
        received_sales: receipts
            .iter()
            .filter(|s| s.received_at.timestamp() >= from && s.received_at.timestamp() < to)
            .count() as u64,
        receipt_coverage: coverage(receipts.iter().map(|s| s.received_at.timestamp()), from, to),
        ..Default::default()
    };
    let mut removals: HashMap<Key, Vec<&WindowEvent>> = HashMap::new();
    let mut sales: HashMap<Key, Vec<&SaleReceiptRow>> = HashMap::new();
    let mut adds: HashMap<(i32, i32, u8, u16, i32), Vec<i64>> = HashMap::new();
    for event in events
        .iter()
        .filter(|e| e.source != ListingEventSource::Snapshot)
    {
        match event.kind {
            ListingEventKind::Removed => {
                removals.entry(event_key(event)).or_default().push(event);
            }
            ListingEventKind::Added | ListingEventKind::Updated => {
                adds.entry((
                    event.world_id,
                    event.item_id,
                    event.hq,
                    event.quantity,
                    event.retainer_id,
                ))
                .or_default()
                .push(event.event_time.timestamp());
                if event.kind == ListingEventKind::Updated {
                    adds.entry((
                        event.world_id,
                        event.item_id,
                        event.hq,
                        event.prev_quantity,
                        event.retainer_id,
                    ))
                    .or_default()
                    .push(event.event_time.timestamp());
                }
            }
        }
    }
    for sale in receipts {
        sales.entry(sale_key(sale)).or_default().push(sale);
    }
    for rows in removals.values_mut() {
        rows.sort_by_key(|e| e.event_time);
    }
    for rows in sales.values_mut() {
        rows.sort_by_key(|s| s.received_at);
    }
    for times in adds.values_mut() {
        times.sort_unstable();
    }
    let mut ages = Vec::new();
    for (key, candidates) in &removals {
        for removal in candidates
            .iter()
            .filter(|e| e.event_time.timestamp() >= from && e.event_time.timestamp() < to)
        {
            let time = removal.event_time.timestamp();
            if time > settled {
                result.pending += 1;
                continue;
            }
            if adds
                .get(&(key.0, key.1, key.2, key.4, removal.retainer_id))
                .is_some_and(|times| {
                    times
                        .get(times.partition_point(|t| *t < time - MATCH_SECS))
                        .is_some_and(|t| *t <= time + MATCH_SECS)
                })
            {
                result.repriced += 1;
                continue;
            }
            // Only actual websocket removals are eligible. Catch-up/manual
            // snapshots can describe much older board changes.
            if removal.source != ListingEventSource::Websocket {
                result.unmatched += 1;
                continue;
            }
            let bucket = sales.get(key).map_or(&[][..], |v| v.as_slice());
            let lo = bucket.partition_point(|s| s.received_at.timestamp() < time - MATCH_SECS);
            let hi = bucket.partition_point(|s| s.received_at.timestamp() <= time + MATCH_SECS);
            let possible = &bucket[lo..hi];
            if possible.is_empty() {
                result.unmatched += 1;
                continue;
            }
            let receipt_time = possible[0].received_at.timestamp();
            let lo = candidates
                .partition_point(|r| r.event_time.timestamp() < receipt_time - MATCH_SECS);
            let hi = candidates
                .partition_point(|r| r.event_time.timestamp() <= receipt_time + MATCH_SECS);
            if possible.len() != 1 || hi - lo != 1 {
                result.ambiguous += 1;
                continue;
            }
            let sale = possible[0];
            let age = sale.sold_at.timestamp() - removal.reviewed_at.timestamp();
            let receipt_age = sale.received_at.timestamp() - sale.sold_at.timestamp();
            if key.3 == 0
                || key.4 == 0
                || removal.reviewed_at.timestamp() <= 0
                || age < -60
                || !(-60..=86400).contains(&receipt_age)
            {
                result.unmatched += 1;
                continue;
            }
            ages.push(age.max(0) as u64);
            result.matched += 1;
        }
    }
    ages.sort_unstable();
    if !ages.is_empty() {
        result.median_time_to_sell_secs =
            Some((ages[(ages.len() - 1) / 2] + ages[ages.len() / 2]) / 2);
    }
    result
}

/// A ratio of scope totals, with missing world snapshots distinct from zero sales.
/// A delayed refresh can temporarily make stock unavailable. Never reuse a
/// vanished rolling-window group's old positive snapshot indefinitely.
pub async fn stock(
    ch: &ClickHouseClient,
    worlds: &[i32],
    days: u16,
    to: i64,
) -> Result<HashMap<(i32, bool), Option<u64>>, ClickHouseError> {
    use crate::rollups::{
        LISTING_ALIVE_REFRESH_SECS, SALE_STATS_1D_REFRESH_SECS, SALE_STATS_7D_REFRESH_SECS,
        SALE_STATS_LONG_REFRESH_SECS,
    };
    let sale_cadence = match days {
        1 => SALE_STATS_1D_REFRESH_SECS,
        7 => SALE_STATS_7D_REFRESH_SECS,
        30 | 90 => SALE_STATS_LONG_REFRESH_SECS,
        _ => return Err(ClickHouseError::Backfill("invalid stock window".into())),
    };
    if to < i64::from(days) * 86400 + 600 || to > i64::from(u32::MAX) {
        return Err(ClickHouseError::Backfill("invalid stock window".into()));
    }
    let from = to - i64::from(days) * 86400;
    let sale_cutoff = to - sale_cadence as i64;
    let alive_cutoff = to - LISTING_ALIVE_REFRESH_SECS as i64;
    #[derive(Row, Deserialize)]
    struct Row {
        item_id: i32,
        hq: u8,
        worlds: u64,
        units: u64,
    }
    if worlds.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = worlds
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let rows = ch
        .client()
        .query(&format!(
            "SELECT item_id, hq,
        uniqExactIf(world_id,
            computed_at >= toDateTime({sale_cutoff}) AND computed_at <= toDateTime({to})
            AND (units_sold = 0 OR (last_sold_unix >= {from} AND last_sold_unix < {to}))) AS worlds,
        sum(units_sold) AS units
        FROM sale_stats_window FINAL WHERE world_id IN ({ids}) AND window_days = {days}
        GROUP BY item_id, hq{LIMITS}"
        ))
        .fetch_all::<Row>()
        .await?;
    let expected = worlds
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len() as u64;
    #[derive(Row, Deserialize)]
    struct Alive {
        item_id: i32,
        hq: u8,
        worlds: u64,
    }
    let alive = ch.client().query(&format!("SELECT item_id, hq,
        uniqExactIf(world_id, computed_at >= toDateTime({alive_cutoff}) AND computed_at <= toDateTime({to})) AS worlds
        FROM listing_alive FINAL WHERE world_id IN ({ids}) GROUP BY item_id,hq{LIMITS}")).fetch_all::<Alive>().await?;
    let alive = alive
        .into_iter()
        .map(|r| ((r.item_id, r.hq != 0), r.worlds))
        .collect::<HashMap<_, _>>();
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                (r.item_id, r.hq != 0),
                (r.worlds == expected && alive.get(&(r.item_id, r.hq != 0)) == Some(&expected))
                    .then_some(r.units),
            )
        })
        .collect())
}

pub fn set_stock(stats: &mut ListingWindowStats, alive_units: u64, sold_units: Option<u64>) {
    use ultros_api_types::listing_stats::StockStatus;
    stats.stock_status = match sold_units {
        None => StockStatus::Unavailable,
        Some(0) => StockStatus::NoSales,
        Some(_) => StockStatus::Estimated,
    };
    stats.days_of_stock = sold_units
        .filter(|units| *units > 0)
        .map(|units| alive_units as f64 * f64::from(stats.window_days) / units as f64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    fn event(kind: ListingEventKind, time: i64, retainer: i32) -> ListingEventRow {
        ListingEventRow {
            event_time: Utc.timestamp_opt(time, 0).unwrap(),
            kind,
            source: ListingEventSource::Websocket,
            item_id: 1,
            hq: 0,
            world_id: 1,
            listing_id: retainer.to_string(),
            pg_listing_id: retainer,
            retainer_id: retainer,
            price_per_unit: 100,
            quantity: 2,
            prev_price: 0,
            prev_quantity: 0,
            reviewed_at: Utc.timestamp_opt(1000, 0).unwrap(),
        }
    }
    fn sale(time: i64) -> SaleReceiptRow {
        SaleReceiptRow {
            pg_id: 1,
            received_at: Utc.timestamp_opt(time, 0).unwrap(),
            sold_at: Utc.timestamp_opt(2000, 0).unwrap(),
            item_id: 1,
            hq: 0,
            world_id: 1,
            price_per_unit: 100,
            quantity: 2,
        }
    }
    #[test]
    fn receipt_matching_is_conservative_and_defines_age_origin() {
        let removed = event(ListingEventKind::Removed, 2100, 1);
        let stats = match_sales(std::slice::from_ref(&removed), &[sale(2101)], 1500, 3000);
        assert_eq!(stats.matched, 1);
        assert_eq!(stats.median_time_to_sell_secs, Some(1000));
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[sale(2401)], 1500, 3100).unmatched,
            1
        );
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[sale(2400)], 1500, 3100).matched,
            1
        );
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[sale(1800)], 1500, 3100).unmatched,
            1,
            "future game timestamp exceeds skew"
        );
        let mut readd = event(ListingEventKind::Added, 2200, 1);
        readd.price_per_unit = 80;
        assert_eq!(
            match_sales(&[removed.clone(), readd], &[sale(2101)], 1500, 3000).repriced,
            1
        );
        let other = event(ListingEventKind::Removed, 2102, 2);
        assert_eq!(
            match_sales(&[removed.clone(), other], &[sale(2101)], 1500, 3000).ambiguous,
            2
        );
        let mut other_sale = sale(2102);
        other_sale.pg_id = 2;
        assert_eq!(
            match_sales(
                std::slice::from_ref(&removed),
                &[sale(2101), other_sale],
                1500,
                3000
            )
            .ambiguous,
            1
        );
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[sale(2101)], 1500, 2500).pending,
            1
        );
        let mut snapshot = removed.clone();
        snapshot.source = ListingEventSource::Snapshot;
        assert_eq!(
            match_sales(&[snapshot], &[sale(2101)], 1500, 3000).matched,
            0
        );
        let mut other_world = sale(2101);
        other_world.world_id = 2;
        assert_eq!(
            match_sales(&[removed], &[other_world], 1500, 3000).matched,
            0
        );
    }
    #[test]
    fn age_skew_quality_quantity_and_backfill_age_are_not_guessed() {
        let removed = event(ListingEventKind::Removed, 2100, 1);
        let mut before = sale(1800);
        before.sold_at = Utc.timestamp_opt(1700, 0).unwrap();
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[before], 1500, 3100).matched,
            1
        );
        let mut skew = sale(2101);
        skew.sold_at = Utc.timestamp_opt(940, 0).unwrap();
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[skew.clone()], 1500, 3100)
                .median_time_to_sell_secs,
            Some(0)
        );
        skew.sold_at = Utc.timestamp_opt(939, 0).unwrap();
        assert_eq!(
            match_sales(std::slice::from_ref(&removed), &[skew], 1500, 3100).unmatched,
            1
        );
        let mut hq = sale(2101);
        hq.hq = 1;
        let mut quantity = sale(2101);
        quantity.quantity = 1;
        let mut price = sale(2101);
        price.price_per_unit = 101;
        for mismatch in [hq, quantity, price] {
            assert_eq!(
                match_sales(std::slice::from_ref(&removed), &[mismatch], 1500, 3100).unmatched,
                1
            );
        }
        let late = event(ListingEventKind::Removed, 90000, 1);
        let mut backfill = sale(90000);
        backfill.sold_at = Utc.timestamp_opt(3599, 0).unwrap();
        assert_eq!(match_sales(&[late], &[backfill], 89000, 91000).unmatched, 1);
        let mut unknown_review = removed;
        unknown_review.reviewed_at = Utc.timestamp_opt(0, 0).unwrap();
        assert_eq!(
            match_sales(&[unknown_review], &[sale(2101)], 1500, 3100).unmatched,
            1
        );
    }

    #[test]
    fn coverage_and_stock_do_not_invent_samples_or_infinity() {
        let c = coverage([1000, 2000].into_iter(), 0, 90 * 86400);
        assert_eq!(c.observed_span_secs, 1000);
        assert!(!c.continuity_verified);
        let mut stats = ListingWindowStats {
            window_days: 7,
            ..Default::default()
        };
        set_stock(&mut stats, 100, None);
        assert_eq!(
            stats.stock_status,
            ultros_api_types::listing_stats::StockStatus::Unavailable
        );
        set_stock(&mut stats, 100, Some(0));
        assert_eq!(
            stats.stock_status,
            ultros_api_types::listing_stats::StockStatus::NoSales
        );
        assert!(stats.days_of_stock.is_none());
        set_stock(&mut stats, 100, Some(350));
        assert_eq!(stats.days_of_stock, Some(2.0));
        set_stock(&mut stats, 0, Some(350));
        assert_eq!(stats.days_of_stock, Some(0.0));
    }
}
