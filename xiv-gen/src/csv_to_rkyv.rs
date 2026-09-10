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
    // Everything feeding `availability` is read from the English tree, whatever
    // `lang` is: gates are ids (shops, quests, achievements, festivals) and an
    // id means the same thing in every language, while only names are
    // translated. That is not just an optimization — the CN/KO/TC forks ship
    // the SaintCoinach header layout, in which `GilShop.FestivalId` and the
    // `GilShopItem` gate columns have no names to match, so reading them
    // per-locale would classify every seasonal shop as `Open` for those three.
    // It also keeps ~34MB of `Quest.csv` from being parsed six extra times.
    let en_path = format!("{}/", root.join("csv").join("en").display());
    let gates = GateIndex {
        achievement_category: read_csv_vec::<AchievementCategoryOf>(&format!(
            "{en_path}Achievement.csv"
        ))
        .into_iter()
        .map(|a| (a.key_id, a.achievement_category))
        .collect(),
        quest_genre: read_csv_vec::<QuestGenreOf>(&format!("{en_path}Quest.csv"))
            .into_iter()
            .map(|q| (q.key_id, q.journal_genre))
            .collect(),
    };
    let shop_gates: HashMap<GilShopId, GilShopGates> =
        read_csv_vec::<GilShopGates>(&format!("{en_path}GilShop.csv"))
            .into_iter()
            .map(|s| (s.key_id, s))
            .collect();
    // Keyed by (shop, item) rather than the row's own (shop, subrow): the
    // subrow index is a position, and the per-language forks are cut from
    // different game versions, so position N need not be the same product in
    // each. No (shop, item) pair carries conflicting gates upstream, which
    // makes the product key both unambiguous and stable across versions.
    let item_gates: HashMap<(GilShopId, i32), GilShopItemGates> =
        read_csv_vec::<GilShopItemGates>(&format!("{en_path}GilShopItem.csv"))
            .into_iter()
            .map(|i| ((i.key_id.0, i.item), i))
            .collect();
    let e_npc_residents: HashMap<ENpcResidentId, ENpcResident> =
        read_csv_to_map(&format!("{}ENpcResident.csv", base_path));
    // Names come from `lang`; the festival id is stamped on from the English
    // gates, for the header-layout reason above.
    let mut gil_shops: HashMap<GilShopId, GilShop> =
        read_csv_to_map(&format!("{}GilShop.csv", base_path));
    for (id, shop) in gil_shops.iter_mut() {
        shop.festival_id = shop_gates.get(id).map_or(0, |g| g.festival_id);
    }
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
            .fold(HashMap::new(), |mut map, mut m| {
                if let Some(item) = item_gates.get(&(m.key_id.0, m.item)) {
                    m.availability =
                        classify_availability(shop_gates.get(&m.key_id.0), item, &gates);
                }
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

/// `AchievementCategory` rows that mean "seasonal event". Two categories share
/// the name; both are event achievements (`Horsing About`, `Cold as Ice`).
///
/// Ids, not names: `AchievementCategory.Name` is localized, and this runs once
/// per locale. Re-derive with
/// `grep -n 'Seasonal' csv/en/AchievementCategory.csv`.
const SEASONAL_ACHIEVEMENT_CATEGORIES: [i32; 2] = [38, 58];

/// `JournalGenre` rows for seasonal events — a contiguous block covering
/// `Seasonal Events` through `Other Seasonal Events`, taking in Heavensturn,
/// Valentione's, Little Ladies' Day, Hatching-tide, Moonfire Faire, the Rising,
/// All Saints' Wake and the Starlight Celebration.
///
/// Ids again, for the same reason. Re-derive with
/// `grep -n 'Event' csv/en/JournalGenre.csv`; a new event genre appended
/// outside this range will read as [`VendorAvailability::Unlockable`] until the
/// range is widened, which fails toward showing an item rather than hiding one.
const SEASONAL_JOURNAL_GENRES: std::ops::RangeInclusive<i32> = 237..=250;

/// Gate columns of `GilShop` that are needed only to classify its rows.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "GilShop")]
struct GilShopGates {
    #[xiv_gen(column = "#")]
    key_id: GilShopId,
    #[xiv_gen(column = "Quest")]
    quest: i32,
    #[xiv_gen(column = "FestivalId")]
    festival_id: i32,
}

/// Gate columns of `GilShopItem`, likewise generation-only.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "GilShopItem")]
struct GilShopItemGates {
    #[xiv_gen(column = "#")]
    key_id: crate::subrow_key::SubrowKey<GilShopId>,
    #[xiv_gen(column = "Item")]
    item: i32,
    #[xiv_gen(column = "QuestRequired[{}]", count = 2)]
    quest_required: [i32; 2],
    #[xiv_gen(column = "AchievementRequired")]
    achievement_required: i32,
}

/// Just enough of `Achievement` to find the category a gate belongs to.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "Achievement")]
struct AchievementCategoryOf {
    #[xiv_gen(column = "#")]
    key_id: i32,
    #[xiv_gen(column = "AchievementCategory")]
    achievement_category: i32,
}

/// Just enough of `Quest` to find the journal genre a gate belongs to.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "Quest")]
struct QuestGenreOf {
    #[xiv_gen(column = "#")]
    key_id: i32,
    #[xiv_gen(column = "JournalGenre")]
    journal_genre: i32,
}

/// Resolved gate categories, read once and reused for every shop row.
struct GateIndex {
    /// achievement id -> `AchievementCategory`
    achievement_category: HashMap<i32, i32>,
    /// quest id -> `JournalGenre`
    quest_genre: HashMap<i32, i32>,
}

impl GateIndex {
    fn seasonal_achievement(&self, achievement: i32) -> bool {
        self.achievement_category
            .get(&achievement)
            .is_some_and(|c| SEASONAL_ACHIEVEMENT_CATEGORIES.contains(c))
    }

    fn seasonal_quest(&self, quest: i32) -> bool {
        self.quest_genre
            .get(&quest)
            .is_some_and(|g| SEASONAL_JOURNAL_GENRES.contains(g))
    }
}

/// Classify one shop row from its own gates and its shop's.
///
/// Which column a gate sits in says nothing about how hard it is — quest gates
/// are mostly main-scenario progress, and the largest group of achievement
/// gates is the sightseeing log, both of which any player can still complete.
/// Only the category of the referenced row separates those from an event that
/// has been over for a decade.
fn classify_availability(
    shop: Option<&GilShopGates>,
    item: &GilShopItemGates,
    gates: &GateIndex,
) -> VendorAvailability {
    let shop_quest = shop.map_or(0, |s| s.quest);
    let quest_gates = [shop_quest, item.quest_required[0], item.quest_required[1]];

    // A seasonal quest/achievement outranks a festival shop: it means the
    // player had to be there at the time, which no future occurrence undoes.
    if item.achievement_required != 0 && gates.seasonal_achievement(item.achievement_required) {
        return VendorAvailability::SeasonalUnlock;
    }
    if quest_gates
        .iter()
        .any(|q| *q != 0 && gates.seasonal_quest(*q))
    {
        return VendorAvailability::SeasonalUnlock;
    }
    if shop.is_some_and(|s| s.festival_id != 0) {
        return VendorAvailability::SeasonalShop;
    }
    if item.achievement_required != 0 || quest_gates.iter().any(|q| *q != 0) {
        return VendorAvailability::Unlockable;
    }
    VendorAvailability::Open
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `Out of Sight`, the sightseeing-log achievement behind the Paintings —
    /// the largest group of achievement-gated marketable rows, and one any
    /// player can still earn.
    const SIGHTSEEING_CATEGORY: i32 = 12;
    /// `Main Quests`, the largest quest-gate genre.
    const MAIN_QUEST_GENRE: i32 = 1;

    fn gates() -> GateIndex {
        GateIndex {
            achievement_category: HashMap::from([
                (875, SEASONAL_ACHIEVEMENT_CATEGORIES[0]), // Horsing About
                (700, SIGHTSEEING_CATEGORY),               // Out of Sight
            ]),
            quest_genre: HashMap::from([
                (66832, 238),              // Thank Heavensturn for You
                (66754, MAIN_QUEST_GENRE), // Brotherhood of Ash
            ]),
        }
    }

    fn shop(quest: i32, festival_id: i32) -> GilShopGates {
        GilShopGates {
            key_id: GilShopId(262630),
            quest,
            festival_id,
        }
    }

    fn row(quest_required: [i32; 2], achievement_required: i32) -> GilShopItemGates {
        GilShopItemGates {
            key_id: crate::subrow_key::SubrowKey(GilShopId(262630), 0),
            item: 2644,
            quest_required,
            achievement_required,
        }
    }

    fn classify(shop: Option<&GilShopGates>, item: &GilShopItemGates) -> VendorAvailability {
        classify_availability(shop, item, &gates())
    }

    #[test]
    fn ungated_row_in_an_ungated_shop_is_open() {
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([0, 0], 0)),
            VendorAvailability::Open
        );
    }

    #[test]
    fn a_missing_shop_row_still_classifies_from_the_item_gates() {
        assert_eq!(classify(None, &row([0, 0], 0)), VendorAvailability::Open);
        assert_eq!(
            classify(None, &row([0, 0], 875)),
            VendorAvailability::SeasonalUnlock
        );
    }

    /// The guard against the naive "any gate means unavailable" rule: story
    /// progress and the sightseeing log are gates a player can simply go do.
    #[test]
    fn story_and_sightseeing_gates_are_merely_unlockable() {
        assert_eq!(
            classify(Some(&shop(66754, 0)), &row([0, 0], 0)),
            VendorAvailability::Unlockable
        );
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([0, 0], 700)),
            VendorAvailability::Unlockable
        );
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([66754, 0], 0)),
            VendorAvailability::Unlockable
        );
    }

    /// Usagi Kabuto's Calamity Salvager row: a year-round vendor that only
    /// sells to players who earned a 2013 event achievement.
    #[test]
    fn seasonal_event_achievement_is_a_seasonal_unlock() {
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([0, 0], 875)),
            VendorAvailability::SeasonalUnlock
        );
    }

    #[test]
    fn seasonal_event_quest_is_a_seasonal_unlock_from_either_slot() {
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([66832, 0], 0)),
            VendorAvailability::SeasonalUnlock
        );
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([0, 66832], 0)),
            VendorAvailability::SeasonalUnlock
        );
        assert_eq!(
            classify(Some(&shop(66832, 0)), &row([0, 0], 0)),
            VendorAvailability::SeasonalUnlock
        );
    }

    /// Usagi Kabuto's other row: the festival vendor that only exists while
    /// that Heavensturn occurrence is live.
    #[test]
    fn festival_shop_without_a_personal_gate_is_a_seasonal_shop() {
        assert_eq!(
            classify(Some(&shop(0, 5)), &row([0, 0], 0)),
            VendorAvailability::SeasonalShop
        );
    }

    /// Needing to have taken part outranks the shop's own schedule: a future
    /// occurrence reopens the shop but never re-awards the old achievement.
    #[test]
    fn a_seasonal_unlock_outranks_a_festival_shop() {
        assert_eq!(
            classify(Some(&shop(0, 5)), &row([0, 0], 875)),
            VendorAvailability::SeasonalUnlock
        );
    }

    /// An unknown gate id (a quest or achievement missing from the sheet) must
    /// not silently read as ungated.
    #[test]
    fn unrecognized_gate_ids_are_unlockable_not_open() {
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([0, 0], i32::MAX)),
            VendorAvailability::Unlockable
        );
        assert_eq!(
            classify(Some(&shop(0, 0)), &row([i32::MAX, 0], 0)),
            VendorAvailability::Unlockable
        );
    }

    #[test]
    fn ordering_runs_least_to_most_restricted() {
        use VendorAvailability::*;
        let mut all = [SeasonalUnlock, Open, SeasonalShop, Unlockable];
        all.sort();
        assert_eq!(all, [Open, Unlockable, SeasonalShop, SeasonalUnlock]);
        // An item sold by several shops is as reachable as its easiest row.
        assert_eq!(
            [SeasonalUnlock, Unlockable].into_iter().min(),
            Some(Unlockable)
        );
    }

    #[test]
    fn only_the_seasonal_states_are_unobtainable() {
        use VendorAvailability::*;
        assert!(Open.is_obtainable());
        assert!(Unlockable.is_obtainable());
        assert!(!SeasonalShop.is_obtainable());
        assert!(!SeasonalUnlock.is_obtainable());
    }
}
