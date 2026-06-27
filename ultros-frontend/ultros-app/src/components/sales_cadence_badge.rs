use crate::analysis::SalesCadence;
use crate::i18n::*;
use crate::sales_cadence::{SalesCadenceTone, get_sales_cadence_display};
use leptos::prelude::*;

#[component]
pub fn SalesCadenceBadge(cadence: SalesCadence, sales_per_day: f32) -> impl IntoView {
    let i18n = use_i18n();
    let display = get_sales_cadence_display(cadence, sales_per_day);
    let classes = match display.tone {
        SalesCadenceTone::Success => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-emerald-300 border-emerald-400/40 \
             bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]"
        }
        SalesCadenceTone::Warning => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-amber-300 border-amber-400/40 \
             bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]"
        }
        SalesCadenceTone::Error => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-red-300 border-red-400/40 \
             bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]"
        }
        SalesCadenceTone::Neutral => {
            "inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold \
             border text-[color:var(--color-text)] border-[color:var(--color-outline)] \
             bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
        }
    };

    view! { <span class=classes>{display.format_label(i18n)}</span> }
}
