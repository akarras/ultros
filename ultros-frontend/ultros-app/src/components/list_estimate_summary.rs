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
    let missing = estimate.lines_not_requested();
    if missing > 0 {
        if estimate.total > 0 {
            return t_string!(
                i18n,
                lists_estimate_partial_missing,
                items = missing,
                units = estimate.unpriced_units
            )
            .to_string();
        }
        return t_string!(
            i18n,
            lists_estimate_missing_items,
            items = missing,
            units = estimate.unpriced_units
        )
        .to_string();
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
    if estimate.coverage == Coverage::Empty {
        return coverage_text(i18n, estimate);
    }
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
        (_, Coverage::Empty) => Some(0),
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

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use ultros_calc::list_estimate::{
        estimate_cart,
        fixtures::{listing, request},
    };

    fn cart(rows: &[(i32, i32, i32)]) -> CartEstimate {
        let offers: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(index, &(_, _, stock))| {
                let id = index as i32 + 1;
                vec![listing(id, id, false, 10, stock)]
            })
            .collect();
        estimate_cart(
            rows.iter()
                .enumerate()
                .map(|(index, &(need, acquired, _))| {
                    let id = index as i32 + 1;
                    (
                        request(id, id, None, need, acquired),
                        offers[index].as_slice(),
                    )
                }),
        )
    }

    fn render(estimate: CartEstimate, feed: PriceFeed) -> String {
        view! { <ListEstimateSummary estimate=Signal::stored(estimate) feed=Signal::stored(feed) /> }.to_html()
    }

    #[test]
    fn no_remaining_demand_is_zero_without_a_price_request() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<Locale>();
            provide_context(i18n);
            for estimate in [cart(&[]), cart(&[(4, 4, 0)])] {
                for feed in [
                    PriceFeed::Loading,
                    PriceFeed::Missing(MissingReason::NotRequested),
                    PriceFeed::Missing(MissingReason::Failed),
                ] {
                    assert_eq!(displayed_total(feed, &estimate), Some(0));
                    assert!(status_text(i18n, feed, &estimate).contains("Nothing left to buy"));
                    let html = render(estimate.clone(), feed);
                    assert!(html.contains("0 gil"));
                    assert!(html.contains("data-incomplete=\"false\""));
                }
            }
        });
    }

    #[test]
    fn unrequested_rows_do_not_claim_successful_no_supply() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<Locale>();
            provide_context(i18n);
            let feed = PriceFeed::observed(chrono::Utc::now());
            for rows in [&[(2, 0, 2), (3, 0, 0)][..], &[(3, 0, 0)][..]] {
                let mut estimate = cart(rows);
                estimate.lines.last_mut().unwrap().status =
                    ultros_calc::list_estimate::LineStatus::NotRequested;
                let status = status_text(i18n, feed, &estimate);
                assert!(status.contains("Price lookup needed: 1"));
                assert!(status.contains("Unpriced units:"));
                assert!(!status.contains("No listed prices"));
                if estimate.total > 0 {
                    assert!(status.contains("Known subtotal only"));
                    assert_eq!(displayed_total(feed, &estimate), Some(20));
                } else {
                    assert_eq!(displayed_total(feed, &estimate), None);
                }
            }
        });
    }

    #[test]
    fn partial_subtotals_and_missing_units_survive_helper_and_rendered_summary() {
        let _ = any_spawner::Executor::init_futures_executor();
        type StockRows = &'static [(i32, i32, i32)];
        let cases: &[(&str, StockRows, Option<i64>, i64, usize)] = &[
            ("one unpriced unit", &[(2, 0, 1)], Some(10), 1, 0),
            ("one partial", &[(5, 0, 2)], Some(20), 3, 0),
            ("all partial", &[(5, 0, 2), (4, 0, 1)], Some(30), 6, 0),
            (
                "partial and no supply",
                &[(5, 0, 2), (4, 0, 0)],
                Some(20),
                7,
                0,
            ),
            (
                "complete and partial",
                &[(2, 0, 2), (4, 0, 1)],
                Some(30),
                3,
                1,
            ),
            ("no supply", &[(5, 0, 0)], None, 5, 0),
            ("acquired", &[(5, 5, 2)], Some(0), 0, 0),
            ("empty", &[], Some(0), 0, 0),
            ("complete", &[(2, 0, 2)], Some(20), 0, 1),
        ];
        for &(name, rows, total, missing, fully_priced) in cases {
            let owner = Owner::new();
            owner.with(|| {
                let i18n = leptos_i18n::context::init_i18n_context::<Locale>();
                provide_context(i18n);
                let estimate = cart(rows);
                let feed = PriceFeed::observed(chrono::Utc::now());
                assert_eq!(displayed_total(feed, &estimate), total, "{name}");
                let status = status_text(i18n, feed, &estimate);
                let html = render(estimate, feed);
                assert!(html.contains(&status), "{name}: {html}");
                match total {
                    Some(total) => assert!(html.contains(&format!("{total} gil")), "{name}"),
                    None => assert!(html.contains('—'), "{name}"),
                }
                if missing > 0 && total.is_some() {
                    assert!(status.contains("Known subtotal only"), "{name}");
                    assert!(
                        status.contains(&format!("Unpriced units: {missing}")),
                        "{name}"
                    );
                    assert!(
                        status.contains(&format!(
                            "Fully priced items: {fully_priced}/{}",
                            rows.len()
                        )),
                        "{name}"
                    );
                    assert!(html.contains("data-incomplete=\"true\""), "{name}");
                    assert!(!html.contains("No listed prices"), "{name}");
                } else if total.is_none() {
                    assert!(status.contains("No listed prices"), "{name}");
                } else {
                    assert!(html.contains("data-incomplete=\"false\""), "{name}");
                    assert!(!status.contains("Known subtotal only"), "{name}");
                }
            });
        }
    }

    #[test]
    fn feed_states_remain_distinct_from_partial_supply() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<Locale>();
            provide_context(i18n);
            let estimate = cart(&[(5, 0, 2)]);
            for (feed, message) in [
                (PriceFeed::Loading, "Loading prices"),
                (
                    PriceFeed::Missing(MissingReason::NotRequested),
                    "Look up prices",
                ),
                (
                    PriceFeed::Missing(MissingReason::Failed),
                    "Prices unavailable right now",
                ),
            ] {
                assert_eq!(displayed_total(feed, &estimate), None);
                assert!(status_text(i18n, feed, &estimate).contains(message));
                let html = render(estimate.clone(), feed);
                assert!(html.contains(message));
                assert!(!html.contains("20 gil"));
                assert!(!html.contains("Known subtotal only"));
                assert!(html.contains("data-incomplete=\"false\""));
            }
            let cached = PriceFeed::Observed {
                fetched_at: chrono::Utc::now(),
                refresh_failed: true,
            };
            assert_eq!(displayed_total(cached, &estimate), Some(20));
            let html = render(estimate, cached);
            assert!(html.contains("20 gil"));
            assert!(html.contains("Known subtotal only"));
            assert!(html.contains("The latest refresh failed"));
        });
    }
}
