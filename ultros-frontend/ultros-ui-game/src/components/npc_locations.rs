//! Where an NPC stands, and who issues a leve — both read out of the game
//! data pack (`Data::npc_placements` and `Data::leve_issuers`).

use crate::{global_state::xiv_data::tracked_data, i18n::*};
use leptos::prelude::*;
use ultros_ui::components::app_link::AppLink;
use xiv_gen::{ENpcResidentId, LeveId, MapId, NpcPlacement, PlaceNameId, TerritoryTypeId};

/// The page for an NPC.
pub fn npc_href(npc_id: i32) -> String {
    format!("/npc/{npc_id}")
}

/// Zone label for a placement in the current locale: the territory's place
/// name, plus the map's sub-area when it has one (`Ul'dah - Steps of Thal ·
/// Merchant Strip`).
pub fn placement_label(data: &xiv_gen::Data, placement: &NpcPlacement) -> String {
    let name = |id: i32| {
        data.place_names
            .get(&PlaceNameId(id))
            .map(|p| p.name.as_str())
            .filter(|n| !n.is_empty())
    };
    let zone = data
        .territory_types
        .get(&TerritoryTypeId(placement.territory.0))
        .and_then(|t| name(t.place_name))
        .or_else(|| {
            data.maps
                .get(&MapId(placement.map.0))
                .and_then(|m| name(m.place_name))
        });
    let sub = data
        .maps
        .get(&MapId(placement.map.0))
        .and_then(|m| name(m.place_name_sub));
    match (zone, sub) {
        (Some(zone), Some(sub)) => format!("{zone} · {sub}"),
        (Some(zone), None) => zone.to_string(),
        (None, Some(sub)) => sub.to_string(),
        (None, None) => String::new(),
    }
}

/// `New Gridania · X: 11.8, Y: 13.4`.
pub fn placement_text(data: &xiv_gen::Data, placement: &NpcPlacement) -> String {
    let label = placement_label(data, placement);
    if label.is_empty() {
        format!("X: {:.1}, Y: {:.1}", placement.x, placement.y)
    } else {
        format!("{label} · X: {:.1}, Y: {:.1}", placement.x, placement.y)
    }
}

#[component]
pub fn NpcLocations(npc_id: i32) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let placements = data
        .npc_placements
        .get(&ENpcResidentId(npc_id))
        .map(Vec::as_slice)
        .unwrap_or_default();
    view! {
        <div data-npc-locations=npc_id class="flex flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
            {if placements.is_empty() {
                view! { <span>{t!(i18n, npc_location_unknown)}</span> }.into_any()
            } else {
                placements.iter().map(|placement| {
                    let seasonal = placement.festival_id != 0;
                    view! {
                        <span>
                            {placement_text(data, placement)}
                            {seasonal.then(|| view! {
                                <span class="ml-1 text-amber-300">{t!(i18n, npc_location_seasonal)}</span>
                            })}
                        </span>
                    }
                }).collect_view().into_any()
            }}
        </div>
    }
}

/// One link per NPC, name plus the zone they stand in — the compact form for
/// a shop that a dozen NPCs offer (every city's scrip exchange), where a full
/// [`NpcLocations`] block per NPC would swamp the card. Renders nothing for an
/// empty list, so a shop with no known NPC looks as it did before.
#[component]
pub fn NpcLinkList(npcs: Vec<ENpcResidentId>) -> impl IntoView {
    let data = tracked_data();
    if npcs.is_empty() {
        return None;
    }
    Some(view! {
        <div class="flex flex-wrap gap-x-3 gap-y-1 text-xs">
            {npcs.into_iter().map(|npc_id| {
                let name = data
                    .e_npc_residents
                    .get(&npc_id)
                    .map(|npc| npc.singular.clone())
                    .unwrap_or_else(|| npc_id.0.to_string());
                let zone = data
                    .npc_placements
                    .get(&npc_id)
                    .and_then(|p| p.first())
                    .map(|p| placement_label(data, p))
                    .unwrap_or_default();
                view! {
                    <span class="inline-flex items-baseline gap-1 min-w-0">
                        <AppLink href=npc_href(npc_id.0) attr:class="text-brand-200 hover:underline">
                            {name}
                        </AppLink>
                        {(!zone.is_empty()).then(|| view! {
                            <span class="text-[color:var(--color-text-muted)] truncate">"· " {zone}</span>
                        })}
                    </span>
                }
            }).collect_view()}
        </div>
    })
}

#[component]
pub fn LeveIssuers(leve_id: i32) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let issuers = data
        .leve_issuers
        .get(&LeveId(leve_id))
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
                        <AppLink href=npc_href(npc_id.0) attr:class="text-brand-200 hover:underline">
                            {data.e_npc_residents.get(&npc_id)
                                .map(|npc| npc.singular.clone())
                                .unwrap_or_else(|| npc_id.0.to_string())}
                        </AppLink>
                        <NpcLocations npc_id=npc_id.0 />
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
            // Leve 21, "In with the New": issued by Gontrant in New Gridania.
            let html = view! { <LeveIssuers leve_id=21 /> }.to_html();
            assert!(html.contains("Gontrant"), "{html}");
            assert!(html.contains("New Gridania"), "{html}");
            assert!(html.contains("X: 11.8, Y: 13.4"), "{html}");
            assert!(html.contains("href=\"/npc/1000101\""), "{html}");
            assert!(!html.contains("Maisenta"));
            assert_eq!(html, view! { <LeveIssuers leve_id=21 /> }.to_html());
            // A housing servant has no world position.
            assert!(
                view! { <NpcLocations npc_id=1016176 /> }
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
    fn pack_places_the_city_vendors_the_level_sheet_misses() {
        let data = tracked_data();
        // Ilorie and Admiranda, Old Gridania: absent from the Level sheet.
        for (npc, x, y) in [(1000216, 14.4, 9.7), (1000218, 14.5, 10.1)] {
            let placements = &data.npc_placements[&ENpcResidentId(npc)];
            let p = placements
                .iter()
                .find(|p| (p.x - x).abs() < 0.1 && (p.y - y).abs() < 0.1)
                .unwrap_or_else(|| panic!("npc {npc} not at {x},{y}: {placements:?}"));
            assert_eq!(placement_label(data, p), "Old Gridania");
        }
        for placements in data.npc_placements.values() {
            assert!(!placements.is_empty());
            assert!(
                placements
                    .iter()
                    .all(|p| p.x.is_finite() && p.y.is_finite())
            );
        }
        assert_eq!(
            data.leve_issuers[&LeveId(21)],
            vec![ENpcResidentId(1000101)]
        );
    }
}
