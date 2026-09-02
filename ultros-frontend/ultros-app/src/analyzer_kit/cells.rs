//! The kit's cell vocabulary: a small value enum rendered by one match,
//! so per-variant markup lives in exactly one place and every
//! resource-backed variant keeps one DOM shape across its states.

use ultros_api_types::trends::ConfidenceBand;

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Gil(i32),
    RoiBadge(i32),
    Count(u64),
    Confidence(ConfidenceBand),
    LastSoldUnix(i64),
    /// A gil amount with a percent sub-line (VWAP and its % vs price).
    /// `amount <= 0` renders the dash; the sub-line is always present.
    GilWithPct {
        amount: i32,
        pct: Option<f32>,
    },
    /// The page renders this cell itself.
    Custom,
}
