//! Confidence is an ordered assessment, independent of its translated label.
use std::cmp::Ordering;
use ultros_api_types::trends::ConfidenceBand;

/// Rank known assessments from unusable to high confidence. Pending or absent
/// assessments stay after known values in either direction.
pub fn compare_confidence(
    left: Option<ConfidenceBand>,
    right: Option<ConfidenceBand>,
    ascending: bool,
) -> Ordering {
    let rank = |band| match band {
        Some(ConfidenceBand::Unusable) => Some(0),
        Some(ConfidenceBand::Low) => Some(1),
        Some(ConfidenceBand::Medium) => Some(2),
        Some(ConfidenceBand::High) => Some(3),
        Some(ConfidenceBand::Unknown) | None => None,
    };
    match (rank(left), rank(right)) {
        (Some(left), Some(right)) => {
            if ascending {
                left.cmp(&right)
            } else {
                right.cmp(&left)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ConfidenceBand::*;

    #[test]
    fn confidence_sorts_by_strength_with_absent_assessments_last_in_both_directions() {
        for (ascending, known) in [
            (true, [Unusable, Low, Medium, High]),
            (false, [High, Medium, Low, Unusable]),
        ] {
            let mut assessments = [
                None,
                Some(Medium),
                Some(High),
                Some(Unknown),
                Some(Unusable),
                Some(Low),
            ];
            assessments.sort_by(|left, right| compare_confidence(*left, *right, ascending));
            assert_eq!(assessments[..4], known.map(Some));
            assert_eq!(assessments[4..], [None, Some(Unknown)]);
            assert_eq!(
                compare_confidence(Some(Low), Some(Low), ascending),
                Ordering::Equal
            );
        }
    }
}
