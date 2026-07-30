//! Sparse, blobless fetching of the pinned upstream game-data sources.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::manifest::{Manifest, Source};

/// The name of the source that owns the assembled CSV tree. Every other CSV
/// source is checked out *inside* it (see [`source_dir`]).
pub const DATAMINING_SOURCE: &str = "ffxiv-datamining";

/// The name of the source holding the upstream icon PNGs.
pub const ICONS_SOURCE: &str = "universalis-assets";

/// Languages that `ffxiv-datamining` itself ships under `csv/<lang>/`.
pub const DATAMINING_LANGS: [&str; 4] = ["en", "ja", "de", "fr"];

/// Every sheet `xiv_gen::csv_to_rkyv::read_data_from` reads. Adding a sheet
/// there means adding it here, or the sparse checkout will not contain it.
pub const SHEETS: [&str; 31] = [
    "Item",
    "Recipe",
    "ClassJob",
    "ClassJobCategory",
    "BaseParam",
    "SpecialShop",
    "Leve",
    "LeveRewardItem",
    "LeveRewardItemGroup",
    "ENpcBase",
    "ENpcResident",
    "GilShop",
    "GilShopItem",
    "TopicSelect",
    "PreHandler",
    "ItemSearchCategory",
    "ItemUICategory",
    "ItemSortCategory",
    "CompanyCraftSequence",
    "CompanyCraftPart",
    "CompanyCraftProcess",
    "CompanyCraftSupplyItem",
    "CompanyCraftDraftCategory",
    "CompanyCraftType",
    "CompanyCraftDraft",
    "RetainerTask",
    "RetainerTaskNormal",
    "RecipeLevelTable",
    "CollectablesShopItem",
    "CollectablesShopRewardScrip",
    "CraftLeve",
];

/// Sparse-checkout patterns for the sheets under `prefix`, which is a
/// repo-root-relative directory without a trailing slash (`""` for the root).
pub fn sheet_patterns(prefix: &str) -> Vec<String> {
    SHEETS
        .iter()
        .map(|sheet| format!("{prefix}/{sheet}.csv"))
        .collect()
}

/// Sparse-checkout patterns for the source named `name`. Each per-language fork
/// keeps its CSVs at its own depth, so the prefix is per-source.
pub fn sparse_paths_for(name: &str) -> Vec<String> {
    match name {
        DATAMINING_SOURCE => DATAMINING_LANGS
            .iter()
            .flat_map(|lang| sheet_patterns(&format!("/csv/{lang}")))
            .collect(),
        // The ko fork nests its CSVs under `csv/`; cn and tc keep theirs at the root.
        "ffxiv-datamining-ko" => sheet_patterns("/csv"),
        ICONS_SOURCE => vec!["/icon2x/".to_string()],
        _ => sheet_patterns(""),
    }
}

/// Working directory for the source named `name` under `cache_root`. Sources
/// with a `checkout_subdir` are checked out *inside* the ffxiv-datamining tree,
/// which is the layout `read_data_from` expects (and what the submodules did).
pub fn source_dir(cache_root: &Path, name: &str, source: &Source) -> PathBuf {
    match &source.checkout_subdir {
        Some(subdir) => cache_root.join(DATAMINING_SOURCE).join(subdir),
        None => cache_root.join(name),
    }
}

/// Where the generator reads its inputs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// An ffxiv-datamining tree, i.e. a directory containing `csv/<lang>/*.csv`.
    pub datamining: PathBuf,
    /// The upstream `icon2x` PNG directory, unless icons were skipped.
    pub icons: Option<PathBuf>,
}

impl Layout {
    /// Layout of an already-populated ultros checkout (`--offline-source`),
    /// where both trees are still present as submodules.
    pub fn offline(checkout: &Path, skip_icons: bool) -> Layout {
        Layout {
            datamining: checkout.join("xiv-gen").join("ffxiv-datamining"),
            icons: (!skip_icons).then(|| {
                checkout
                    .join("ultros-frontend")
                    .join("ultros-xiv-icons")
                    .join("universalis-assets")
                    .join("icon2x")
            }),
        }
    }
}

/// Sparse-checkout patterns to use for `name`: the manifest's explicit list when
/// it has one, otherwise the per-source default.
pub fn patterns_for(name: &str, source: &Source) -> Vec<String> {
    if source.sparse_paths.is_empty() {
        sparse_paths_for(name)
    } else {
        source.sparse_paths.clone()
    }
}

/// Fetches every source in `manifest` into `cache_root` and returns the
/// assembled input layout.
pub fn fetch_all(
    cache_root: &Path,
    manifest: &Manifest,
    skip_icons: bool,
) -> anyhow::Result<Layout> {
    std::fs::create_dir_all(cache_root)
        .with_context(|| format!("creating cache root {}", cache_root.display()))?;

    let mut names: Vec<&str> = manifest.sources.keys().map(String::as_str).collect();
    // The datamining tree has to exist before the per-language forks are
    // checked out inside it.
    names.sort_by_key(|name| (*name != DATAMINING_SOURCE, *name));

    let mut datamining = None;
    let mut icons = None;
    for name in names {
        if skip_icons && name == ICONS_SOURCE {
            println!("skipping {name} (--skip-icons)");
            continue;
        }
        let source = &manifest.sources[name];
        let patterns = patterns_for(name, source);
        println!(
            "fetching {name} @ {} ({} sparse patterns)",
            source.sha,
            patterns.len()
        );
        let dir = fetch_source(cache_root, name, source, &patterns)?;
        match name {
            DATAMINING_SOURCE => datamining = Some(dir),
            ICONS_SOURCE => icons = Some(dir.join("icon2x")),
            _ => {}
        }
    }

    let Some(datamining) = datamining else {
        bail!("manifest has no `{DATAMINING_SOURCE}` source");
    };
    if !skip_icons && icons.is_none() {
        bail!("manifest has no `{ICONS_SOURCE}` source; pass --skip-icons to build without icons");
    }
    Ok(Layout { datamining, icons })
}

/// Fetches `source` at its pinned SHA into the cache and checks out only the
/// files matching `sparse_paths`. Blobs outside the sparse set are never
/// downloaded (`--filter=blob:none`), so this stays cheap on re-runs.
pub fn fetch_source(
    cache_root: &Path,
    name: &str,
    source: &Source,
    sparse_paths: &[String],
) -> anyhow::Result<PathBuf> {
    let dir = source_dir(cache_root, name, source);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating checkout dir {}", dir.display()))?;

    if dir.join(".git").exists() {
        // Keep a warm cache usable after the manifest's url changes.
        git(&dir, &["remote", "set-url", "origin", &source.url])?;
    } else {
        git(&dir, &["init", "-q"])?;
        git(&dir, &["remote", "add", "origin", &source.url])?;
    }

    git(
        &dir,
        &[
            "fetch",
            "-q",
            "--depth=1",
            "--filter=blob:none",
            "origin",
            &source.sha,
        ],
    )?;
    git(&dir, &["sparse-checkout", "init", "--no-cone"])?;

    let patterns_file = dir.join(".git").join("info").join("sparse-checkout");
    let mut patterns = sparse_paths.join("\n");
    patterns.push('\n');
    std::fs::write(&patterns_file, patterns)
        .with_context(|| format!("writing {}", patterns_file.display()))?;

    git(&dir, &["checkout", "-q", "FETCH_HEAD"])?;
    Ok(dir)
}

/// Resolves the current SHA of `source`'s tracked branch (or its default HEAD).
/// `run_dir` only has to exist — `ls-remote` does not need a repository.
pub fn latest_sha(run_dir: &Path, source: &Source) -> anyhow::Result<String> {
    let refname = source.branch.as_deref().unwrap_or("HEAD");
    let output = git(run_dir, &["ls-remote", &source.url, refname])?;
    let sha = output
        .split_whitespace()
        .next()
        .with_context(|| format!("`{}` has no ref `{refname}`", source.url))?;
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("`git ls-remote {} {refname}` returned {sha:?}", source.url);
    }
    Ok(sha.to_string())
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("running `git {}`", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "`git {}` in {} failed ({}):\n{}",
            args.join(" "),
            dir.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(checkout_subdir: Option<&str>) -> Source {
        Source {
            url: "https://example.invalid/repo.git".to_string(),
            sha: "0".repeat(40),
            branch: None,
            sparse_paths: Vec::new(),
            checkout_subdir: checkout_subdir.map(str::to_string),
        }
    }

    #[test]
    fn sheets_list_covers_every_read_sheet() {
        assert_eq!(SHEETS.len(), 31);
        let mut sorted = SHEETS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), SHEETS.len(), "SHEETS contains duplicates");
        assert!(SHEETS.contains(&"Item"));
        assert!(SHEETS.contains(&"CollectablesShopRewardScrip"));
    }

    #[test]
    fn sheet_patterns_are_language_scoped_csv_paths() {
        let patterns = sheet_patterns("/csv/en");
        assert_eq!(patterns.len(), SHEETS.len());
        assert!(patterns.contains(&"/csv/en/Item.csv".to_string()));
        for pattern in &patterns {
            assert!(
                pattern.starts_with("/csv/en/") && pattern.ends_with(".csv"),
                "unexpected pattern {pattern}"
            );
        }
    }

    #[test]
    fn sheet_patterns_with_empty_prefix_are_repo_root_relative() {
        let patterns = sheet_patterns("");
        assert!(patterns.contains(&"/Item.csv".to_string()));
    }

    #[test]
    fn datamining_sparse_paths_cover_all_four_languages() {
        let patterns = sparse_paths_for(DATAMINING_SOURCE);
        assert_eq!(patterns.len(), SHEETS.len() * DATAMINING_LANGS.len());
        for lang in DATAMINING_LANGS {
            assert!(patterns.contains(&format!("/csv/{lang}/Recipe.csv")));
        }
    }

    #[test]
    fn per_language_forks_use_their_own_depth() {
        // The ko fork keeps its CSVs one level deeper than cn/tc do.
        assert!(
            sparse_paths_for("ffxiv-datamining-ko").contains(&"/csv/Item.csv".to_string()),
            "ko patterns should be nested under /csv"
        );
        assert!(
            sparse_paths_for("ffxiv-datamining-cn").contains(&"/Item.csv".to_string()),
            "cn patterns should be repo-root relative"
        );
        assert!(
            sparse_paths_for("ffxiv-datamining-tc").contains(&"/Item.csv".to_string()),
            "tc patterns should be repo-root relative"
        );
    }

    #[test]
    fn universalis_assets_fetches_the_whole_icon_directory() {
        assert_eq!(sparse_paths_for("universalis-assets"), vec!["/icon2x/"]);
    }

    #[test]
    fn subdir_sources_check_out_inside_the_datamining_tree() {
        let cache = Path::new("/cache");
        assert_eq!(
            source_dir(cache, DATAMINING_SOURCE, &source(None)),
            cache.join(DATAMINING_SOURCE)
        );
        assert_eq!(
            source_dir(cache, "ffxiv-datamining-cn", &source(Some("csv/cn"))),
            cache.join(DATAMINING_SOURCE).join("csv/cn")
        );
        assert_eq!(
            source_dir(cache, "universalis-assets", &source(None)),
            cache.join("universalis-assets")
        );
    }

    #[test]
    fn manifest_sparse_paths_override_the_defaults() {
        let mut explicit = source(None);
        explicit.sparse_paths = vec!["/only/this.csv".to_string()];
        assert_eq!(
            patterns_for(DATAMINING_SOURCE, &explicit),
            explicit.sparse_paths
        );
        assert_eq!(
            patterns_for(DATAMINING_SOURCE, &source(None)),
            sparse_paths_for(DATAMINING_SOURCE)
        );
    }

    #[test]
    fn offline_layout_points_at_the_submodule_checkouts() {
        let checkout = Path::new("/repo");
        let layout = Layout::offline(checkout, false);
        assert_eq!(layout.datamining, checkout.join("xiv-gen/ffxiv-datamining"));
        assert_eq!(
            layout.icons,
            Some(checkout.join("ultros-frontend/ultros-xiv-icons/universalis-assets/icon2x"))
        );
        assert_eq!(Layout::offline(checkout, true).icons, None);
    }
}
