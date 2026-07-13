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

/// Map band to inline-strip color + arrow. Different shape from a card —
/// just a colored arrow + label; no chip background.
fn band_classes(band: HeatBand) -> (&'static str, &'static str) {
    match band {
        HeatBand::Hot => ("text-emerald-300", "↑↑"),
        HeatBand::Warm => ("text-emerald-200/90", "↑"),
        HeatBand::Stable => ("text-[color:var(--color-text)]", "→"),
        HeatBand::Cool => ("text-red-300", "↓"),
        HeatBand::NoData => ("text-[color:var(--color-text-muted)]", "—"),
    }
}

#[component]
fn HeatPip(cat: CategoryHeat) -> impl IntoView {
    let i18n = use_i18n();
    let (text_class, arrow) = band_classes(cat.band);
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

    view! {
        <span class="inline-flex items-baseline gap-1.5 whitespace-nowrap">
            <span class="text-[color:var(--color-text)] text-sm">{name}</span>
            <span class=format!("text-base font-semibold leading-none {text_class}")>{arrow}</span>
            <span class=format!("text-xs font-semibold {text_class}")>{band_label}</span>
        </span>
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
        <section class="dashboard-section">
            <header class="flex items-baseline justify-between mb-2">
                <h2 class="dashboard-section-title">{t!(i18n, market_heat_title)}</h2>
                <span class="text-xs text-[color:var(--color-text-muted)]">
                    {t!(i18n, market_heat_subtitle)}
                </span>
            </header>
            <Suspense fallback=move || view! {
                <div class="h-6 bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] animate-pulse rounded" />
            }>
                {move || {
                    heat.get().map(|maybe| match maybe {
                        Some(resp) => {
                            let mut pieces: Vec<AnyView> = Vec::new();
                            let total = resp.categories.len();
                            for (idx, c) in resp.categories.into_iter().enumerate() {
                                pieces.push(view! { <HeatPip cat=c /> }.into_any());
                                if idx + 1 < total {
                                    pieces.push(view! {
                                        <span class="text-[color:var(--line)] mx-3" aria-hidden="true">"|"</span>
                                    }.into_any());
                                }
                            }
                            view! {
                                <div class="flex flex-wrap items-baseline gap-y-2 leading-snug">
                                    {pieces}
                                </div>
                            }.into_any()
                        },
                        None => view! {
                            <div class="text-sm text-[color:var(--color-text-muted)] py-2">
                                {t!(i18n, market_heat_no_data)}
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
    fn test_category_label_key() {
        // Test standard 1-5 categories match exactly to their i18n keys
        assert_eq!(category_label_key(1), "market_heat_cat_weapons");
        assert_eq!(category_label_key(2), "market_heat_cat_tools");
        assert_eq!(category_label_key(3), "market_heat_cat_armor");
        assert_eq!(category_label_key(4), "market_heat_cat_items");
        assert_eq!(category_label_key(5), "market_heat_cat_housing");

        // Test out-of-bounds edge cases that should fallback to "other"
        assert_eq!(category_label_key(6), "market_heat_cat_other");
        assert_eq!(category_label_key(0), "market_heat_cat_other");
        assert_eq!(category_label_key(99), "market_heat_cat_other");
    }

    #[test]
    fn test_band_classes() {
        // Verify each heat band accurately maps to its styling class and text indicator
        assert_eq!(band_classes(HeatBand::Hot), ("text-emerald-300", "↑↑"));
        assert_eq!(band_classes(HeatBand::Warm), ("text-emerald-200/90", "↑"));
        assert_eq!(band_classes(HeatBand::Stable), ("text-[color:var(--color-text)]", "→"));
        assert_eq!(band_classes(HeatBand::Cool), ("text-red-300", "↓"));
        assert_eq!(band_classes(HeatBand::NoData), ("text-[color:var(--color-text-muted)]", "—"));
    }
}
