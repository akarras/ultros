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

/// Data that exists in no CSV sheet and is folded into the pack at generation.
#[derive(Debug, Default, Clone)]
pub struct Supplements {
    /// Every NPC placement extracted from the client, keyed by `ENpcBase` id
    /// (which equals the `ENpcResident` id). Only the NPCs the app can show —
    /// gil-shop vendors and leve issuers — make it into the pack.
    pub npc_placements: HashMap<ENpcResidentId, Vec<NpcPlacement>>,
    /// Leve id -> issuing NPCs, from Teamcraft's hand-kept levemete table.
    pub leve_issuers: HashMap<LeveId, Vec<ENpcResidentId>>,
}

/// Reads `lang` from an ffxiv-datamining tree rooted at `root`, i.e. a directory
/// containing `csv/<lang>/*.csv`, with no supplements: `npc_placements` and
/// `leve_issuers` come out empty.
pub fn read_data_from(root: &Path, lang: Language) -> Data {
    read_data_with(root, lang, &Supplements::default())
}

/// [`read_data_from`] plus the client-derived `supplements`.
pub fn read_data_with(root: &Path, lang: Language, supplements: &Supplements) -> Data {
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
    // The full sheet, needed to decide which NPCs can be indexed at all. It is
    // pruned to the ones the app can actually show before it goes in the pack.
    let mut e_npc_residents: IdMap<ENpcResidentId, ENpcResident> =
        read_csv_to_map(&format!("{}ENpcResident.csv", base_path));
    // Names come from `lang`; the festival id is stamped on from the English
    // gates, for the header-layout reason above.
    let mut gil_shops: IdMap<GilShopId, GilShop> =
        read_csv_to_map(&format!("{}GilShop.csv", base_path));
    for (id, shop) in gil_shops.iter_mut() {
        shop.festival_id = shop_gates.get(id).map_or(0, |g| g.festival_id);
    }
    // Costs are per-locale rows, but which of them are tomestone/scrip indexes
    // rather than item ids is read from the English sheet: the CN/TC forks'
    // header layout leaves the `CostType` columns unnamed.
    let mut special_shops: IdMap<SpecialShopId, SpecialShop> =
        read_csv_to_map(&format!("{}SpecialShop.csv", base_path));
    resolve_currency_costs(
        &mut special_shops,
        &read_csv_vec::<SpecialShopCostTypes>(&format!("{en_path}SpecialShop.csv"))
            .into_iter()
            .map(|s| (s.key_id, s))
            .collect(),
        &tomestone_items(&read_csv_vec::<TomestonesItemRow>(&format!(
            "{en_path}TomestonesItem.csv"
        ))),
    );
    let collectables_shops: IdMap<CollectablesShopId, CollectablesShop> =
        read_csv_to_map(&format!("{}CollectablesShop.csv", base_path));
    // Read once to build the shop -> NPC indexes, then drop: these sheets
    // exist only to answer "which NPCs offer this shop?", and `ENpcBase` is by
    // far the largest table in the set. The handler sheets are ids only, so
    // the English tree serves every language.
    let npc_shops = build_npc_shop_indexes(
        &read_csv_vec::<ENpcBase>(&format!("{}ENpcBase.csv", base_path)),
        &ShopRoutes::read(&en_path),
        &e_npc_residents,
        &gil_shops,
        &special_shops,
        &collectables_shops,
    );
    let leves: IdMap<LeveId, Leve> = read_csv_to_map(&format!("{}Leve.csv", base_path));
    let leve_issuers: IdMap<LeveId, Vec<ENpcResidentId>> = supplements
        .leve_issuers
        .iter()
        .filter(|(leve, _)| leves.contains_key(leve))
        .map(|(leve, npcs)| {
            let mut npcs: Vec<_> = npcs
                .iter()
                .copied()
                .filter(|npc| e_npc_residents.contains_key(npc))
                .collect();
            npcs.sort_unstable_by_key(|n| n.0);
            npcs.dedup();
            (*leve, npcs)
        })
        .filter(|(_, npcs)| !npcs.is_empty())
        .collect();
    // Drop every NPC the app has no way to reach. `/npc/:id` exists only for a
    // gil shop, exchange or collectables counter (`game_sources::shop_npcs`),
    // and the leve analyzer names issuers; nothing else looks a resident up. Of
    // the sheet's ~60k rows about 900 survive, which is worth ~650 KB of the
    // ~4.5 MB English pack — the second largest table in it existed to answer
    // lookups for ids the app never forms.
    let shown_npcs: std::collections::HashSet<ENpcResidentId> = npc_shops
        .gil
        .values()
        .chain(npc_shops.special.values())
        .chain(npc_shops.collectables.values())
        .chain(leve_issuers.values())
        .flatten()
        .copied()
        .collect();
    e_npc_residents.retain(|id, _| shown_npcs.contains(id));

    let npc_placements = shown_npc_placements(&supplements.npc_placements, shown_npcs.iter());
    Data {
        items: read_csv_to_map(&format!("{}Item.csv", base_path)),
        recipes: read_csv_to_map(&format!("{}Recipe.csv", base_path)),
        class_jobs: read_csv_to_map(&format!("{}ClassJob.csv", base_path)),
        class_job_categorys: read_csv_to_map(&format!("{}ClassJobCategory.csv", base_path)),
        base_params: read_csv_to_map(&format!("{}BaseParam.csv", base_path)),
        special_shops,
        leves,
        leve_reward_items: read_csv_to_map(&format!("{}LeveRewardItem.csv", base_path)),
        leve_reward_item_groups: read_csv_to_map(&format!("{}LeveRewardItemGroup.csv", base_path)),
        e_npc_residents,
        gil_shops,
        gil_shop_items: read_csv_vec::<GilShopItem>(&format!("{}GilShopItem.csv", base_path))
            .into_iter()
            .fold(
                HashMap::<GilShopId, Vec<GilShopItem>>::new(),
                |mut map, mut m| {
                    if let Some(item) = item_gates.get(&(m.key_id.0, m.item)) {
                        m.availability =
                            classify_availability(shop_gates.get(&m.key_id.0), item, &gates);
                    }
                    map.entry(m.key_id.0).or_default().push(m);
                    map
                },
            )
            .into_iter()
            .collect(),
        gil_shop_npcs: npc_shops.gil,
        special_shop_npcs: npc_shops.special,
        collectables_shop_npcs: npc_shops.collectables,
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
        collectables_shops,
        collectables_shop_items: read_csv_vec::<CollectablesShopItem>(&format!(
            "{}CollectablesShopItem.csv",
            base_path
        ))
        .into_iter()
        .fold(
            HashMap::<CollectablesShopItemId, Vec<CollectablesShopItem>>::new(),
            |mut map, m| {
                map.entry(CollectablesShopItemId(m.key_id.0))
                    .or_default()
                    .push(m);
                map
            },
        )
        .into_iter()
        .collect(),
        collectables_shop_reward_scrips: read_csv_to_map(&format!(
            "{}CollectablesShopRewardScrip.csv",
            base_path
        )),
        craft_leves: read_csv_to_map(&format!("{}CraftLeve.csv", base_path)),
        place_names: read_csv_to_map(&format!("{}PlaceName.csv", base_path)),
        // Ids and numbers only, so the English sheet serves every language
        // (the forks' header layout leaves some of these columns unnamed).
        maps: read_csv_to_map(&format!("{en_path}Map.csv")),
        territory_types: read_csv_to_map(&format!("{en_path}TerritoryType.csv")),
        npc_placements,
        leve_issuers,
    }
}

/// The placements of just the NPCs in `shown`, each list sorted so the pack
/// is independent of extraction order.
fn shown_npc_placements<'a>(
    all: &HashMap<ENpcResidentId, Vec<NpcPlacement>>,
    shown: impl Iterator<Item = &'a ENpcResidentId>,
) -> IdMap<ENpcResidentId, Vec<NpcPlacement>> {
    let mut out: IdMap<ENpcResidentId, Vec<NpcPlacement>> = IdMap::new();
    for npc in shown {
        if out.contains_key(npc) {
            continue;
        }
        if let Some(placements) = all.get(npc).filter(|p| !p.is_empty()) {
            let mut placements = placements.clone();
            placements.sort_by(|a, b| {
                (a.territory.0, a.map.0, a.festival_id)
                    .cmp(&(b.territory.0, b.map.0, b.festival_id))
                    .then(a.x.total_cmp(&b.x))
                    .then(a.y.total_cmp(&b.y))
            });
            out.insert(*npc, placements);
        }
    }
    out
}

/// The handler sheets that stand between an NPC's data slots and its shops,
/// read from the English tree (ids only) and dropped after the walk.
///
/// A slot value is an id in whichever sheet the game's event-handler numbering
/// puts it in; the routes followed here are the ones that end in a shop:
///
/// - the slot *is* a shop;
/// - `TopicSelect` — a menu whose `Shop[]` entries are shops;
/// - `PreHandler` — a quest-gated redirect to any of the above, or to an
///   `InclusionShop`;
/// - `InclusionShop` — the tabbed exchange window (scrip and tomestone
///   vendors), whose `Category[]` rows each name an `InclusionShopSeries`,
///   whose subrows are the special shops;
/// - `CustomTalk` — a script whose `ScriptArg`s may name shops (achievement
///   and gear-reoutfitting vendors) and whose `CustomTalkNestHandlers` may
///   name any of the above (Grand Company quartermasters).
///
/// Every candidate is checked against the shop tables before it is recorded
/// (`build_npc_shop_indexes`): a script argument is an arbitrary number and a
/// data slot holds ids from many sheets, so anything that merely *looks* like
/// a shop id is dropped there.
pub(crate) struct ShopRoutes {
    topic_selects: HashMap<TopicSelectId, TopicSelect>,
    pre_handlers: HashMap<PreHandlerId, PreHandler>,
    /// inclusion shop -> the special shops its categories' series list
    inclusion_shops: HashMap<i32, Vec<i32>>,
    /// custom talk -> script arguments, then nested handlers
    custom_talks: HashMap<i32, Vec<i32>>,
}

impl ShopRoutes {
    pub(crate) fn read(en_path: &str) -> Self {
        let series: HashMap<i32, Vec<i32>> =
            read_csv_vec::<InclusionShopSeriesShop>(&format!("{en_path}InclusionShopSeries.csv"))
                .into_iter()
                .filter(|s| s.special_shop != 0)
                .fold(HashMap::new(), |mut map, s| {
                    map.entry(s.key_id.0).or_default().push(s.special_shop);
                    map
                });
        let categories: HashMap<i32, i32> = read_csv_vec::<InclusionShopCategorySeries>(&format!(
            "{en_path}InclusionShopCategory.csv"
        ))
        .into_iter()
        .map(|c| (c.key_id, c.series))
        .collect();
        let inclusion_shops =
            read_csv_vec::<InclusionShopCategories>(&format!("{en_path}InclusionShop.csv"))
                .into_iter()
                .map(|shop| {
                    let shops = shop
                        .category
                        .iter()
                        .filter(|c| **c != 0)
                        .filter_map(|c| categories.get(c))
                        .filter_map(|s| series.get(s))
                        .flatten()
                        .copied()
                        .collect();
                    (shop.key_id, shops)
                })
                .collect();
        let mut custom_talks: HashMap<i32, Vec<i32>> =
            read_csv_vec::<CustomTalkArgs>(&format!("{en_path}CustomTalk.csv"))
                .into_iter()
                .map(|t| {
                    let args = t.script_arg.iter().copied().filter(|a| *a != 0).collect();
                    (t.key_id, args)
                })
                .collect();
        for nest in
            read_csv_vec::<CustomTalkNestHandler>(&format!("{en_path}CustomTalkNestHandlers.csv"))
        {
            if nest.nest_handler != 0 {
                custom_talks
                    .entry(nest.key_id.0)
                    .or_default()
                    .push(nest.nest_handler);
            }
        }
        Self {
            topic_selects: read_csv_to_hash_map(&format!("{en_path}TopicSelect.csv")),
            pre_handlers: read_csv_to_hash_map(&format!("{en_path}PreHandler.csv")),
            inclusion_shops,
            custom_talks,
        }
    }

    /// Every id reachable from one data slot, the slot itself first. The walk
    /// is bounded: handlers only ever point "down" (a `PreHandler` at a menu,
    /// a nested talk at a shop), and a stray cycle in the data must not hang
    /// the generator.
    fn reachable(&self, slot: i32, out: &mut Vec<i32>) {
        self.walk(slot, out, 4);
    }

    fn walk(&self, id: i32, out: &mut Vec<i32>, depth: u8) {
        out.push(id);
        if depth == 0 {
            return;
        }
        if let Some(ts) = self.topic_selects.get(&TopicSelectId(id)) {
            out.extend(ts.shop.iter().copied().filter(|s| *s != 0));
        }
        if let Some(shops) = self.inclusion_shops.get(&id) {
            out.extend(shops.iter().copied());
        }
        if let Some(ph) = self.pre_handlers.get(&PreHandlerId(id))
            && ph.target != 0
        {
            self.walk(ph.target, out, depth - 1);
        }
        if let Some(handlers) = self.custom_talks.get(&id) {
            for h in handlers {
                self.walk(*h, out, depth - 1);
            }
        }
    }
}

/// `CustomTalk` row of `CmnDefRowenasCollectablesShop`: the one script every
/// Collectable Appraiser runs. The sheets carry no link from it to the
/// `RewardType = 1` collectables shops it opens (the scrip turn-in counters),
/// so appraisers are attributed to all of them by rule. Re-derive with
/// `grep -n CmnDefRowenasCollectablesShop csv/en/CustomTalk.csv`.
const ROWENAS_COLLECTABLES_TALK: i32 = 721585;

/// `CollectablesShop.RewardType` of the shops that pay scrip for a turn-in
/// (`2` is a material exchange that hands back items).
const COLLECTABLES_REWARD_SCRIP: i32 = 1;

/// The shop -> NPC indexes, one per shop kind.
pub(crate) struct NpcShopIndexes {
    pub gil: IdMap<GilShopId, Vec<ENpcResidentId>>,
    pub special: IdMap<SpecialShopId, Vec<ENpcResidentId>>,
    pub collectables: IdMap<CollectablesShopId, Vec<ENpcResidentId>>,
}

/// Invert `ENpcBase.ENpcData` into `shop -> npcs` for every shop kind.
///
/// Each of an NPC's 32 data slots is expanded through [`ShopRoutes`] and every
/// id that comes out is checked against the three shop tables. Without that
/// filter the indexes would gain a key for nearly every distinct slot value
/// across ~60k NPCs — larger than the sheets they replace. An NPC that reaches
/// the same shop by two routes is recorded once.
///
/// Only NPCs with an `ENpcResident` row are indexed: one without has no name
/// or location to render, so it could never appear as a source.
pub(crate) fn build_npc_shop_indexes(
    npc_bases: &[ENpcBase],
    routes: &ShopRoutes,
    residents: &IdMap<ENpcResidentId, ENpcResident>,
    gil_shops: &IdMap<GilShopId, GilShop>,
    special_shops: &IdMap<SpecialShopId, SpecialShop>,
    collectables_shops: &IdMap<CollectablesShopId, CollectablesShop>,
) -> NpcShopIndexes {
    let mut gil: HashMap<GilShopId, Vec<ENpcResidentId>> = HashMap::new();
    let mut special: HashMap<SpecialShopId, Vec<ENpcResidentId>> = HashMap::new();
    let mut collectables: HashMap<CollectablesShopId, Vec<ENpcResidentId>> = HashMap::new();
    let mut reachable = Vec::new();
    for npc in npc_bases {
        let resident = ENpcResidentId(npc.key_id.0);
        if !residents.contains_key(&resident) {
            continue;
        }
        reachable.clear();
        for slot in npc.e_npc_data.iter().copied().filter(|s| *s != 0) {
            routes.reachable(slot as i32, &mut reachable);
        }
        if reachable.contains(&ROWENAS_COLLECTABLES_TALK) {
            reachable.extend(
                collectables_shops
                    .values()
                    .filter(|s| s.reward_type == COLLECTABLES_REWARD_SCRIP)
                    .map(|s| s.key_id.0),
            );
        }
        reachable.sort_unstable();
        reachable.dedup();
        for id in reachable.iter().copied() {
            if gil_shops.contains_key(&GilShopId(id)) {
                gil.entry(GilShopId(id)).or_default().push(resident);
            }
            if special_shops.contains_key(&SpecialShopId(id)) {
                special.entry(SpecialShopId(id)).or_default().push(resident);
            }
            if collectables_shops.contains_key(&CollectablesShopId(id)) {
                collectables
                    .entry(CollectablesShopId(id))
                    .or_default()
                    .push(resident);
            }
        }
    }
    // `npc_bases` arrives in CSV order, which is already stable, but sorting
    // makes the pack independent of upstream row ordering and lets the vendor
    // panel render the same sequence on the server and after hydration.
    fn sort_npcs<K>(index: &mut HashMap<K, Vec<ENpcResidentId>>) {
        for npcs in index.values_mut() {
            npcs.sort_unstable_by_key(|n| n.0);
            npcs.dedup();
        }
    }
    sort_npcs(&mut gil);
    sort_npcs(&mut special);
    sort_npcs(&mut collectables);
    // The per-shop `Vec`s are built by `entry`, so the working index is a
    // `HashMap`; the pack wants it in shop order.
    NpcShopIndexes {
        gil: gil.into_iter().collect(),
        special: special.into_iter().collect(),
        collectables: collectables.into_iter().collect(),
    }
}

/// `Item[n].CostType[k]` value meaning "`ItemCost` is a `TomestonesItem`
/// index" — the game re-points those indexes every patch so a shop can keep
/// selling for "the current tomestone" without a data edit.
const COST_TYPE_TOMESTONE: u8 = 2;
/// `Item[n].CostType[k]` value meaning "`ItemCost` is a scrip index" (see
/// [`scrip_item`]).
const COST_TYPE_SCRIP: u8 = 3;

/// Tomestone index -> item, from the `TomestonesItem` rows that are currently
/// assigned an index (`Tomestones != 0`).
pub(crate) fn tomestone_items(rows: &[TomestonesItemRow]) -> HashMap<u16, u16> {
    rows.iter()
        .filter(|r| r.tomestones != 0)
        .map(|r| (r.tomestones as u16, r.item as u16))
        .collect()
}

/// Rewrite the tomestone and scrip costs of every special shop from the
/// sheet's index encoding to the item they stand for.
///
/// In the sheet a Poetics cost is `ItemCost = 1` (which is also Gil's item id)
/// and a Purple Crafters' Scrip cost is `ItemCost = 2` (Fire Shard's); only
/// `CostType` tells them apart from a real item cost, and nothing downstream
/// wants to carry that distinction. A slot whose cost type is unknown, or
/// whose index has no item, is left as it was.
pub(crate) fn resolve_currency_costs(
    shops: &mut IdMap<SpecialShopId, SpecialShop>,
    cost_types: &HashMap<SpecialShopId, SpecialShopCostTypes>,
    tomestones: &HashMap<u16, u16>,
) {
    let resolve = |cost_type: u8, cost: u16| -> Option<u16> {
        match cost_type {
            COST_TYPE_TOMESTONE => tomestones.get(&cost).copied(),
            COST_TYPE_SCRIP => scrip_item(cost as u32).map(|item| item.0 as u16),
            _ => None,
        }
    };
    for (id, shop) in shops.iter_mut() {
        let Some(types) = cost_types.get(id) else {
            continue;
        };
        let columns = [
            (&mut shop.item_cost_0, &types.cost_type_0),
            (&mut shop.item_cost_1, &types.cost_type_1),
            (&mut shop.item_cost_2, &types.cost_type_2),
        ];
        for (costs, kinds) in columns {
            for (cost, kind) in costs.iter_mut().zip(kinds.iter()) {
                if *cost != 0
                    && let Some(item) = resolve(*kind, *cost)
                {
                    *cost = item;
                }
            }
        }
    }
}

/// The cost-type columns of `SpecialShop`, read from the English sheet only
/// (see [`resolve_currency_costs`]).
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "SpecialShop")]
pub(crate) struct SpecialShopCostTypes {
    #[xiv_gen(column = "#")]
    key_id: SpecialShopId,
    #[xiv_gen(column = "Item[{}].CostType[0]", count = 60)]
    cost_type_0: Vec<u8>,
    #[xiv_gen(column = "Item[{}].CostType[1]", count = 60)]
    cost_type_1: Vec<u8>,
    #[xiv_gen(column = "Item[{}].CostType[2]", count = 60)]
    cost_type_2: Vec<u8>,
}

/// `TomestonesItem`: which item each live tomestone index stands for.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "TomestonesItem")]
pub(crate) struct TomestonesItemRow {
    #[xiv_gen(column = "#")]
    key_id: i32,
    #[xiv_gen(column = "Item")]
    item: i32,
    #[xiv_gen(column = "Tomestones")]
    tomestones: i32,
}

/// `InclusionShop.Category[]`, the categories of one tabbed exchange window.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "InclusionShop")]
struct InclusionShopCategories {
    #[xiv_gen(column = "#")]
    key_id: i32,
    #[xiv_gen(column = "Category[{}]", count = 30)]
    category: [i32; 30],
}

/// `InclusionShopCategory.InclusionShopSeries`: the series a category shows.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "InclusionShopCategory")]
struct InclusionShopCategorySeries {
    #[xiv_gen(column = "#")]
    key_id: i32,
    #[xiv_gen(column = "InclusionShopSeries")]
    series: i32,
}

/// `InclusionShopSeries` subrows: the special shops of one series.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "InclusionShopSeries")]
struct InclusionShopSeriesShop {
    #[xiv_gen(column = "#")]
    key_id: crate::subrow_key::SubrowKey<i32>,
    #[xiv_gen(column = "SpecialShop")]
    special_shop: i32,
}

/// `CustomTalk.Script[n].ScriptArg`: the numbers a talk script is given.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "CustomTalk")]
struct CustomTalkArgs {
    #[xiv_gen(column = "#")]
    key_id: i32,
    #[xiv_gen(column = "Script[{}].ScriptArg", count = 30)]
    script_arg: [i32; 30],
}

/// `CustomTalkNestHandlers` subrows: the handlers a talk script opens.
#[derive(Debug, Clone, FromCsv)]
#[xiv_gen(sheet = "CustomTalkNestHandlers")]
struct CustomTalkNestHandler {
    #[xiv_gen(column = "#")]
    key_id: crate::subrow_key::SubrowKey<i32>,
    #[xiv_gen(column = "NestHandler")]
    nest_handler: i32,
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

/// Reads every row of one sheet. Public so the pack generator can read the
/// sheets it needs for client extraction (`TerritoryType`, `Map`) the same way.
pub fn read_sheet<T: FromCsv>(path: &Path) -> Vec<T> {
    read_csv_vec(&path.display().to_string())
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

/// Same as [`read_csv_to_map`] for the handler sheets, which are read to walk
/// the NPC -> shop routes and then dropped rather than packed.
fn read_csv_to_hash_map<K, T>(path: &str) -> HashMap<K, T>
where
    T: FromCsv + HasId<Id = K>,
    K: std::hash::Hash + Eq,
{
    read_csv_vec::<T>(path)
        .into_iter()
        .map(|item| (item.get_id(), item))
        .collect()
}

fn read_csv_to_map<K, T>(path: &str) -> IdMap<K, T>
where
    T: FromCsv + HasId<Id = K>,
    K: RowId,
{
    read_csv_vec::<T>(path)
        .into_iter()
        .map(|item| (item.get_id(), item))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn special_shop(id: i32, costs: [(u16, u32); 2]) -> SpecialShop {
        let mut item_cost_0 = vec![0u16; 60];
        let mut count_cost_0 = vec![0u32; 60];
        for (slot, (cost, count)) in costs.into_iter().enumerate() {
            item_cost_0[slot] = cost;
            count_cost_0[slot] = count;
        }
        SpecialShop {
            key_id: SpecialShopId(id),
            name: String::new(),
            item: vec![0; 60],
            item_receive_0: vec![0; 60],
            count_receive_0: vec![0; 60],
            item_receive_1: vec![0; 60],
            count_receive_1: vec![0; 60],
            item_cost_0,
            count_cost_0,
            item_cost_1: vec![0; 60],
            count_cost_1: vec![0; 60],
            item_cost_2: vec![0; 60],
            count_cost_2: vec![0; 60],
        }
    }

    fn cost_types(id: i32, kinds: [u8; 2]) -> SpecialShopCostTypes {
        let mut cost_type_0 = vec![0u8; 60];
        cost_type_0[..2].copy_from_slice(&kinds);
        SpecialShopCostTypes {
            key_id: SpecialShopId(id),
            cost_type_0,
            cost_type_1: vec![0; 60],
            cost_type_2: vec![0; 60],
        }
    }

    /// The sheet spells a Poetics cost `ItemCost = 1` (Gil's id) and a
    /// Purple Crafters' Scrip cost `ItemCost = 2` (Fire Shard's); the pack
    /// must carry the real items, and leave real item costs alone even when
    /// they happen to be small ids.
    #[test]
    fn currency_costs_resolve_to_items_by_cost_type() {
        let mut shops = IdMap::from_iter([
            // Poetics + purple scrip
            (SpecialShopId(1), special_shop(1, [(1, 345), (2, 250)])),
            // 566 sheet rows really do cost gil; a gil cost is CostType 0
            (SpecialShopId(2), special_shop(2, [(1, 1000), (5000, 1)])),
            // no cost-type row at all (fork skew): untouched
            (SpecialShopId(3), special_shop(3, [(2, 1), (0, 0)])),
            // an index nothing maps to stays as it was
            (SpecialShopId(4), special_shop(4, [(9, 1), (5, 1)])),
        ]);
        let types = HashMap::from([
            (
                SpecialShopId(1),
                cost_types(1, [COST_TYPE_TOMESTONE, COST_TYPE_SCRIP]),
            ),
            (SpecialShopId(2), cost_types(2, [0, 0])),
            (
                SpecialShopId(4),
                cost_types(4, [COST_TYPE_TOMESTONE, COST_TYPE_SCRIP]),
            ),
        ]);
        let tomestones = tomestone_items(&[
            TomestonesItemRow {
                key_id: 3,
                item: 28,
                tomestones: 1,
            },
            TomestonesItemRow {
                key_id: 22,
                item: 48,
                tomestones: 2,
            },
            TomestonesItemRow {
                key_id: 0,
                item: 23,
                tomestones: 0,
            },
        ]);
        assert_eq!(tomestones, HashMap::from([(1, 28), (2, 48)]));
        resolve_currency_costs(&mut shops, &types, &tomestones);
        let costs = |id: i32| shops[&SpecialShopId(id)].item_cost_0[..2].to_vec();
        assert_eq!(costs(1), vec![28, 33913]);
        assert_eq!(costs(2), vec![1, 5000]);
        assert_eq!(costs(3), vec![2, 0]);
        assert_eq!(costs(4), vec![9, 5]);
        // counts are not the resolver's business
        assert_eq!(shops[&SpecialShopId(1)].count_cost_0[..2], [345, 250]);
    }

    fn npc(id: i32, slots: &[i32]) -> ENpcBase {
        let mut e_npc_data = [0u32; 32];
        for (i, s) in slots.iter().enumerate() {
            e_npc_data[i] = *s as u32;
        }
        ENpcBase {
            key_id: ENpcBaseId(id),
            e_npc_data,
        }
    }

    fn resident(id: i32) -> (ENpcResidentId, ENpcResident) {
        (
            ENpcResidentId(id),
            ENpcResident {
                key_id: ENpcResidentId(id),
                singular: format!("npc {id}"),
                map: 0,
            },
        )
    }

    fn routes() -> ShopRoutes {
        let topic = |id: i32, shops: &[i32]| {
            let mut shop = [0i32; 10];
            shop[..shops.len()].copy_from_slice(shops);
            (
                TopicSelectId(id),
                TopicSelect {
                    key_id: TopicSelectId(id),
                    shop,
                },
            )
        };
        let pre = |id: i32, target: i32| {
            (
                PreHandlerId(id),
                PreHandler {
                    key_id: PreHandlerId(id),
                    target,
                },
            )
        };
        ShopRoutes {
            topic_selects: HashMap::from([topic(3276802, &[1769579, 262145])]),
            pre_handlers: HashMap::from([
                pre(3539066, 3801094), // scrip exchange -> inclusion shop
                pre(3539000, 3276802), // -> menu
                pre(3539001, 262146),  // -> gil shop directly (material suppliers)
                pre(3539002, 3539002), // points at itself: must terminate
            ]),
            inclusion_shops: HashMap::from([(3801094, vec![1770477, 1770478])]),
            custom_talks: HashMap::from([
                (721000, vec![1769813, 3539001]), // an arg that is a shop, a nested handler
                (ROWENAS_COLLECTABLES_TALK, vec![]),
            ]),
        }
    }

    fn special_shops(ids: &[i32]) -> IdMap<SpecialShopId, SpecialShop> {
        ids.iter()
            .map(|id| (SpecialShopId(*id), special_shop(*id, [(0, 0), (0, 0)])))
            .collect()
    }

    #[test]
    fn npc_shop_indexes_follow_every_handler_route() {
        let gil_shops: IdMap<GilShopId, GilShop> = [262145, 262146]
            .into_iter()
            .map(|id| {
                (
                    GilShopId(id),
                    GilShop {
                        key_id: GilShopId(id),
                        name: String::new(),
                        festival_id: 0,
                    },
                )
            })
            .collect();
        let special = special_shops(&[1769579, 1769813, 1770477, 1770478]);
        let collectables: IdMap<CollectablesShopId, CollectablesShop> =
            [(3866626, 1), (3866627, 1), (3866625, 2)]
                .into_iter()
                .map(|(id, reward_type)| {
                    (
                        CollectablesShopId(id),
                        CollectablesShop {
                            key_id: CollectablesShopId(id),
                            name: String::new(),
                            shop_items: [0; 11],
                            reward_type,
                        },
                    )
                })
                .collect();
        let residents: IdMap<_, _> = [1, 2, 3, 4, 5, 6].into_iter().map(resident).collect();
        let npcs = [
            npc(1, &[3539066]),                   // scrip exchange, via inclusion shop
            npc(2, &[3276802, 262145]),           // menu + the same shop directly: once
            npc(3, &[3539001, 3539002]),          // prehandler straight to a gil shop; a cycle
            npc(4, &[721000]),                    // custom talk: arg shop + nested prehandler
            npc(5, &[ROWENAS_COLLECTABLES_TALK]), // appraiser: every scrip-paying shop
            npc(6, &[3866625]),                   // material exchange, direct
            npc(7, &[262145]),                    // no resident row: dropped
        ];
        let out = build_npc_shop_indexes(
            &npcs,
            &routes(),
            &residents,
            &gil_shops,
            &special,
            &collectables,
        );
        let ids = |v: &Vec<ENpcResidentId>| v.iter().map(|n| n.0).collect::<Vec<_>>();
        assert_eq!(ids(&out.special[&SpecialShopId(1770477)]), vec![1]);
        assert_eq!(ids(&out.special[&SpecialShopId(1770478)]), vec![1]);
        assert_eq!(ids(&out.special[&SpecialShopId(1769579)]), vec![2]);
        assert_eq!(ids(&out.gil[&GilShopId(262145)]), vec![2]);
        assert_eq!(ids(&out.gil[&GilShopId(262146)]), vec![3, 4]);
        assert_eq!(ids(&out.special[&SpecialShopId(1769813)]), vec![4]);
        assert_eq!(
            ids(&out.collectables[&CollectablesShopId(3866626)]),
            vec![5]
        );
        assert_eq!(
            ids(&out.collectables[&CollectablesShopId(3866627)]),
            vec![5]
        );
        assert_eq!(
            ids(&out.collectables[&CollectablesShopId(3866625)]),
            vec![6]
        );
        // a handler id that is not a shop never becomes a key
        assert!(!out.special.contains_key(&SpecialShopId(3801094)));
        assert_eq!(
            out.gil.len() + out.special.len() + out.collectables.len(),
            9
        );
    }

    #[test]
    fn shown_placements_keep_only_named_npcs_and_sort_them() {
        let p = |t: i32, x: f32, y: f32| NpcPlacement {
            map: MapId(1),
            territory: TerritoryTypeId(t),
            x,
            y,
            festival_id: 0,
        };
        let all = HashMap::from([
            (
                ENpcResidentId(7),
                vec![p(2, 5.0, 5.0), p(1, 9.0, 1.0), p(1, 3.0, 8.0)],
            ),
            (ENpcResidentId(8), vec![p(1, 1.0, 1.0)]),
            (ENpcResidentId(9), vec![]),
        ]);
        let shown = [
            ENpcResidentId(7),
            ENpcResidentId(9),
            ENpcResidentId(7),
            ENpcResidentId(42),
        ];
        let out = shown_npc_placements(&all, shown.iter());
        assert_eq!(out.len(), 1);
        let xs: Vec<_> = out[&ENpcResidentId(7)]
            .iter()
            .map(|p| (p.territory.0, p.x))
            .collect();
        assert_eq!(xs, vec![(1, 3.0), (1, 9.0), (2, 5.0)]);
    }

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
