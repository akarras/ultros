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
        <section class="panel rounded-2xl p-4 sm:p-5 border border-[color:var(--color-outline)]">
            <header class="flex items-center justify-between mb-3">
                <h2 class="text-sm uppercase tracking-wider text-[color:var(--color-text-muted)] flex items-center gap-2">
                    <span class="text-amber-300">"🔥"</span>
                    {t!(i18n, top_opportunity_title)}
                </h2>
                <A
                    href=move || world_for_link.get()
                        .map(|w| format!("/flip-finder/{w}"))
                        .unwrap_or_else(|| "/flip-finder".to_string())
                    attr:class="text-xs text-[color:var(--brand-fg)] hover:underline"
                >
                    {t!(i18n, top_opportunity_view_all)}
                </A>
            </header>
            <Suspense fallback=move || view! {
                <div class="h-32 rounded-xl bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
            }>
                {move || {
                    let world_str = world.get().unwrap_or_default();
                    deal.get().map(|maybe| match maybe {
                        Some(d) => view! { <DealCard deal=d world_name=world_str /> }.into_any(),
                        None => view! {
                            <div class="text-center py-8 text-[color:var(--color-text-muted)] rounded-xl border border-dashed border-[color:var(--color-outline)]">
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

    view! {
        <a
            href=href
            class="grid grid-cols-[auto_1fr] sm:grid-cols-[auto_1fr_auto] gap-4 items-center rounded-xl p-3 sm:p-4 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)] transition-colors"
        >
            <div class="shrink-0">
                <ItemIcon item_id icon_size=IconSize::Large />
            </div>
            <div class="min-w-0">
                <div class="text-lg font-bold text-[color:var(--color-text)] truncate">{name}</div>
                <div class="grid grid-cols-2 sm:grid-cols-4 gap-x-4 gap-y-2 mt-2 text-sm">
                    <div>
                        <div class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                            {t!(i18n, top_opportunity_buy)}
                        </div>
                        <div class="font-mono text-[color:var(--color-text)]">
                            <Gil amount=buy />
                        </div>
                    </div>
                    <div>
                        <div class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                            {t!(i18n, top_opportunity_sell)}
                        </div>
                        <div class="font-mono text-[color:var(--color-text)]">
                            <Gil amount=sell />
                        </div>
                    </div>
                    <div>
                        <div class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                            {t!(i18n, top_opportunity_profit)}
                        </div>
                        <div class="font-mono font-bold text-emerald-300">
                            <Gil amount=deal.profit />
                        </div>
                    </div>
                    <div>
                        <div class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                            {t!(i18n, top_opportunity_roi)}
                        </div>
                        <div class="font-mono text-[color:var(--color-text)]">
                            {format!("{:.0}%", deal.return_on_investment)}
                        </div>
                    </div>
                </div>
            </div>
            <div class="hidden sm:flex items-end flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
                <span class="font-mono">{demand_label}</span>
            </div>
        </a>
    }
}
