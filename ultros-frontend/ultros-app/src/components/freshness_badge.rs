use crate::freshness::{FreshnessTone, get_freshness_verdict_display};
use crate::i18n::*;
use chrono::Duration;
use leptos::prelude::*;
use ultros_api_types::freshness::FreshnessVerdict;

#[component]
pub fn FreshnessBadge(verdict: FreshnessVerdict, age: Option<Duration>) -> impl IntoView {
    let i18n = use_i18n();
    let display = get_freshness_verdict_display(verdict, age);
    let classes = match display.tone {
        FreshnessTone::Success => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-emerald-300 border-emerald-400/40 \
             bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]"
        }
        FreshnessTone::Warning => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-amber-300 border-amber-400/40 \
             bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]"
        }
        FreshnessTone::Error => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-red-300 border-red-400/40 \
             bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]"
        }
        FreshnessTone::Neutral => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-[color:var(--color-text)] border-[color:var(--color-outline)] \
             bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
        }
    };

    view! { <span class=classes>{display.format_label(i18n)}</span> }
}
