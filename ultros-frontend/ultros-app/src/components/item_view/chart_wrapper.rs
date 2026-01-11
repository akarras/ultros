use crate::components::price_history_chart::PriceHistoryChart;
use crate::components::toggle::Toggle;
use crate::error::AppError;
use chrono::{TimeDelta, Utc};
use leptos::prelude::*;
use ultros_api_types::CurrentlyShownItem;

#[component]
pub fn ChartWrapper(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
    item_id: Memo<i32>,
    world: Memo<String>,
) -> impl IntoView {
    let (hq_only, set_hq_only) = signal(false);
    let (days_range, set_days_range) = signal(30i32); // 0 = All

    /* moved into Transition branch to avoid reading resource outside Suspense/Transition */

    view! {
        <Transition fallback=move || {
            view! {
                <div class="animate-pulse panel h-[35em] text-[color:var(--color-text)]">
                    <div class="h-full w-full flex items-center justify-center">
                        <div class="w-16 h-16 border-4 border-brand-400/40 border-t-transparent rounded-full animate-spin" />
                    </div>
                </div>
            }
        }>
            {move || {
                let error = listing_resource
                    .with(|l| l.as_ref().and_then(|r| r.as_ref().err()).map(|e| e.to_string()));
                if let Some(msg) = error {
                    view! {
                        <div role="alert" class="bg-red-900/30 text-red-200 border border-red-700/40 rounded-xl p-4">
                            <strong class="font-semibold">"Error:"</strong>
                            <span class="ml-2">{msg}</span>
                            <div class="text-sm text-red-300/80 mt-1">"Unable to load recent sales. Please try refreshing."</div>
                        </div>
                    }.into_any()
                } else {
                    let base_sales = Memo::new(move |_| {
                        listing_resource
                            .with(|l| {
                                l.as_ref()
                                    .and_then(|l| l.as_ref().map(|l| l.sales.clone()).ok())
                            })
                            .unwrap_or_default()
                    });

                    let filtered_sales = Memo::new(move |_| {
                        let mut sales = base_sales();
                        if hq_only() {
                            sales.retain(|s| s.hq);
                        }
                        let days = days_range();
                        if days > 0 {
                            let cutoff = (Utc::now() - TimeDelta::days(days as i64)).naive_utc();
                            sales.retain(|s| s.sold_date >= cutoff);
                        }
                        sales
                    });

                    view! {
                        <div class="space-y-4">
                            <div class="panel p-4 text-[color:var(--color-text)]">
                                <div class="flex flex-wrap items-center justify-between gap-3">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <div class="inline-flex rounded-md overflow-hidden border border-[color:var(--color-outline)]">
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors",
                                                    if days_range() == 7 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(7)
                                            >
                                                "7d"
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 30 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(30)
                                            >
                                                "30d"
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 90 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(90)
                                            >
                                                "90d"
                                            </button>
                                            <button
                                                class=move || [
                                                    "px-3 py-1.5 text-sm transition-colors border-l border-[color:var(--color-outline)]",
                                                    if days_range() == 0 { "bg-brand-600/25 text-brand-100" } else { "bg-brand-900/30 text-[color:var(--color-text)]" },
                                                ].join(" ")
                                                on:click=move |_| set_days_range(0)
                                            >
                                                "All"
                                            </button>
                                        </div>
                                        <div class="ml-2">
                                            <Toggle
                                                checked=hq_only
                                                set_checked=set_hq_only
                                                checked_label="HQ only"
                                                unchecked_label="All qualities"
                                            />
                                        </div>
                                    </div>
                                    <a
                                        class="btn-primary"
                                        target="_blank"
                                        href=move || format!("/itemcard/{}/{}", world(), item_id())
                                    >
                                        "Download PNG"
                                    </a>
                                </div>
                            </div>

                            {move || {
                                if filtered_sales.with(|s| s.is_empty()) {
                                    view! {
                                        <div role="status" class="bg-amber-900/30 text-amber-200 border border-amber-700/40 rounded-xl p-4">
                                            "No sales found for the selected filters. Try expanding the time range or disabling HQ-only."
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="panel p-6 text-[color:var(--color-text)]">
                                            <PriceHistoryChart sales=filtered_sales />
                                        </div>
                                    }.into_any()
                                }
                            }}

                            {move || {
                                let no_listings = listing_resource.with(|l| {
                                    l.as_ref().and_then(|r| r.as_ref().ok()).map(|l| l.listings.is_empty()).unwrap_or(false)
                                });
                                no_listings.then(|| view! {
                                    <div role="status" class="bg-amber-900/30 text-amber-200 border border-amber-700/40 rounded-xl p-4">
                                        "No active listings found for this world. Try checking other worlds or come back later."
                                    </div>
                                })
                            }}
                        </div>
                    }.into_any()
                }
            }}
        </Transition>
    }.into_any()
}
