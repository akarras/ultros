//! Visual chip for the analyzer's `ConfidenceBand`.
//!
//! Maps the API enum to a tinted pill with a tooltip explaining the band.
//! Defaults to rendering nothing for `Unknown` — Pass-1 results that haven't
//! been deep-scanned yet shouldn't visually pollute the row.

use leptos::prelude::*;
use leptos_i18n::I18nContext;
use ultros_api_types::trends::ConfidenceBand;

use crate::i18n::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceTone {
    Success,
    Neutral,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceLabel {
    High,
    Medium,
    Low,
    Unusable,
}

impl ConfidenceLabel {
    pub fn get_text(&self, i18n: I18nContext<Locale, I18nKeys>) -> String {
        match self {
            Self::High => t_string!(i18n, confidence_band_high).to_string(),
            Self::Medium => t_string!(i18n, confidence_band_medium).to_string(),
            Self::Low => t_string!(i18n, confidence_band_low).to_string(),
            Self::Unusable => t_string!(i18n, confidence_band_unusable).to_string(),
        }
    }
}

pub fn get_confidence_verdict_display(
    band: ConfidenceBand,
) -> Option<(ConfidenceLabel, ConfidenceTone)> {
    match band {
        ConfidenceBand::Unknown => None,
        ConfidenceBand::High => Some((ConfidenceLabel::High, ConfidenceTone::Success)),
        ConfidenceBand::Medium => Some((ConfidenceLabel::Medium, ConfidenceTone::Neutral)),
        ConfidenceBand::Low => Some((ConfidenceLabel::Low, ConfidenceTone::Warning)),
        ConfidenceBand::Unusable => Some((ConfidenceLabel::Unusable, ConfidenceTone::Error)),
    }
}

/// Small chip showing the confidence band. Hidden when `band == Unknown` —
/// the analyzer returns Unknown for results without ClickHouse-backed
/// deep-scan data, and we don't want every row in a CH-outage scenario to
/// flash a question mark.
///
/// `sample_size` shows up in the tooltip when present so the user can
/// double-check "based on N sales" without us spending a row column on it.
#[component]
pub fn ConfidenceBadge(
    band: ConfidenceBand,
    #[prop(default = 0)] sample_size: u32,
) -> impl IntoView {
    let i18n = use_i18n();

    let (label_enum, tone) = match get_confidence_verdict_display(band) {
        Some(v) => v,
        None => return ().into_any(),
    };

    let label = label_enum.get_text(i18n);

    let tooltip = match label_enum {
        ConfidenceLabel::High => t_string!(i18n, confidence_band_high_help).to_string(),
        ConfidenceLabel::Medium => t_string!(i18n, confidence_band_medium_help).to_string(),
        ConfidenceLabel::Low => t_string!(i18n, confidence_band_low_help).to_string(),
        ConfidenceLabel::Unusable => t_string!(i18n, confidence_band_unusable_help).to_string(),
    };

    // Tooltip text: band help, optionally with sample count appended.
    let tooltip_full = if sample_size > 0 {
        format!(
            "{tooltip} ({})",
            t_string!(i18n, confidence_band_sample_size)
                .to_string()
                .replace("%n%", &sample_size.to_string())
        )
    } else {
        tooltip
    };

    view! {
        <span
            class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold border"
            class=("text-emerald-300", move || tone == ConfidenceTone::Success)
            class=("border-emerald-400/40", move || tone == ConfidenceTone::Success)
            class=("bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]", move || tone == ConfidenceTone::Success)
            class=("text-[color:var(--color-text)]", move || tone == ConfidenceTone::Neutral)
            class=("border-[color:var(--color-outline)]", move || tone == ConfidenceTone::Neutral)
            class=("bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]", move || tone == ConfidenceTone::Neutral)
            class=("text-amber-300", move || tone == ConfidenceTone::Warning)
            class=("border-amber-400/40", move || tone == ConfidenceTone::Warning)
            class=("bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]", move || tone == ConfidenceTone::Warning)
            class=("text-red-300", move || tone == ConfidenceTone::Error)
            class=("border-red-400/40", move || tone == ConfidenceTone::Error)
            class=("bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]", move || tone == ConfidenceTone::Error)
            title=tooltip_full>
            {label}
        </span>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_confidence_verdict_display() {
        // High: verifies that the highest confidence band correctly maps to the 'Success' tone
        // which drives the emerald CSS classes, and outputs the 'High' label.
        let display = get_confidence_verdict_display(ConfidenceBand::High).unwrap();
        assert_eq!(display.0, ConfidenceLabel::High);
        assert_eq!(display.1, ConfidenceTone::Success);

        // Medium: verifies the fallback 'Usable' band maps to a 'Neutral' tone (grey/brand ring).
        let display = get_confidence_verdict_display(ConfidenceBand::Medium).unwrap();
        assert_eq!(display.0, ConfidenceLabel::Medium);
        assert_eq!(display.1, ConfidenceTone::Neutral);

        // Low: verifies the rough estimate band maps to 'Warning' tone (amber).
        let display = get_confidence_verdict_display(ConfidenceBand::Low).unwrap();
        assert_eq!(display.0, ConfidenceLabel::Low);
        assert_eq!(display.1, ConfidenceTone::Warning);

        // Unusable: verifies that data flagged as unusable maps to the 'Error' tone (red).
        let display = get_confidence_verdict_display(ConfidenceBand::Unusable).unwrap();
        assert_eq!(display.0, ConfidenceLabel::Unusable);
        assert_eq!(display.1, ConfidenceTone::Error);

        // Unknown (Pass-1 results): Edge case. Verifies that when confidence is unknown (e.g. data hasn't
        // been deep-scanned yet), we explicitly return None so the UI renders nothing rather than
        // flashing a question mark or breaking the layout.
        assert_eq!(
            get_confidence_verdict_display(ConfidenceBand::Unknown),
            None
        );
    }
}
