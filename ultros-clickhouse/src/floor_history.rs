//! Bounded, as-of floor samples. Carry each world's state before taking the
//! scope minimum: taking min(event prices) would retain undercuts after removal.
use crate::{ClickHouseClient, ClickHouseError};
use clickhouse::Row;
use serde::Deserialize;
use std::collections::BTreeMap;
use ultros_api_types::{
    HqFilter,
    floor_history::{FloorHistory, FloorPoint},
};

#[derive(Clone, Debug, Row, Deserialize)]
struct Change {
    timestamp: i64,
    world_id: i32,
    hq: u8,
    price: u32,
}

fn sample(mut rows: Vec<Change>, from: i64, to: i64, step: i64) -> Vec<FloorPoint> {
    rows.sort_by_key(|r| r.timestamp);
    let mut rows = rows.into_iter().peekable();
    let mut state = BTreeMap::new();
    let mut points = Vec::new();
    let mut timestamp = from;
    loop {
        while rows.peek().is_some_and(|r| r.timestamp <= timestamp) {
            let r = rows.next().expect("peeked row");
            state.insert((r.world_id, r.hq), r.price);
        }
        points.push(FloorPoint {
            timestamp,
            price: state.values().copied().filter(|p| *p > 0).min(),
        });
        if timestamp == to {
            break;
        }
        timestamp = (timestamp + step).min(to);
    }
    points
}

pub async fn history(
    ch: &ClickHouseClient,
    item_id: i32,
    worlds: &[i32],
    hq: HqFilter,
    requested_from: i64,
    to: i64,
) -> Result<FloorHistory, ClickHouseError> {
    let empty = || FloorHistory {
        from: requested_from,
        to,
        bucket_seconds: 3600,
        points: vec![],
    };
    if worlds.is_empty() || requested_from >= to {
        return Ok(empty());
    }
    let predicate = predicate(&[item_id], worlds, hq);
    let first = ch.client().query(&format!(
        "SELECT toInt64(minOrNull(event_time)) FROM floor_changes WHERE {predicate} AND event_time < toDateTime({to}) SETTINGS max_execution_time = 15"
    )).fetch_one::<Option<i64>>().await?;
    let Some(first) = first else {
        return Ok(empty());
    };
    let from = requested_from.max(first);
    let step = ((to - from + 479) / 480).max(60);
    let rows = load_changes(
        ch,
        &[item_id],
        worlds,
        ChangeWindow {
            from,
            to,
            hq,
            step: Some(step),
        },
    )
    .await?;
    let rows = rows
        .into_iter()
        .map(|r| Change {
            timestamp: r.timestamp,
            world_id: r.world_id,
            hq: r.hq,
            price: r.price,
        })
        .collect();
    Ok(FloorHistory {
        from,
        to,
        bucket_seconds: step,
        points: sample(rows, from, to, step),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn change(timestamp: i64, world_id: i32, hq: u8, price: u32) -> Change {
        Change {
            timestamp,
            world_id,
            hq,
            price,
        }
    }
    #[test]
    fn carries_quiet_worlds_and_removes_exhausted_floors() {
        let points = sample(
            vec![
                change(0, 1, 0, 100),
                change(0, 2, 0, 150),
                change(10, 1, 0, 0),
                change(20, 2, 0, 0),
            ],
            0,
            30,
            10,
        );
        assert_eq!(
            points.iter().map(|p| p.price).collect::<Vec<_>>(),
            vec![Some(100), Some(150), None, None]
        );
    }
    #[test]
    fn does_not_invent_history_and_keeps_quality_states_separate() {
        let points = sample(
            vec![
                change(10, 1, 0, 100),
                change(10, 1, 1, 180),
                change(20, 1, 0, 0),
            ],
            0,
            25,
            10,
        );
        assert_eq!(
            points.iter().map(|p| p.price).collect::<Vec<_>>(),
            vec![None, Some(100), Some(180), Some(180)]
        );
        assert_eq!(points.last().unwrap().timestamp, 25);
    }
}

/// Exact transitions shared by window bounds and bounded multi-item history.
/// Hard limits throw instead of returning a truncated successful history.
#[derive(Clone, Debug, Row, Deserialize)]
pub(crate) struct WindowChange {
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub timestamp: i64,
    pub price: u32,
}

pub(crate) async fn window_changes(
    ch: &ClickHouseClient,
    items: &[i32],
    worlds: &[i32],
    from: i64,
    to: i64,
) -> Result<Vec<WindowChange>, ClickHouseError> {
    load_changes(
        ch,
        items,
        worlds,
        ChangeWindow {
            from,
            to,
            hq: HqFilter::Any,
            step: None,
        },
    )
    .await
}

struct ChangeWindow {
    from: i64,
    to: i64,
    hq: HqFilter,
    step: Option<i64>,
}

fn predicate(items: &[i32], worlds: &[i32], hq: HqFilter) -> String {
    let ids = |ids: &[i32]| ids.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
    let items = if items.is_empty() {
        String::new()
    } else {
        format!(" AND item_id IN ({})", ids(items))
    };
    let quality = match hq {
        HqFilter::Any => "",
        HqFilter::Hq => " AND hq=1",
        HqFilter::Nq => " AND hq=0",
    };
    format!("world_id IN ({}){items}{quality}", ids(worlds))
}

/// One baseline/tie-breaking implementation serves both the existing sampled
/// chart and exact analyzer windows. Sampling changes only the output timestamp
/// grouping, and always assigns observations to the interval's closing boundary.
async fn load_changes(
    ch: &ClickHouseClient,
    items: &[i32],
    worlds: &[i32],
    window: ChangeWindow,
) -> Result<Vec<WindowChange>, ClickHouseError> {
    if worlds.is_empty() {
        return Ok(vec![]);
    }
    let ChangeWindow { from, to, hq, step } = window;
    let predicate = predicate(items, worlds, hq);
    let timestamp = match step {
        Some(step) => format!(
            "toInt64(least({to}, {from} + (intDiv(toInt64(event_time) - {from} - 1, {step}) + 1) * {step}))"
        ),
        None => "toInt64(event_time)".into(),
    };
    // Keep the original timestamp on the seed: it is evidence, carried to from.
    let sql = format!("SELECT item_id, hq, world_id, toInt64(max(event_time)) AS timestamp,
        argMax(price_per_unit, tuple(event_time, -toInt64(price_per_unit))) AS price
        FROM floor_changes WHERE {predicate} AND event_time <= toDateTime({from})
        GROUP BY item_id, hq, world_id
        UNION ALL
        SELECT item_id, hq, world_id, {timestamp} AS timestamp,
        argMax(price_per_unit, tuple(event_time, -toInt64(price_per_unit))) AS price
        FROM floor_changes WHERE {predicate} AND event_time > toDateTime({from}) AND event_time < toDateTime({to})
        GROUP BY item_id, hq, world_id, timestamp
        SETTINGS max_execution_time=10, max_result_rows=2000000, result_overflow_mode='throw', max_memory_usage=536870912");
    Ok(ch.client().query(&sql).fetch_all::<WindowChange>().await?)
}

/// The floor ClickHouse currently believes for one (item, hq, world) key.
/// `price == 0` is an emptied board; a key with no row at all is absent.
#[derive(Clone, Debug, PartialEq, Eq, Row, Deserialize)]
pub struct LatestFloor {
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub price: u32,
}

/// Every key's latest floor, resolved exactly as `load_changes` resolves a
/// window's baseline (latest event, cheaper of same-second rows). The boot
/// resync diffs Postgres against this so keys ClickHouse has never seen get
/// their anchor row regardless of what the analyzer snapshot restored.
pub async fn latest_floors(ch: &ClickHouseClient) -> Result<Vec<LatestFloor>, ClickHouseError> {
    Ok(ch
        .client()
        .query(
            "SELECT item_id, hq, world_id,
            argMax(price_per_unit, tuple(event_time, -toInt64(price_per_unit))) AS price
            FROM floor_changes GROUP BY item_id, hq, world_id
            SETTINGS optimize_aggregation_in_order=1, max_execution_time=60,
            max_memory_usage=1073741824",
        )
        .fetch_all::<LatestFloor>()
        .await?)
}

/// Replay simultaneous changes together. A partial scope cannot establish a
/// known floor (or emptiness); unknown worlds may hold a cheaper listing.
pub(crate) fn bounds(
    rows: &[WindowChange],
    worlds: &[i32],
    from: i64,
    to: i64,
) -> ultros_api_types::floor_history::FloorBounds {
    use ultros_api_types::floor_history::FloorBounds;
    let mut ordered = rows.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|r| r.timestamp);
    let expected = worlds
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let mut state = BTreeMap::new();
    let mut bounds = FloorBounds::default();
    let mut cursor = from;
    let mut index = 0;
    loop {
        while index < ordered.len() && ordered[index].timestamp <= cursor {
            let row = ordered[index];
            state.insert(row.world_id, row.price);
            index += 1;
        }
        if cursor >= to {
            break;
        }
        let next = ordered.get(index).map_or(to, |r| r.timestamp.min(to));
        let duration = (next - cursor) as u64;
        if state.len() == expected && expected > 0 {
            bounds.known_secs += duration;
            if let Some(price) = state.values().copied().filter(|p| *p > 0).min() {
                bounds.min = Some(bounds.min.map_or(price, |old| old.min(price)));
                bounds.max = Some(bounds.max.map_or(price, |old| old.max(price)));
            } else {
                bounds.empty_secs += duration;
            }
        } else {
            bounds.unknown_secs += duration;
        }
        cursor = next;
    }
    bounds
}

pub async fn batch(
    ch: &ClickHouseClient,
    worlds: &[i32],
    request: &ultros_api_types::floor_history::FloorHistoryRequest,
) -> Result<ultros_api_types::floor_history::FloorHistoryBatch, ClickHouseError> {
    use ultros_api_types::floor_history::{FloorHistoryBatch, ItemFloorHistory};
    if !request.valid() {
        return Err(ClickHouseError::Backfill(
            "invalid floor history request".into(),
        ));
    }
    let rows = window_changes(ch, &request.item_ids, worlds, request.from, request.to).await?;
    let mut series = Vec::new();
    for item_id in request
        .item_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        for hq in [false, true]
            .into_iter()
            .filter(|hq| request.hq.is_none_or(|wanted| wanted == *hq))
        {
            let rows = rows
                .iter()
                .filter(|r| r.item_id == item_id && (r.hq != 0) == hq)
                .cloned()
                .collect::<Vec<_>>();
            let bounds = bounds(&rows, worlds, request.from, request.to);
            let mut points = sample(
                rows.iter()
                    .map(|r| Change {
                        timestamp: r.timestamp,
                        world_id: r.world_id,
                        hq: r.hq,
                        price: r.price,
                    })
                    .collect(),
                request.from,
                request.to,
                request.interval.seconds(),
            );
            // Legacy chart GET shows tracked-world minima. Analyzer batches require
            // a complete scope baseline and explicitly report unknown intervals.
            let first_by_world = rows.iter().fold(BTreeMap::<i32, i64>::new(), |mut map, r| {
                map.entry(r.world_id)
                    .and_modify(|t| *t = (*t).min(r.timestamp))
                    .or_insert(r.timestamp);
                map
            });
            let known_from = worlds
                .iter()
                .map(|w| first_by_world.get(w).copied())
                .collect::<Option<Vec<_>>>()
                .and_then(|v| v.into_iter().max());
            let mut unknown_timestamps = Vec::new();
            for point in &mut points {
                if known_from.is_none_or(|first| point.timestamp < first) {
                    point.price = None;
                    unknown_timestamps.push(point.timestamp);
                }
            }
            series.push(ItemFloorHistory {
                item_id,
                hq,
                bounds,
                unknown_timestamps,
                history: FloorHistory {
                    from: request.from,
                    to: request.to,
                    bucket_seconds: request.interval.seconds(),
                    points,
                },
            });
        }
    }
    Ok(FloorHistoryBatch { series })
}
