//! Market Pulse KPI strip — four cards (Active Listings, Sales 24h,
//! Market Volume, Items Traded) each carrying a +/- delta-vs-yesterday.
//!
//! Backed by `/api/v1/market_pulse/{world}`. The endpoint pulls a
//! 5-min-bucketed rollup from ClickHouse plus a live PG snapshot of
//! active_listings — single round trip fills all four cards.

use leptos::prelude::*;

use crate::{api::get_market_pulse, error::AppError, i18n::*};

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

/// One KPI metric column — inline layout (no card background) so a row of
/// four reads like a typewriter strip rather than a card grid. The visual
/// separator between columns is provided by the parent via `metric-divider`.
/// `delta_pct = None` means yesterday was zero — render "—" instead.
#[component]
fn KpiCard(label: AnyView, value: String, #[prop(into)] delta_pct: Option<f32>) -> impl IntoView {
    // Delta chip: green when positive, red when negative, muted dash when None.
    let (delta_text, delta_class): (String, &'static str) = match delta_pct {
        Some(p) if p >= 0.0 => (format!("+{p:.1}%"), "text-emerald-300"),
        Some(p) => (format!("{p:.1}%"), "text-red-300"),
        None => ("—".to_string(), "text-[color:var(--color-text-muted)]"),
    };

    view! {
        <div class="flex flex-col gap-1 min-w-0 px-3 sm:px-5 py-1">
            <span class="text-[10px] sm:text-xs uppercase tracking-[0.14em] text-[color:var(--color-text-muted)] truncate">{label}</span>
            <span class="text-2xl sm:text-3xl font-semibold text-[color:var(--color-text)] tabular-nums leading-none">
                {value}
            </span>
            <span class=format!("text-xs font-semibold tabular-nums {delta_class}")>
                {delta_text}
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
        <section class="dashboard-section">
            <Suspense fallback=move || view! {
                <div class="grid grid-cols-2 sm:grid-cols-4 gap-x-px">
                    {(0..4).map(|_| view! {
                        <div class="h-[5.25rem] bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse rounded" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    pulse.get().map(|result| match result.as_ref() {
                        Ok(p) => view! {
                            <div class="grid grid-cols-2 sm:grid-cols-4 divide-x divide-[color:var(--line)]">
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
                                    value=compact_number(p.gil_volume_today)
                                    delta_pct=p.gil_volume_delta_pct()
                                />
                                <KpiCard
                                    label=t!(i18n, market_pulse_unit_volume).into_any()
                                    value=compact_number(p.unit_volume_today)
                                    delta_pct=p.unit_volume_delta_pct()
                                />
                            </div>
                        }.into_any(),
                        // Server soft-fails CH errors to a zeroed DTO so this
                        // branch only fires for hard errors (e.g. unknown world
                        // or analyzer warming up after a fresh deploy). Render
                        // a visible muted line at the same height as the loaded
                        // strip so the layout doesn't shift when it recovers.
                        Err(_) => view! {
                            <div class="flex items-center justify-center h-[5.25rem] text-sm text-[color:var(--color-text-muted)]">
                                {t!(i18n, market_pulse_load_failed)}
                            </div>
                        }.into_any(),
                    })
                }}
            </Suspense>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_number_formatting() {
        // Less than 10k: exactly separated
        assert_eq!(compact_number(0), "0");
        assert_eq!(compact_number(999), "999");
        assert_eq!(compact_number(9_999), "9,999");

        // 10k to 1M: formatted in K
        assert_eq!(compact_number(10_000), "10.0K");
        assert_eq!(compact_number(10_500), "10.5K");
        assert_eq!(compact_number(999_999), "1000.0K");

        // 1M to 1B: formatted in M
        assert_eq!(compact_number(1_000_000), "1.0M");
        assert_eq!(compact_number(1_500_000), "1.5M");
        assert_eq!(compact_number(999_999_999), "1000.0M");

        // Over 1B: formatted in B
        assert_eq!(compact_number(1_000_000_000), "1.00B");
        assert_eq!(compact_number(1_550_000_000), "1.55B");
        assert_eq!(compact_number(1_555_000_000), "1.55B"); // float truncation to 2 decimal places
        assert_eq!(compact_number(1_559_000_000), "1.56B"); // rounding
    }
}
