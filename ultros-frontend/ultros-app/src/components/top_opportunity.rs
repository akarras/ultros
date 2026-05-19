//! Home-page Top Opportunities card.
//!
//! Shows the top 5 safe flips on the world: one featured row with the
//! large icon + prominent profit, then 4 compact follow-ups. Each row
//! links to the item page on the user's home world. Reuses
//! `get_best_deals`, which already runs the server-side
//! `ResaleQualityFilter` — but we also defense-in-depth the FE side by
//! suppressing rows with `launder_suspicion > 0.7` in case the server's
//! `show_suspicious` ever flips on by accident.
//!
//! Buy / Sell aren't on the wire today; we derive them from
//! `profit + ROI`:
//!     buy  = profit * 100 / ROI
//!     sell = buy + profit

use leptos::prelude::*;
use leptos_router::components::A;

use crate::{
    api::{BestDealsParams, ResaleStatsDto, get_best_deals},
    components::{confidence_badge::ConfidenceBadge, gil::Gil, item_icon::ItemIcon},
    global_state::xiv_data::tracked_data,
    i18n::*,
};
use ultros_api_types::icon_size::IconSize;

/// How many deals to render in the card (1 featured + N-1 compact).
const VISIBLE_DEALS: usize = 5;

#[component]
pub fn TopOpportunities(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let deals = LocalResource::new(move || {
        let w = world.get();
        async move {
            let w = w?;
            // Ask for a few extras so the FE-side launder guard can drop
            // some without leaving us short.
            let params = BestDealsParams {
                min_profit: Some(10_000),
                filter_sale: Some("Week"),
                limit: Some(20),
                show_suspicious: Some(false),
            };
            get_best_deals(&w, params).await.ok().map(|mut v| {
                v.retain(|d| {
                    // Bogus ROI math (cheapest=1gil etc.) and FE-side
                    // launder defense-in-depth. Server already drops
                    // Unusable, but checking here means a flipped server
                    // flag can't expose junk on the home page.
                    d.return_on_investment > 0.0 && d.profit > 0 && d.launder_suspicion <= 0.7
                });
                v.into_iter().take(VISIBLE_DEALS).collect::<Vec<_>>()
            })
        }
    });

    let world_for_link = world;

    view! {
        <section class="dashboard-section">
            <header class="flex items-baseline justify-between mb-3">
                <h2 class="dashboard-section-title">
                    <span class="text-amber-300 mr-1">"🔥"</span>
                    {t!(i18n, top_opportunities_title)}
                </h2>
                <A
                    href=move || world_for_link.get()
                        .map(|w| format!("/flip-finder/{w}"))
                        .unwrap_or_else(|| "/flip-finder".to_string())
                    attr:class="text-xs text-[color:var(--accent)] hover:underline"
                >
                    {t!(i18n, top_opportunities_view_all)}
                </A>
            </header>
            <Suspense fallback=move || view! {
                <div class="space-y-2">
                    <div class="h-24 rounded bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                    {(0..4).map(|_| view! {
                        <div class="h-10 rounded bg-[color:color-mix(in_srgb,var(--color-text)_3%,transparent)] animate-pulse" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    let world_str = world.get().unwrap_or_default();
                    deals.get().map(|maybe| match maybe {
                        Some(list) if !list.is_empty() => {
                            let mut iter = list.into_iter();
                            let featured = iter.next();
                            let rest: Vec<_> = iter.collect();
                            view! {
                                <div class="flex flex-col gap-1">
                                    {featured.map(|d| view! {
                                        <FeaturedDeal deal=d world_name=world_str.clone() />
                                    })}
                                    {rest
                                        .into_iter()
                                        .map(|d| view! {
                                            <CompactDeal deal=d world_name=world_str.clone() />
                                        })
                                        .collect_view()}
                                </div>
                            }.into_any()
                        },
                        _ => view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] py-4">
                                {t!(i18n, top_opportunities_empty)}
                            </div>
                        }.into_any(),
                    })
                }}
            </Suspense>
        </section>
    }
}

fn derive_buy_sell(deal: &ResaleStatsDto) -> (i32, i32) {
    let buy = if deal.return_on_investment > 0.0 {
        (deal.profit as f64 * 100.0 / deal.return_on_investment as f64).round() as i32
    } else {
        0
    };
    let sell = buy + deal.profit;
    (buy, sell)
}

#[component]
fn FeaturedDeal(deal: ResaleStatsDto, world_name: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| t_string!(i18n, unknown_item).to_string());
    let (buy, sell) = derive_buy_sell(&deal);
    let demand_label = deal.sold_within.clone();
    let band = deal.confidence_band;
    let sample = deal.sample_size_30d;
    let href = format!("/item/{world_name}/{item_id}");

    view! {
        <a
            href=href
            class="grid grid-cols-[auto_1fr] sm:grid-cols-[auto_1fr_auto] gap-4 sm:gap-6 items-center py-1 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] transition-colors rounded"
        >
            <div class="shrink-0">
                <ItemIcon item_id icon_size=IconSize::Large />
            </div>
            <div class="min-w-0">
                <div class="flex items-baseline gap-2 flex-wrap">
                    <div class="text-lg font-semibold text-[color:var(--color-text)] truncate">{name}</div>
                    <ConfidenceBadge band sample_size=sample />
                </div>
                <div class="mt-1 flex items-baseline gap-3 text-xs text-[color:var(--color-text-muted)] flex-wrap">
                    <span class="flex items-baseline gap-1">
                        <span class="uppercase tracking-wider">{t!(i18n, top_opportunities_buy)}</span>
                        <span class="font-mono text-[color:var(--color-text)]"><Gil amount=buy /></span>
                    </span>
                    <span class="text-[color:var(--line)]">"·"</span>
                    <span class="flex items-baseline gap-1">
                        <span class="uppercase tracking-wider">{t!(i18n, top_opportunities_sell)}</span>
                        <span class="font-mono text-[color:var(--color-text)]"><Gil amount=sell /></span>
                    </span>
                    <span class="text-[color:var(--line)]">"·"</span>
                    <span class="flex items-baseline gap-1">
                        <span class="uppercase tracking-wider">{t!(i18n, top_opportunities_roi)}</span>
                        <span class="font-mono text-[color:var(--color-text)]">{format!("{:.0}%", deal.return_on_investment)}</span>
                    </span>
                </div>
            </div>
            <div class="flex flex-col items-end text-right shrink-0">
                <span class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                    {t!(i18n, top_opportunities_profit)}
                </span>
                <span class="text-2xl sm:text-3xl font-semibold font-mono text-emerald-300 leading-none tabular-nums">
                    <Gil amount=deal.profit />
                </span>
                <span class="text-[10px] text-[color:var(--color-text-muted)] font-mono mt-1">{demand_label}</span>
            </div>
        </a>
    }
}

#[component]
fn CompactDeal(deal: ResaleStatsDto, world_name: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| t_string!(i18n, unknown_item).to_string());
    let (buy, sell) = derive_buy_sell(&deal);
    let href = format!("/item/{world_name}/{item_id}");

    view! {
        <a
            href=href
            class="grid grid-cols-[auto_1fr_auto] items-center gap-3 py-2 border-t border-[color:var(--line)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] transition-colors"
        >
            <div class="shrink-0">
                <ItemIcon item_id icon_size=IconSize::Small />
            </div>
            <div class="min-w-0 flex flex-col gap-0.5">
                <div class="text-sm font-medium text-[color:var(--color-text)] truncate">{name}</div>
                <div class="flex items-baseline gap-2 text-[10px] text-[color:var(--color-text-muted)] font-mono">
                    <span><Gil amount=buy /></span>
                    <span class="text-[color:var(--line)]">"→"</span>
                    <span><Gil amount=sell /></span>
                </div>
            </div>
            <div class="flex flex-col items-end text-right shrink-0">
                <span class="text-sm font-semibold font-mono text-emerald-300 tabular-nums">
                    <Gil amount=deal.profit />
                </span>
                <span class="text-[10px] text-[color:var(--color-text-muted)] font-mono">
                    {format!("+{:.0}%", deal.return_on_investment)}
                </span>
            </div>
        </a>
    }
}
