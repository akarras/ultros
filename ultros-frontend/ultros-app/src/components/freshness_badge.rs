use crate::freshness::{FreshnessTone, get_freshness_verdict_display};
use crate::i18n::*;
use chrono::Duration;
use leptos::prelude::*;
use ultros_api_types::freshness::FreshnessVerdict;

#[component]
pub fn FreshnessBadge(
    verdict: FreshnessVerdict,
    age: Option<Duration>,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let display = get_freshness_verdict_display(verdict, age);

    view! {
        <span
            class="inline-flex items-center py-0.5 rounded-full text-xs font-semibold border"
            class=("px-1.5", move || compact)
            class=("px-2", move || !compact)
            class=("text-emerald-300", move || display.tone == FreshnessTone::Success)
            class=("border-emerald-400/40", move || display.tone == FreshnessTone::Success)
            class=("bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]", move || {
                display.tone == FreshnessTone::Success
            })
            class=("text-amber-300", move || display.tone == FreshnessTone::Warning)
            class=("border-amber-400/40", move || display.tone == FreshnessTone::Warning)
            class=("bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]", move || {
                display.tone == FreshnessTone::Warning
            })
            class=("text-red-300", move || display.tone == FreshnessTone::Error)
            class=("border-red-400/40", move || display.tone == FreshnessTone::Error)
            class=("bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]", move || {
                display.tone == FreshnessTone::Error
            })
            class=("text-[color:var(--color-text)]", move || display.tone == FreshnessTone::Neutral)
            class=("border-[color:var(--color-outline)]", move || {
                display.tone == FreshnessTone::Neutral
            })
            class=("bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]", move || {
                display.tone == FreshnessTone::Neutral
            })
        >
            {display.format_label(i18n)}
        </span>
    }
}
