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
