//! `/npc/:id`: one vendor NPC — where they stand, on a map, and everything
//! they sell for gil.
//!
//! Everything on the page comes from the game-data pack: the NPC's shops via
//! the `gil_shop_npcs` reverse index, their placements via `npc_placements`,
//! and the zone names via `PlaceName`. No market data, so the page is
//! synchronous and renders the same on the server and after hydration.

use std::collections::BTreeMap;

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use xiv_gen::{ENpcResidentId, ItemId, MapId, NpcPlacement};

use crate::components::app_link::AppLink;
use crate::components::gil::Gil;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::related_items::VendorGateBadge;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use ultros_ui_game::components::npc_locations::{placement_label, placement_text};
use ultros_ui_game::components::zone_map::{MapPin, ZoneMap};

/// The gil shops `npc` offers, in shop-id order, each with its rows.
fn shops_for_npc(
    data: &'static xiv_gen::Data,
    npc: ENpcResidentId,
) -> Vec<(&'static xiv_gen::GilShop, &'static [xiv_gen::GilShopItem])> {
    let mut shops: Vec<_> = data
        .gil_shop_npcs
        .iter()
        .filter(|(_, npcs)| npcs.contains(&npc))
        .filter_map(|(shop_id, _)| {
            let shop = data.gil_shops.get(shop_id)?;
            let rows = data
                .gil_shop_items
                .get(shop_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            Some((shop, rows))
        })
        .collect();
    // `gil_shop_npcs` is a HashMap; sort so SSR and hydration agree.
    shops.sort_by_key(|(shop, _)| shop.key_id.0);
    shops.dedup_by_key(|(shop, _)| shop.key_id.0);
    shops
}

/// Placements grouped by map, in map-id order, permanent spots first.
fn placements_by_map(placements: &[NpcPlacement]) -> Vec<(MapId, Vec<NpcPlacement>)> {
    let mut by_map: BTreeMap<i32, Vec<NpcPlacement>> = BTreeMap::new();
    for p in placements {
        by_map.entry(p.map.0).or_default().push(*p);
    }
    let mut groups: Vec<_> = by_map
        .into_iter()
        .map(|(map, list)| (MapId(map), list))
        .collect();
    groups.sort_by_key(|(map, list)| (list.iter().all(|p| p.festival_id != 0), map.0));
    groups
}

/// A shop's name as the game gives it, falling back to a generic label for
/// the many shops whose row is unnamed.
fn shop_title(shop: &xiv_gen::GilShop, fallback: &str) -> String {
    if shop.name.trim().is_empty() {
        fallback.to_string()
    } else {
        shop.name.clone()
    }
}

#[component]
pub fn NpcView() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let npc_id = Memo::new(move |_| {
        params
            .with(|p| p.get("id").and_then(|id| id.parse::<i32>().ok()))
            .unwrap_or(0)
    });
    let data = tracked_data();

    view! {
        {move || {
            let id = npc_id.get();
            let Some(resident) = data.e_npc_residents.get(&ENpcResidentId(id)) else {
                return view! {
                    <MetaTitle title=move || t_string!(i18n, npc_not_found).to_string() />
                    <div class="panel p-6 text-[color:var(--color-text-muted)]">
                        {t!(i18n, npc_not_found)}
                    </div>
                }
                .into_any();
            };
            let name = if resident.singular.is_empty() {
                id.to_string()
            } else {
                resident.singular.clone()
            };
            let placements = data
                .npc_placements
                .get(&ENpcResidentId(id))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let groups = placements_by_map(placements);
            let shops = shops_for_npc(data, ENpcResidentId(id));
            let item_count: usize = shops.iter().map(|(_, rows)| rows.len()).sum();
            let zone = placements
                .first()
                .map(|p| placement_label(data, p))
                .unwrap_or_default();
            let selected_map = RwSignal::new(groups.first().map(|(map, _)| *map));
            let title_name = name.clone();
            let desc_name = name.clone();
            let desc_zone = zone.clone();
            let group_views = groups.clone();

            view! {
                <MetaTitle title=move || t_string!(i18n, npc_page_title, name = title_name.clone()).to_string() />
                <MetaDescription text=move || {
                    t_string!(i18n, npc_page_desc, name = desc_name.clone(), zone = desc_zone.clone()).to_string()
                } />
                <div class="flex flex-col gap-4">
                    <div class="flex flex-col gap-1">
                        <h1 class="text-2xl font-bold text-brand-100">{name.clone()}</h1>
                        <div class="text-sm text-[color:var(--color-text-muted)]">
                            {if zone.is_empty() {
                                t_string!(i18n, npc_location_unknown).to_string()
                            } else {
                                zone.clone()
                            }}
                        </div>
                    </div>
                    <div class="grid grid-cols-1 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-4">
                        <section class="panel p-4 flex flex-col gap-3 min-w-0">
                            <h2 class="text-lg font-bold text-brand-200">{t!(i18n, npc_location_title)}</h2>
                            {if groups.is_empty() {
                                view! {
                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                        {t!(i18n, npc_location_none_long)}
                                    </p>
                                }.into_any()
                            } else {
                                view! {
                                    <ul class="flex flex-col gap-1 text-sm">
                                        {group_views.iter().flat_map(|(map, list)| {
                                            let map = *map;
                                            list.iter().map(move |p| {
                                                let p = *p;
                                                view! {
                                                    <li>
                                                        <button
                                                            type="button"
                                                            class="text-left hover:underline text-brand-100"
                                                            class:font-bold=move || selected_map.get() == Some(map)
                                                            on:click=move |_| selected_map.set(Some(map))
                                                        >
                                                            {placement_text(data, &p)}
                                                        </button>
                                                        {(p.festival_id != 0).then(|| view! {
                                                            <span class="ml-1 text-xs text-amber-300">{t!(i18n, npc_location_seasonal)}</span>
                                                        })}
                                                    </li>
                                                }
                                            })
                                        }).collect_view()}
                                    </ul>
                                    {move || {
                                        let map_id = selected_map.get()?;
                                        let (_, list) = groups.iter().find(|(m, _)| *m == map_id)?;
                                        let size_factor = data.maps.get(&map_id)?.size_factor;
                                        let pins = list
                                            .iter()
                                            .map(|p| MapPin { x: p.x, y: p.y, label: placement_text(data, p) })
                                            .collect();
                                        Some(view! { <ZoneMap map_id size_factor pins /> })
                                    }}
                                }.into_any()
                            }}
                        </section>
                        <section class="panel p-4 flex flex-col gap-4 min-w-0">
                            <h2 class="text-lg font-bold text-brand-200 flex items-center gap-2">
                                {t!(i18n, npc_sells_title)}
                                <span class="item-detail-count">{item_count}</span>
                            </h2>
                            {if shops.is_empty() {
                                view! {
                                    <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, npc_no_items)}</p>
                                }.into_any()
                            } else {
                                shops.iter().map(|(shop, rows)| {
                                    let fallback = t_string!(i18n, npc_shop_unnamed).to_string();
                                    view! {
                                        <div class="flex flex-col gap-2">
                                            <h3 class="text-sm font-semibold text-[color:var(--color-text-muted)]">
                                                {shop_title(shop, &fallback)}
                                            </h3>
                                            <ul class="flex flex-col divide-y divide-[color:var(--color-outline)]">
                                                {rows.iter().filter_map(|row| {
                                                    let item = data.items.get(&ItemId(row.item))?;
                                                    let item_id = row.item;
                                                    Some(view! {
                                                        <li class="flex items-center gap-3 py-1.5">
                                                            <ItemIcon item_id icon_size=IconSize::Small />
                                                            <AppLink href=format!("/item/{item_id}") attr:class="flex-1 min-w-0 truncate text-brand-100 hover:underline">
                                                                {item.name.clone()}
                                                            </AppLink>
                                                            <VendorGateBadge availability=row.availability />
                                                            <Gil amount=item.price_mid as i32 />
                                                        </li>
                                                    })
                                                }).collect_view()}
                                            </ul>
                                        </div>
                                    }
                                }).collect_view().into_any()
                            }}
                        </section>
                    </div>
                </div>
            }
            .into_any()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shops_and_placements_group_deterministically() {
        let data = tracked_data();
        // Gontrant issues leves but sells nothing; Ilorie sells arms.
        assert!(shops_for_npc(data, ENpcResidentId(1000101)).is_empty());
        let shops = shops_for_npc(data, ENpcResidentId(1000216));
        assert!(!shops.is_empty());
        assert!(shops.windows(2).all(|w| w[0].0.key_id.0 < w[1].0.key_id.0));
        assert!(shops.iter().all(|(_, rows)| !rows.is_empty()));

        let p = |map: i32, fest: u16| NpcPlacement {
            map: MapId(map),
            territory: xiv_gen::TerritoryTypeId(1),
            x: 1.0,
            y: 1.0,
            festival_id: fest,
        };
        let groups = placements_by_map(&[p(9, 3), p(4, 0), p(9, 3), p(4, 0)]);
        let order: Vec<_> = groups.iter().map(|(m, l)| (m.0, l.len())).collect();
        assert_eq!(order, vec![(4, 2), (9, 2)]);
        let groups = placements_by_map(&[p(2, 5), p(7, 0)]);
        assert_eq!(
            groups[0].0,
            MapId(7),
            "a permanent spot outranks a seasonal one"
        );
    }

    #[test]
    fn unnamed_shops_get_the_fallback_title() {
        let shop = xiv_gen::GilShop {
            key_id: xiv_gen::GilShopId(1),
            name: "  ".into(),
            festival_id: 0,
        };
        assert_eq!(shop_title(&shop, "Shop"), "Shop");
        let shop = xiv_gen::GilShop {
            name: "Purchase Arms".into(),
            ..shop
        };
        assert_eq!(shop_title(&shop, "Shop"), "Purchase Arms");
    }
}
