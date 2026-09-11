//! The cart's estimated total and a one-line account of how much of the
//! cart that number actually covers, plus where and when the prices behind
//! it came from. The arithmetic, coverage rules and feed states live in
//! `ultros_calc::list_estimate`; this only turns them into text.

use crate::components::relative_time::RelativeToNow;
use crate::i18n::*;
use leptos::prelude::*;
use thousands::Separable;
use ultros_calc::list_estimate::{CartEstimate, Coverage, MissingReason, PriceFeed};

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

/// The feed decides first: without listings there is no coverage to report,
/// only why there are none. With listings — fresh or marked — the coverage
/// line applies.
pub(crate) fn status_text(i18n: I18n, feed: PriceFeed, estimate: &CartEstimate) -> String {
    match feed {
        PriceFeed::Loading => t_string!(i18n, lists_estimate_feed_loading).to_string(),
        PriceFeed::Missing(MissingReason::NotRequested) => {
            t_string!(i18n, lists_estimate_feed_not_requested).to_string()
        }
        PriceFeed::Missing(MissingReason::Failed) => {
            t_string!(i18n, lists_estimate_feed_failed).to_string()
        }
        PriceFeed::Observed { .. } => coverage_text(i18n, estimate),
    }
}

/// The number to show, if any. A cart with no listings behind it — nothing
/// loaded, or loaded and nothing matched — must not read as free, so it
/// shows a dash. An empty cart genuinely costs nothing and shows zero.
pub(crate) fn displayed_total(feed: PriceFeed, estimate: &CartEstimate) -> Option<i64> {
    match (feed.has_prices(), estimate.coverage) {
        (false, _) | (true, Coverage::None) => None,
        (true, _) => Some(estimate.total),
    }
}

#[component]
pub fn ListEstimateSummary(
    estimate: Signal<CartEstimate>,
    /// Where the listings behind `estimate` stand. Defaults to observed so a
    /// caller that only has rows still gets a coverage line.
    #[prop(default = Signal::derive(|| PriceFeed::observed(chrono::Utc::now())))]
    feed: Signal<PriceFeed>,
    /// The world, datacenter or region the listings were served for.
    #[prop(default = Signal::derive(|| None))]
    scope: Signal<Option<String>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let total = move || {
        estimate.with(|estimate| {
            displayed_total(feed.get(), estimate)
                .map(|total| {
                    t_string!(
                        i18n,
                        lists_workspace_gil,
                        price = total.separate_with_commas()
                    )
                    .to_string()
                })
                .unwrap_or_else(|| "—".to_string())
        })
    };
    let incomplete = move || feed.get().has_prices() && estimate.with(CartEstimate::is_incomplete);
    let status = move || estimate.with(|estimate| status_text(i18n, feed.get(), estimate));
    let refresh_failed = move || {
        matches!(
            feed.get(),
            PriceFeed::Observed {
                refresh_failed: true,
                ..
            }
        )
    };
    view! {
        <section
            class="panel rounded-xl px-4 py-3 flex flex-col gap-1"
            data-testid="list-estimate-summary"
            aria-label=t_string!(i18n, lists_estimate_label)
        >
            <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
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
            </div>
            <Show when=move || feed.get().has_prices()>
                <p class="text-xs text-[color:var(--color-text-muted)]" data-testid="list-estimate-freshness">
                    {move || feed.get().fetched_at().map(|fetched_at| view! {
                        <span>{t!(i18n, lists_estimate_fetched)}" "<RelativeToNow timestamp=fetched_at.naive_utc() /></span>
                    })}
                    <Show when=refresh_failed>
                        <span>" · "{t!(i18n, lists_estimate_refresh_failed)}</span>
                    </Show>
                    {move || scope.get().map(|scope| view! {
                        <span>" · "{t_string!(i18n, lists_estimate_scope, scope = scope).to_string()}</span>
                    })}
                </p>
            </Show>
        </section>
    }
}
