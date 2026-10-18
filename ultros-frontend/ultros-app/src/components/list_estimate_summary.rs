//! The cart's estimated total and a one-line account of how much of the
//! cart that number actually covers, plus where and when the prices behind
//! it came from. The arithmetic, coverage rules and feed states live in
//! `ultros_calc::list_estimate`; this only turns them into text.

use crate::components::icon::Icon;
use crate::components::relative_time::RelativeToNow;
use crate::global_state::LocalWorldData;
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use thousands::Separable;
use ultros_api_types::world_helper::{AnyResult, AnySelector};
use ultros_calc::list_estimate::{CartEstimate, Coverage, MissingReason, PriceFeed, WorldTotal};

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

/// One datacenter's slice of the estimate: its total and the worlds the
/// priced units would come from, dearest world first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DatacenterTotal {
    pub name: String,
    pub total: i64,
    pub worlds: Vec<(String, WorldTotal)>,
}

/// Group per-world totals under their datacenters. `lookup` resolves a
/// world id to `(datacenter id, datacenter name, world name)`; an
/// unresolved world groups under the unknown label with its id as its
/// name, so a missing world table degrades the labels, never the sums.
pub(crate) fn group_by_datacenter(
    worlds: Vec<WorldTotal>,
    unknown: &str,
    lookup: impl Fn(i32) -> Option<(i32, String, String)>,
) -> Vec<DatacenterTotal> {
    let mut groups: Vec<(i32, DatacenterTotal)> = Vec::new();
    for world in worlds {
        let (datacenter_id, datacenter_name, world_name) = lookup(world.world_id)
            .unwrap_or_else(|| (0, unknown.to_string(), world.world_id.to_string()));
        let group = match groups
            .iter_mut()
            .find(|(id, _)| *id == datacenter_id)
            .map(|(_, group)| group)
        {
            Some(group) => group,
            None => {
                groups.push((
                    datacenter_id,
                    DatacenterTotal {
                        name: datacenter_name,
                        total: 0,
                        worlds: Vec::new(),
                    },
                ));
                &mut groups.last_mut().expect("just pushed").1
            }
        };
        group.total = group.total.saturating_add(world.total);
        group.worlds.push((world_name, world));
    }
    let mut groups: Vec<_> = groups.into_iter().map(|(_, group)| group).collect();
    // Worlds arrive dearest-first and keep that order inside a group;
    // groups themselves sort the same way, name breaking ties.
    groups.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.name.cmp(&b.name)));
    groups
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
    /// `(acquired, needed)` units across the whole cart, before any filter.
    /// `None` hides the line (an empty cart has no progress to report).
    #[prop(default = Signal::derive(|| None))]
    progress: Signal<Option<(i32, i32)>>,
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
    let (breakdown_open, set_breakdown_open) = signal(false);
    let worlds = StoredValue::new(use_context::<LocalWorldData>().and_then(|data| data.0.ok()));
    let unknown = t_string!(i18n, list_summary_unknown).to_string();
    let breakdown = Memo::new(move |_| {
        estimate.with(|estimate| {
            group_by_datacenter(estimate.world_totals(), &unknown, |world_id| {
                worlds.with_value(|worlds| {
                    let helper = worlds.as_ref()?;
                    let world = helper.lookup_selector(AnySelector::World(world_id))?;
                    let AnyResult::World(world) = world else {
                        return None;
                    };
                    let datacenter_name = helper
                        .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                        .map(|datacenter| datacenter.get_name().to_string())
                        .unwrap_or_else(|| unknown.clone());
                    Some((world.datacenter_id, datacenter_name, world.name.clone()))
                })
            })
        })
    });
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
            {move || progress.get().filter(|(_, needed)| *needed > 0).map(|(acquired, needed)| {
                let pct = 100 * i64::from(acquired) / i64::from(needed);
                view! {
                    <p class="text-xs text-[color:var(--color-text-muted)]" data-testid="list-estimate-progress">
                        {t_string!(i18n, list_view_units_acquired_progress, acquired = acquired, quantity = needed, pct = pct).to_string()}
                    </p>
                }
            })}
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
            <Show when=move || feed.get().has_prices() && !breakdown.with(|groups| groups.is_empty())>
                <button type="button" class="inline-flex items-center gap-1 self-start text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" data-testid="list-estimate-breakdown-toggle" aria-expanded=move || breakdown_open.get().to_string() aria-controls="list-estimate-breakdown" on:click=move |_| set_breakdown_open.set(!breakdown_open.get_untracked())>
                    <span>{t!(i18n, lists_estimate_breakdown)}</span>
                    <span class="inline-flex transition-transform" class:rotate-180=move || breakdown_open.get()><Icon icon=i::BiChevronDownRegular /></span>
                </button>
                <Show when=move || breakdown_open.get()>
                    <div id="list-estimate-breakdown" class="space-y-2" data-testid="list-estimate-breakdown">
                        <For each=move || breakdown.get() key=|group| group.name.clone() children=move |group| {
                            view! {
                                <div>
                                    <p class="flex flex-wrap items-baseline justify-between gap-x-4 text-sm">
                                        <span class="font-semibold">{group.name.clone()}</span>
                                        <span class="tabular-nums">{t_string!(i18n, lists_workspace_gil, price = group.total.separate_with_commas()).to_string()}</span>
                                    </p>
                                    <ul class="mt-0.5 space-y-0.5">
                                        <For each=move || group.worlds.clone() key=|(name, world)| (world.world_id, name.clone()) children=move |(name, world)| {
                                            view! {
                                                <li class="flex flex-wrap items-baseline justify-between gap-x-4 pl-4 text-xs text-[color:var(--color-text-muted)]">
                                                    <span>{name}" · "{t_string!(i18n, lists_estimate_breakdown_units, units = world.units).to_string()}</span>
                                                    <span class="tabular-nums">{t_string!(i18n, lists_workspace_gil, price = world.total.separate_with_commas()).to_string()}</span>
                                                </li>
                                            }
                                        } />
                                    </ul>
                                </div>
                            }
                        } />
                    </div>
                </Show>
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
                vec![listing(id.into(), id, false, 10, stock)]
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
    fn breakdown_groups_worlds_under_datacenters_and_keeps_the_sums() {
        let lookup = |world_id: i32| match world_id {
            1 => Some((10, "Aether".to_string(), "Gilgamesh".to_string())),
            2 => Some((10, "Aether".to_string(), "Midgardsormr".to_string())),
            3 => Some((20, "Crystal".to_string(), "Balmung".to_string())),
            _ => None,
        };
        let total = |world_id, total| WorldTotal {
            world_id,
            total,
            units: 1,
            listings: 1,
        };
        let groups = group_by_datacenter(
            vec![total(2, 50), total(3, 40), total(1, 30), total(99, 5)],
            "Unknown",
            lookup,
        );
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].name, "Aether");
        assert_eq!(groups[0].total, 80);
        assert_eq!(
            groups[0]
                .worlds
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Midgardsormr", "Gilgamesh"],
            "worlds stay dearest-first inside a datacenter"
        );
        assert_eq!(groups[1].name, "Crystal");
        assert_eq!(groups[1].total, 40);
        assert_eq!(groups[2].name, "Unknown");
        assert_eq!(groups[2].worlds[0].0, "99");
        assert_eq!(groups.iter().map(|g| g.total).sum::<i64>(), 125);
    }

    #[test]
    fn breakdown_toggle_renders_only_when_units_are_priced() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let i18n = leptos_i18n::context::init_i18n_context::<Locale>();
            provide_context(i18n);
            let feed = PriceFeed::observed(chrono::Utc::now());
            let priced = render(cart(&[(2, 0, 2)]), feed);
            assert!(
                priced.contains("list-estimate-breakdown-toggle"),
                "{priced}"
            );
            assert!(priced.contains("Cost by datacenter and world"), "{priced}");
            assert!(
                !priced.contains("data-testid=\"list-estimate-breakdown\""),
                "the breakdown starts collapsed: {priced}"
            );
            let unpriced = render(cart(&[(2, 0, 0)]), feed);
            assert!(
                !unpriced.contains("list-estimate-breakdown-toggle"),
                "{unpriced}"
            );
        });
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
