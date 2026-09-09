use crate::analysis::format_duration_short;
use crate::i18n::{I18nKeys, Locale, t_string};
use chrono::{Duration, NaiveDateTime};
use leptos_i18n::I18nContext;
use ultros_api_types::freshness::FreshnessVerdict;
use ultros_api_types::{SaleHistory, WorldItemLastUpdated};

/// Inputs for the freshness/cadence badges, derived from the listings payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreshnessInputs {
    /// Time since Ultros last ingested market data for this item anywhere in
    /// the page's scope. `None` when nothing has ever been ingested.
    pub age: Option<Duration>,
    /// Sales velocity across the whole page scope (world, DC, or region).
    /// This is what the cadence badge shows: "how fast does it sell *here*".
    pub scope_sales_per_day: Option<f32>,
    /// Scope velocity normalized to a single market board. Freshness is a
    /// per-board judgement, so this is what the verdict thresholds consume.
    pub per_world_sales_per_day: Option<f32>,
}

/// Derives the freshness-badge inputs from the item page's listings payload.
///
/// Age basis: the newest `last_updated` ingest marker in scope — when Ultros
/// actually last heard about this item's boards — NOT `ActiveListing::timestamp`,
/// which is Universalis' `last_review_time` (when the seller last touched the
/// listing in-game). A board unrefreshed for days must not look "Fresh" just
/// because a retainer re-listed right before the last ingest.
///
/// Velocity scoping: `sales` are merged across every world in the page scope
/// (capped at the newest 200), so on a DC/region view the raw rate is roughly
/// `world_count`× a single board's rate. The per-200-cap window makes per-world
/// subsets sparse and biased, so instead of filtering by world we divide the
/// scope rate by `world_count` to approximate the average board's velocity.
/// On a single-world page (`world_count == 1`) this is a no-op.
pub fn derive_freshness_inputs(
    last_updated: &[WorldItemLastUpdated],
    sales: &[SaleHistory],
    world_count: usize,
    now: NaiveDateTime,
) -> FreshnessInputs {
    let age = last_updated
        .iter()
        .map(|updated| updated.updated_at)
        .max()
        .map(|updated_at| now - updated_at);
    let scope_sales_per_day = ultros_api_types::freshness::sales_per_day(sales);
    FreshnessInputs {
        age,
        scope_sales_per_day,
        per_world_sales_per_day: scope_sales_per_day
            .map(|velocity| velocity / world_count.max(1) as f32),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FreshnessTone {
    Success,
    Warning,
    Error,
    Neutral,
}

impl FreshnessTone {
    pub fn css_classes(&self) -> &'static str {
        match self {
            Self::Success => {
                "text-emerald-300 border-emerald-400/40 bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]"
            }
            Self::Warning => {
                "text-amber-300 border-amber-400/40 bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]"
            }
            Self::Error => {
                "text-red-300 border-red-400/40 bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]"
            }
            Self::Neutral => {
                "text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FreshnessLabel {
    Fresh,
    Caution,
    VerifyInGame,
    NoData,
}

impl FreshnessLabel {
    #[allow(dead_code)]
    pub fn get_text(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        match self {
            Self::Fresh => t_string!(i18n, freshness_fresh).to_string(),
            Self::Caution => t_string!(i18n, freshness_caution).to_string(),
            Self::VerifyInGame => t_string!(i18n, freshness_verify).to_string(),
            Self::NoData => t_string!(i18n, freshness_no_data).to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct FreshnessVerdictDisplay {
    pub tone: FreshnessTone,
    pub label: FreshnessLabel,
    pub age_formatted: Option<String>,
}

impl FreshnessVerdictDisplay {
    #[allow(dead_code)]
    pub fn format_label(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        if let Some(age) = &self.age_formatted {
            t_string!(i18n, freshness_data_age, age = age).to_string()
        } else {
            self.label.get_text(i18n)
        }
    }

    /// Tooltip explaining the verdict, since the visible label only shows the
    /// data age. Composes "{verdict}: {explanation}".
    #[allow(dead_code)]
    pub fn tooltip(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        let explanation = match self.label {
            FreshnessLabel::Fresh => t_string!(i18n, freshness_tooltip_fresh),
            FreshnessLabel::Caution => t_string!(i18n, freshness_tooltip_caution),
            FreshnessLabel::VerifyInGame => t_string!(i18n, freshness_tooltip_verify),
            FreshnessLabel::NoData => t_string!(i18n, freshness_tooltip_no_data),
        };
        format!("{}: {}", self.label.get_text(i18n), explanation)
    }
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
    use chrono::{DateTime, Duration};
    use ultros_api_types::freshness::{FreshnessVerdict, calculate_freshness_verdict};

    fn sale_at(seconds: i64) -> SaleHistory {
        SaleHistory {
            id: seconds as i32,
            quantity: 1,
            price_per_item: 100,
            buying_character_id: 1,
            hq: false,
            sold_item_id: 1,
            sold_date: DateTime::from_timestamp(seconds, 0).unwrap().naive_utc(),
            world_id: 1,
            buyer_name: None,
        }
    }

    fn ingest_at(world_id: i32, seconds: i64) -> WorldItemLastUpdated {
        WorldItemLastUpdated {
            world_id,
            updated_at: DateTime::from_timestamp(seconds, 0).unwrap().naive_utc(),
        }
    }

    fn ts(seconds: i64) -> NaiveDateTime {
        DateTime::from_timestamp(seconds, 0).unwrap().naive_utc()
    }

    #[test]
    fn test_age_comes_from_ingest_time_not_listings() {
        // The seller last touched their listings days ago (which is what
        // ActiveListing::timestamp would say), but Ultros ingested the board
        // 30 seconds ago: the badge must judge the 30-second ingest age.
        let now = ts(1_000_000);
        let inputs = derive_freshness_inputs(
            &[ingest_at(1, 1_000_000 - 30)],
            &[sale_at(0), sale_at(86_400)],
            1,
            now,
        );
        assert_eq!(inputs.age, Some(Duration::seconds(30)));
        assert_eq!(
            calculate_freshness_verdict(inputs.age, inputs.per_world_sales_per_day),
            FreshnessVerdict::Fresh
        );

        // Conversely, a stale ingest is stale even if a seller re-listed just
        // before it: 4 days since the last ingest at 1 sale/day => VerifyInGame.
        let inputs = derive_freshness_inputs(
            &[ingest_at(1, 1_000_000 - 4 * 86_400)],
            &[sale_at(0), sale_at(86_400)],
            1,
            now,
        );
        assert_eq!(inputs.age, Some(Duration::days(4)));
        assert_eq!(
            calculate_freshness_verdict(inputs.age, inputs.per_world_sales_per_day),
            FreshnessVerdict::VerifyInGame
        );
    }

    #[test]
    fn test_no_ingest_marker_means_no_data() {
        let inputs = derive_freshness_inputs(&[], &[sale_at(0), sale_at(86_400)], 1, ts(1_000));
        assert_eq!(inputs.age, None);
        assert_eq!(
            calculate_freshness_verdict(inputs.age, inputs.per_world_sales_per_day),
            FreshnessVerdict::NoData
        );
    }

    #[test]
    fn test_newest_ingest_in_scope_wins() {
        let now = ts(10_000);
        let inputs = derive_freshness_inputs(
            &[
                ingest_at(1, 1_000),
                ingest_at(2, 9_000),
                ingest_at(3, 5_000),
            ],
            &[],
            3,
            now,
        );
        assert_eq!(inputs.age, Some(Duration::seconds(1_000)));
    }

    #[test]
    fn test_empty_and_single_sale_are_no_data() {
        let now = ts(10_000);
        // Empty sales: unknown velocity, not a confident zero.
        let inputs = derive_freshness_inputs(&[ingest_at(1, 9_990)], &[], 1, now);
        assert_eq!(inputs.scope_sales_per_day, None);
        assert_eq!(inputs.per_world_sales_per_day, None);
        assert_eq!(
            calculate_freshness_verdict(inputs.age, inputs.per_world_sales_per_day),
            FreshnessVerdict::NoData
        );

        // One sale: consistent with the empty case.
        let inputs = derive_freshness_inputs(&[ingest_at(1, 9_990)], &[sale_at(0)], 1, now);
        assert_eq!(inputs.per_world_sales_per_day, None);
        assert_eq!(
            calculate_freshness_verdict(inputs.age, inputs.per_world_sales_per_day),
            FreshnessVerdict::NoData
        );
    }

    #[test]
    fn test_velocity_normalized_by_world_count() {
        let now = ts(2 * 86_400);
        // 9 sales over one day across an 8-world DC: scope rate 8/day,
        // per-board rate 1/day.
        let sales: Vec<_> = (0..9).map(|i| sale_at(i * 86_400 / 8)).collect();
        let inputs = derive_freshness_inputs(&[ingest_at(1, 2 * 86_400 - 60)], &sales, 8, now);
        assert_eq!(inputs.scope_sales_per_day, Some(8.0));
        assert_eq!(inputs.per_world_sales_per_day, Some(1.0));

        // A world count of 0 (unknown scope) must not divide by zero.
        let inputs = derive_freshness_inputs(&[], &sales, 0, now);
        assert_eq!(inputs.per_world_sales_per_day, Some(8.0));
    }

    #[test]
    fn test_threshold_selection_at_normalized_boundaries() {
        // 1 sale/day per board => 12h Fresh / 36h Caution boundaries.
        let sales: Vec<_> = (0..9).map(|i| sale_at(i * 86_400 / 8)).collect();
        let base = 10 * 86_400;
        for (age_hours, expected) in [
            (12, FreshnessVerdict::Fresh),
            (13, FreshnessVerdict::Caution),
            (36, FreshnessVerdict::Caution),
            (37, FreshnessVerdict::VerifyInGame),
        ] {
            let now = ts(base + age_hours * 3_600);
            let inputs = derive_freshness_inputs(&[ingest_at(1, base)], &sales, 8, now);
            assert_eq!(
                calculate_freshness_verdict(inputs.age, inputs.per_world_sales_per_day),
                expected,
                "expected {expected:?} at {age_hours}h"
            );
        }
    }

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
    fn test_verdict_to_display_mapping_exhaustive() {
        use FreshnessVerdict::*;

        let cases = vec![
            (Fresh, FreshnessLabel::Fresh, FreshnessTone::Success),
            (Caution, FreshnessLabel::Caution, FreshnessTone::Warning),
            (
                VerifyInGame,
                FreshnessLabel::VerifyInGame,
                FreshnessTone::Error,
            ),
            (NoData, FreshnessLabel::NoData, FreshnessTone::Neutral),
        ];

        for (verdict, expected_label, expected_tone) in cases {
            let display = get_freshness_verdict_display(verdict, None);
            assert_eq!(
                display.label, expected_label,
                "Verdict {:?} should map to label {:?}",
                verdict, expected_label
            );
            assert_eq!(
                display.tone, expected_tone,
                "Verdict {:?} should map to tone {:?}",
                verdict, expected_tone
            );
        }
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

    #[test]
    fn test_freshness_tone_css_classes() {
        assert_eq!(
            FreshnessTone::Success.css_classes(),
            "text-emerald-300 border-emerald-400/40 bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]"
        );
        assert_eq!(
            FreshnessTone::Warning.css_classes(),
            "text-amber-300 border-amber-400/40 bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]"
        );
        assert_eq!(
            FreshnessTone::Error.css_classes(),
            "text-red-300 border-red-400/40 bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]"
        );
        assert_eq!(
            FreshnessTone::Neutral.css_classes(),
            "text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
        );
    }
}
