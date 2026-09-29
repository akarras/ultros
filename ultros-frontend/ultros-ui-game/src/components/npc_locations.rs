//! Where an NPC stands, and who issues a leve — both read out of the game
//! data pack (`Data::npc_placements` and `Data::leve_issuers`).

use std::collections::HashSet;

use crate::{global_state::xiv_data::tracked_data, i18n::*};
use leptos::prelude::*;
pub use ultros_game_sources::placement_label;
use ultros_ui::components::app_link::AppLink;
use ultros_ui::components::hover_card::{AccentHairline, HOVER_CARD_CHROME, HoverCard};
use xiv_gen::{ENpcResidentId, ItemId, LeveId, NpcPlacement};

use super::zone_map::map_image_href;

/// The page for an NPC.
pub fn npc_href(npc_id: i32) -> String {
    format!("/npc/{npc_id}")
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

/// Number of distinct items other than `current_item` that `npc` sells for gil
/// or hands out through a special-shop exchange.
fn other_catalog_item_count(
    data: &xiv_gen::Data,
    npc: ENpcResidentId,
    current_item: ItemId,
) -> usize {
    let mut items = HashSet::new();
    for (shop_id, npcs) in &data.gil_shop_npcs {
        if !npcs.contains(&npc) {
            continue;
        }
        if let Some(rows) = data.gil_shop_items.get(shop_id) {
            items.extend(rows.iter().filter_map(|row| {
                let item = ItemId(row.item);
                (item.0 > 0 && item != current_item).then_some(item)
            }));
        }
    }
    for (shop_id, npcs) in &data.special_shop_npcs {
        if !npcs.contains(&npc) {
            continue;
        }
        if let Some(shop) = data.special_shops.get(shop_id) {
            items.extend(
                shop.entries()
                    .flat_map(|entry| entry.receive)
                    .map(|(item, _)| item)
                    .filter(|item| item.0 > 0 && *item != current_item),
            );
        }
    }
    items.len()
}

const PREVIEW_SCALE: f64 = 2.5;
const PREVIEW_ASPECT_RATIO: f64 = 16.0 / 9.0;

fn preview_offsets(fx: f64, fy: f64) -> (f64, f64) {
    // The square map is scaled in viewport widths, but CSS top percentages
    // use viewport heights. Clamp each axis in its own units.
    let vertical_scale = PREVIEW_SCALE * PREVIEW_ASPECT_RATIO;
    (
        (0.5 - fx * PREVIEW_SCALE).clamp(1.0 - PREVIEW_SCALE, 0.0),
        (0.5 - fy * vertical_scale).clamp(1.0 - vertical_scale, 0.0),
    )
}

/// A non-interactive, zoomed map for a hover card. The full NPC page owns the
/// pannable map; this preview deliberately has no controls because moving the
/// pointer off the anchor closes the overlay.
#[component]
fn NpcMapPreview(placement: NpcPlacement, label: String) -> impl IntoView {
    let data = tracked_data();
    let map = data.maps.get(&placement.map)?;
    let fx = f64::from(map.fraction(placement.x));
    let fy = f64::from(map.fraction(placement.y));
    let (left, top) = preview_offsets(fx, fy);
    Some(view! {
        <div
            data-npc-map-preview=placement.map.0
            class="relative w-full overflow-hidden rounded-md border border-[color:var(--color-outline)] bg-black/40"
            style=format!("aspect-ratio:{PREVIEW_ASPECT_RATIO}")
        >
            <div
                class="absolute aspect-square"
                style=format!("width:{}%;left:{}%;top:{}%", PREVIEW_SCALE * 100.0, left * 100.0, top * 100.0)
            >
                <img
                    src=map_image_href(placement.map)
                    alt=""
                    class="block h-auto w-full"
                    loading="lazy"
                />
                <div
                    class="zone-map-pin"
                    style=format!("left:{}%;top:{}%", fx * 100.0, fy * 100.0)
                    title=label
                >
                    <span class="zone-map-pin-dot"></span>
                </div>
            </div>
        </div>
    })
}

/// One rich link per NPC: coordinates inline, with a hover/focus preview of
/// the map and the rest of that NPC's catalog. The whole chip is clickable.
/// Renders nothing for an empty list, so a shop with no known NPC looks as it
/// did before.
#[component]
pub fn NpcLinkList(npcs: Vec<ENpcResidentId>, current_item: ItemId) -> impl IntoView {
    let i18n = use_i18n();
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
                let placement = data
                    .npc_placements
                    .get(&npc_id)
                    .and_then(|p| p.first())
                    .copied();
                let location = placement
                    .map(|p| placement_text(data, &p))
                    .unwrap_or_else(|| t_string!(i18n, npc_location_unknown).to_string());
                let other_items = other_catalog_item_count(data, npc_id, current_item);
                let name = StoredValue::new(name);
                let location = StoredValue::new(location);
                view! {
                    <HoverCard
                        open_delay_ms=250
                        class="min-w-0"
                        content=move || {
                            let tooltip_name = name.get_value();
                            let tooltip_location = location.get_value();
                            let map_label = format!("{} · {}", tooltip_name, tooltip_location);
                            view! {
                                <div class=format!("{HOVER_CARD_CHROME} w-[22rem] max-w-[calc(100vw-1rem)] p-3 flex flex-col gap-2")>
                                    <AccentHairline />
                                    <div class="flex flex-col gap-0.5">
                                        <span class="font-bold text-[color:var(--color-text)]">{tooltip_name}</span>
                                        <span class="text-xs text-[color:var(--color-text-muted)]">{tooltip_location}</span>
                                        <span class="text-xs text-brand-300">
                                            {t_string!(i18n, npc_other_catalog_items, count = other_items).to_string()}
                                        </span>
                                    </div>
                                    {placement.map(|placement| view! {
                                        <NpcMapPreview placement label=map_label />
                                    })}
                                </div>
                            }
                        }
                    >
                        <AppLink
                            href=npc_href(npc_id.0)
                            attr:class="group flex min-w-0 flex-col rounded-md border border-[color:var(--color-outline)] px-2.5 py-2 text-left transition-colors hover:border-brand-300/60 hover:bg-brand-900/20 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-300"
                        >
                            <span class="font-semibold text-brand-200 group-hover:underline">{name.get_value()}</span>
                            <span class="truncate text-[color:var(--color-text-muted)]">{location.get_value()}</span>
                        </AppLink>
                    </HoverCard>
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
    fn preview_centres_interior_pins_and_keeps_edges_in_view() {
        let vertical_scale = PREVIEW_SCALE * PREVIEW_ASPECT_RATIO;
        for (fx, fy) in [(0.5, 0.5), (0.3, 0.7), (0.7, 0.3)] {
            let (left, top) = preview_offsets(fx, fy);
            assert!((left + fx * PREVIEW_SCALE - 0.5).abs() < 1e-9);
            assert!((top + fy * vertical_scale - 0.5).abs() < 1e-9);
        }
        for fx in [0.0, 0.01, 0.99, 1.0] {
            for fy in [0.0, 0.01, 0.99, 1.0] {
                let (left, top) = preview_offsets(fx, fy);
                let x = left + fx * PREVIEW_SCALE;
                let y = top + fy * vertical_scale;
                assert!((-1e-9..=1.0 + 1e-9).contains(&x), "x={x}");
                assert!((-1e-9..=1.0 + 1e-9).contains(&y), "y={y}");
                assert!(left <= 0.0 && left + PREVIEW_SCALE >= 1.0);
                assert!(top <= 0.0 && top + vertical_scale >= 1.0);
            }
        }
    }

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
    fn exchange_npc_links_show_coordinates_and_count_other_catalog_items() {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<Locale>());
            let data = tracked_data();
            let npc = ENpcResidentId(1001617);
            let current = ItemId(39595);
            let count = other_catalog_item_count(data, npc, current);
            assert!(
                count > 100,
                "scrip exchange catalog unexpectedly small: {count}"
            );
            let html = view! { <NpcLinkList npcs=vec![npc] current_item=current /> }.to_html();
            assert!(html.contains("href=\"/npc/1001617\""), "{html}");
            assert!(html.contains("X: 14.3, Y: 10.8"), "{html}");
            assert!(
                !html.contains("data-npc-map-preview"),
                "hover content is client-only"
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
