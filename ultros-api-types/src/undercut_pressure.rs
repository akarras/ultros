//! Undercut pressure for one item on one world (item page pane and cards),
//! plus the per-world rows the datacenter/region market board shows.
use serde::{Deserialize, Serialize};

/// How contested a bucket was. `Unknown` = before the world's floor anchor,
/// where Ultros cannot tell a quiet board from an untracked one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureState {
    #[default]
    Unknown,
    Calm,
    Churn,
    War,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressureBucket {
    /// Epoch-aligned bucket start (unix seconds), same alignment as `PriceSeries`.
    pub start: i64,
    /// Same-listing drops below 1% of the previous price.
    pub trims: u32,
    /// Same-listing drops of 1% or more.
    pub cuts: u32,
    /// Distinct retainers that undercut in this bucket (trims and cuts both count).
    pub sellers: u16,
    /// As-of floor at the bucket start / last second. `None` = empty or unknown.
    pub floor_open: Option<u32>,
    pub floor_close: Option<u32>,
    pub state: PressureState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarSpan {
    pub start: i64,
    /// Exclusive end (last war bucket's start + bucket width).
    pub end: i64,
    pub undercuts: u32,
    pub sellers: u16,
    /// Floor change over the span as a fraction (−0.18 = fell 18%).
    pub floor_change: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WarStatus {
    #[default]
    None,
    Active,
    Ended {
        at: i64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PressureSummary {
    /// As-of floor now vs 24 h ago, as a fraction.
    #[serde(default)]
    pub floor_trend_24h: Option<f64>,
    #[serde(default)]
    pub war: WarStatus,
    /// Most recent war span in the last 24 h.
    #[serde(default)]
    pub last_war: Option<WarSpan>,
    /// Share of the chart window's known buckets in Churn or War.
    #[serde(default)]
    pub contested_share: Option<f64>,
    #[serde(default)]
    pub typical_undercuts_per_hour: Option<f64>,
    /// Median life of a floor price that started and ended in the window.
    #[serde(default)]
    pub floor_holds_median_secs: Option<i64>,
    /// Floor episodes that ended because the floor listing left (bought, pulled or raised).
    #[serde(default)]
    pub episodes_left: u32,
    /// Floor episodes that ended because a cheaper listing took the floor.
    #[serde(default)]
    pub episodes_undercut: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UndercutPressure {
    pub world_id: i32,
    pub from: i64,
    pub to: i64,
    pub bucket_seconds: i64,
    /// The world's floor anchor; buckets before it are `Unknown`.
    #[serde(default)]
    pub coverage_from: Option<i64>,
    /// Median undercuts per known bucket in the window.
    #[serde(default)]
    pub baseline: Option<f64>,
    pub buckets: Vec<PressureBucket>,
    #[serde(default)]
    pub wars: Vec<WarSpan>,
    #[serde(default)]
    pub summary: PressureSummary,
}

/// Phase 2: one row of the datacenter/region market board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldPressureRow {
    pub world_id: i32,
    /// Worst state over the last 24 h: War > Churn > Calm > Unknown.
    pub state_24h: PressureState,
    #[serde(default)]
    pub war_sellers: Option<u16>,
    #[serde(default)]
    pub floor_holds_median_secs: Option<i64>,
    /// Newest non-snapshot `added` event (unix seconds).
    #[serde(default)]
    pub newest_added: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopePressure {
    pub rows: Vec<WorldPressureRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UndercutPressure {
        UndercutPressure {
            world_id: 34,
            from: 0,
            to: 7200,
            bucket_seconds: 3600,
            coverage_from: Some(0),
            baseline: Some(1.0),
            buckets: vec![PressureBucket {
                start: 0,
                trims: 1,
                cuts: 3,
                sellers: 2,
                floor_open: Some(1000),
                floor_close: Some(900),
                state: PressureState::War,
            }],
            wars: vec![WarSpan {
                start: 0,
                end: 3600,
                undercuts: 4,
                sellers: 2,
                floor_change: Some(-0.1),
            }],
            summary: PressureSummary {
                war: WarStatus::Ended { at: 3600 },
                episodes_left: 2,
                ..Default::default()
            },
        }
    }

    #[test]
    fn round_trips() {
        let value = sample();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(
            serde_json::from_str::<UndercutPressure>(&json).unwrap(),
            value
        );
        let row = WorldPressureRow {
            world_id: 1,
            state_24h: PressureState::Churn,
            war_sellers: None,
            floor_holds_median_secs: Some(60),
            newest_added: Some(5),
        };
        let json = serde_json::to_string(&ScopePressure {
            rows: vec![row.clone()],
        })
        .unwrap();
        assert_eq!(
            serde_json::from_str::<ScopePressure>(&json).unwrap().rows,
            vec![row]
        );
    }

    #[test]
    fn war_status_is_tagged_snake_case() {
        assert_eq!(
            serde_json::to_string(&WarStatus::Ended { at: 5 }).unwrap(),
            r#"{"kind":"ended","at":5}"#
        );
        assert_eq!(
            serde_json::to_string(&WarStatus::Active).unwrap(),
            r#"{"kind":"active"}"#
        );
    }

    #[test]
    fn minimal_body_deserializes_with_defaults() {
        let body = r#"{"world_id":1,"from":0,"to":1,"bucket_seconds":3600,"buckets":[]}"#;
        let value: UndercutPressure = serde_json::from_str(body).unwrap();
        assert_eq!(value.summary, PressureSummary::default());
        assert!(value.wars.is_empty() && value.coverage_from.is_none());
    }

    #[test]
    fn states_order_by_severity() {
        assert!(PressureState::War > PressureState::Churn);
        assert!(PressureState::Churn > PressureState::Calm);
        assert!(PressureState::Calm > PressureState::Unknown);
    }
}
