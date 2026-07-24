use crate::analysis::SalesCadence;
use crate::i18n::*;
use crate::sales_cadence::{SalesCadenceTone, get_sales_cadence_display};
use leptos::prelude::*;

#[component]
pub fn SalesCadenceBadge(
    cadence: SalesCadence,
    sales_per_day: f32,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let display = get_sales_cadence_display(cadence, sales_per_day);
    let full_label = display.format_label(i18n);
    let text = if compact {
        display.format_compact(i18n)
    } else {
        full_label.clone()
    };

    view! {
        <span
            title=compact.then(|| full_label.clone())
            class="inline-flex items-center py-0.5 rounded-full text-xs font-semibold border whitespace-nowrap max-w-full overflow-hidden"
            class=("px-1.5", compact)
            class=("px-2", !compact)
            class=("text-emerald-300", display.tone == SalesCadenceTone::Success)
            class=("border-emerald-400/40", display.tone == SalesCadenceTone::Success)
            class=("bg-[color:color-mix(in_srgb,#10b981_14%,transparent)]", display.tone == SalesCadenceTone::Success)
            class=("text-amber-300", display.tone == SalesCadenceTone::Warning)
            class=("border-amber-400/40", display.tone == SalesCadenceTone::Warning)
            class=("bg-[color:color-mix(in_srgb,#f59e0b_12%,transparent)]", display.tone == SalesCadenceTone::Warning)
            class=("text-red-300", display.tone == SalesCadenceTone::Error)
            class=("border-red-400/40", display.tone == SalesCadenceTone::Error)
            class=("bg-[color:color-mix(in_srgb,#ef4444_12%,transparent)]", display.tone == SalesCadenceTone::Error)
            class=("text-[color:var(--color-text)]", display.tone == SalesCadenceTone::Neutral)
            class=("border-[color:var(--color-outline)]", display.tone == SalesCadenceTone::Neutral)
            class=("bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]", display.tone == SalesCadenceTone::Neutral)
        >
            {text}
        </span>
    }
}
