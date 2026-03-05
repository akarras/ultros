/// Contains all the code needed to read a csv file and save it to a .bincode database
/// Recommended to just let xiv-gen-db handle this unless you need a different backing store.
use crate::*;
use std::collections::HashMap;

pub fn read_data(lang: Language) -> Data {
    let base_path = match lang {
        Language::Ko => format!(
            "{}/ffxiv-datamining/csv/ko/csv/",
            env!("CARGO_MANIFEST_DIR")
        ),
        _ => format!(
            "{}/ffxiv-datamining/csv/{}/",
            env!("CARGO_MANIFEST_DIR"),
            lang.to_path_part()
        ),
    };
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
        e_npc_bases: read_csv_to_map(&format!("{}ENpcBase.csv", base_path)),
        e_npc_residents: read_csv_to_map(&format!("{}ENpcResident.csv", base_path)),
        gil_shops: read_csv_to_map(&format!("{}GilShop.csv", base_path)),
        gil_shop_items: read_csv_vec::<GilShopItem>(&format!("{}GilShopItem.csv", base_path))
            .into_iter()
            .fold(HashMap::new(), |mut map, m| {
                map.entry(m.key_id.0).or_default().push(m);
                map
            }),
        topic_selects: read_csv_to_map(&format!("{}TopicSelect.csv", base_path)),
        pre_handlers: read_csv_to_map(&format!("{}PreHandler.csv", base_path)),
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

            // Map SaintCoinach's Item{Receive}[x][0] to English's Item[x].Item[0]
            if s.starts_with("ItemReceive[") && s.ends_with("][0]") {
                let num = &s[12..s.len() - 4];
                s = format!("Item[{}].Item[0]", num);
            }
            s
        })
        .collect();

    records
        .map(|r| T::from_csv_row(&header, &r.unwrap()))
        .collect()
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
