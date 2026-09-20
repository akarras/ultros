//! `/npc/:id`: one NPC — where they stand, on a map, everything they sell
//! for gil, every exchange they run and every collectable they take.
//!
//! Everything on the page comes from the game-data pack: the NPC's shops via
//! the `*_shop_npcs` reverse indexes, their placements via `npc_placements`,
//! and the zone names via `PlaceName`. No market data, so the page is
//! synchronous and renders the same on the server and after hydration.

use std::collections::BTreeMap;

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use xiv_gen::{ENpcResidentId, ItemId, MapId, NpcPlacement};

use crate::components::app_link::AppLink;
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::related_items::VendorGateBadge;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use ultros_ui_game::components::npc_locations::{placement_label, placement_text};
use ultros_ui_game::components::small_item_display::SmallItemDisplay;
use ultros_ui_game::components::zone_map::{MapPin, ZoneMap};

/// The gil shops `npc` offers, in shop-id order, each with its rows.
pub(crate) fn shops_for_npc(
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

/// The special shops (exchanges) `npc` offers, in shop-id order, only those
/// with at least one trade — the sheet carries a few hundred empty rows.
pub(crate) fn exchanges_for_npc(
    data: &'static xiv_gen::Data,
    npc: ENpcResidentId,
) -> Vec<&'static xiv_gen::SpecialShop> {
    let mut shops: Vec<_> = data
        .special_shop_npcs
        .iter()
        .filter(|(_, npcs)| npcs.contains(&npc))
        .filter_map(|(shop_id, _)| data.special_shops.get(shop_id))
        .filter(|shop| shop.entries().next().is_some())
        .collect();
    // `special_shop_npcs` is a HashMap; sort so SSR and hydration agree.
    shops.sort_by_key(|shop| shop.key_id.0);
    shops
}

/// One group of collectable turn-ins on an NPC page: everything a counter
/// takes that pays the same reward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectableGroup {
    /// The scrip paid, or `None` for a material exchange (the sheet that says
    /// what those hand back is not in the pack, so only the intake is shown).
    pub reward: Option<ItemId>,
    /// The shop's own name, for the material exchanges; the scrip counters are
    /// titled by their reward instead (their rows are named in Japanese even
    /// in the English sheet).
    pub shop_name: String,
    /// `(item, scrip amount)`, ascending by item id; the amount is 0 for a
    /// material exchange.
    pub items: Vec<(ItemId, u32)>,
}

/// The collectable turn-ins `npc` accepts, grouped by reward. Scrip counters
/// merge into one group per scrip whatever shop row they came from; each
/// material exchange is its own group, in shop-id order after the scrips.
pub(crate) fn collectables_for_npc(
    data: &'static xiv_gen::Data,
    npc: ENpcResidentId,
) -> Vec<CollectableGroup> {
    let mut shops: Vec<_> = data
        .collectables_shop_npcs
        .iter()
        .filter(|(_, npcs)| npcs.contains(&npc))
        .filter_map(|(shop_id, _)| data.collectables_shops.get(shop_id))
        .collect();
    shops.sort_by_key(|shop| shop.key_id.0);

    let mut by_scrip: BTreeMap<i32, Vec<(ItemId, u32)>> = BTreeMap::new();
    let mut exchanges = Vec::new();
    for shop in shops {
        let mut intake = Vec::new();
        for group in shop.shop_items.iter().filter(|g| **g != 0) {
            let Some(rows) = data
                .collectables_shop_items
                .get(&xiv_gen::CollectablesShopItemId(*group))
            else {
                continue;
            };
            for row in rows {
                if !data.items.contains_key(&ItemId(row.item)) {
                    continue;
                }
                if shop.reward_type != COLLECTABLES_REWARD_SCRIP {
                    intake.push((ItemId(row.item), 0));
                    continue;
                }
                // A scrip counter row with no scrip or paying nothing is a
                // placeholder; the analyzer skips those too.
                let reward = data.collectables_shop_reward_scrips.get(
                    &xiv_gen::CollectablesShopRewardScripId(row.collectables_shop_reward_scrip),
                );
                if let Some(reward) = reward
                    && reward.high_reward > 0
                    && let Some(scrip) = xiv_gen::scrip_item(reward.currency as u32)
                {
                    by_scrip
                        .entry(scrip.0)
                        .or_default()
                        .push((ItemId(row.item), reward.high_reward as u32));
                }
            }
        }
        if !intake.is_empty() {
            intake.sort_unstable_by_key(|(item, _)| item.0);
            intake.dedup();
            exchanges.push(CollectableGroup {
                reward: None,
                shop_name: shop.name.clone(),
                items: intake,
            });
        }
    }
    let mut groups: Vec<_> = by_scrip
        .into_iter()
        .map(|(scrip, mut items)| {
            items.sort_unstable_by_key(|(item, _)| item.0);
            items.dedup();
            CollectableGroup {
                reward: Some(ItemId(scrip)),
                shop_name: String::new(),
                items,
            }
        })
        .collect();
    groups.extend(exchanges);
    groups
}

/// `CollectablesShop.RewardType` of the counters that pay scrip.
const COLLECTABLES_REWARD_SCRIP: i32 = 1;

/// `/scrip-sources?scrip=` filter key for a scrip item, so a counter's group
/// can link to the analyzer pre-filtered to what it pays.
fn scrip_sources_href(scrip: ItemId) -> Option<String> {
    let key = match scrip.0 {
        33913 => "PurpleCrafters",
        33914 => "PurpleGatherers",
        41784 => "OrangeCrafters",
        41785 => "OrangeGatherers",
        _ => return None,
    };
    Some(format!("/scrip-sources?scrip={key}"))
}

/// Above this many shops, an NPC's exchange groups start collapsed and their
/// rows are only rendered once opened: the scrip exchange offers 49 shops of
/// up to 60 trades, which is not a page anyone scrolls.
const OPEN_BY_DEFAULT_UP_TO: usize = 3;

/// A titled, collapsible group whose body is rendered only while open.
///
/// Closed groups ship no rows in the SSR HTML and build none on hydration;
/// the `toggle` event copies the element's own `open` state into the signal
/// (rather than flipping it), so a click that lands before hydration cannot
/// leave the box open with no rows in it.
#[component]
fn CollapsibleGroup(
    title: String,
    count: usize,
    open_by_default: bool,
    children: ChildrenFn,
) -> impl IntoView {
    let open = RwSignal::new(open_by_default);
    view! {
        <details
            class="group flex flex-col gap-2"
            open=open_by_default
            on:toggle=move |ev| {
                let is_open = event_target::<web_sys::Element>(&ev).has_attribute("open");
                if open.get_untracked() != is_open {
                    open.set(is_open);
                }
            }
        >
            <summary class="flex cursor-pointer list-none items-center gap-2 text-sm font-semibold text-[color:var(--color-text-muted)] hover:text-brand-200">
                <Icon icon=icondata::BiChevronDownRegular attr:class="shrink-0 transition-transform group-open:rotate-180" />
                <span class="truncate">{title}</span>
                <span class="item-detail-count">{count}</span>
            </summary>
            {move || open.get().then(|| children())}
        </details>
    }
}

/// `250 ×` followed by the item, as the item page's exchange cards draw it.
#[component]
fn CostChip(item_id: ItemId, count: u32) -> impl IntoView {
    let data = tracked_data();
    let item = data.items.get(&item_id)?;
    Some(view! {
        <div class="flex min-w-0 items-center gap-1 px-2 py-1 rounded border border-[color:var(--color-outline)]">
            <span class="shrink-0 whitespace-nowrap font-bold text-brand-200">{count} "×"</span>
            <SmallItemDisplay item />
        </div>
    })
}

/// One trade of a special shop: what you get, then what it costs.
#[component]
fn ExchangeRow(entry: xiv_gen::SpecialShopEntry) -> impl IntoView {
    let data = tracked_data();
    view! {
        <li class="flex flex-col sm:flex-row sm:items-center gap-2 py-1.5">
            <div class="flex flex-col gap-1 flex-1 min-w-0">
                {entry.receive.into_iter().filter_map(|(item_id, count)| {
                    let item = data.items.get(&item_id)?;
                    Some(view! {
                        <div class="flex items-center gap-3 min-w-0">
                            <ItemIcon item_id=item_id.0 icon_size=IconSize::Small />
                            <AppLink href=format!("/item/{}", item_id.0) attr:class="min-w-0 truncate text-brand-100 hover:underline">
                                {item.name.clone()}
                            </AppLink>
                            {(count > 1).then(|| view! {
                                <span class="shrink-0 text-xs text-[color:var(--color-text-muted)]">"×" {count}</span>
                            })}
                        </div>
                    })
                }).collect_view()}
            </div>
            <div class="flex flex-wrap items-center gap-1 text-xs text-[color:var(--color-text-muted)] sm:justify-end">
                {entry.cost.into_iter().map(|(item_id, count)| view! { <CostChip item_id count /> }).collect_view()}
            </div>
        </li>
    }
}

#[component]
fn ExchangesSection(shops: Vec<&'static xiv_gen::SpecialShop>) -> impl IntoView {
    let i18n = use_i18n();
    let trade_count: usize = shops.iter().map(|shop| shop.entries().count()).sum();
    let open_by_default = shops.len() <= OPEN_BY_DEFAULT_UP_TO;
    view! {
        <section class="panel p-4 flex flex-col gap-4 min-w-0" data-testid="npc-exchanges">
            <h2 class="text-lg font-bold text-brand-200 flex items-center gap-2">
                <Icon icon=icondata::BsArrowLeftRight attr:class="text-brand-300" />
                {t!(i18n, npc_exchanges_title)}
                <span class="item-detail-count">{trade_count}</span>
            </h2>
            {shops.into_iter().map(|shop| {
                let fallback = t_string!(i18n, npc_shop_unnamed).to_string();
                let entries: Vec<_> = shop.entries().collect();
                let count = entries.len();
                let entries = StoredValue::new(entries);
                view! {
                    <CollapsibleGroup title=exchange_title(shop, &fallback) count open_by_default>
                        <ul class="flex flex-col divide-y divide-[color:var(--color-outline)] pl-6">
                            {entries.with_value(|entries| entries.iter().cloned().map(|entry| view! { <ExchangeRow entry /> }).collect_view())}
                        </ul>
                    </CollapsibleGroup>
                }
            }).collect_view()}
        </section>
    }
}

/// A special shop's name as the game gives it, else the generic fallback.
fn exchange_title(shop: &xiv_gen::SpecialShop, fallback: &str) -> String {
    if shop.name.trim().is_empty() {
        fallback.to_string()
    } else {
        shop.name.clone()
    }
}

#[component]
fn CollectablesSection(groups: Vec<CollectableGroup>) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let item_count: usize = groups.iter().map(|g| g.items.len()).sum();
    let open_by_default = groups.len() <= OPEN_BY_DEFAULT_UP_TO;
    view! {
        <section class="panel p-4 flex flex-col gap-4 min-w-0" data-testid="npc-collectables">
            <h2 class="text-lg font-bold text-brand-200 flex items-center gap-2">
                <Icon icon=icondata::FaHandHoldingSolid attr:class="text-brand-300" />
                {t!(i18n, npc_collectables_title)}
                <span class="item-detail-count">{item_count}</span>
            </h2>
            {groups.into_iter().map(|group| {
                let fallback = t_string!(i18n, npc_shop_unnamed).to_string();
                let title = match group.reward.and_then(|r| data.items.get(&r)) {
                    Some(scrip) => scrip.name.clone(),
                    None if group.shop_name.trim().is_empty() => fallback,
                    None => group.shop_name.clone(),
                };
                let count = group.items.len();
                let reward = group.reward;
                let items = StoredValue::new(group.items);
                view! {
                    <CollapsibleGroup title count open_by_default>
                        {reward.and_then(scrip_sources_href).map(|href| view! {
                            <AppLink href attr:class="ml-6 text-xs text-brand-300 hover:underline">
                                {t!(i18n, npc_collectables_scrip_sources_link)}
                            </AppLink>
                        })}
                        <ul class="flex flex-col divide-y divide-[color:var(--color-outline)] pl-6">
                            {items.with_value(|items| items.iter().copied().filter_map(|(item_id, amount)| {
                                let item = data.items.get(&item_id)?;
                                Some(view! {
                                    <li class="flex items-center gap-3 py-1.5">
                                        <ItemIcon item_id=item_id.0 icon_size=IconSize::Small />
                                        <AppLink href=format!("/item/{}", item_id.0) attr:class="flex-1 min-w-0 truncate text-brand-100 hover:underline">
                                            {item.name.clone()}
                                        </AppLink>
                                        {reward.map(|scrip| view! { <CostChip item_id=scrip count=amount /> })}
                                    </li>
                                })
                            }).collect_view())}
                        </ul>
                    </CollapsibleGroup>
                }
            }).collect_view()}
        </section>
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
            let exchanges = exchanges_for_npc(data, ENpcResidentId(id));
            let collectables = collectables_for_npc(data, ENpcResidentId(id));
            // The gil panel is the one place an NPC with nothing at all says
            // so; an exchange-only NPC does not need "no gil shop" beside its
            // exchanges.
            let show_gil = !shops.is_empty() || (exchanges.is_empty() && collectables.is_empty());
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
                        <div class="flex flex-col gap-4 min-w-0">
                            {show_gil.then(|| view! {
                            <section class="panel p-4 flex flex-col gap-4 min-w-0" data-testid="npc-sells">
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
                            })}
                            {(!exchanges.is_empty()).then(|| view! { <ExchangesSection shops=exchanges.clone() /> })}
                            {(!collectables.is_empty()).then(|| view! { <CollectablesSection groups=collectables.clone() /> })}
                        </div>
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
    fn exchange_and_collectable_groups_come_from_the_pack() {
        let data = tracked_data();
        // The Mor Dhona scrip exchange: 49 tabbed shops behind an InclusionShop.
        let scrip_exchange = exchanges_for_npc(data, ENpcResidentId(1001617));
        assert!(scrip_exchange.len() > 40, "{}", scrip_exchange.len());
        assert!(
            scrip_exchange
                .windows(2)
                .all(|w| w[0].key_id.0 < w[1].key_id.0)
        );
        assert!(scrip_exchange.iter().all(|s| s.entries().next().is_some()));
        let purple = scrip_exchange
            .iter()
            .find(|s| s.key_id.0 == 1770488)
            .expect("Purple Scrip Exchange (Lv. 90 Materials)");
        assert!(
            purple
                .entries()
                .any(|e| e.cost.iter().any(|(item, n)| item.0 == 33913 && *n == 250))
        );
        // Ilorie sells arms for gil and runs no exchange.
        assert!(exchanges_for_npc(data, ENpcResidentId(1000216)).is_empty());

        // A Collectable Appraiser: one group per scrip, ascending, all sorted.
        let appraiser = collectables_for_npc(data, ENpcResidentId(1001616));
        let scrips: Vec<_> = appraiser
            .iter()
            .filter_map(|g| g.reward)
            .map(|r| r.0)
            .collect();
        assert_eq!(scrips, vec![33913, 33914, 41784, 41785]);
        for group in &appraiser {
            assert!(!group.items.is_empty());
            assert!(group.items.iter().all(|(_, amount)| *amount > 0));
            assert!(group.items.windows(2).all(|w| w[0].0.0 < w[1].0.0));
        }
        assert!(appraiser.iter().map(|g| g.items.len()).sum::<usize>() > 1000);
        // Limbeth trades materials back, and the intake is shown under the
        // shop's own name with no scrip.
        let limbeth = collectables_for_npc(data, ENpcResidentId(1027566));
        assert!(!limbeth.is_empty());
        assert!(limbeth.iter().all(|g| g.reward.is_none()));
        assert!(
            limbeth
                .iter()
                .any(|g| g.shop_name == "Resplendent Materials Exchange"),
            "{:?}",
            limbeth.iter().map(|g| &g.shop_name).collect::<Vec<_>>()
        );
        assert!(collectables_for_npc(data, ENpcResidentId(1000216)).is_empty());
        assert_eq!(
            scrip_sources_href(ItemId(41785)).as_deref(),
            Some("/scrip-sources?scrip=OrangeGatherers")
        );
        assert_eq!(scrip_sources_href(ItemId(1)), None);
    }

    /// Closed groups ship no rows: the scrip exchange's 49 shops would
    /// otherwise put ~1,500 trades into every page load.
    #[test]
    fn many_shops_render_collapsed_and_few_render_open() {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<Locale>());
            // The cost chips read the price-zone cookie through the request.
            provide_context(axum::http::Request::new(()).into_parts().0);
            provide_context(crate::global_state::cookies::Cookies::new());
            let data = tracked_data();
            let many = exchanges_for_npc(data, ENpcResidentId(1001617));
            let html = view! { <ExchangesSection shops=many.clone() /> }.to_html();
            assert!(html.contains("Purple Scrip Exchange"), "{html}");
            assert!(!html.contains("<details open"), "{html}");
            assert!(
                !html.contains("href=\"/item/39595\""),
                "closed groups render no rows"
            );
            assert_eq!(html, view! { <ExchangesSection shops=many /> }.to_html());

            // A single shop: open, with the trade rows and their cost chips.
            let few: Vec<_> = exchanges_for_npc(data, ENpcResidentId(1001617))
                .into_iter()
                .filter(|s| s.key_id.0 == 1770488)
                .collect();
            let html = view! { <ExchangesSection shops=few /> }.to_html();
            assert!(html.contains("<details open"), "{html}");
            assert!(html.contains("href=\"/item/39595\""), "{html}");
            assert!(
                html.contains("Purple Crafters&#x27; Scrip")
                    || html.contains("Purple Crafters' Scrip"),
                "{html}"
            );
            assert!(!html.contains("Fire Shard"), "{html}");

            let appraiser = collectables_for_npc(data, ENpcResidentId(1001616));
            let html = view! { <CollectablesSection groups=appraiser /> }.to_html();
            assert!(html.contains("Orange Gatherers"), "{html}");
            assert!(!html.contains("<details open"), "{html}");
        });
    }

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
