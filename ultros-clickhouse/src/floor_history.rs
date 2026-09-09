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
    let worlds = worlds
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let quality = match hq {
        HqFilter::Any => "",
        HqFilter::Hq => " AND hq = 1",
        HqFilter::Nq => " AND hq = 0",
    };
    // Only typed integers and a closed enum enter SQL. Include a pre-window
    // seed per (world, quality); without it quiet markets disappear on zoom.
    let predicate = format!("item_id = {item_id} AND world_id IN ({worlds}){quality}");
    let first = ch.client().query(&format!(
        "SELECT toInt64(minOrNull(event_time)) FROM floor_changes WHERE {predicate} AND event_time < toDateTime({to}) SETTINGS max_execution_time = 15"
    )).fetch_one::<Option<i64>>().await?;
    let Some(first) = first else {
        return Ok(empty());
    };
    let from = requested_from.max(first);
    let step = ((to - from + 479) / 480).max(60);
    // Sample at the END of each interval so a future price is never moved
    // backwards in time. Same-second ties have no sequence in the source;
    // choose zero conservatively, otherwise the lower price, deterministically.
    let sql = format!(
        r#"
        SELECT toInt64({from}) AS timestamp, world_id, hq,
               argMax(price_per_unit, tuple(event_time, -toInt64(price_per_unit))) AS price
        FROM floor_changes WHERE {predicate} AND event_time <= toDateTime({from})
        GROUP BY world_id, hq
        UNION ALL
        SELECT toInt64(least({to}, {from} + (intDiv(toInt64(event_time) - {from} - 1, {step}) + 1) * {step})) AS timestamp,
               world_id, hq, argMax(price_per_unit, tuple(event_time, -toInt64(price_per_unit))) AS price
        FROM floor_changes WHERE {predicate} AND event_time > toDateTime({from}) AND event_time < toDateTime({to})
        GROUP BY timestamp, world_id, hq
        SETTINGS max_execution_time = 15
    "#
    );
    let rows = ch.client().query(&sql).fetch_all::<Change>().await?;
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
