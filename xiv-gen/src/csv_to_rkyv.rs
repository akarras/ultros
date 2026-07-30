/// Contains all the code needed to read a csv file and produce a `Data` struct
/// ready to be serialized (e.g. with rkyv).
/// Recommended to just let xiv-gen-db handle this unless you need a different backing store.
use crate::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A resolved `ffxiv-datamining` checkout.
pub struct ResolvedDataDir {
    /// The resolved directory. Deliberately un-canonicalized: build scripts
    /// register it with `cargo:rerun-if-changed`, and canonicalizing on
    /// Windows yields `\\?\` paths that cargo mishandles.
    pub path: PathBuf,
    /// Messages the consuming build script should surface as `cargo:warning`
    /// (fallback engaged, submodule pin drift). This module never prints
    /// cargo directives itself — that's the build script's job.
    pub warnings: Vec<String>,
}

/// Locate the `ffxiv-datamining` checkout the CSVs are read from. Resolved
/// once and cached for the lifetime of the process.
///
/// Resolution order:
/// 1. `FFXIV_DATAMINING_DIR` — explicit override (the consuming build script
///    emits `cargo:rerun-if-env-changed` for it).
/// 2. The submodule next to this crate, when fully populated (including the
///    nested cn/ko/tc submodules).
/// 3. The main git worktree's copy of the submodule. Linked worktrees rarely
///    have submodules initialized, but the main checkout usually does — this
///    lets worktree builds work with zero setup.
///
/// Build scripts should call this before [`read_data`]: emit `cargo:warning`
/// for each entry in [`ResolvedDataDir::warnings`], register `<path>/csv`
/// with `cargo:rerun-if-changed` (otherwise a datamining bump never re-runs
/// the script), and panic on `Err` — see `xiv-gen-db/build.rs`. [`read_data`]
/// panics on `Err` too, but silently drops the warnings.
pub fn resolved_datamining_dir() -> &'static Result<ResolvedDataDir, String> {
    static DIR: OnceLock<Result<ResolvedDataDir, String>> = OnceLock::new();
    DIR.get_or_init(resolve_datamining_dir)
}

fn resolve_datamining_dir() -> Result<ResolvedDataDir, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(dir) = std::env::var_os("FFXIV_DATAMINING_DIR") {
        let dir = PathBuf::from(dir);
        if !datamining_populated(&dir) {
            return Err(format!(
                "FFXIV_DATAMINING_DIR is set to {} but the expected csv files \
                 (csv/{{en,cn,tc}}/Item.csv, csv/ko/csv/Item.csv) are not all present there",
                dir.display()
            ));
        }
        return Ok(ResolvedDataDir {
            path: dir,
            warnings: Vec::new(),
        });
    }
    let local = manifest_dir.join("ffxiv-datamining");
    if datamining_populated(&local) {
        return Ok(ResolvedDataDir {
            path: local,
            warnings: Vec::new(),
        });
    }
    if let Some(main) = main_worktree(manifest_dir) {
        let candidate = main.join("xiv-gen").join("ffxiv-datamining");
        if candidate != local && datamining_populated(&candidate) {
            let mut warnings = Vec::new();
            if let Some(drift) = pin_drift_warning(manifest_dir, "ffxiv-datamining", &candidate) {
                warnings.push(drift);
            }
            warnings.push(format!(
                "ffxiv-datamining submodule not populated in this checkout; \
                 falling back to {}",
                candidate.display()
            ));
            return Ok(ResolvedDataDir {
                path: candidate,
                warnings,
            });
        }
    }
    Err(format!(
        "could not find a populated ffxiv-datamining checkout. Either initialize the \
         submodule (see CLAUDE.md), or set FFXIV_DATAMINING_DIR to an existing checkout. \
         Looked at {} and the main worktree.",
        local.display()
    ))
}

fn datamining_dir() -> &'static Path {
    match resolved_datamining_dir() {
        Ok(resolved) => &resolved.path,
        Err(message) => panic!("{message}"),
    }
}

/// `true` when every language's data is present. The probe set deliberately
/// includes cn/tc/ko because those live in *nested* submodules — a
/// non-recursive init only populates en/ja/de/fr, and probing the nested
/// files catches that half-initialized state up front (falling back or
/// failing here) instead of dying mid-build on `cn/Item.csv`. `csv/ko`
/// genuinely nests one level deeper than its siblings — that's the ko repo's
/// own layout.
fn datamining_populated(dir: &Path) -> bool {
    [
        "csv/en/Item.csv",
        "csv/cn/Item.csv",
        "csv/tc/Item.csv",
        "csv/ko/csv/Item.csv",
    ]
    .iter()
    .all(|probe| dir.join(probe).is_file())
}

pub fn read_data(lang: Language) -> Data {
    let root = datamining_dir().display();
    let base_path = match lang {
        Language::Ko => format!("{root}/csv/ko/csv/"),
        _ => format!("{root}/csv/{}/", lang.to_path_part()),
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
        .unwrap_or_else(|error| {
            panic!(
                "Failed to open csv at {path}: {error}. The datamining dir resolved to {}; \
                 if that checkout is stale or partial, re-initialize the submodule (see \
                 CLAUDE.md) or set FFXIV_DATAMINING_DIR to a populated checkout.",
                datamining_dir().display()
            )
        });
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

// Worktree-fallback helpers (main-worktree discovery, pin-drift detection)
// shared with `ultros-frontend/ultros-xiv-icons/build.rs` via `include!`.
// Kept last because the file carries its own `#[cfg(test)]` module and
// clippy's `items_after_test_module` wants nothing after it.
include!("worktree_fallback.rs");
