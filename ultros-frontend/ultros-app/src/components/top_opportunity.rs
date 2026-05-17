//! Home-page Top Opportunity featured card.
//!
//! Shows the single best flip available right now: large item icon,
//! projected profit, ROI, Buy / Sell prices, and a Demand chip derived
//! from `sold_within`. Matches the "TOP OPPORTUNITY RIGHT NOW" panel in
//! the dashboard mockup, scaled down to one row for now.
//!
//! Reuses `get_best_deals` (which already runs through the Phase 2
//! deep-scan filter, so laundered items are pre-suppressed). Buy/Sell
//! prices aren't exposed in `ResaleStatsDto` today, but we can derive
//! them from profit + ROI without a wire-type change:
//!     buy  = profit * 100 / ROI
//!     sell = buy + profit

use leptos::prelude::*;
use leptos_router::components::A;

use crate::{
    api::{ResaleStatsDto, get_best_deals},
    components::{gil::Gil, item_icon::ItemIcon},
    global_state::xiv_data::tracked_data,
    i18n::*,
};
use ultros_api_types::icon_size::IconSize;

#[component]
pub fn TopOpportunity(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let deal = LocalResource::new(move || {
        let w = world.get();
        async move {
            let w = w?;
            get_best_deals(&w).await.ok().and_then(|mut v| {
                // Hide rows with bogus ROI math (denominator wobbles when
                // the cheapest source world prices an item at 1 gil etc.).
                v.retain(|d| d.return_on_investment > 0.0 && d.profit > 0);
                v.into_iter().next()
            })
        }
    });
    let world_for_link = world;

    view! {
        <section class="dashboard-section">
            <header class="flex items-baseline justify-between mb-3">
                <h2 class="dashboard-section-title">
                    <span class="text-amber-300 mr-1">"🔥"</span>
                    {t!(i18n, top_opportunity_title)}
                </h2>
                <A
                    href=move || world_for_link.get()
                        .map(|w| format!("/flip-finder/{w}"))
                        .unwrap_or_else(|| "/flip-finder".to_string())
                    attr:class="text-xs text-[color:var(--accent)] hover:underline"
                >
                    {t!(i18n, top_opportunity_view_all)}
                </A>
            </header>
            <Suspense fallback=move || view! {
                <div class="h-24 rounded bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
            }>
                {move || {
                    let world_str = world.get().unwrap_or_default();
                    deal.get().map(|maybe| match maybe {
                        Some(d) => view! { <DealCard deal=d world_name=world_str /> }.into_any(),
                        None => view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] py-4">
                                {t!(i18n, top_opportunity_empty)}
                            </div>
                        }.into_any(),
                    })
                }}
            </Suspense>
        </section>
    }
}

#[component]
fn DealCard(deal: ResaleStatsDto, world_name: String) -> impl IntoView {
    let i18n = use_i18n();
    let item_id = deal.item_id;
    let name = tracked_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .map(|i| i.name.as_str().to_string())
        .unwrap_or_else(|| t_string!(i18n, unknown_item).to_string());

    // Derive buy/sell from profit + ROI. ROI is a percentage so
    // buy = profit * 100 / ROI. Guard against ROI<=0 (the resource
    // already filters those out, but defense in depth).
    let buy = if deal.return_on_investment > 0.0 {
        (deal.profit as f64 * 100.0 / deal.return_on_investment as f64).round() as i32
    } else {
        0
    };
    let sell = buy + deal.profit;
    let demand_label = deal.sold_within.clone();

    let href = format!("/item/{}/{}", world_name, item_id);

    // Split layout: item identity on the left (icon + name + demand), the
    // numeric stack on the right with profit prominent and Buy/Sell/ROI
    // tucked underneath. No big background — the section provides its own
    // breathing room via dashboard-section.
    view! {
        <a
            href=href
            class="grid grid-cols-[auto_1fr] sm:grid-cols-[auto_1fr_auto] gap-4 sm:gap-6 items-center py-1 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] transition-colors rounded"
        >
            <div class="shrink-0">
                <ItemIcon item_id icon_size=IconSize::Large />
            </div>
            <div class="min-w-0">
                <div class="text-lg font-semibold text-[color:var(--color-text)] truncate">{name}</div>
                <div class="mt-1 flex items-baseline gap-3 text-xs text-[color:var(--color-text-muted)] flex-wrap">
                    <span class="flex items-baseline gap-1">
                        <span class="uppercase tracking-wider">{t!(i18n, top_opportunity_buy)}</span>
                        <span class="font-mono text-[color:var(--color-text)]"><Gil amount=buy /></span>
                    </span>
                    <span class="text-[color:var(--line)]">"·"</span>
                    <span class="flex items-baseline gap-1">
                        <span class="uppercase tracking-wider">{t!(i18n, top_opportunity_sell)}</span>
                        <span class="font-mono text-[color:var(--color-text)]"><Gil amount=sell /></span>
                    </span>
                    <span class="text-[color:var(--line)]">"·"</span>
                    <span class="flex items-baseline gap-1">
                        <span class="uppercase tracking-wider">{t!(i18n, top_opportunity_roi)}</span>
                        <span class="font-mono text-[color:var(--color-text)]">{format!("{:.0}%", deal.return_on_investment)}</span>
                    </span>
                </div>
            </div>
            <div class="flex flex-col items-end text-right shrink-0">
                <span class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                    {t!(i18n, top_opportunity_profit)}
                </span>
                <span class="text-2xl sm:text-3xl font-semibold font-mono text-emerald-300 leading-none tabular-nums">
                    <Gil amount=deal.profit />
                </span>
                <span class="text-[10px] text-[color:var(--color-text-muted)] font-mono mt-1">{demand_label}</span>
            </div>
        </a>
    }
}
