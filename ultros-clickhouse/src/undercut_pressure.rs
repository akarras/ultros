//! Undercut pressure for one item on one world. Pure reducers turn
//! same-listing price drops and exact floor transitions into buckets,
//! calm/churn/war states, war spans and floor-episode stats; `load` does the
//! two bounded ClickHouse reads. Spec:
//! docs/superpowers/specs/2026-09-22-undercut-pressure-design.md
//!
//! `pressure()` (and everything it calls) is reachable only from this
//! module's own tests until Task 5 adds `load()`, the crate's public entry
//! point that does the two ClickHouse reads and calls `pressure()` — so
//! `dead_code` fires on the whole module in isolation. Allowed here, to be
//! removed once `load()` calls in.
#![allow(dead_code)]
use crate::floor_history::WindowChange;
use clickhouse::Row;
use serde::Deserialize;
use std::collections::HashSet;
use ultros_api_types::undercut_pressure::{
    PressureBucket, PressureState, PressureSummary, UndercutPressure, WarSpan, WarStatus,
};

// Thresholds are first guesses, calibrated against prod before phase 1
// merged (see the PR). Keep them together so tuning is a one-file change.
/// A drop below this share of the previous price is a trim (covers the
/// 1-gil undercut on anything above 100 gil); at or above it, a cut.
pub const TRIM_FRACTION: f64 = 0.01;
/// Floor erosion a war bucket needs, per hour of bucket length...
pub const WAR_EROSION_PER_HOUR: f64 = 0.01;
/// ...capped for day-and-longer buckets.
pub const WAR_EROSION_CAP: f64 = 0.10;
/// A war bucket needs this multiple of the baseline...
pub const WAR_BASELINE_MULTIPLE: f64 = 2.0;
/// ...and at least this many undercuts...
pub const WAR_MIN_UNDERCUTS: u32 = 3;
/// ...from at least this many distinct retainers (ping-pong).
pub const WAR_MIN_SELLERS: u16 = 2;
/// A busy item's bucket below this share of its baseline reads as calm...
pub const CALM_BASELINE_SHARE: f64 = 0.5;
/// ...once the baseline is at least this high.
pub const CALM_MIN_BASELINE: f64 = 2.0;
pub(crate) const HOUR: i64 = 3600;
pub(crate) const DAY: i64 = 86_400;

#[derive(Clone, Debug, PartialEq, Row, Deserialize)]
pub struct UndercutEvent {
    pub time: i64,
    pub retainer_id: i32,
    pub prev_price: u32,
    pub price: u32,
}

/// Distinct as-of floors from `start` to `end`: the floor in force at
/// `start`, then one point per change that moves it. Each quality's last
/// observed price carries forward; from the anchor on, a quality never seen
/// is a known-empty board. The floor is the cheapest non-zero price, `None`
/// when empty. No points exist before the anchor (unknown), and with no
/// anchor the world has no coverage at all.
pub(crate) fn floor_points(
    rows: &[WindowChange],
    anchor: Option<i64>,
    start: i64,
    end: i64,
) -> Vec<(i64, Option<u32>)> {
    let Some(anchor) = anchor else {
        return vec![];
    };
    let start = start.max(anchor);
    let mut rows: Vec<&WindowChange> = rows.iter().collect();
    rows.sort_by_key(|r| r.timestamp);
    let floor = |state: &[u32; 2]| state.iter().copied().filter(|p| *p > 0).min();
    let mut state = [0u32; 2];
    let mut rows = rows.into_iter().peekable();
    while let Some(r) = rows.next_if(|r| r.timestamp <= start) {
        state[usize::from(r.hq.min(1))] = r.price;
    }
    let mut points = vec![(start, floor(&state))];
    for r in rows.take_while(|r| r.timestamp < end) {
        state[usize::from(r.hq.min(1))] = r.price;
        let next = floor(&state);
        if points.last().is_some_and(|(_, p)| *p != next) {
            points.push((r.timestamp, next));
        }
    }
    points
}

/// Floor in force at `t` per `floor_points`; `None` before the first point.
pub(crate) fn floor_at(points: &[(i64, Option<u32>)], t: i64) -> Option<u32> {
    let i = points.partition_point(|(ts, _)| *ts <= t);
    i.checked_sub(1).and_then(|i| points[i].1)
}

pub(crate) fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

pub(crate) struct Classified {
    pub buckets: Vec<PressureBucket>,
    /// Cutting retainers per bucket, parallel to `buckets` (war-span union).
    pub sellers: Vec<HashSet<i32>>,
    pub baseline: Option<f64>,
    pub bucket_seconds: i64,
}

fn is_trim(e: &UndercutEvent) -> bool {
    f64::from(e.prev_price - e.price) / f64::from(e.prev_price) < TRIM_FRACTION
}

/// Buckets `[floor(from), to)` at `bucket_seconds`, epoch-aligned like
/// `price_series`. Only events inside `[from, to)` count. A bucket is known
/// once it starts at or after the anchor.
pub(crate) fn classify(
    events: &[UndercutEvent],
    points: &[(i64, Option<u32>)],
    anchor: Option<i64>,
    from: i64,
    to: i64,
    bucket_seconds: i64,
) -> Classified {
    let is_known = |start: i64| anchor.is_some_and(|a| start >= a);
    let mut buckets = Vec::new();
    let mut sellers = Vec::new();
    let mut start = from.div_euclid(bucket_seconds) * bucket_seconds;
    while start < to {
        let end = start + bucket_seconds;
        let known = is_known(start);
        let lo = events.partition_point(|e| e.time < start.max(from));
        let hi = events.partition_point(|e| e.time < end.min(to)).max(lo);
        let (mut trims, mut cuts, mut who) = (0u32, 0u32, HashSet::new());
        if known {
            for e in &events[lo..hi] {
                if is_trim(e) {
                    trims += 1;
                } else {
                    cuts += 1;
                }
                who.insert(e.retainer_id);
            }
        }
        buckets.push(PressureBucket {
            start,
            trims,
            cuts,
            sellers: u16::try_from(who.len()).unwrap_or(u16::MAX),
            floor_open: if known { floor_at(points, start) } else { None },
            floor_close: if known {
                floor_at(points, end.min(to) - 1)
            } else {
                None
            },
            state: PressureState::Unknown,
        });
        sellers.push(who);
        start = end;
    }
    let baseline = median(
        buckets
            .iter()
            .filter(|b| is_known(b.start))
            .map(|b| f64::from(b.trims + b.cuts))
            .collect(),
    );
    if let Some(baseline) = baseline {
        for b in buckets.iter_mut().filter(|b| is_known(b.start)) {
            b.state = state_for(b, baseline, bucket_seconds);
        }
    }
    Classified {
        buckets,
        sellers,
        baseline,
        bucket_seconds,
    }
}

pub(crate) fn state_for(
    bucket: &PressureBucket,
    baseline: f64,
    bucket_seconds: i64,
) -> PressureState {
    let total = bucket.trims + bucket.cuts;
    let erosion = match (bucket.floor_open, bucket.floor_close) {
        (Some(open), Some(close)) if open > 0 => {
            (f64::from(open) - f64::from(close)) / f64::from(open)
        }
        _ => 0.0,
    };
    let needed = (WAR_EROSION_PER_HOUR * bucket_seconds as f64 / HOUR as f64).min(WAR_EROSION_CAP);
    let volume = (WAR_BASELINE_MULTIPLE * baseline).max(f64::from(WAR_MIN_UNDERCUTS));
    // The epsilon keeps the spec's `>=` when float division lands a hair low.
    if erosion >= needed - 1e-12 && f64::from(total) >= volume && bucket.sellers >= WAR_MIN_SELLERS
    {
        PressureState::War
    } else if total == 0
        || (baseline >= CALM_MIN_BASELINE && f64::from(total) < CALM_BASELINE_SHARE * baseline)
    {
        PressureState::Calm
    } else {
        PressureState::Churn
    }
}

/// Maximal runs of consecutive war buckets.
pub(crate) fn war_spans(c: &Classified) -> Vec<WarSpan> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < c.buckets.len() {
        if c.buckets[i].state != PressureState::War {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < c.buckets.len() && c.buckets[j].state == PressureState::War {
            j += 1;
        }
        let run = &c.buckets[i..j];
        let who: HashSet<i32> = c.sellers[i..j].iter().flatten().copied().collect();
        let (open, close) = (run[0].floor_open, run[run.len() - 1].floor_close);
        spans.push(WarSpan {
            start: run[0].start,
            end: run[run.len() - 1].start + c.bucket_seconds,
            undercuts: run.iter().map(|b| b.trims + b.cuts).sum(),
            sellers: u16::try_from(who.len()).unwrap_or(u16::MAX),
            floor_change: open
                .zip(close)
                .filter(|(o, _)| *o > 0)
                .map(|(o, c)| f64::from(c) / f64::from(o) - 1.0),
        });
        i = j;
    }
    spans
}

pub struct PressureParams {
    pub world_id: i32,
    /// Chart window; buckets cover `[from, to)`.
    pub from: i64,
    pub to: i64,
    pub bucket_seconds: i64,
    pub now: i64,
    /// The world's floor anchor (`floor_history::anchors`).
    pub anchor: Option<i64>,
}

/// Closed floor episodes that start at a real transition inside
/// `[from, to)`. `points[0]` is the carried state at the read start (its
/// true start is unknown) and the last point is still open, so both are
/// excluded. Returns the median duration and the (left, undercut) counts.
pub(crate) fn episodes(
    points: &[(i64, Option<u32>)],
    from: i64,
    to: i64,
) -> (Option<i64>, u32, u32) {
    let (mut durations, mut left, mut undercut) = (Vec::new(), 0u32, 0u32);
    for (i, pair) in points.windows(2).enumerate() {
        let ((t0, p0), (t1, p1)) = (pair[0], pair[1]);
        let Some(p0) = p0 else { continue };
        if i == 0 || t0 < from || t0 >= to {
            continue;
        }
        durations.push((t1 - t0) as f64);
        match p1 {
            Some(p1) if p1 < p0 => undercut += 1,
            _ => left += 1,
        }
    }
    (median(durations).map(|m| m.round() as i64), left, undercut)
}

/// Everything the item page pane and cards need for one item on one world.
/// `events` and `floor_rows` must cover `[min(from, now - DAY), max(to, now))`.
///
/// `pub(crate)`, not `pub`: `floor_rows` takes `WindowChange`, which is
/// itself `pub(crate)` (Task 1) — a `pub fn` here would leak a type external
/// callers couldn't name and fails `private_interfaces` under `-D warnings`.
/// Task 5's `load()` is the crate's public entry point; it does the two
/// ClickHouse reads and calls this reducer.
pub(crate) fn pressure(
    events: &[UndercutEvent],
    floor_rows: &[WindowChange],
    p: &PressureParams,
) -> UndercutPressure {
    let mut events = events.to_vec();
    events.sort_by_key(|e| e.time);
    let points = floor_points(
        floor_rows,
        p.anchor,
        p.from.min(p.now - DAY),
        p.to.max(p.now),
    );
    let chart = classify(&events, &points, p.anchor, p.from, p.to, p.bucket_seconds);
    let wars = war_spans(&chart);

    // The 24 h cards read the same at every zoom: their own hourly buckets.
    let day = classify(&events, &points, p.anchor, p.now - DAY, p.now, HOUR);
    let last_war = war_spans(&day).pop();
    let current_hour = p.now.div_euclid(HOUR) * HOUR;
    let war = match &last_war {
        Some(w) if w.end >= current_hour => WarStatus::Active,
        Some(w) => WarStatus::Ended { at: w.end },
        None => WarStatus::None,
    };
    let floor_trend_24h = floor_at(&points, p.now)
        .zip(floor_at(&points, p.now - DAY))
        .filter(|(_, then)| *then > 0)
        .map(|(now, then)| f64::from(now) / f64::from(then) - 1.0);

    let known: Vec<&PressureBucket> = chart
        .buckets
        .iter()
        .filter(|b| b.state != PressureState::Unknown)
        .collect();
    let contested_share = (!known.is_empty()).then(|| {
        let busy = known
            .iter()
            .filter(|b| matches!(b.state, PressureState::Churn | PressureState::War))
            .count();
        busy as f64 / known.len() as f64
    });
    let (floor_holds_median_secs, episodes_left, episodes_undercut) =
        episodes(&points, p.from, p.to);

    UndercutPressure {
        world_id: p.world_id,
        from: p.from,
        to: p.to,
        bucket_seconds: p.bucket_seconds,
        coverage_from: p.anchor,
        baseline: chart.baseline,
        summary: PressureSummary {
            floor_trend_24h,
            war,
            last_war,
            contested_share,
            typical_undercuts_per_hour: chart
                .baseline
                .map(|b| b / (p.bucket_seconds as f64 / HOUR as f64)),
            floor_holds_median_secs,
            episodes_left,
            episodes_undercut,
        },
        buckets: chart.buckets,
        wars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PressureState::*;

    fn ev(time: i64, retainer_id: i32, prev_price: u32, price: u32) -> UndercutEvent {
        UndercutEvent {
            time,
            retainer_id,
            prev_price,
            price,
        }
    }
    fn fc(timestamp: i64, hq: u8, price: u32) -> WindowChange {
        WindowChange {
            item_id: 1,
            hq,
            world_id: 34,
            timestamp,
            price,
        }
    }
    fn states(c: &Classified) -> Vec<PressureState> {
        c.buckets.iter().map(|b| b.state).collect()
    }

    #[test]
    fn hq_any_takes_min_of_qualities() {
        let rows = [fc(0, 0, 1000), fc(0, 1, 900), fc(50, 1, 0)];
        let points = floor_points(&rows, Some(0), 0, 100);
        assert_eq!(floor_at(&points, 10), Some(900));
        assert_eq!(
            floor_at(&points, 60),
            Some(1000),
            "HQ board emptied, NQ floor remains"
        );
    }

    #[test]
    fn floor_is_unknown_before_the_anchor_and_empty_after_it() {
        let points = floor_points(&[], Some(100), 0, 200);
        assert_eq!(floor_at(&points, 50), None);
        assert_eq!(floor_at(&points, 150), None);
        let c = classify(&[], &points, Some(100), 0, 200, 100);
        assert_eq!(
            states(&c),
            vec![Unknown, Calm],
            "anchored world with no rows is a known-empty board"
        );
    }

    #[test]
    fn floor_points_dedupe_and_carry_seed_rows() {
        // Seed row at 5 precedes the window start 10; equal-price rows collapse.
        let rows = [fc(5, 0, 700), fc(20, 0, 700), fc(30, 0, 650)];
        assert_eq!(
            floor_points(&rows, Some(0), 10, 100),
            vec![(10, Some(700)), (30, Some(650))]
        );
    }

    #[test]
    fn trims_and_cuts_split_at_one_percent() {
        let events = [ev(10, 1, 1000, 991), ev(20, 2, 1000, 990)];
        let points = floor_points(&[fc(0, 0, 1000)], Some(0), 0, 3600);
        let c = classify(&events, &points, Some(0), 0, 3600, 3600);
        assert_eq!(
            (c.buckets[0].trims, c.buckets[0].cuts, c.buckets[0].sellers),
            (1, 1, 2)
        );
    }

    #[test]
    fn buckets_are_epoch_aligned_and_events_clip_to_the_window() {
        let from = 10 * HOUR + 1800;
        let events = [
            ev(10 * HOUR + 100, 1, 100, 90),
            ev(10 * HOUR + 1900, 1, 100, 90),
        ];
        let points = floor_points(&[fc(0, 0, 100)], Some(0), from, 13 * HOUR);
        let c = classify(&events, &points, Some(0), from, 13 * HOUR, HOUR);
        let starts: Vec<i64> = c.buckets.iter().map(|b| b.start).collect();
        assert_eq!(starts, vec![10 * HOUR, 11 * HOUR, 12 * HOUR]);
        assert_eq!(
            c.buckets[0].cuts, 1,
            "the event before `from` is outside the window"
        );
    }

    /// Six hourly buckets with one churn undercut each except bucket 3,
    /// which carries `war` while the floor moves per `floor`.
    fn six_hours(war: &[UndercutEvent], floor: &[WindowChange]) -> Classified {
        let mut events: Vec<UndercutEvent> = [0, 1, 2, 4, 5]
            .iter()
            .map(|h| ev(h * HOUR + 10, 9, 1000, 999))
            .collect();
        events.extend_from_slice(war);
        events.sort_by_key(|e| e.time);
        let mut rows = vec![fc(0, 0, 1000)];
        rows.extend_from_slice(floor);
        let points = floor_points(&rows, Some(0), 0, 6 * HOUR);
        classify(&events, &points, Some(0), 0, 6 * HOUR, HOUR)
    }
    fn war_events(retainers: [i32; 4]) -> Vec<UndercutEvent> {
        retainers
            .iter()
            .enumerate()
            .map(|(i, r)| ev(3 * HOUR + 100 + i as i64, *r, 1000, 960))
            .collect()
    }

    #[test]
    fn war_needs_erosion_volume_and_two_sellers() {
        let drop = [fc(3 * HOUR + 200, 0, 950)];
        assert_eq!(
            states(&six_hours(&war_events([1, 2, 1, 2]), &drop)),
            vec![Churn, Churn, Churn, War, Churn, Churn]
        );
        assert_eq!(
            six_hours(&war_events([1, 1, 1, 1]), &drop).buckets[3].state,
            Churn,
            "one seller is not a war"
        );
        assert_eq!(
            six_hours(&war_events([1, 2, 1, 2]), &[]).buckets[3].state,
            Churn,
            "flat floor is churn"
        );
    }

    #[test]
    fn calm_is_relative_to_a_busy_baseline() {
        let mut events = vec![];
        for h in 0..4 {
            for i in 0..10 {
                events.push(ev(h * HOUR + i, 1, 100, 99));
            }
        }
        for i in 0..3 {
            events.push(ev(4 * HOUR + i, 1, 100, 99));
        }
        let points = floor_points(&[fc(0, 0, 100)], Some(0), 0, 6 * HOUR);
        let c = classify(&events, &points, Some(0), 0, 6 * HOUR, HOUR);
        assert_eq!(c.baseline, Some(10.0));
        assert_eq!(states(&c), vec![Churn, Churn, Churn, Churn, Calm, Calm]);
    }

    #[test]
    fn erosion_requirement_scales_with_bucket_and_caps() {
        let bucket = |close| PressureBucket {
            start: 0,
            trims: 0,
            cuts: 5,
            sellers: 2,
            floor_open: Some(1000),
            floor_close: Some(close),
            state: Unknown,
        };
        assert_eq!(state_for(&bucket(990), 1.0, HOUR), War, "1% in an hour");
        assert_eq!(
            state_for(&bucket(960), 1.0, 6 * HOUR),
            Churn,
            "6h bucket needs 6%"
        );
        assert_eq!(
            state_for(&bucket(910), 1.0, DAY),
            Churn,
            "day bucket needs the 10% cap"
        );
        assert_eq!(state_for(&bucket(900), 1.0, DAY), War);
    }

    #[test]
    fn war_spans_merge_consecutive_buckets_and_dedupe_sellers() {
        // Eight hourly buckets: one churn undercut in each except 1 and 2,
        // which carry three cuts each. Totals [1,3,3,1,1,1,1,1] → baseline 1
        // → war needs max(2, 3) = 3. (With only four buckets the war buckets
        // would drag the median up and no longer qualify.)
        let mut events: Vec<UndercutEvent> = [0, 3, 4, 5, 6, 7]
            .iter()
            .map(|h| ev(h * HOUR + 5, 9, 1000, 999))
            .collect();
        for (h, rs) in [(1, [1, 2, 1]), (2, [2, 3, 2])] {
            for (i, r) in rs.iter().enumerate() {
                events.push(ev(h * HOUR + 100 + i as i64, *r, 1000, 900));
            }
        }
        events.sort_by_key(|e| e.time);
        let rows = [
            fc(0, 0, 1000),
            fc(HOUR + 200, 0, 900),
            fc(2 * HOUR + 200, 0, 800),
        ];
        let points = floor_points(&rows, Some(0), 0, 8 * HOUR);
        let c = classify(&events, &points, Some(0), 0, 8 * HOUR, HOUR);
        assert_eq!(
            states(&c),
            vec![Churn, War, War, Churn, Churn, Churn, Churn, Churn]
        );
        let spans = war_spans(&c);
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        // Sellers {1,2} ∪ {2,3} = {1,2,3}.
        assert_eq!(
            (span.start, span.end, span.undercuts, span.sellers),
            (HOUR, 3 * HOUR, 6, 3)
        );
        assert!((span.floor_change.unwrap() - (800.0 / 1000.0 - 1.0)).abs() < 1e-9);
    }

    #[test]
    fn window_before_anchor_is_all_unknown() {
        let points = floor_points(&[fc(0, 0, 100)], Some(10 * DAY), 0, 2 * DAY);
        let c = classify(
            &[ev(10, 1, 100, 50)],
            &points,
            Some(10 * DAY),
            0,
            2 * DAY,
            DAY,
        );
        assert_eq!(states(&c), vec![Unknown, Unknown]);
        assert_eq!(c.baseline, None);
        assert!(
            c.buckets
                .iter()
                .all(|b| b.trims + b.cuts == 0 && b.floor_open.is_none())
        );
        assert!(war_spans(&c).is_empty());
    }

    #[test]
    fn episodes_exclude_censored_and_classify_outcomes() {
        // (0) is the carried state at the read start: left-censored.
        let points = vec![
            (0, Some(1000)),
            (100, Some(900)),
            (400, Some(880)), // 100..400 ended by undercut
            (700, Some(950)), // 400..700 left (floor rose)
            (800, None),      // 700..800 left (board emptied)
            (900, Some(800)), // still open: right-censored
        ];
        assert_eq!(episodes(&points, 0, 1000), (Some(300), 2, 1));
        assert_eq!(
            episodes(&points, 500, 1000),
            (Some(100), 1, 0),
            "only episodes starting in the window"
        );
    }

    fn params(from: i64, to: i64, bucket_seconds: i64, now: i64) -> PressureParams {
        PressureParams {
            world_id: 34,
            from,
            to,
            bucket_seconds,
            now,
            anchor: Some(0),
        }
    }

    #[test]
    fn summary_24h_is_hourly_and_independent_of_chart_window() {
        use ultros_api_types::undercut_pressure::WarStatus;
        let now = 10 * DAY + 1800;
        // A quiet day (one trim every 3 hours) then a war in the current hour.
        let mut events: Vec<UndercutEvent> = (1..8)
            .map(|k| ev(now - k * 3 * HOUR, 9, 1000, 999))
            .collect();
        events.extend((0..4).map(|i| ev(now - 1500 + i, 1 + (i as i32 % 2), 1000, 900)));
        let rows = [
            fc(0, 0, 1100),
            fc(now - DAY + 10, 0, 1000),
            fc(now - 1400, 0, 900),
        ];
        let out = pressure(&events, &rows, &params(0, 2 * DAY, DAY, now));
        assert_eq!(
            out.buckets.len(),
            2,
            "chart buckets follow the chart window only"
        );
        assert_eq!(out.summary.war, WarStatus::Active);
        assert_eq!(out.summary.last_war.as_ref().map(|w| w.sellers), Some(2));
        let trend = out.summary.floor_trend_24h.unwrap();
        assert!((trend - (900.0 / 1100.0 - 1.0)).abs() < 1e-9);
    }

    #[test]
    fn war_status_ends_and_expires() {
        use ultros_api_types::undercut_pressure::WarStatus;
        let now = 10 * DAY;
        let war_at = now - 5 * HOUR;
        let events: Vec<UndercutEvent> = (0..4)
            .map(|i| ev(war_at + 100 + i, 1 + (i as i32 % 2), 1000, 900))
            .collect();
        let rows = [fc(0, 0, 1000), fc(war_at + 200, 0, 900)];
        let out = pressure(&events, &rows, &params(now - DAY, now, HOUR, now));
        assert_eq!(out.summary.war, WarStatus::Ended { at: war_at + HOUR });
        let later = pressure(&events, &rows, &params(now - DAY, now, HOUR, now + 2 * DAY));
        assert_eq!(later.summary.war, WarStatus::None);
    }

    #[test]
    fn summary_contested_share_rate_and_episodes() {
        let events: Vec<UndercutEvent> = (0..4).map(|h| ev(h * HOUR + 10, 9, 1000, 999)).collect();
        let rows = [fc(0, 0, 1000), fc(HOUR, 0, 990), fc(HOUR + 600, 0, 1200)];
        let out = pressure(&events, &rows, &params(0, 6 * HOUR, HOUR, 6 * HOUR));
        // Buckets 0-3 hold one undercut (churn), 4-5 none (calm); baseline = median([1,1,1,1,0,0]) = 1.
        assert_eq!(out.summary.contested_share, Some(4.0 / 6.0));
        assert_eq!(out.summary.typical_undercuts_per_hour, Some(1.0));
        // The only closed episode starting in the window: HOUR..HOUR+600, 990 → 1200 (left).
        assert_eq!(out.summary.floor_holds_median_secs, Some(600));
        assert_eq!(
            (out.summary.episodes_left, out.summary.episodes_undercut),
            (1, 0)
        );
        assert_eq!(out.coverage_from, Some(0));
    }

    #[test]
    fn no_anchor_means_no_coverage_anywhere() {
        let p = PressureParams {
            anchor: None,
            ..params(0, 2 * HOUR, HOUR, 2 * HOUR)
        };
        let out = pressure(&[ev(10, 1, 100, 50)], &[fc(0, 0, 100)], &p);
        assert!(out.buckets.iter().all(|b| b.state == Unknown));
        assert_eq!(out.summary, Default::default());
        assert_eq!((out.baseline, out.coverage_from), (None, None));
    }
}
