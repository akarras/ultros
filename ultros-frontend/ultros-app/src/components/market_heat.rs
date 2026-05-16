//! Home-page Market Heat band.
//!
//! Renders one chip per FFXIV top-level item category showing its 24h
//! volume-weighted heat: Hot / Warm / Stable / Cool / NoData. Matches
//! the "MARKET HEAT" row in the dashboard mockup, scaled to the 5
//! game-defined groupings (vs the mockup's 6 editorial buckets — the
//! game's groupings are stable and locale-friendly out of the gate;
//! finer-grained curation can ship later).

use leptos::prelude::*;
use ultros_api_types::market_heat::{CategoryHeat, HeatBand};

use crate::{api::get_market_heat, i18n::*};

/// Map game-defined Category id (1-5) to the i18n key for its name.
fn category_label_key(id: u8) -> &'static str {
    match id {
        1 => "market_heat_cat_weapons",
        2 => "market_heat_cat_tools",
        3 => "market_heat_cat_armor",
        4 => "market_heat_cat_items",
        5 => "market_heat_cat_housing",
        _ => "market_heat_cat_other",
    }
}

/// Map band to color classes (chip background tint, arrow color). Uses
/// the existing emerald/amber/red palette consistent with
/// ConfidenceBadge and Sparkline.
fn band_classes(band: HeatBand) -> (&'static str, &'static str, &'static str) {
    match band {
        HeatBand::Hot => (
            "text-emerald-300",
            "bg-[color:color-mix(in_srgb,#10b981_14%,transparent)] border-emerald-400/40",
            "↑↑",
        ),
        HeatBand::Warm => (
            "text-emerald-200/90",
            "bg-[color:color-mix(in_srgb,#10b981_8%,transparent)] border-emerald-400/30",
            "↑",
        ),
        HeatBand::Stable => (
            "text-[color:var(--color-text)]",
            "bg-[color:color-mix(in_srgb,var(--brand-ring)_6%,transparent)] border-[color:var(--color-outline)]",
            "→",
        ),
        HeatBand::Cool => (
            "text-red-300",
            "bg-[color:color-mix(in_srgb,#ef4444_10%,transparent)] border-red-400/30",
            "↓",
        ),
        HeatBand::NoData => (
            "text-[color:var(--color-text-muted)]",
            "bg-transparent border-[color:var(--color-outline)]/40",
            "—",
        ),
    }
}

#[component]
fn HeatChip(cat: CategoryHeat) -> impl IntoView {
    let i18n = use_i18n();
    let (text_class, container_class, arrow) = band_classes(cat.band);
    let label_key = category_label_key(cat.category_id);
    let name = match label_key {
        "market_heat_cat_weapons" => t_string!(i18n, market_heat_cat_weapons).to_string(),
        "market_heat_cat_tools" => t_string!(i18n, market_heat_cat_tools).to_string(),
        "market_heat_cat_armor" => t_string!(i18n, market_heat_cat_armor).to_string(),
        "market_heat_cat_items" => t_string!(i18n, market_heat_cat_items).to_string(),
        "market_heat_cat_housing" => t_string!(i18n, market_heat_cat_housing).to_string(),
        _ => t_string!(i18n, market_heat_cat_other).to_string(),
    };
    let band_label = match cat.band {
        HeatBand::Hot => t_string!(i18n, market_heat_band_hot).to_string(),
        HeatBand::Warm => t_string!(i18n, market_heat_band_warm).to_string(),
        HeatBand::Stable => t_string!(i18n, market_heat_band_stable).to_string(),
        HeatBand::Cool => t_string!(i18n, market_heat_band_cool).to_string(),
        HeatBand::NoData => t_string!(i18n, market_heat_band_no_data).to_string(),
    };
    let pct_text = match cat.band {
        HeatBand::NoData => "—".to_string(),
        _ => format!("{:+.1}%", cat.avg_pct_change_24h),
    };

    view! {
        <div class=format!(
            "flex-1 min-w-[120px] rounded-xl border px-3 py-2 {container_class}"
        )>
            <div class="text-[10px] uppercase tracking-wider text-[color:var(--color-text-muted)]">
                {name}
            </div>
            <div class=format!("flex items-baseline gap-1 mt-0.5 {text_class}")>
                <span class="text-base font-semibold leading-none">{arrow}</span>
                <span class="text-sm font-semibold">{band_label}</span>
            </div>
            <div class="text-xs text-[color:var(--color-text-muted)] font-mono mt-0.5">
                {pct_text}
            </div>
        </div>
    }
}

#[component]
pub fn MarketHeat(world: Signal<Option<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let heat = LocalResource::new(move || {
        let w = world.get();
        async move {
            let w = w?;
            get_market_heat(&w).await.ok()
        }
    });

    view! {
        <section class="panel rounded-2xl p-4 sm:p-5 border border-[color:var(--color-outline)]">
            <header class="flex items-center justify-between mb-3">
                <h2 class="text-sm uppercase tracking-wider text-[color:var(--color-text-muted)]">
                    {t!(i18n, market_heat_title)}
                </h2>
                <span class="text-xs text-[color:var(--color-text-muted)]">
                    {t!(i18n, market_heat_subtitle)}
                </span>
            </header>
            <Suspense fallback=move || view! {
                <div class="flex flex-wrap gap-2">
                    {(0..5).map(|_| view! {
                        <div class="flex-1 min-w-[120px] h-16 rounded-xl bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse" />
                    }).collect_view()}
                </div>
            }>
                {move || {
                    heat.get().map(|maybe| match maybe {
                        Some(resp) => view! {
                            <div class="flex flex-wrap gap-2">
                                {resp.categories
                                    .into_iter()
                                    .map(|c| view! { <HeatChip cat=c /> })
                                    .collect_view()}
                            </div>
                        }.into_any(),
                        None => view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] py-4">
                                {t!(i18n, market_heat_no_data)}
                            </div>
                        }.into_any(),
                    })
                }}
            </Suspense>
        </section>
    }
}
