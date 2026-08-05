//! Flip-verification math for the item page's `?compare-buy-from=` card.
//!
//! Estimates use the exact flip-finder pipeline (`crate::analysis`):
//! sniper-clamped median of recent sales, capped by the sell world's
//! troll-guarded floor; profit is after the 5% market-board tax.

use crate::analysis::{flip_estimated_sale_price, flip_profit, median_in_place_i32, sniper_clamp};
use crate::api::get_listings;
use crate::components::freshness_badge::FreshnessBadge;
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::skeleton::BoxSkeleton;
use crate::components::world_name::WorldName;
use crate::error::AppError;
use crate::freshness::derive_freshness_inputs;
use crate::global_state::LocalWorldData;
use crate::i18n::{t, t_string, use_i18n};
use crate::query_defaults::filter_query_signal;
use crate::routes::item_view::with_or;
use crate::routes::item_view_scope::COMPARE_BUY_FROM_PARAM;
use leptos::prelude::*;
use leptos_router::location::Url;
use std::sync::Arc;
use ultros_api_types::freshness::calculate_freshness_verdict;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::{ActiveListing, CurrentlyShownItem};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FlipVerdict {
    pub hq: bool,
    /// Cheapest buy-world listing for this quality.
    pub buy_listing: ActiveListing,
    pub estimated_sale_price: i32,
    /// After the 5% tax. Negative profits are kept — "this flip is dead" is
    /// exactly what the card exists to say.
    pub profit_per_unit: i32,
    /// `profit_per_unit * buy_listing.quantity`.
    pub stack_profit: i32,
}

fn cheapest_buy(buy: &CurrentlyShownItem, hq: bool) -> Option<&ActiveListing> {
    buy.listings
        .iter()
        .map(|(listing, _)| listing)
        .filter(|listing| listing.hq == hq && listing.price_per_unit > 0)
        .min_by_key(|listing| listing.price_per_unit)
}

fn sell_median(sell: &CurrentlyShownItem, hq: bool) -> i32 {
    let prices: Vec<i32> = sell
        .sales
        .iter()
        .filter(|sale| sale.hq == hq && sale.price_per_item > 0)
        .map(|sale| sale.price_per_item)
        .collect();
    let mut clamped = sniper_clamp(prices);
    median_in_place_i32(&mut clamped)
}

fn sell_floor(sell: &CurrentlyShownItem, hq: bool) -> Option<i32> {
    sell.listings
        .iter()
        .map(|(listing, _)| listing)
        .filter(|listing| listing.hq == hq && listing.price_per_unit > 0)
        .map(|listing| listing.price_per_unit)
        .min()
}

fn verdict_for_quality(
    buy: &CurrentlyShownItem,
    sell: &CurrentlyShownItem,
    hq: bool,
) -> Option<FlipVerdict> {
    let buy_listing = cheapest_buy(buy, hq)?.clone();
    let median = sell_median(sell, hq);
    if median == 0 {
        // No recent sales of this quality on the sell world — no estimate,
        // no verdict. Mirrors the flip-finder, whose rows come from sales.
        return None;
    }
    let estimated_sale_price = flip_estimated_sale_price(median, sell_floor(sell, hq));
    let profit_per_unit = flip_profit(estimated_sale_price, buy_listing.price_per_unit, true);
    let stack_profit = profit_per_unit.saturating_mul(buy_listing.quantity);
    Some(FlipVerdict {
        hq,
        buy_listing,
        estimated_sale_price,
        profit_per_unit,
        stack_profit,
    })
}

/// Best-profit verdict across NQ/HQ, or `None` when neither quality has both
/// a buy listing and at least one recent sell-world sale.
pub(crate) fn flip_verdict(
    buy: &CurrentlyShownItem,
    sell: &CurrentlyShownItem,
) -> Option<FlipVerdict> {
    [false, true]
        .into_iter()
        .filter_map(|hq| verdict_for_quality(buy, sell, hq))
        .max_by_key(|verdict| verdict.profit_per_unit)
}

/// Resolves `?compare-buy-from=` into `(buy_world_id, sell_world_id,
/// canonical_buy_world_name)`, requiring both sides to name real worlds
/// (not a DC/region) and to differ from each other.
fn resolve_route(
    world_data: &ultros_api_types::world_helper::WorldHelper,
    compare_world: Option<String>,
    page_world: &str,
) -> Option<(i32, i32, String)> {
    let raw = compare_world?;
    let buy_name = Url::unescape(&raw);
    let buy_any = world_data.lookup_world_by_name(&buy_name)?;
    let buy = buy_any.as_world()?;

    let sell_name = Url::unescape(page_world);
    let sell_any = world_data.lookup_world_by_name(&sell_name)?;
    let sell = sell_any.as_world()?;

    if buy.id == sell.id {
        return None;
    }

    Some((buy.id, sell.id, buy.name.clone()))
}

/// Card rendered above `<DecisionHeader>` when `?compare-buy-from=<world>` is
/// present in the URL: fetches the named world's listings for this item and
/// shows the same buy/sell/profit math as the flip-finder, so a player who
/// clicked through from a flip-finder row can verify the flip is still live
/// before committing gil to it.
#[component]
pub(crate) fn FlipRouteCard(
    item_id: Memo<i32>,
    world: Memo<String>,
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();

    let (compare_world, set_compare_world) = filter_query_signal::<String>(COMPARE_BUY_FROM_PARAM);

    let route = Memo::new({
        let world_data = world_data.clone();
        move |_| resolve_route(&world_data, compare_world.get(), &world())
    });

    let buy_resource = Resource::new(
        move || (item_id(), route.get().map(|(_, _, name)| name)),
        |(item_id, buy_world)| async move {
            match buy_world {
                None => None,
                Some(name) => Some(
                    get_listings(item_id, &name)
                        .await
                        .map(Arc::new)
                        .inspect_err(|e| tracing::error!(error = ?e, "Error getting compare-buy-from listings")),
                ),
            }
        },
    );

    view! {
        {move || {
            let Some((buy_id, sell_id, _buy_name)) = with_or(&route, None, |r| r.clone()) else {
                return ().into_any();
            };
            view! {
                <Transition fallback=move || view! { <BoxSkeleton rows=1 /> }>
                    {move || {
                        with_or(&buy_resource, ().into_any(), |buy_ref| {
                            with_or(&listing_resource, ().into_any(), |sell_ref| {
                                let buy_data: Option<Arc<CurrentlyShownItem>> = match buy_ref {
                                    Some(Some(Ok(data))) => Some(data.clone()),
                                    _ => None,
                                };
                                let buy_error = matches!(buy_ref, Some(Some(Err(_))));
                                let sell_data: Option<Arc<CurrentlyShownItem>> = match sell_ref {
                                    Some(Ok(data)) => Some(data.clone()),
                                    _ => None,
                                };

                                let verdict = buy_data
                                    .as_ref()
                                    .zip(sell_data.as_ref())
                                    .and_then(|(buy, sell)| flip_verdict(buy, sell));

                                let quality_chip = |hq: bool| {
                                    let label = if hq {
                                        t_string!(i18n, hq).to_string()
                                    } else {
                                        t_string!(i18n, nq).to_string()
                                    };
                                    view! {
                                        <span class="rounded border border-emerald-300/40 px-1 text-[10px] font-bold leading-4 text-emerald-100">
                                            {label}
                                        </span>
                                    }
                                };

                                let buy_cell = if buy_error {
                                    view! {
                                        <span class="text-sm text-red-300">
                                            {t!(i18n, item_compare_unavailable)}
                                        </span>
                                    }
                                    .into_any()
                                } else if let Some(buy) = buy_data.as_ref() {
                                    if buy.listings.is_empty() {
                                        view! {
                                            <span class="text-sm text-[color:var(--color-text-muted)]">
                                                {t!(i18n, item_compare_no_listings)}
                                            </span>
                                        }
                                        .into_any()
                                    } else if let Some(v) = verdict.as_ref() {
                                        let freshness_inputs = derive_freshness_inputs(
                                            &buy.last_updated,
                                            &buy.sales,
                                            1,
                                            chrono::Utc::now().naive_utc(),
                                        );
                                        let freshness_verdict = calculate_freshness_verdict(
                                            freshness_inputs.age,
                                            freshness_inputs.per_world_sales_per_day,
                                        );
                                        view! {
                                            <div class="flex items-center gap-2 flex-wrap">
                                                <div class="font-bold">
                                                    <Gil amount=v.buy_listing.price_per_unit />
                                                </div>
                                                <span>" × "{v.buy_listing.quantity}</span>
                                                {quality_chip(v.hq)}
                                                <FreshnessBadge
                                                    verdict=freshness_verdict
                                                    age=freshness_inputs.age
                                                    compact=true
                                                />
                                            </div>
                                        }
                                        .into_any()
                                    } else {
                                        ().into_any()
                                    }
                                } else {
                                    ().into_any()
                                };

                                let sell_cell = if let Some(v) = verdict.as_ref() {
                                    let velocity = sell_data.as_ref().and_then(|sell| {
                                        derive_freshness_inputs(
                                            &sell.last_updated,
                                            &sell.sales,
                                            1,
                                            chrono::Utc::now().naive_utc(),
                                        )
                                        .scope_sales_per_day
                                    });
                                    view! {
                                        <div class="flex flex-col gap-0.5">
                                            <div class="flex items-center gap-1">
                                                <span>{t!(i18n, item_compare_est_sale_price)}":"</span>
                                                <div class="font-bold">
                                                    <Gil amount=v.estimated_sale_price />
                                                </div>
                                            </div>
                                            <span class="text-xs text-[color:var(--color-text-muted)]">
                                                "("{t!(i18n, item_compare_median_recent)}")"
                                            </span>
                                            {velocity
                                                .map(|velocity| {
                                                    view! {
                                                        <span class="text-xs text-[color:var(--color-text-muted)]">
                                                            {format!("~{velocity:.1}")}" "{t!(i18n, item_compare_sales_per_day)}
                                                        </span>
                                                    }
                                                    .into_any()
                                                })
                                                .unwrap_or_else(|| ().into_any())}
                                        </div>
                                    }
                                    .into_any()
                                } else {
                                    view! {
                                        <span class="text-sm text-[color:var(--color-text-muted)]">
                                            {t!(i18n, item_compare_no_recent_sales)}
                                        </span>
                                    }
                                    .into_any()
                                };

                                let verdict_cell = if let Some(v) = verdict.as_ref() {
                                    let negative = v.profit_per_unit < 0;
                                    let amount_class = if negative { "text-red-300" } else { "font-bold" };
                                    view! {
                                        <div class="flex flex-col gap-0.5">
                                            <div class=amount_class>
                                                <Gil amount=v.profit_per_unit />" "{t!(i18n, item_compare_per_unit)}
                                            </div>
                                            {(v.buy_listing.quantity > 1)
                                                .then(|| {
                                                    view! {
                                                        <div class=amount_class>
                                                            <Gil amount=v.stack_profit />" "{t!(i18n, item_compare_stack_total)}
                                                        </div>
                                                    }
                                                })}
                                            {negative
                                                .then(|| {
                                                    view! {
                                                        <span class="text-xs text-red-300">
                                                            {t!(i18n, item_compare_not_profitable)}
                                                        </span>
                                                    }
                                                })}
                                        </div>
                                    }
                                    .into_any()
                                } else {
                                    ().into_any()
                                };

                                view! {
                                    <div class="flex flex-col gap-3 rounded-xl border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] p-3 sm:p-4 mb-4">
                                        <div class="flex items-center justify-between gap-2">
                                            <div class="flex items-center gap-2 flex-wrap">
                                                <Icon
                                                    icon=icondata::FaArrowRightArrowLeftSolid
                                                    attr:class="text-sm shrink-0"
                                                />
                                                <span class="font-semibold">
                                                    {t!(i18n, item_compare_flip_route)}
                                                </span>
                                                <span class="inline-flex items-center gap-1 text-[color:var(--color-text-muted)]">
                                                    <WorldName id=AnySelector::World(buy_id) />
                                                    " → "
                                                    <WorldName id=AnySelector::World(sell_id) />
                                                </span>
                                            </div>
                                            <button
                                                aria-label=t_string!(i18n, item_compare_dismiss).to_string()
                                                on:click=move |_| set_compare_world.set(None)
                                            >
                                                <Icon icon=icondata::FaXmarkSolid />
                                            </button>
                                        </div>
                                        <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
                                            <div class="flex flex-col gap-1">
                                                <span class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                    {t!(i18n, item_compare_buy_on)}" "
                                                    <WorldName id=AnySelector::World(buy_id) />
                                                </span>
                                                {buy_cell}
                                            </div>
                                            <div class="flex flex-col gap-1">
                                                <span class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                    {t!(i18n, item_compare_sell_on)}" "
                                                    <WorldName id=AnySelector::World(sell_id) />
                                                </span>
                                                {sell_cell}
                                            </div>
                                            <div class="flex flex-col gap-1">
                                                <span class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                    {t!(i18n, item_compare_profit_after_tax)}
                                                </span>
                                                {verdict_cell}
                                            </div>
                                        </div>
                                    </div>
                                }
                                .into_any()
                            })
                        })
                    }}
                </Transition>
            }
            .into_any()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::{Retainer, SaleHistory};

    fn listing(
        id: i32,
        world_id: i32,
        price_per_unit: i32,
        quantity: i32,
        hq: bool,
    ) -> (ActiveListing, Retainer) {
        (
            ActiveListing {
                id,
                world_id,
                item_id: 1,
                retainer_id: id,
                price_per_unit,
                quantity,
                hq,
                timestamp: chrono::Utc::now().naive_utc(),
            },
            Retainer {
                id,
                world_id,
                name: format!("Retainer {id}"),
                retainer_city_id: 1,
            },
        )
    }

    fn sale(price_per_item: i32, hq: bool) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity: 1,
            price_per_item,
            buying_character_id: 0,
            hq,
            sold_item_id: 1,
            sold_date: chrono::Utc::now().naive_utc(),
            world_id: 2,
            buyer_name: None,
        }
    }

    fn shown(
        listings: Vec<(ActiveListing, Retainer)>,
        sales: Vec<SaleHistory>,
    ) -> CurrentlyShownItem {
        CurrentlyShownItem {
            listings,
            sales,
            last_updated: Vec::new(),
        }
    }

    #[test]
    fn verdict_uses_median_capped_by_sell_floor_and_taxes_profit() {
        let buy = shown(vec![listing(1, 1, 500, 3, false)], Vec::new());
        let sell = shown(
            vec![listing(2, 2, 900, 1, false)], // sell floor 900 < median 1000
            vec![sale(1000, false), sale(1000, false), sale(1200, false)],
        );
        let verdict = flip_verdict(&buy, &sell).unwrap();
        assert_eq!(verdict.estimated_sale_price, 900);
        // (900 * 0.95) as i32 - 500 = 855 - 500
        assert_eq!(verdict.profit_per_unit, 355);
        assert_eq!(verdict.stack_profit, 355 * 3);
    }

    #[test]
    fn verdict_none_without_buy_listings() {
        let buy = shown(Vec::new(), Vec::new());
        let sell = shown(Vec::new(), vec![sale(1000, false)]);
        assert!(flip_verdict(&buy, &sell).is_none());
    }

    #[test]
    fn verdict_none_without_recent_sell_sales() {
        let buy = shown(vec![listing(1, 1, 500, 1, false)], Vec::new());
        let sell = shown(vec![listing(2, 2, 900, 1, false)], Vec::new());
        assert!(flip_verdict(&buy, &sell).is_none());
    }

    #[test]
    fn verdict_picks_better_profit_quality() {
        let buy = shown(
            vec![listing(1, 1, 500, 1, false), listing(2, 1, 600, 1, true)],
            Vec::new(),
        );
        let sell = shown(Vec::new(), vec![sale(700, false), sale(2000, true)]);
        let verdict = flip_verdict(&buy, &sell).unwrap();
        assert!(verdict.hq); // (2000*0.95)-600 = 1300 beats (700*0.95)-500 = 165
    }

    #[test]
    fn verdict_keeps_negative_profit() {
        let buy = shown(vec![listing(1, 1, 5_000, 1, false)], Vec::new());
        let sell = shown(Vec::new(), vec![sale(1000, false)]);
        let verdict = flip_verdict(&buy, &sell).unwrap();
        assert!(verdict.profit_per_unit < 0);
    }
}
