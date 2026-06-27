use crate::analysis::format_duration_short;
use crate::i18n::{Locale, t_string};
use chrono::Duration;
use ultros_api_types::freshness::FreshnessVerdict;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessTone {
    Success,
    Warning,
    Error,
    Neutral,
}

pub struct FreshnessDisplay {
    pub label: String,
    pub tone: FreshnessTone,
}

/// Returns a display label and tone for a freshness verdict.
///
/// If `age` is provided, it is appended to the label, e.g. "Fresh (2h 15m)".
pub fn get_freshness_display(
    i18n: leptos_i18n::I18nContext<Locale>,
    verdict: FreshnessVerdict,
    age: Option<Duration>,
) -> FreshnessDisplay {
    let (label_text, tone) = match verdict {
        FreshnessVerdict::Fresh => (
            t_string!(i18n, freshness_fresh).to_string(),
            FreshnessTone::Success,
        ),
        FreshnessVerdict::Caution => (
            t_string!(i18n, freshness_caution).to_string(),
            FreshnessTone::Warning,
        ),
        FreshnessVerdict::VerifyInGame => (
            t_string!(i18n, freshness_verify).to_string(),
            FreshnessTone::Error,
        ),
        FreshnessVerdict::NoData => (
            t_string!(i18n, freshness_no_data).to_string(),
            FreshnessTone::Neutral,
        ),
    };

    let label = if let Some(age) = age {
        let age_str = format_duration_short(age.num_seconds().max(0) as u64);
        t_string!(i18n, freshness_label_with_age, label = label_text, age = age_str).to_string()
    } else {
        label_text
    };

    FreshnessDisplay { label, tone }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use ultros_api_types::freshness::FreshnessVerdict;
    use leptos::prelude::Owner;

    #[test]
    fn test_get_freshness_display() {
        let owner = Owner::new();
        owner.with(|| {
            let i18n = crate::i18n::provide_i18n_context();

            // Fresh
            let display = get_freshness_display(i18n, FreshnessVerdict::Fresh, None);
            assert_eq!(display.label, "Fresh");
            assert_eq!(display.tone, FreshnessTone::Success);

            let display = get_freshness_display(i18n, FreshnessVerdict::Fresh, Some(Duration::hours(2)));
            assert_eq!(display.label, "Fresh (2h)");
            assert_eq!(display.tone, FreshnessTone::Success);

            // Caution
            let display = get_freshness_display(i18n, FreshnessVerdict::Caution, None);
            assert_eq!(display.label, "Caution");
            assert_eq!(display.tone, FreshnessTone::Warning);

            let display = get_freshness_display(i18n, FreshnessVerdict::Caution, Some(Duration::days(2)));
            assert_eq!(display.label, "Caution (2d)");
            assert_eq!(display.tone, FreshnessTone::Warning);

            // Verify
            let display = get_freshness_display(i18n, FreshnessVerdict::VerifyInGame, None);
            assert_eq!(display.label, "Verify In-Game");
            assert_eq!(display.tone, FreshnessTone::Error);

            let display = get_freshness_display(i18n, FreshnessVerdict::VerifyInGame, Some(Duration::days(5)));
            assert_eq!(display.label, "Verify In-Game (5d)");
            assert_eq!(display.tone, FreshnessTone::Error);

            // No Data
            let display = get_freshness_display(i18n, FreshnessVerdict::NoData, None);
            assert_eq!(display.label, "No Data");
            assert_eq!(display.tone, FreshnessTone::Neutral);

            let display = get_freshness_display(i18n, FreshnessVerdict::NoData, Some(Duration::hours(1)));
            assert_eq!(display.label, "No Data (1h)");
            assert_eq!(display.tone, FreshnessTone::Neutral);
        });
    }

    #[test]
    fn test_age_formatting_boundaries() {
        let owner = Owner::new();
        owner.with(|| {
            let i18n = crate::i18n::provide_i18n_context();

            // 59 seconds
            let display = get_freshness_display(i18n, FreshnessVerdict::Fresh, Some(Duration::seconds(59)));
            assert_eq!(display.label, "Fresh (59s)");

            // 60 seconds -> 1m
            let display = get_freshness_display(i18n, FreshnessVerdict::Fresh, Some(Duration::seconds(60)));
            assert_eq!(display.label, "Fresh (1m)");

            // 3600 seconds -> 1h
            let display = get_freshness_display(i18n, FreshnessVerdict::Fresh, Some(Duration::seconds(3600)));
            assert_eq!(display.label, "Fresh (1h)");

            // Negative duration (should be treated as 0s)
            let display = get_freshness_display(i18n, FreshnessVerdict::Fresh, Some(Duration::seconds(-10)));
            assert_eq!(display.label, "Fresh (0s)");
        });
    }
}
