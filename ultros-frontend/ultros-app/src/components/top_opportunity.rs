//! Home-page Top Opportunities card.
//!
//! One featured flip plus four compact follow-ups, ranked by absolute profit
//! among rows that cleared the server-side eligibility gates (vendor anchor,
//! velocity floor, ROI ceiling — see `ultros/src/resale_eligibility.rs`). The
//! card asks for those gates explicitly rather than relying on server
//! defaults, and its "view all" link carries the same ranking and floor into
//! the Flip Finder so the two surfaces agree about the same item.
//!
//! Credibility signals here are all buffer-derived (velocity, recent price
//! range) rather than ClickHouse-derived: the rollup covers ~7% of traded
//! items, so anything built on it would be blank on most cards. Where a CH
//! row does exist, the 30d VWAP upgrades the recent-range slot.
//!
//! Buy / Sell come from the wire (`buy_price` / `est_sale_price`). They
//! used to be back-solved from `profit + ROI`, which broke once `profit`
//! went post-tax: `buy + profit` is the gil you keep, not the price you
//! list at. `derive_buy_sell` keeps the old derivation only as a fallback
//! for a server that predates those fields.

use crate::components::app_link::AppLink;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use thousands::Separable;
use ultros_api_types::world_helper::AnySelector;

use crate::{
    analysis::get_sales_cadence,
    api::{BestDealsParams, ResaleStatsDto, get_best_deals},
    components::{gil::Gil, item_icon::ItemIcon, sales_cadence_badge::SalesCadenceBadge},
    global_state::{LocalWorldData, xiv_data::tracked_data},
    i18n::*,
};
use ultros_api_types::icon_size::IconSize;

/// How many deals to render in the card (1 featured + N-1 compact).
const VISIBLE_DEALS: usize = 5;
/// Matches the Flip Finder default so the handoff link applies the same floor.
const MIN_VELOCITY: f32 = 0.2;
const MIN_BUFFER_SALES: u8 = 2;
const MAX_ROI: f32 = 5000.0;

/// Resolve a world id to its name.
///
/// Named `lookup_world_name` rather than `world_name` because both deal
/// components take a `world_name: String` prop that would shadow it. Returns
/// a `String` rather than a view because the route line interpolates it into
/// a translated sentence.
fn lookup_world_name(world_id: i32) -> Option<String> {
    use_context::<LocalWorldData>()?
        .0
        .ok()?
        .lookup_selector(AnySelector::World(world_id))
        .map(|w| w.get_name().to_string())
}

fn item_name(item_id: i32, i18n: I18nContext<Locale, I18nKeys>) -> String {
    tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| t_string!(i18n, unknown_item).to_string())
}

/// Buy and (pre-tax) sell prices for the card's "Buy X · Sell Y" line.
///
/// Both come straight off the wire. They used to be back-solved from
/// `profit` and `return_on_investment`, which stopped working once `profit`
/// went post-tax — `buy + profit` is the gil you keep, not the price you
/// list at — and which was already wrong whenever the server clamped ROI.
///
/// The fallback only fires against a server too old to send the fields.
fn derive_buy_sell(deal: &ResaleStatsDto) -> (i32, i32) {
    if deal.buy_price > 0 {
        return (deal.buy_price, deal.est_sale_price);
    }
    let buy = if deal.return_on_investment > 0.0 {
        (deal.profit as f64 * 100.0 / deal.return_on_investment as f64).round() as i32
    } else {
        0
    };
    (buy, buy + deal.profit)
}

/// `12,800 → 21,450` is meaningless to a screen reader; this labels the pair.
fn buy_sell_label(i18n: I18nContext<Locale, I18nKeys>, buy: i32, sell: i32) -> String {
    format!(
        "{} {} — {} {}",
        t_string!(i18n, top_opportunities_buy),
        buy,
        t_string!(i18n, top_opportunities_sell),
        sell
    )
}

#[component]
pub fn TopOpportunities(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let deals = LocalResource::new(move || {
        let w = world.get();
        async move {
            let w = w?;
            let params = BestDealsParams {
                min_profit: Some(10_000),
                filter_sale: Some("Week"),
                limit: Some(20),
                show_suspicious: Some(false),
                min_velocity: Some(MIN_VELOCITY),
                min_buffer_sales: Some(MIN_BUFFER_SALES),
                max_roi: Some(MAX_ROI),
            };
            // Err and empty stay distinct: rendering an outage as "the market
            // is quiet" would be a lie, and a small version of exactly the
            // honesty problem this card was rebuilt to fix.
            Some(get_best_deals(&w, params).await.map(|mut v| {
                // FE-side launder defense-in-depth, in case the server's
                // show_suspicious flag ever flips on by accident.
                v.retain(|d| {
                    d.return_on_investment > 0.0 && d.profit > 0 && d.launder_suspicion <= 0.7
                });
                v.truncate(VISIBLE_DEALS);
                v
            }))
        }
    });

    let flip_finder_href = move || {
        world
            .get()
            .map(|w| format!("/flip-finder/{w}?sort=profit&vel={MIN_VELOCITY}"))
            .unwrap_or_else(|| "/flip-finder".to_string())
    };

    view! {
        <section class="dashboard-section">
            <header class="flex items-baseline justify-between mb-3">
                <h2 class="dashboard-section-title">{t!(i18n, top_opportunities_title)}</h2>
                <AppLink
                    href=flip_finder_href
                    attr:class="text-xs text-[color:var(--accent)] hover:underline"
                >
                    {t!(i18n, top_opportunities_view_all)}
                </AppLink>
            </header>
            <Suspense fallback=move || view! {
                <div class="space-y-2">
                    <div class="h-32 rounded bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                    {(0..4).map(|_| view! {
                        <div class="h-12 rounded bg-[color:color-mix(in_srgb,var(--color-text)_3%,transparent)] animate-pulse" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    let world_str = world.get().unwrap_or_default();
                    deals.get().map(|maybe| match maybe {
                        Some(Ok(list)) if !list.is_empty() => {
                            let mut iter = list.into_iter();
                            let featured = iter.next();
                            let rest: Vec<_> = iter.collect();
                            view! {
                                <div class="flex flex-col gap-1">
                                    {featured.map(|d| view! {
                                        <FeaturedDeal deal=d home_world=world_str.clone() />
                                    })}
                                    {rest
                                        .into_iter()
                                        .map(|d| view! {
                                            <CompactDeal deal=d home_world=world_str.clone() />
                                        })
                                        .collect_view()}
                                </div>
                            }.into_any()
                        },
                        Some(Err(_)) => view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] py-4">
                                {t!(i18n, top_opportunities_error)}
                            </div>
                        }.into_any(),
                        _ => view! { <EmptyState world=world /> }.into_any(),
                    })
                }}
            </Suspense>
        </section>
    }
}

/// Reachable in practice now that a velocity floor applies. Says what the
/// card's promise is at the moment that promise is most credible, and hands
/// off to an unfiltered Flip Finder so the floor reads as adjustable rather
/// than as the tool being broken.
#[component]
fn EmptyState(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let world_label = move || world.get().unwrap_or_default();
    let browse_href = move || {
        world
            .get()
            .map(|w| format!("/flip-finder/{w}?sort=profit&vel=0"))
            .unwrap_or_else(|| "/flip-finder".to_string())
    };
    view! {
        <div class="py-6 flex flex-col gap-2 items-start">
            <div class="text-sm font-medium text-[color:var(--color-text)]">
                {move || {
                    t_string!(i18n, top_opportunities_empty_title, world = world_label())
                        .to_string()
                }}
            </div>
            <div class="text-xs text-[color:var(--color-text-muted)] max-w-prose">
                {t!(i18n, top_opportunities_empty_body)}
            </div>
            <AppLink href=browse_href attr:class="text-xs text-[color:var(--accent)] hover:underline">
                {t!(i18n, top_opportunities_empty_cta)}
            </AppLink>
        </div>
    }
}

/// "Buy on Faerie → list on Sargatanas". The whole premise of the tool is a
/// cross-world arbitrage, and before this the card never named the other
/// world.
#[component]
fn RouteLine(source_world_id: i32, home_world: String) -> impl IntoView {
    let i18n = use_i18n();
    match lookup_world_name(source_world_id) {
        Some(source) => view! {
            <div class="text-xs text-[color:var(--color-text-muted)] mt-1">
                {t_string!(i18n, top_opportunities_route, source = source, home = home_world)
                    .to_string()}
            </div>
        }
        .into_any(),
        None => ().into_any(),
    }
}

#[component]
fn FeaturedDeal(deal: ResaleStatsDto, home_world: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = item_name(item_id, i18n);
    let (buy, sell) = derive_buy_sell(&deal);
    let href = format!("/item/{home_world}/{item_id}");
    let aria = buy_sell_label(i18n, buy, sell);

    // Buffer-derived by default (100% coverage); ClickHouse upgrades it.
    // Separated like every other gil figure on the card — a bare `3849999`
    // beside a `3,849,999` reads as a different kind of number.
    let anchor = if deal.vwap_30d > 0 {
        t_string!(
            i18n,
            top_opportunities_vwap_30d,
            price = deal.vwap_30d.separate_with_commas()
        )
        .to_string()
    } else {
        t_string!(
            i18n,
            top_opportunities_recent_range,
            low = deal.recent_price_low.separate_with_commas(),
            high = deal.recent_price_high.separate_with_commas()
        )
        .to_string()
    };

    view! {
        <a
            href=href
            class="card-link block rounded p-3 bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] transition-colors group"
        >
            <div class="flex gap-3 items-start">
                <div class="shrink-0">
                    <ItemIcon item_id icon_size=IconSize::Large />
                </div>
                <div class="min-w-0 flex-1">
                    // Name owns its own line: sharing a row with the profit
                    // figure is what truncated "Archeo Kingdom Partisan" to
                    // "Arc".
                    <div class="text-base font-semibold text-[color:var(--color-text)] leading-snug line-clamp-2 group-hover:underline">
                        {name}
                    </div>
                    <RouteLine source_world_id=deal.world_id home_world=home_world.clone() />
                </div>
            </div>
            <div class="flex items-end justify-between gap-3 mt-3 pt-3 border-t border-[color:var(--line)]">
                <div class="min-w-0">
                    <div class="text-[10px] uppercase tracking-wider text-[color:var(--color-text-muted)]">
                        {t!(i18n, top_opportunities_profit_each)}
                    </div>
                    <div class="text-2xl font-semibold font-mono text-emerald-300 leading-none tabular-nums">
                        <Gil amount=deal.profit />
                    </div>
                    // `Gil` renders a block-level `flex` div, so two of them
                    // with a text node between stack into three lines no
                    // matter how much room there is. The wrapper has to be a
                    // flex row itself for the pair to read as one line.
                    <div
                        class="text-[11px] text-[color:var(--color-text-muted)] font-mono mt-1 flex items-center gap-1 whitespace-nowrap"
                        aria-label=aria
                    >
                        <Gil amount=buy />" → "<Gil amount=sell />
                    </div>
                </div>
                <div class="flex flex-col items-end gap-1 shrink-0">
                    {deal.velocity_per_day.map(|v| {
                        let cadence = get_sales_cadence(v, deal.buffer_sale_count as usize);
                        view! { <SalesCadenceBadge cadence sales_per_day=v compact=true /> }
                    })}
                    <span class="text-[11px] text-[color:var(--color-text-muted)] font-mono">
                        {anchor}
                    </span>
                </div>
            </div>
        </a>
    }
}

#[component]
fn CompactDeal(deal: ResaleStatsDto, home_world: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = item_name(item_id, i18n);
    let (buy, sell) = derive_buy_sell(&deal);
    let href = format!("/item/{home_world}/{item_id}");
    let aria = buy_sell_label(i18n, buy, sell);

    let source = lookup_world_name(deal.world_id).unwrap_or_default();
    let velocity = deal
        .velocity_per_day
        .map(|v| t_string!(i18n, sales_cadence_compact, velocity = format!("{v:.1}")).to_string())
        .unwrap_or_default();
    let subline = match (source.is_empty(), velocity.is_empty()) {
        (false, false) => format!("{source} · {velocity}"),
        (false, true) => source,
        (true, false) => velocity,
        (true, true) => String::new(),
    };

    view! {
        <a
            href=href
            class="card-link grid grid-cols-[auto_1fr_auto] items-center gap-3 py-2 px-1 rounded border-t border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_8%,transparent)] transition-colors group"
        >
            <div class="shrink-0">
                <ItemIcon item_id icon_size=IconSize::Small />
            </div>
            <div class="min-w-0 flex flex-col gap-0.5">
                <div class="text-sm font-medium text-[color:var(--color-text)] truncate group-hover:underline">
                    {name}
                </div>
                <div class="text-[10px] text-[color:var(--color-text-muted)] truncate">
                    {subline}
                </div>
            </div>
            <div class="flex flex-col items-end text-right shrink-0">
                <span class="text-sm font-semibold font-mono text-emerald-300 tabular-nums">
                    <Gil amount=deal.profit />
                </span>
                <span
                    class="text-[10px] text-[color:var(--color-text-muted)] font-mono flex items-center gap-1 whitespace-nowrap"
                    aria-label=aria
                >
                    <Gil amount=buy />" → "<Gil amount=sell />
                </span>
            </div>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deal(profit: i32, roi: f32, buy_price: i32, est_sale_price: i32) -> ResaleStatsDto {
        ResaleStatsDto {
            profit,
            item_id: 5,
            hq: false,
            sold_within: "Week".to_string(),
            return_on_investment: roi,
            buy_price,
            est_sale_price,
            world_id: 63,
            confidence_band: Default::default(),
            vwap_30d: 0,
            sample_size_30d: 0,
            launder_suspicion: 0.0,
            velocity_per_day: Some(0.4),
            buffer_sale_count: 6,
            recent_price_low: 0,
            recent_price_high: 0,
        }
    }

    /// Sell must be the pre-tax list price, not `buy + profit` — with the
    /// 5% cut applied server-side those differ, and quoting the take as a
    /// list price would have users listing 5% too low.
    #[test]
    fn buy_sell_come_from_the_wire_fields() {
        // Buy 500, list 1000, net 950, keep 450.
        let (buy, sell) = derive_buy_sell(&deal(450, 90.0, 500, 1000));
        assert_eq!(buy, 500);
        assert_eq!(sell, 1000);
    }

    /// A clamped ROI made the old back-solve produce a nonsense buy price.
    #[test]
    fn clamped_roi_does_not_distort_buy_price() {
        let (buy, sell) = derive_buy_sell(&deal(94_999, 100_000.0, 1, 100_000));
        assert_eq!(buy, 1);
        assert_eq!(sell, 100_000);
    }

    /// Pre-`buy_price` servers send 0 for the new fields; fall back to the
    /// old derivation rather than rendering a 0-gil buy.
    #[test]
    fn falls_back_when_the_server_omits_the_fields() {
        let (buy, sell) = derive_buy_sell(&deal(500, 100.0, 0, 0));
        assert_eq!(buy, 500);
        assert_eq!(sell, 1000);
    }
}
