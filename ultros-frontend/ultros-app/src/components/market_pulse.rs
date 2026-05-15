//! Market Pulse KPI strip — four cards (Active Listings, Sales 24h,
//! Market Volume, Items Traded) each carrying a +/- delta-vs-yesterday.
//!
//! Backed by `/api/v1/market_pulse/{world}`. The endpoint pulls a
//! 5-min-bucketed rollup from ClickHouse plus a live PG snapshot of
//! active_listings — single round trip fills all four cards.

use leptos::prelude::*;

use crate::{api::get_market_pulse, components::gil::Gil, error::AppError, i18n::*};

/// Format a number with K/M/B suffix for compact display in KPI cards.
/// We don't render the full thousands-separated number because the cards
/// have a fixed visual budget and "245.6M" reads faster than "245,610,432".
fn compact_number(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        thousands::Separable::separate_with_commas(&n)
    }
}

/// One KPI card. Renders the value, a label, and a +/- delta chip.
/// `delta_pct = None` means yesterday was zero — render "—" instead.
#[component]
fn KpiCard(label: AnyView, value: String, #[prop(into)] delta_pct: Option<f32>) -> impl IntoView {
    let i18n = use_i18n();
    // Delta chip: green when positive, red when negative, muted dash when None.
    let (delta_text, delta_class): (String, &'static str) = match delta_pct {
        Some(p) if p >= 0.0 => (
            format!("+{p:.1}%"),
            "text-emerald-300 bg-[color:color-mix(in_srgb,#10b981_12%,transparent)] \
             border-emerald-400/30",
        ),
        Some(p) => (
            format!("{p:.1}%"),
            "text-red-300 bg-[color:color-mix(in_srgb,#ef4444_10%,transparent)] \
             border-red-400/30",
        ),
        None => (
            "—".to_string(),
            "text-[color:var(--color-text-muted)] border-[color:var(--color-outline)]",
        ),
    };
    let vs_yesterday = t_string!(i18n, market_pulse_vs_yesterday).to_string();

    view! {
        <div class="panel p-4 rounded-2xl border border-[color:var(--color-outline)] flex flex-col gap-2 min-w-0">
            <span class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)] truncate">{label}</span>
            <span class="text-2xl sm:text-3xl font-extrabold text-[color:var(--color-text)] tabular-nums leading-none">
                {value}
            </span>
            <span class=format!(
                "inline-flex items-center gap-1 self-start text-xs font-semibold px-2 py-0.5 rounded-full border {delta_class}"
            )>
                {delta_text}
                <span class="text-[color:var(--color-text-muted)] font-normal ml-1">{vs_yesterday}</span>
            </span>
        </div>
    }
}

/// The full strip — fires the API request on the current world and
/// renders four cards once it lands. Skeletons while loading.
#[component]
pub fn MarketPulse(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let pulse = LocalResource::new(move || {
        let w = world.get();
        async move {
            let Some(w) = w else {
                return Err(AppError::NoHomeWorld);
            };
            get_market_pulse(&w).await
        }
    });

    view! {
        <Suspense fallback=move || view! {
            <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
                {(0..4).map(|_| view! {
                    <div class="panel p-4 rounded-2xl border border-[color:var(--color-outline)] h-[6.5rem] bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                }).collect_view()}
            </div>
        }>
            {move || {
                pulse.get().map(|result| match result.as_ref() {
                    Ok(p) => view! {
                        <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
                            <KpiCard
                                label=t!(i18n, market_pulse_active_listings).into_any()
                                value=compact_number(p.active_listings)
                                delta_pct=None
                            />
                            <KpiCard
                                label=t!(i18n, market_pulse_sales_24h).into_any()
                                value=compact_number(p.sales_today)
                                delta_pct=p.sales_delta_pct()
                            />
                            <KpiCard
                                label=t!(i18n, market_pulse_gil_volume).into_any()
                                value={
                                    // Use the Gil component visually inline by composing a string;
                                    // KpiCard takes a String to keep the API uniform.
                                    compact_number(p.gil_volume_today)
                                }
                                delta_pct=p.gil_volume_delta_pct()
                            />
                            <KpiCard
                                label=t!(i18n, market_pulse_unit_volume).into_any()
                                value=compact_number(p.unit_volume_today)
                                delta_pct=p.unit_volume_delta_pct()
                            />
                        </div>
                    }.into_any(),
                    Err(_) => view! {
                        // Soft-fail: empty placeholder. The home page has plenty of other content.
                        <div></div>
                    }.into_any(),
                })
            }}
        </Suspense>
    }
}

// Mark Gil as used so clippy doesn't complain when the import lives here for
// future "Market Volume" rendering with the proper gil icon (TODO).
#[allow(dead_code)]
fn _unused_gil_marker() -> impl IntoView {
    view! { <Gil amount=0 /> }
}
