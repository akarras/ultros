//! Sell and craft verdicts at the top of the item page.
//!
//! The arithmetic lives in `ultros_calc::verdict`; this module adapts the
//! page's listings payload and contexts into it and renders two compact
//! cards inside `#overview`.

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;
use leptos_router::location::Url;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer, SaleHistory};
use ultros_calc::verdict::{
    BoardVerdict, FloorWarning, ListingSample, SaleRate, SaleSample, SellVerdict, sell_verdict,
};
use xiv_gen::ItemId;

use crate::components::gil::Gil;
use crate::components::skeleton::SingleLineSkeleton;
use crate::error::AppError;
use crate::global_state::LocalWorldData;
use crate::global_state::cookies::Cookies;
use crate::global_state::home_world::use_home_world;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string};
use crate::routes::item_view::{get_or_default, with_or};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

const CARD_CLASS: &str = "item-surface flex min-w-0 flex-col gap-1.5 p-3 text-sm";
const MUTED: &str = "text-[color:var(--color-text-muted)]";

/// The world whose board the sell card describes: the page's world, else the
/// home world when it is in scope. An excluded world is never used.
pub(crate) fn sell_world(
    page_world: Option<i32>,
    scope_worlds: &[i32],
    excluded: &HashSet<i32>,
    home_world: Option<i32>,
) -> Option<i32> {
    page_world
        .or_else(|| home_world.filter(|home| scope_worlds.contains(home)))
        .filter(|world| !excluded.contains(world))
}

/// The vendor anchor `real_price` uses against laundered sales — the same
/// value `RealPriceSummary` passes, so both show the same real price.
fn laundering_vendor_price(item_id: i32) -> Option<i32> {
    tracked_data()
        .items
        .get(&ItemId(item_id))
        .map(|item| item.price_mid as i32)
        .filter(|price| *price > 0)
}

fn sale_samples(sales: &[SaleHistory]) -> Vec<SaleSample> {
    sales
        .iter()
        .map(|sale| SaleSample {
            world_id: sale.world_id,
            price_per_item: sale.price_per_item,
            quantity: sale.quantity,
            hq: sale.hq,
            sold_at: sale.sold_date.and_utc().timestamp(),
        })
        .collect()
}

fn quality_chip(hq: bool) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <span class="rounded border border-[color:var(--color-outline)] px-1 text-[10px] font-bold leading-4 text-[color:var(--color-text-muted)]">
            {if hq { t!(i18n, hq).into_any() } else { t!(i18n, nq).into_any() }}
        </span>
    }
    .into_any()
}

fn one_decimal(value: f64) -> String {
    format!("{value:.1}")
}

fn whole_percent(value: f64) -> String {
    format!("{value:.0}")
}

#[component]
pub(crate) fn ItemVerdicts(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    #[prop(into)] excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <section
            class="@container mt-4"
            aria-label=move || t_string!(i18n, item_verdict_region_label).to_string()
        >
            <div class="grid grid-cols-1 gap-3 @min-[40rem]:grid-cols-2">
                <SellVerdictCard listing_resource filtered_listings excluded_worlds world item_id />
            </div>
        </section>
    }
}

#[component]
fn SellVerdictCard(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    filtered_listings: Signal<ListingRows>,
    excluded_worlds: Signal<HashSet<i32>>,
    world: Memo<String>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let world_data = use_context::<LocalWorldData>().and_then(|data| data.0.ok());
    // The item page always provides `Cookies`; the guard keeps the card
    // buildable in a bare owner (see `item_verdicts_builds_without_contexts`).
    let home_world = use_context::<Cookies>().map(|_| use_home_world().0);
    // `now`-dependent text (sale rate, days of stock, ETA) waits for
    // hydration so the server and the first client render agree.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| hydrated.set(true));

    view! {
        <Transition fallback=move || view! { <div class=CARD_CLASS><SingleLineSkeleton /></div> }>
            {move || {
                let show_rate = hydrated.get();
                let world_data = world_data.clone();
                listing_resource.with(|data_ref| {
                    let Some(Ok(data)) = data_ref.as_ref() else {
                        return ().into_any();
                    };
                    let scope_name = Url::unescape(&get_or_default(&world));
                    let scope = world_data
                        .as_ref()
                        .and_then(|helper| helper.lookup_world_by_name(&scope_name));
                    let page_world = scope.as_ref().and_then(|s| s.as_world().map(|w| w.id));
                    let scope_worlds: Vec<i32> = scope
                        .as_ref()
                        .map(|s| s.all_worlds().map(|w| w.id).collect())
                        .unwrap_or_default();
                    let home = home_world.and_then(|signal| {
                        with_or(&signal, None, |w| w.as_ref().map(|w| w.id))
                    });
                    let excluded = get_or_default(&excluded_worlds);
                    let Some(world_id) = sell_world(page_world, &scope_worlds, &excluded, home)
                    else {
                        return view! {
                            <div class=CARD_CLASS data-testid="sell-verdict">
                                <p class=MUTED>{t!(i18n, item_verdict_sell_pick_world)}</p>
                            </div>
                        }
                        .into_any();
                    };
                    let world_name = world_data
                        .as_ref()
                        .and_then(|helper| helper.lookup_selector(AnySelector::World(world_id)))
                        .map(|w| w.get_name().to_string())
                        .unwrap_or_default();
                    let listings: Vec<ListingSample> = get_or_default(&filtered_listings)
                        .iter()
                        .map(|(listing, _)| ListingSample {
                            world_id: listing.world_id,
                            price_per_unit: listing.price_per_unit,
                            quantity: listing.quantity,
                            hq: listing.hq,
                        })
                        .collect();
                    let verdict = sell_verdict(
                        &listings,
                        &sale_samples(&data.sales),
                        world_id,
                        laundering_vendor_price(get_or_default(&item_id)),
                        chrono::Utc::now().timestamp(),
                    );
                    sell_card_body(verdict, world_name, scope_name, show_rate)
                })
            }}
        </Transition>
    }
}

fn sell_card_body(
    verdict: SellVerdict,
    world_name: String,
    scope_name: String,
    show_rate: bool,
) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let heading =
        t_string!(i18n, item_verdict_sell_heading, world = world_name.as_str()).to_string();
    let board = match verdict.board {
        Some(board) => board_lines(board, verdict.rate, show_rate),
        None => {
            let quality = if verdict.hq {
                t_string!(i18n, hq).to_string()
            } else {
                t_string!(i18n, nq).to_string()
            };
            view! {
                <p class=MUTED>
                    {t_string!(i18n, item_verdict_no_listings, quality = quality.as_str(), world = world_name.as_str()).to_string()}
                </p>
            }
            .into_any()
        }
    };
    let real = match verdict.real_price {
        Some(real) => view! {
            <div class="flex flex-wrap items-center gap-x-1.5">
                <span class="text-blue-300">{t!(i18n, real_price)}</span>
                <Gil amount=real />
                {verdict.real_price_scope_wide.then(|| view! {
                    <span class=MUTED>
                        {t_string!(i18n, item_verdict_real_price_across, scope = scope_name.as_str()).to_string()}
                    </span>
                })}
            </div>
        }
        .into_any(),
        None => view! { <p class=MUTED>{t!(i18n, item_verdict_no_recent_sales)}</p> }.into_any(),
    };
    view! {
        <div class=CARD_CLASS data-testid="sell-verdict">
            <div class="flex items-center justify-between gap-2">
                <h2 class="text-base font-bold text-brand-200">{heading}</h2>
                {quality_chip(verdict.hq)}
            </div>
            {board}
            {real}
        </div>
    }
    .into_any()
}

fn board_lines(board: BoardVerdict, rate: SaleRate, show_rate: bool) -> AnyView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let wall = board.wall;
    let patient = board.patient.map(|patient| {
        let eta = board
            .patient_eta_days
            .filter(|_| show_rate)
            .map(|days| t_string!(i18n, item_verdict_eta_days, days = one_decimal(days)).to_string());
        view! {
            <div class="flex items-baseline justify-between gap-2">
                <span class=MUTED>{t!(i18n, item_verdict_patient)}</span>
                <span class="flex flex-wrap items-baseline justify-end gap-x-1.5">
                    <span class="font-bold"><Gil amount=patient /></span>
                    <span class=MUTED>
                        {t_string!(i18n, item_verdict_patient_hint, units = wall.units.to_string()).to_string()}
                    </span>
                    {eta.map(|eta| view! { <span class=MUTED>"· "{eta}</span> })}
                </span>
            </div>
        }
    });
    let gap = board.gap.map(|gap| {
        view! {
            <div class=format!("flex flex-wrap items-center gap-x-1.5 {MUTED}")>
                <span>{t_string!(i18n, item_verdict_gap, percent = whole_percent(gap.percent)).to_string()}</span>
                <span>"→"</span>
                <Gil amount=gap.price />
            </div>
        }
    });
    let stock = show_rate.then(|| match (rate, board.days_of_stock) {
        (SaleRate::UnitsPerDay(_), Some(days)) => view! {
            <p class=MUTED>{t_string!(i18n, item_verdict_days_of_stock, days = one_decimal(days)).to_string()}</p>
        }
        .into_any(),
        _ => view! { <p class=MUTED>{t!(i18n, item_verdict_too_few_sales)}</p> }.into_any(),
    });
    let warning = board.warning.map(|warning| {
        let text = match warning {
            FloorWarning::UnderRealPrice { percent } => t_string!(
                i18n,
                item_verdict_floor_under_real,
                percent = whole_percent(percent)
            )
            .to_string(),
            FloorWarning::AboveRecentSales => t_string!(i18n, item_verdict_floor_above_real).to_string(),
        };
        view! {
            <span class="self-start rounded-full border border-amber-300/40 bg-amber-500/10 px-2 py-0.5 text-xs text-amber-200">
                {text}
            </span>
        }
    });
    view! {
        <div class="flex items-baseline justify-between gap-2">
            <span class=MUTED>{t!(i18n, item_verdict_fast)}</span>
            <span class="flex flex-wrap items-baseline justify-end gap-x-1.5">
                <span class="font-bold"><Gil amount=board.fast /></span>
                <span class=MUTED>{t!(i18n, item_verdict_fast_hint)}</span>
            </span>
        </div>
        {patient}
        <div class="my-1 border-t border-[color:var(--color-outline)]"></div>
        <div class=format!("flex flex-wrap items-center gap-x-1.5 {MUTED}")>
            <span>{t!(i18n, item_verdict_wall)}":"</span>
            <span>
                {t_string!(
                    i18n,
                    item_verdict_wall_counts,
                    listings = wall.listings.to_string(),
                    units = wall.units.to_string()
                ).to_string()}
            </span>
            <Gil amount=wall.low />
            {(wall.high != wall.low).then(|| view! { <span>"–"</span><Gil amount=wall.high /> })}
        </div>
        {gap}
        {stock}
        {warning}
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sell_world_prefers_the_page_world() {
        assert_eq!(
            sell_world(Some(40), &[40], &HashSet::new(), Some(41)),
            Some(40)
        );
    }

    #[test]
    fn sell_world_uses_the_home_world_inside_the_scope() {
        assert_eq!(
            sell_world(None, &[40, 41, 42], &HashSet::new(), Some(41)),
            Some(41)
        );
    }

    #[test]
    fn sell_world_is_none_when_home_is_outside_the_scope_or_unset() {
        assert_eq!(sell_world(None, &[40, 41], &HashSet::new(), Some(99)), None);
        assert_eq!(sell_world(None, &[40, 41], &HashSet::new(), None), None);
    }

    #[test]
    fn sell_world_ignores_excluded_worlds() {
        let excluded = HashSet::from([41]);
        assert_eq!(sell_world(None, &[40, 41], &excluded, Some(41)), None);
        assert_eq!(sell_world(Some(41), &[41], &excluded, None), None);
    }

    /// Mirrors `real_price_summary_builds_without_an_i18n_context` in
    /// `item_view.rs`: the cards read i18n and every context through
    /// non-panicking accessors, so building them in a bare owner (no i18n,
    /// no cookies, no world data) must not panic.
    #[test]
    fn item_verdicts_builds_without_contexts() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let listing_resource = Resource::new(|| (), |_| async { Err(AppError::ParamMissing) });
            let filtered_listings: Signal<Vec<(ActiveListing, Arc<Retainer>)>> =
                Signal::derive(Vec::new);
            let excluded_worlds: Signal<HashSet<i32>> = Signal::derive(HashSet::new);
            let world = Memo::new(|_| "Gilgamesh".to_string());
            let item_id = Memo::new(|_| 5057);
            let _ = view! {
                <ItemVerdicts listing_resource filtered_listings excluded_worlds world item_id />
            };
        });
    }
}
