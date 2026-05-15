//! Visual chip for the analyzer's `ConfidenceBand`.
//!
//! Maps the API enum to a tinted pill with a tooltip explaining the band.
//! Defaults to rendering nothing for `Unknown` — Pass-1 results that haven't
//! been deep-scanned yet shouldn't visually pollute the row.

use leptos::prelude::*;
use ultros_api_types::trends::ConfidenceBand;

use crate::i18n::*;

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
    let (label, classes, tooltip): (String, &'static str, String) = match band {
        ConfidenceBand::Unknown => {
            // Render nothing — keeps the row clean in CH-degraded mode.
            return ().into_any();
        }
        ConfidenceBand::High => (
            t_string!(i18n, confidence_band_high).to_string(),
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-emerald-300 border-emerald-400/40 \
             bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]",
            t_string!(i18n, confidence_band_high_help).to_string(),
        ),
        ConfidenceBand::Medium => (
            t_string!(i18n, confidence_band_medium).to_string(),
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-[color:var(--color-text)] border-[color:var(--color-outline)] \
             bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]",
            t_string!(i18n, confidence_band_medium_help).to_string(),
        ),
        ConfidenceBand::Low => (
            t_string!(i18n, confidence_band_low).to_string(),
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-amber-300 border-amber-400/40 \
             bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]",
            t_string!(i18n, confidence_band_low_help).to_string(),
        ),
        ConfidenceBand::Unusable => (
            t_string!(i18n, confidence_band_unusable).to_string(),
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-red-300 border-red-400/40 \
             bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]",
            t_string!(i18n, confidence_band_unusable_help).to_string(),
        ),
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
        <span class=classes title=tooltip_full>
            {label}
        </span>
    }
    .into_any()
}
