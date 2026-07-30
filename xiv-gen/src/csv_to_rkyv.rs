/// Contains all the code needed to read a csv file and produce a `Data` struct
/// ready to be serialized (e.g. with rkyv).
/// Recommended to just let xiv-gen-db handle this unless you need a different backing store.
use crate::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Locate the `ffxiv-datamining` checkout the CSVs are read from.
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
/// The same pattern exists in `ultros-frontend/ultros-xiv-icons/build.rs` for
/// the `universalis-assets` submodule; keep the two in sync.
fn datamining_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Some(dir) = std::env::var_os("FFXIV_DATAMINING_DIR") {
            let dir = PathBuf::from(dir);
            assert!(
                datamining_populated(&dir),
                "FFXIV_DATAMINING_DIR is set to {} but the expected csv files \
                 (csv/{{en,cn,tc}}/Item.csv, csv/ko/csv/Item.csv) are not all present there",
                dir.display()
            );
            return dir;
        }
        let local = manifest_dir.join("ffxiv-datamining");
        if datamining_populated(&local) {
            return local;
        }
        if let Some(main) = main_worktree(manifest_dir) {
            let candidate = main.join("xiv-gen").join("ffxiv-datamining");
            if candidate != local && datamining_populated(&candidate) {
                warn_on_pin_drift(manifest_dir, &candidate);
                println!(
                    "cargo:warning=ffxiv-datamining submodule not populated in this checkout; \
                     falling back to {}",
                    candidate.display()
                );
                return candidate;
            }
        }
        panic!(
            "could not find a populated ffxiv-datamining checkout. Either initialize the \
             submodule (see CLAUDE.md), or set FFXIV_DATAMINING_DIR to an existing checkout. \
             Looked at {} and the main worktree.",
            local.display()
        )
    })
}

/// `true` when every language's data is present, including the nested cn/ko/tc
/// submodules (a non-recursive init only populates en/ja/de/fr). `csv/ko`
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

/// Path of the main (first) git worktree, from `git worktree list --porcelain`.
fn main_worktree(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse_main_worktree(std::str::from_utf8(&output.stdout).ok()?)
}

fn parse_main_worktree(porcelain: &str) -> Option<PathBuf> {
    porcelain
        .lines()
        .find_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
}

/// Warn when this checkout pins a different submodule commit than the main
/// worktree actually has checked out — the fallback would then build against
/// the wrong data. Best-effort: silent on any git failure.
fn warn_on_pin_drift(manifest_dir: &Path, fallback: &Path) {
    let rev_parse = |dir: &Path, rev: &str| -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", rev])
            .current_dir(dir)
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
    };
    let pinned = rev_parse(manifest_dir, "HEAD:./ffxiv-datamining");
    let actual = rev_parse(fallback, "HEAD");
    if let (Some(pinned), Some(actual)) = (pinned, actual)
        && pinned != actual
    {
        println!(
            "cargo:warning=ffxiv-datamining pin drift: this checkout pins {pinned} but the \
             main worktree has {actual} checked out; building against {actual}"
        );
    }
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

#[cfg(test)]
mod tests {
    use super::parse_main_worktree;
    use std::path::Path;

    #[test]
    fn parses_first_worktree_entry() {
        let porcelain = "worktree C:/Users/x/code/ultros\nHEAD abc123\nbranch refs/heads/main\n\n\
                         worktree C:/Users/x/code/ultros/.claude/worktrees/foo\nHEAD def456\n";
        assert_eq!(
            parse_main_worktree(porcelain).as_deref(),
            Some(Path::new("C:/Users/x/code/ultros"))
        );
    }

    #[test]
    fn handles_missing_output() {
        assert_eq!(parse_main_worktree(""), None);
    }
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
