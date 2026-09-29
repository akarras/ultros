//! Browser startup projection. The full archive remains the server/extractor
//! source of truth; descriptions and unreferenced residents load on demand.
use crate::Data;
use std::collections::HashSet;

/// Keep the existing wire layout while removing payloads the startup client
/// does not consume. Retaining the layout lets old full archives remain usable
/// by extraction tools; zeroed fields must not become browser dependencies.
pub fn startup_data(source: &Data) -> Data {
    let mut data = source.clone();
    let mut npcs: HashSet<_> = data
        .gil_shop_npcs
        .values()
        .chain(data.special_shop_npcs.values())
        .chain(data.collectables_shop_npcs.values())
        .chain(data.leve_issuers.values())
        .flatten()
        .copied()
        .collect();
    npcs.extend(data.npc_placements.keys().copied());
    data.e_npc_residents.retain(|id, _| npcs.contains(id));
    for item in data.items.values_mut() {
        item.description.clear();
        item.icon = 0;
    }
    for npc in data.e_npc_residents.values_mut() {
        npc.map = 0;
    }
    for territory in data.territory_types.values_mut() {
        territory.bg.clear();
    }
    for map in data.maps.values_mut() {
        map.offset_x = 0;
        map.offset_y = 0;
        map.place_name_region = 0;
    }
    data.company_craft_drafts.clear();
    data.company_craft_draft_categorys.clear();
    data.company_craft_types.clear();
    for shop in data.special_shops.values_mut() {
        assert_eq!(shop.item, shop.item_receive_0, "shop duplicate diverged");
        shop.item.clear();
    }
    for row in data.leve_reward_items.values_mut() {
        assert_eq!(
            row.leve_reward_item_group,
            [
                row.leve_reward_item_group_0,
                row.leve_reward_item_group_1,
                row.leve_reward_item_group_2,
                row.leve_reward_item_group_3,
                row.leve_reward_item_group_4,
                row.leve_reward_item_group_5,
                row.leve_reward_item_group_6,
                row.leve_reward_item_group_7
            ],
            "leve duplicate diverged"
        );
        row.leve_reward_item_group = [0; 8];
    }
    for row in data.leve_reward_item_groups.values_mut() {
        assert_eq!(
            row.item,
            [
                row.item_0, row.item_1, row.item_2, row.item_3, row.item_4, row.item_5, row.item_6,
                row.item_7, row.item_8
            ],
            "leve item duplicate diverged"
        );
        row.item = [0; 9];
    }
    data
}
