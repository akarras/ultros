//! Game-data record sets shared by the pages that display them and the
//! search index that indexes them (`ultros-search/src/lib.rs`).
//!
//! Its own crate so the server's search index can use it without depending
//! on the Leptos app.
//!
//! Pure functions over `&xiv_gen::Data`, no Leptos: the indexer runs on the
//! server at startup and the pages run in the browser, and both must agree
//! on what a currency, a venture reward or a scrip turn-in *is*. The currency
//! search was wrong for a long time precisely because the indexer re-derived
//! the page's logic instead of calling it.

use xiv_gen::{
    ClassJobCategoryId, Data, ENpcResidentId, ItemId, ItemUiCategoryId, MapId, NpcPlacement,
    PlaceNameId, RetainerTaskId, RetainerTaskNormalId, TerritoryTypeId,
};

/// `ItemUICategory` rows a special-shop cost can sit in and still count as a
/// currency: Currency = 100, Miscellany = 61, Other = 63. Row ids are stable
/// across game locales; the `name` column is not (GlitchTip #6849).
pub const CURRENCY_UI_CATEGORIES: [ItemUiCategoryId; 3] = [
    ItemUiCategoryId(100),
    ItemUiCategoryId(61),
    ItemUiCategoryId(63),
];

/// Items the special shops take as payment for something marketable — the
/// set the Currency Exchange landing page lists.
///
/// Walks every special-shop row, keeps rows whose *received* item is
/// marketable, and collects the *cost* items of those rows (three cost slots
/// per row). `SpecialShop.item` is the received item, not the cost — reading
/// it here is how the search index once listed received items as currencies
/// and never found "Wolf Mark".
pub fn exchange_currencies(data: &Data) -> Vec<ItemId> {
    let marketable = |id: u16| {
        id != 0
            && data
                .items
                .get(&ItemId(id as i32))
                .is_some_and(|item| item.item_search_category != 0)
    };
    let mut currencies: Vec<ItemId> = data
        .special_shops
        .values()
        .flat_map(|shop| {
            (0..shop.item_receive_0.len()).flat_map(move |row| {
                let received = |items: &[u16], counts: &[u32]| {
                    items
                        .get(row)
                        .zip(counts.get(row))
                        .is_some_and(|(&id, &count)| count != 0 && marketable(id))
                };
                let sells_marketable = received(&shop.item_receive_0, &shop.count_receive_0)
                    || received(&shop.item_receive_1, &shop.count_receive_1);
                let costs = [
                    (shop.item_cost_0.get(row), shop.count_cost_0.get(row)),
                    (shop.item_cost_1.get(row), shop.count_cost_1.get(row)),
                    (shop.item_cost_2.get(row), shop.count_cost_2.get(row)),
                ];
                costs.into_iter().filter_map(move |(item, count)| {
                    let (&item, &count) = (item?, count?);
                    (sells_marketable && item != 0 && count != 0).then_some(ItemId(item as i32))
                })
            })
        })
        .filter(|id| {
            data.items.get(id).is_some_and(|item| {
                CURRENCY_UI_CATEGORIES.contains(&ItemUiCategoryId(item.item_ui_category))
                    && item.name != "Gil"
                    && item.name != "MGP"
            })
        })
        .collect();
    currencies.sort_by_key(|id| id.0);
    currencies.dedup();
    currencies
}

mod scrip;

pub use scrip::{ScripTurnIn, ScripType, scrip_turn_ins};

/// The scrip currency item for `scrip`, matched by name inside the Currency
/// UI category (the only place the exact name is unambiguous).
pub fn scrip_item(data: &Data, scrip: ScripType) -> Option<ItemId> {
    let name = scrip.item_name()?;
    data.items
        .values()
        .filter(|item| ItemUiCategoryId(item.item_ui_category) == ItemUiCategoryId(100))
        .find(|item| item.name == name)
        .map(|item| item.key_id)
}

/// One non-random retainer venture and what it brings back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VentureReward {
    pub task: RetainerTaskId,
    pub item: ItemId,
    /// Base quantity (`RetainerTaskNormal.Quantity[0]`).
    pub quantity: i32,
    pub level: u8,
    pub class_job_category: ClassJobCategoryId,
}

/// Every fixed-reward venture, sorted by task id so callers and SSR agree on
/// order. Random ventures (quick explorations) have no fixed item and are
/// skipped, as are placeholder rows with no item or quantity.
pub fn venture_rewards(data: &Data) -> Vec<VentureReward> {
    let mut rewards: Vec<VentureReward> = data
        .retainer_tasks
        .iter()
        .filter(|(_, task)| !task.is_random)
        .filter_map(|(id, task)| {
            let normal = data
                .retainer_task_normals
                .get(&RetainerTaskNormalId(task.task))?;
            if normal.item == 0 || normal.quantity_0 == 0 {
                return None;
            }
            data.items.get(&ItemId(normal.item))?;
            Some(VentureReward {
                task: *id,
                item: ItemId(normal.item),
                quantity: normal.quantity_0,
                level: task.retainer_level,
                class_job_category: ClassJobCategoryId(task.class_job_category),
            })
        })
        .collect();
    rewards.sort_by_key(|r| r.task.0);
    rewards
}

/// Every NPC `/npc/:id` has a page for — a gil shop, an exchange (special
/// shop) or a collectables counter, the same three indexes the sitemap
/// walks — sorted and deduplicated so callers and SSR agree on order.
pub fn shop_npcs(data: &Data) -> Vec<ENpcResidentId> {
    let mut ids: Vec<ENpcResidentId> = data
        .gil_shop_npcs
        .values()
        .chain(data.special_shop_npcs.values())
        .chain(data.collectables_shop_npcs.values())
        .flatten()
        .copied()
        .collect();
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

/// Zone label for a placement in the current locale: the territory's place
/// name, plus the map's sub-area when it has one (`Ul'dah - Steps of Thal ·
/// Merchant Strip`).
pub fn placement_label(data: &Data, placement: &NpcPlacement) -> String {
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

/// Zone of the NPC's first placement, as the NPC page labels it, or empty
/// when the client layout files place it nowhere.
pub fn npc_zone(data: &Data, npc: ENpcResidentId) -> String {
    data.npc_placements
        .get(&npc)
        .and_then(|placements| placements.first())
        .map(|placement| placement_label(data, placement))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> &'static Data {
        xiv_gen_db::data()
    }

    fn name(id: ItemId) -> &'static str {
        data().items.get(&id).map(|i| i.name.as_str()).unwrap_or("")
    }

    #[test]
    fn exchange_currencies_are_the_cost_side_not_the_received_side() {
        let currencies = exchange_currencies(data());
        let names: Vec<_> = currencies.iter().map(|id| name(*id)).collect();
        // Paid in special shops that sell marketable goods.
        assert!(names.contains(&"Wolf Mark"), "{names:?}");
        assert!(names.contains(&"Fae Fancy"), "{names:?}");
        // Received from a special shop and never spent in one: a mount from a
        // MGP shop is an item, not a currency, whatever category it sits in.
        assert!(!names.contains(&"Fenrir Horn"), "{names:?}");
    }

    #[test]
    fn exchange_currencies_exclude_gil_and_mgp_and_are_unique_sorted() {
        let currencies = exchange_currencies(data());
        let names: Vec<_> = currencies.iter().map(|id| name(*id)).collect();
        assert!(!names.contains(&"Gil"));
        assert!(!names.contains(&"MGP"));
        let mut sorted = currencies.clone();
        sorted.sort_by_key(|id| id.0);
        sorted.dedup();
        assert_eq!(currencies, sorted);
    }

    #[test]
    fn venture_rewards_skip_random_and_empty_tasks_and_are_sorted() {
        let rewards = venture_rewards(data());
        assert!(!rewards.is_empty());
        for reward in &rewards {
            let task = &data().retainer_tasks[&reward.task];
            assert!(!task.is_random, "random task {} listed", reward.task.0);
            assert!(reward.item.0 != 0 && reward.quantity != 0);
            assert!(data().items.contains_key(&reward.item));
        }
        let ids: Vec<_> = rewards.iter().map(|r| r.task.0).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn a_known_hunting_venture_is_listed() {
        // Aldgoat Skin is a classic hunting venture reward.
        let rewards = venture_rewards(data());
        assert!(rewards.iter().any(|r| name(r.item) == "Aldgoat Skin"));
    }

    #[test]
    fn filter_key_round_trips() {
        for scrip in [
            ScripType::OrangeCrafters,
            ScripType::OrangeGatherers,
            ScripType::PurpleCrafters,
            ScripType::PurpleGatherers,
            ScripType::WhiteCrafters,
            ScripType::WhiteGatherers,
        ] {
            let key = scrip.filter_key().expect("named scrip has a key");
            assert_eq!(ScripType::from_filter_key(key), Some(scrip));
        }
        assert_eq!(ScripType::Other(9).filter_key(), None);
    }

    #[test]
    fn every_live_crafter_scrip_resolves_to_one_currency_item() {
        for scrip in [ScripType::OrangeCrafters, ScripType::PurpleCrafters] {
            let id = scrip_item(data(), scrip).expect("scrip item exists");
            let item = &data().items[&id];
            assert_eq!(Some(item.name.as_str()), scrip.item_name());
            assert_eq!(item.item_ui_category, 100);
        }
        assert_eq!(scrip_item(data(), ScripType::Other(9)), None);
    }

    #[test]
    fn crafted_turn_ins_exist_and_gatherer_ones_are_marked() {
        let turn_ins = scrip_turn_ins(data());
        assert!(turn_ins.iter().any(|t| !t.scrip_type.is_gatherer()));
        assert!(turn_ins.iter().any(|t| t.scrip_type.is_gatherer()));
    }

    #[test]
    fn shop_npcs_are_unique_sorted_and_named() {
        let npcs = shop_npcs(data());
        assert!(!npcs.is_empty());
        let mut sorted = npcs.clone();
        sorted.sort_by_key(|id| id.0);
        sorted.dedup();
        assert_eq!(npcs, sorted);
        assert!(npcs.iter().any(|id| !npc_zone(data(), *id).is_empty()));
    }
}
