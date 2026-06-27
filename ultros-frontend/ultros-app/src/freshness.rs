use crate::analysis::format_duration_short;
use chrono::Duration;
use ultros_api_types::freshness::FreshnessVerdict;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FreshnessTone {
    Success,
    Warning,
    Error,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FreshnessLabel {
    Fresh,
    Caution,
    VerifyInGame,
    NoData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct FreshnessVerdictDisplay {
    pub tone: FreshnessTone,
    pub label: FreshnessLabel,
    pub age_formatted: Option<String>,
}

/// Pure helper that maps a freshness verdict and optional age into structured display data.
#[allow(dead_code)]
pub fn get_freshness_verdict_display(
    verdict: FreshnessVerdict,
    age: Option<Duration>,
) -> FreshnessVerdictDisplay {
    let (label, tone) = match verdict {
        FreshnessVerdict::Fresh => (FreshnessLabel::Fresh, FreshnessTone::Success),
        FreshnessVerdict::Caution => (FreshnessLabel::Caution, FreshnessTone::Warning),
        FreshnessVerdict::VerifyInGame => (FreshnessLabel::VerifyInGame, FreshnessTone::Error),
        FreshnessVerdict::NoData => (FreshnessLabel::NoData, FreshnessTone::Neutral),
    };

    let age_formatted = age.map(|a| format_duration_short(a.num_seconds().max(0) as u64));

    FreshnessVerdictDisplay {
        tone,
        label,
        age_formatted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use ultros_api_types::freshness::FreshnessVerdict;

    #[test]
    fn test_get_freshness_verdict_display() {
        // Fresh
        let display = get_freshness_verdict_display(FreshnessVerdict::Fresh, None);
        assert_eq!(display.label, FreshnessLabel::Fresh);
        assert_eq!(display.tone, FreshnessTone::Success);
        assert_eq!(display.age_formatted, None);

        let display =
            get_freshness_verdict_display(FreshnessVerdict::Fresh, Some(Duration::hours(2)));
        assert_eq!(display.label, FreshnessLabel::Fresh);
        assert_eq!(display.age_formatted, Some("2h".to_string()));

        // Caution
        let display = get_freshness_verdict_display(FreshnessVerdict::Caution, None);
        assert_eq!(display.label, FreshnessLabel::Caution);
        assert_eq!(display.tone, FreshnessTone::Warning);

        // Verify
        let display = get_freshness_verdict_display(FreshnessVerdict::VerifyInGame, None);
        assert_eq!(display.label, FreshnessLabel::VerifyInGame);
        assert_eq!(display.tone, FreshnessTone::Error);

        // No Data
        let display = get_freshness_verdict_display(FreshnessVerdict::NoData, None);
        assert_eq!(display.label, FreshnessLabel::NoData);
        assert_eq!(display.tone, FreshnessTone::Neutral);
    }

    #[test]
    fn test_age_formatting_boundaries() {
        // 59 seconds
        let display =
            get_freshness_verdict_display(FreshnessVerdict::Fresh, Some(Duration::seconds(59)));
        assert_eq!(display.age_formatted, Some("59s".to_string()));

        // 60 seconds -> 1m
        let display =
            get_freshness_verdict_display(FreshnessVerdict::Fresh, Some(Duration::seconds(60)));
        assert_eq!(display.age_formatted, Some("1m".to_string()));

        // 3600 seconds -> 1h
        let display =
            get_freshness_verdict_display(FreshnessVerdict::Fresh, Some(Duration::seconds(3600)));
        assert_eq!(display.age_formatted, Some("1h".to_string()));

        // Negative duration (should be treated as 0s)
        let display =
            get_freshness_verdict_display(FreshnessVerdict::Fresh, Some(Duration::seconds(-10)));
        assert_eq!(display.age_formatted, Some("0s".to_string()));
    }
}
