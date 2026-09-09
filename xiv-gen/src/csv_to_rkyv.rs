/// Contains all the code needed to read a csv file and produce a `Data` struct
/// ready to be serialized (e.g. with rkyv).
/// Recommended to just let xiv-gen-db handle this unless you need a different backing store.
use crate::*;
use std::collections::HashMap;
use std::path::Path;

/// Reads `lang` from the `ffxiv-datamining` checkout vendored next to this crate.
pub fn read_data(lang: Language) -> Data {
    read_data_from(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/ffxiv-datamining")),
        lang,
    )
}

/// Reads `lang` from an ffxiv-datamining tree rooted at `root`, i.e. a directory
/// containing `csv/<lang>/*.csv`.
pub fn read_data_from(root: &Path, lang: Language) -> Data {
    let base_path = match lang {
        // The ko fork keeps its CSVs one level deeper than its siblings do.
        Language::Ko => root.join("csv").join("ko").join("csv"),
        _ => root.join("csv").join(lang.to_path_part()),
    };
    let base_path = format!("{}/", base_path.display());
    let e_npc_residents: HashMap<ENpcResidentId, ENpcResident> =
        read_csv_to_map(&format!("{}ENpcResident.csv", base_path));
    let gil_shops: HashMap<GilShopId, GilShop> =
        read_csv_to_map(&format!("{}GilShop.csv", base_path));
    // Read once to build `gil_shop_npcs`, then drop: these three sheets exist
    // only to answer "which NPCs offer this shop?", and `ENpcBase` is by far
    // the largest table in the set.
    let gil_shop_npcs = build_gil_shop_npcs(
        &read_csv_vec::<ENpcBase>(&format!("{}ENpcBase.csv", base_path)),
        &read_csv_to_map(&format!("{}TopicSelect.csv", base_path)),
        &read_csv_to_map(&format!("{}PreHandler.csv", base_path)),
        &e_npc_residents,
        &gil_shops,
    );
    Data {
        items: read_csv_to_map(&format!("{}Item.csv", base_path)),
        recipes: read_csv_to_map(&format!("{}Recipe.csv", base_path)),
        class_jobs: read_csv_to_map(&format!("{}ClassJob.csv", base_path)),
        class_job_categorys: read_csv_to_map(&format!("{}ClassJobCategory.csv", base_path)),
        base_params: read_csv_to_map(&format!("{}BaseParam.csv", base_path)),
        special_shops: read_csv_to_map(&format!("{}SpecialShop.csv", base_path)),
        leves: read_csv_to_map(&format!("{}Leve.csv", base_path)),
        leve_reward_items: read_csv_to_map(&format!("{}LeveRewardItem.csv", base_path)),
        leve_reward_item_groups: read_csv_to_map(&format!("{}LeveRewardItemGroup.csv", base_path)),
        e_npc_residents,
        gil_shops,
        gil_shop_items: read_csv_vec::<GilShopItem>(&format!("{}GilShopItem.csv", base_path))
            .into_iter()
            .fold(HashMap::new(), |mut map, m| {
                map.entry(m.key_id.0).or_default().push(m);
                map
            }),
        gil_shop_npcs,
        item_search_categorys: read_csv_to_map(&format!("{}ItemSearchCategory.csv", base_path)),
        item_ui_categorys: read_csv_to_map(&format!("{}ItemUICategory.csv", base_path)),
        item_sort_categorys: read_csv_to_map(&format!("{}ItemSortCategory.csv", base_path)),
        company_craft_sequences: read_csv_to_map(&format!("{}CompanyCraftSequence.csv", base_path)),
        company_craft_parts: read_csv_to_map(&format!("{}CompanyCraftPart.csv", base_path)),
        company_craft_processs: read_csv_to_map(&format!("{}CompanyCraftProcess.csv", base_path)),
        company_craft_supply_items: read_csv_to_map(&format!(
            "{}CompanyCraftSupplyItem.csv",
            base_path
        )),
        company_craft_draft_categorys: read_csv_to_map(&format!(
            "{}CompanyCraftDraftCategory.csv",
            base_path
        )),
        company_craft_types: read_csv_to_map(&format!("{}CompanyCraftType.csv", base_path)),
        company_craft_drafts: read_csv_to_map(&format!("{}CompanyCraftDraft.csv", base_path)),
        retainer_tasks: read_csv_to_map(&format!("{}RetainerTask.csv", base_path)),
        retainer_task_normals: read_csv_to_map(&format!("{}RetainerTaskNormal.csv", base_path)),
        recipe_level_tables: read_csv_to_map(&format!("{}RecipeLevelTable.csv", base_path)),
        collectables_shops: read_csv_to_map(&format!("{}CollectablesShop.csv", base_path)),
        collectables_shop_items: read_csv_vec::<CollectablesShopItem>(&format!(
            "{}CollectablesShopItem.csv",
            base_path
        ))
        .into_iter()
        .fold(HashMap::new(), |mut map, m| {
            map.entry(CollectablesShopItemId(m.key_id.0))
                .or_default()
                .push(m);
            map
        }),
        collectables_shop_reward_scrips: read_csv_to_map(&format!(
            "{}CollectablesShopRewardScrip.csv",
            base_path
        )),
        craft_leves: read_csv_to_map(&format!("{}CraftLeve.csv", base_path)),
    }
}

/// Invert `ENpcBase.ENpcData` into `gil_shop -> npcs`.
///
/// An NPC's data slot reaches a shop three ways, all of which the runtime scan
/// this replaces also followed: the slot *is* the shop id, the slot names a
/// `TopicSelect` whose `Shop` list contains it, or the slot names a
/// `PreHandler` whose target is such a `TopicSelect`. A slot is checked against
/// all three, so an NPC that reaches the same shop by two routes is recorded
/// once per route — matching the previous behavior, which likewise pushed one
/// entry per matching route.
///
/// An NPC data slot holds ids from many sheets (quests, events, shops of other
/// kinds), so every candidate is checked against `gil_shops` before it is
/// recorded. Without that filter the index would gain a key for nearly every
/// distinct slot value across ~60k NPCs — larger than the sheet it replaces.
fn build_gil_shop_npcs(
    npc_bases: &[ENpcBase],
    topic_selects: &HashMap<TopicSelectId, TopicSelect>,
    pre_handlers: &HashMap<PreHandlerId, PreHandler>,
    residents: &HashMap<ENpcResidentId, ENpcResident>,
    gil_shops: &HashMap<GilShopId, GilShop>,
) -> HashMap<GilShopId, Vec<ENpcResidentId>> {
    let mut map: HashMap<GilShopId, Vec<ENpcResidentId>> = HashMap::new();
    for npc in npc_bases {
        let resident = ENpcResidentId(npc.key_id.0);
        // An NPC with no resident row has no name or location to render, so it
        // could never appear as a vendor source.
        if !residents.contains_key(&resident) {
            continue;
        }
        for row in npc.e_npc_data.iter().copied() {
            let row = row as i32;
            let mut push = |shop: i32| {
                let shop = GilShopId(shop);
                if gil_shops.contains_key(&shop) {
                    map.entry(shop).or_default().push(resident);
                }
            };
            push(row);
            if let Some(ts) = topic_selects.get(&TopicSelectId(row)) {
                ts.shop.iter().copied().for_each(&mut push);
            }
            if let Some(ph) = pre_handlers.get(&PreHandlerId(row))
                && let Some(ts) = topic_selects.get(&TopicSelectId(ph.target))
            {
                ts.shop.iter().copied().for_each(&mut push);
            }
        }
    }
    // `npc_bases` arrives in CSV order, which is already stable, but sorting
    // makes the pack independent of upstream row ordering and lets the vendor
    // panel render the same sequence on the server and after hydration.
    for npcs in map.values_mut() {
        npcs.sort_unstable_by_key(|n| n.0);
    }
    map
}

fn read_csv_vec<T: FromCsv>(path: &str) -> Vec<T> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .unwrap_or_else(|_| panic!("Failed to open csv at {}", path));
    let mut records = reader.records();

    let first_row = records.next().expect("Missing header").unwrap();
    let mut header_row = first_row.clone();

    if first_row.get(0) == Some("key") {
        // SaintCoinach format (CN/KO/TC)
        header_row = records.next().expect("Missing second header row").unwrap();
        // Skip the type row
        let _ = records.next();
    }

    let header: Vec<String> = header_row
        .iter()
        .map(|s| {
            let mut s = s
                .replace("{", "")
                .replace("}", "")
                .replace("<%>", "Percent")
                .replace("ItemIngredient", "Ingredient");

            if let Some((slot, item_index)) = split_multi_indexed_column(&s, "ItemReceive[") {
                s = format!("Item[{slot}].Item[{item_index}]");
            } else if let Some((slot, item_index)) = split_multi_indexed_column(&s, "CountReceive[")
            {
                s = format!("Item[{slot}].ReceiveCount[{item_index}]");
            } else if let Some((slot, item_index)) = split_multi_indexed_column(&s, "ItemCost[") {
                s = format!("Item[{slot}].ItemCost[{item_index}]");
            } else if let Some((slot, item_index)) = split_multi_indexed_column(&s, "CountCost[") {
                s = format!("Item[{slot}].CurrencyCost[{item_index}]");
            }
            s
        })
        .collect();

    records
        .map(|r| T::from_csv_row(&header, &r.unwrap()))
        .collect()
}

fn split_multi_indexed_column<'a>(column: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let indexes = column.strip_prefix(prefix)?.strip_suffix(']')?;
    indexes.split_once("][")
}

fn read_csv_to_map<K, T>(path: &str) -> HashMap<K, T>
where
    T: FromCsv + HasId<Id = K>,
    K: std::hash::Hash + Eq,
{
    read_csv_vec::<T>(path)
        .into_iter()
        .map(|item| (item.get_id(), item))
        .collect()
}
