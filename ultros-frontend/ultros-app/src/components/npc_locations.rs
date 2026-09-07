use crate::{global_state::xiv_data::tracked_data, i18n::*};
use leptos::prelude::*;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Deserialize)]
struct Location {
    label: String,
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct Npc {
    name: String,
    locations: Vec<Location>,
}

#[derive(Deserialize)]
struct LocationData {
    npcs: BTreeMap<i32, Npc>,
    leve_issuers: BTreeMap<i32, Vec<i32>>,
}

// Identical bundled data on SSR and hydration; no per-card network requests.
static DATA: LazyLock<LocationData> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../data/npc-locations/runtime.json"))
        .expect("validated NPC location data")
});

#[component]
pub fn NpcLocations(npc_id: i32) -> impl IntoView {
    let i18n = use_i18n();
    let locations = DATA
        .npcs
        .get(&npc_id)
        .map(|npc| npc.locations.as_slice())
        .unwrap_or_default();
    view! {
        <div data-npc-locations=npc_id class="flex flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
            {if locations.is_empty() {
                view! { <span>{t!(i18n, npc_location_unknown)}</span> }.into_any()
            } else {
                locations.iter().map(|location| view! {
                    <span>{format!("{} · X: {:.1}, Y: {:.1}", location.label, location.x, location.y)}</span>
                }).collect_view().into_any()
            }}
        </div>
    }
}

#[component]
pub fn LeveIssuers(leve_id: i32) -> impl IntoView {
    let i18n = use_i18n();
    let issuers = DATA
        .leve_issuers
        .get(&leve_id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    view! {
        <div data-leve-issuers=leve_id class="flex flex-col gap-2 text-sm">
            <span class="text-[color:var(--color-text-muted)]">{t!(i18n, npc_quest_giver)}</span>
            {if issuers.is_empty() {
                view! { <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, npc_quest_giver_unknown)}</span> }.into_any()
            } else {
                issuers.iter().copied().map(|npc_id| view! {
                    <div class="flex flex-col gap-1">
                        <a href=format!("https://garlandtools.org/db/#npc/{npc_id}") class="text-brand-200 hover:underline">
                            {move || tracked_data().e_npc_residents.get(&xiv_gen::ENpcResidentId(npc_id))
                                .map(|npc| npc.singular.clone())
                                .unwrap_or_else(|| DATA.npcs[&npc_id].name.clone())}
                        </a>
                        <NpcLocations npc_id />
                    </div>
                }).collect_view().into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssr_renders_giver_coordinates_and_unknown_fallbacks() {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<Locale>());
            let html = view! { <LeveIssuers leve_id=21 /> }.to_html();
            assert!(html.contains("Gontrant"));
            assert!(html.contains("New Gridania"));
            assert!(html.contains("X: ") && html.contains("Y: "));
            assert!(html.contains("https://garlandtools.org/db/#npc/1000101"));
            assert!(!html.contains("Maisenta"));
            assert_eq!(html, view! { <LeveIssuers leve_id=21 /> }.to_html());
            assert!(
                view! { <NpcLocations npc_id=1000391 /> }
                    .to_html()
                    .contains("Location unavailable")
            );
            assert!(
                view! { <LeveIssuers leve_id=0 /> }
                    .to_html()
                    .contains("Quest giver unavailable")
            );
        });
    }

    #[test]
    fn leve_issuer_is_not_the_delivery_recipient() {
        assert_eq!(DATA.leve_issuers[&21], vec![1000101]);
        assert_eq!(DATA.npcs[&1000101].name, "Gontrant");
        assert!(!DATA.leve_issuers[&21].contains(&1001276));
        assert!(!DATA.npcs[&1000101].locations.is_empty());
    }

    #[test]
    fn bundled_locations_and_issuer_references_are_valid() {
        for issuers in DATA.leve_issuers.values() {
            assert!(!issuers.is_empty());
            assert!(issuers.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(issuers.iter().all(|id| DATA.npcs.contains_key(id)));
        }
        for npc in DATA.npcs.values() {
            for location in &npc.locations {
                assert!(!location.label.is_empty());
                assert!(location.x.is_finite() && location.y.is_finite());
            }
        }
        assert!(DATA.npcs[&1000391].locations.is_empty());
        assert!(!DATA.leve_issuers.contains_key(&0));
    }
}
