//! Bucket-width ladder shared with the server: which time-bucket widths the
//! chart may request, and how a requested width snaps onto the ladder.
//! Actual VWAP/volume bucketing now happens server-side (see
//! `ultros_api_types::price_series`), so this module only keeps the
//! constants both sides must agree on.

const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// Bucket width for VWAP lines / volume bars. `days_range` is the user's
/// selected window (7/30/90); `None` or 0 falls back to the data span.
pub fn bucket_seconds(days_range: Option<i32>, data_span_days: i64) -> i64 {
    let effective_days = match days_range {
        Some(days) if days > 0 => days as i64,
        _ => data_span_days.max(1),
    };
    match effective_days {
        ..=2 => HOUR,
        3..=10 => 6 * HOUR,
        11..=120 => DAY,
        121..=400 => 7 * DAY,
        _ => 30 * DAY,
    }
}

/// The only bucket widths this system produces. The server snaps requested
/// widths onto this ladder so a hand-crafted request cannot ask for a
/// million buckets, and so client-side axis labelling always matches the
/// server's bucketing.
pub const BUCKET_LADDER: [i64; 5] = [HOUR, 6 * HOUR, DAY, 7 * DAY, 30 * DAY];

/// Snap an arbitrary width up to the next ladder step. Values above the top
/// clamp to the widest bucket.
pub fn snap_bucket_seconds(requested: i64) -> i64 {
    BUCKET_LADDER
        .iter()
        .copied()
        .find(|step| *step >= requested)
        .unwrap_or(30 * DAY)
}

/// Next ladder step up, or `None` at the top. Used to widen rather than
/// truncate when a response would exceed the bucket cap.
pub fn widen_bucket(current: i64) -> Option<i64> {
    let snapped = snap_bucket_seconds(current);
    BUCKET_LADDER.iter().copied().find(|step| *step > snapped)
}

/// Bucket width for a time span expressed in seconds — the server's entry
/// point. Delegates to [`bucket_seconds`] so both callers share one ladder.
pub fn bucket_seconds_for_span(span_secs: i64) -> i64 {
    bucket_seconds(None, (span_secs / DAY).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_seconds_scales_with_window() {
        assert_eq!(bucket_seconds(Some(7), 0), 6 * 3_600);
        assert_eq!(bucket_seconds(Some(30), 0), 86_400);
        assert_eq!(bucket_seconds(Some(90), 0), 86_400);
        assert_eq!(bucket_seconds(None, 2), 3_600);
        assert_eq!(bucket_seconds(None, 500), 30 * 86_400);
    }

    #[test]
    fn snap_rounds_to_the_nearest_ladder_step_not_below_it() {
        assert_eq!(snap_bucket_seconds(1), HOUR);
        assert_eq!(snap_bucket_seconds(HOUR), HOUR);
        assert_eq!(snap_bucket_seconds(2 * HOUR), 6 * HOUR);
        assert_eq!(snap_bucket_seconds(DAY), DAY);
        assert_eq!(snap_bucket_seconds(i64::MAX), 30 * DAY);
    }

    #[test]
    fn widen_walks_up_the_ladder_and_stops_at_the_top() {
        assert_eq!(widen_bucket(HOUR), Some(6 * HOUR));
        assert_eq!(widen_bucket(6 * HOUR), Some(DAY));
        assert_eq!(widen_bucket(30 * DAY), None);
    }

    #[test]
    fn span_picks_the_same_width_as_the_days_based_helper() {
        // 30 days of data with no explicit range: both paths must agree, or the
        // server and client bucket differently.
        assert_eq!(bucket_seconds_for_span(30 * DAY), bucket_seconds(None, 30));
    }
}
