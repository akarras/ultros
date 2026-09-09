//! Listing floors sampled at interval boundaries, independently of completed sales.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloorPoint {
    pub timestamp: i64,
    /// Lowest last-observed asking price across tracked worlds/qualities.
    /// None means no tracked listings; never plot this as zero gil.
    pub price: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloorHistory {
    pub from: i64,
    pub to: i64,
    pub bucket_seconds: i64,
    pub points: Vec<FloorPoint>,
}

/// Explicit cadence for bounded analyzer batches; chart GET keeps adaptive sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorInterval {
    Hourly,
    Daily,
}
impl FloorInterval {
    pub fn seconds(self) -> i64 {
        match self {
            Self::Hourly => 3600,
            Self::Daily => 86400,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorHistoryRequest {
    pub item_ids: Vec<i32>,
    pub from: i64,
    pub to: i64,
    pub interval: FloorInterval,
    /// Omitted = both qualities, returned separately to preserve item/HQ keys.
    pub hq: Option<bool>,
}
impl FloorHistoryRequest {
    pub fn valid(&self) -> bool {
        !self.item_ids.is_empty()
            && self.item_ids.len() <= 20
            && self.item_ids.iter().all(|i| *i > 0)
            && self.from >= 0
            && self.to > self.from
            && self.to <= i64::from(u32::MAX)
            && self.to - self.from <= 90 * 86400
            && ((self.to - self.from + self.interval.seconds() - 1) / self.interval.seconds() + 1)
                * self.item_ids.len() as i64
                * if self.hq.is_some() { 1 } else { 2 }
                <= 10_000
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloorHistoryBatch {
    pub series: Vec<ItemFloorHistory>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemFloorHistory {
    pub item_id: i32,
    pub hq: bool,
    pub history: FloorHistory,
    /// Exact transition extrema, not extrema of the sampled closing prices.
    pub bounds: FloorBounds,
    /// Null samples at these timestamps are unknown, rather than known empty boards.
    pub unknown_timestamps: Vec<i64>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloorBounds {
    pub min: Option<u32>,
    pub max: Option<u32>,
    /// Known means every world in scope has a baseline. Empty is a subset.
    pub known_secs: u64,
    pub empty_secs: u64,
    pub unknown_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_batches_reject_large_work_before_querying() {
        let mut req = FloorHistoryRequest {
            item_ids: vec![1],
            from: 0,
            to: 90 * 86400,
            interval: FloorInterval::Hourly,
            hq: None,
        };
        assert!(req.valid());
        req.item_ids = vec![1; 20];
        assert!(!req.valid());
        req.interval = FloorInterval::Daily;
        assert!(req.valid());
        req.item_ids.push(2);
        assert!(!req.valid());
        req.item_ids = vec![1];
        req.to += 1;
        assert!(!req.valid());
        req.to = 1;
        req.from = 1;
        assert!(!req.valid());
        req.from = -1;
        assert!(!req.valid());
        req.from = 0;
        req.item_ids = vec![];
        assert!(!req.valid());
    }
}
