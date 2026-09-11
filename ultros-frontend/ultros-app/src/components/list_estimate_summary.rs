//! The cart's estimated total and a one-line account of how much of the
//! cart that number actually covers. The arithmetic and the coverage rules
//! live in `ultros_calc::list_estimate`; this only turns them into text.

use crate::i18n::*;
use leptos::prelude::*;
use thousands::Separable;
use ultros_calc::list_estimate::{CartEstimate, Coverage};

type I18n = leptos_i18n::I18nContext<Locale, I18nKeys>;

/// The compact status line, by priority: a saturated sum can't be trusted at
/// all, then the coverage states, then the basis sentence for a complete
/// estimate so the number is never shown without what it means.
pub(crate) fn coverage_text(i18n: I18n, estimate: &CartEstimate) -> String {
    if estimate.saturated {
        return t_string!(i18n, lists_estimate_saturated).to_string();
    }
    match estimate.coverage {
        Coverage::None => t_string!(i18n, lists_estimate_none).to_string(),
        Coverage::Partial => t_string!(
            i18n,
            lists_estimate_partial,
            priced = estimate.lines_priced,
            lines = estimate.lines_needing_units(),
            units = estimate.unpriced_units
        )
        .to_string(),
        Coverage::Empty => t_string!(i18n, lists_estimate_empty).to_string(),
        Coverage::Complete => t_string!(i18n, lists_estimate_basis).to_string(),
    }
}

#[component]
pub fn ListEstimateSummary(estimate: Signal<CartEstimate>) -> impl IntoView {
    let i18n = use_i18n();
    let total = move || {
        estimate.with(|estimate| {
            t_string!(
                i18n,
                lists_workspace_gil,
                price = estimate.total.separate_with_commas()
            )
            .to_string()
        })
    };
    let incomplete = move || estimate.with(CartEstimate::is_incomplete);
    let status = move || estimate.with(|estimate| coverage_text(i18n, estimate));
    view! {
        <section
            class="panel rounded-xl px-4 py-3 flex flex-wrap items-center justify-between gap-x-4 gap-y-1"
            data-testid="list-estimate-summary"
            aria-label=t_string!(i18n, lists_estimate_label)
        >
            <div class="flex flex-wrap items-baseline gap-x-2">
                <span class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                    {t!(i18n, lists_estimate_heading)}
                </span>
                <strong class="text-lg tabular-nums" data-testid="list-estimate-total" data-incomplete=move || incomplete().to_string()>
                    {total}
                </strong>
                <Show when=incomplete>
                    <span class="rounded-full border border-amber-500/60 px-2 py-0.5 text-xs font-semibold text-amber-300">
                        {t!(i18n, lists_estimate_incomplete)}
                    </span>
                </Show>
            </div>
            <p class="text-sm text-[color:var(--color-text-muted)]" role="status" data-testid="list-estimate-status">
                {status}
            </p>
        </section>
    }
}
