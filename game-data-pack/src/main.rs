//! Generates the LFS-tracked game-data packs under `data/`.
//!
//! Fetches the pinned upstream sources (sparse + blobless), runs the CSV→rkyv
//! pipeline for every language, and reads three things out of a local FFXIV
//! install: the item icons, the NPC placements (folded into the rkyv packs and
//! recorded in `data/npc-placements.json` so a CSV-only rebuild keeps them),
//! and the map images those placements sit on.

mod browser;
mod db;
mod fetch;
mod icons;
mod manifest;
mod maps;
mod placements;

// Included verbatim by the build scripts of the crates that *read* the packs.
// The generator only writes them, so nothing here calls it outside tests.
#[allow(dead_code)]
mod lfs_guard {
    include!("../../data/lfs_guard.rs");
}

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use fetch::Layout;
use manifest::Manifest;

const USAGE: &str = "\
Usage: cargo run --release -p game-data-pack -- [OPTIONS]

Options:
  --pinned                 Build from the SHAs in data/manifest.toml (default)
  --latest                 Resolve each source's latest SHA, rewrite the
                           manifest, then build
  --offline-source <path>  Build the CSV packs from an already-populated ultros
                           checkout instead of fetching anything
  --skip-icons             Skip everything read from the FFXIV install: the
                           icon pack, the map pack and NPC placement
                           extraction. The rkyv packs then take their NPC
                           placements from the committed
                           data/npc-placements.json.
  --game-path <path>       FFXIV install root to read from (default: search
                           the standard install locations)
";

#[derive(Debug, PartialEq, Eq)]
enum Pins {
    Pinned,
    Latest,
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    pins: Pins,
    offline_source: Option<PathBuf>,
    skip_icons: bool,
    game_path: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:?}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("locating the repo root")?;
    let data_dir = repo_root.join("data");

    let manifest_path = data_dir.join("manifest.toml");
    let layout = match &args.offline_source {
        Some(checkout) => {
            println!("building from {} (no fetching)", checkout.display());
            Layout::offline(checkout)
        }
        None => {
            let mut manifest = Manifest::load(&manifest_path)?;
            let cache_root = repo_root.join(".game-data-cache");
            if args.pins == Pins::Latest {
                std::fs::create_dir_all(&cache_root)
                    .with_context(|| format!("creating cache root {}", cache_root.display()))?;
                update_pins(&mut manifest, &cache_root)?;
                manifest.save(&manifest_path)?;
                println!("updated {}", manifest_path.display());
            }
            fetch::fetch_all(&cache_root, &manifest)?
        }
    };

    // Everything that needs the client is read before the packs are built,
    // because the packs fold the NPC placements in.
    let install = if args.skip_icons {
        None
    } else {
        Some(icon_extract::GameInstall::discover(
            args.game_path.as_deref(),
        )?)
    };
    let placements_path = data_dir.join("npc-placements.json");
    let leve_issuers =
        placements::load_leve_issuers(&data_dir.join("npc-locations").join("leve-issuers.json"))?;
    let npc_placements = match &install {
        Some(install) => {
            println!("\nreading NPC placements from FFXIV {}", install.version);
            let en = layout.datamining.join("csv").join("en");
            let extraction = placements::extract(
                install,
                &xiv_gen::csv_to_rkyv::read_sheet::<xiv_gen::TerritoryType>(
                    &en.join("TerritoryType.csv"),
                ),
                &xiv_gen::csv_to_rkyv::read_sheet::<xiv_gen::Map>(&en.join("Map.csv"))
                    .into_iter()
                    .map(|m| (m.key_id, m))
                    .collect(),
                &xiv_gen::csv_to_rkyv::read_sheet::<xiv_gen::ENpcResident>(
                    &en.join("ENpcResident.csv"),
                )
                .into_iter()
                .filter(|r| r.map != 0)
                .map(|r| (r.key_id, xiv_gen::MapId(r.map)))
                .collect(),
            )?;
            println!(
                "  {} NPCs placed across {} layout directories",
                extraction.placements.len(),
                extraction.directories
            );
            for (path, why) in &extraction.unparsed {
                println!("  skipped {path}: {why}");
            }
            extraction.placements
        }
        None => placements::PlacementsFile::load(&placements_path)
            .with_context(|| {
                format!(
                    "--skip-icons needs the committed {} to fold NPC placements into the packs",
                    placements_path.display()
                )
            })?
            .into_placements()?,
    };
    let supplements = xiv_gen::csv_to_rkyv::Supplements {
        npc_placements,
        leve_issuers,
    };

    let xiv_db_dir = data_dir.join("xiv-db");
    let output = db::build_packs(&layout.datamining, &xiv_db_dir, &supplements)?;
    println!("\nxiv-db packs -> {}", xiv_db_dir.display());
    for pack in &output.packs {
        println!(
            "  {:<3} {:>6} items  {:>7.2}MB raw -> {:>6.2}MB packed",
            pack.lang.to_path_part(),
            pack.items,
            mib(pack.raw_bytes),
            mib(pack.packed_bytes),
        );
    }

    let Some(install) = install else {
        println!("\nicons, maps and placement extraction skipped");
        return Ok(());
    };

    let placed_vendors = output.en_npc_placements.len();
    placements::PlacementsFile::from_placements(&install.version, &output.en_npc_placements)
        .save(&placements_path)?;
    println!("npc placements -> {}", placements_path.display());
    println!(
        "  {placed_vendors} NPCs with a placement ({} gil-shop vendors and {} exchange NPCs in \
         the en data; the rest are housing servants and seasonal stalls)",
        output.en_vendor_npcs, output.en_exchange_npcs
    );

    println!("\nextracting maps from FFXIV {}", install.version);
    let mut map_images = Vec::with_capacity(output.en_maps_used.len());
    let mut maps_missing = Vec::new();
    for (map_id, stem) in &output.en_maps_used {
        match install.map(stem)? {
            Some(image) => map_images.push((*map_id, image)),
            None => maps_missing.push((*map_id, stem.clone())),
        }
    }
    let maps_path = data_dir.join("maps").join("maps.tar.zst");
    let map_stats = maps::build_pack(map_images, &maps_path)?;
    println!("maps -> {}", maps_path.display());
    println!(
        "  {} maps at {}px, {:.2}MB tar -> {:.2}MB zst",
        map_stats.maps,
        maps::MAP_PX,
        mib(map_stats.tar_bytes),
        mib(map_stats.packed_bytes),
    );
    if !maps_missing.is_empty() {
        println!(
            "  {} referenced maps have no texture in the client: {:?}",
            maps_missing.len(),
            maps_missing.iter().take(10).collect::<Vec<_>>()
        );
    }

    println!("\nextracting icons from FFXIV {}", install.version);
    let extracted = extract_icons(&install, &output.en_named_items)?;
    // An indexed-but-unreadable icon is never acceptable: the file is right
    // there and this toolchain cannot read it. Silently counting these as
    // absent is what made a decoder gap look like a stale game client, so they
    // stop the run outright rather than quietly shrinking the pack.
    if !extracted.unreadable.is_empty() {
        let sample = extracted
            .unreadable
            .iter()
            .take(5)
            .map(|(item, why)| format!("\n  item {item}: {why}"))
            .collect::<String>();
        bail!(
            "{} of {} named items have an icon that IS indexed in the install but could not be \
             read.{sample}\nThese are present in the game files — a stale client cannot explain \
             them. Usually a stale `ironworks` that does not know a newer SqPack file kind. Not \
             writing a pack that drops them.",
            extracted.unreadable.len(),
            output.en_named_items.len(),
        );
    }
    if extraction_looks_broken(extracted.missing.len(), &output.en_named_items) {
        bail!(
            "{} of {} named items reference an icon id the install does not index — too many to \
             be a stale-client tail, so the extraction itself is broken (wrong --game-path). Not \
             writing a gutted icon pack.",
            extracted.missing.len(),
            output.en_named_items.len(),
        );
    }
    let Extraction {
        icons,
        item_to_icon,
        missing,
        unreadable: _,
        iconless,
    } = extracted;

    let out_path = data_dir.join("icons").join("images.tar.zst");
    let stats = icons::build_pack(icons, &item_to_icon, &out_path)?;
    println!("icons -> {}", out_path.display());
    println!(
        "  {} unique icons for {} items, {} entries, {:.2}MB tar -> {:.2}MB zst",
        stats.unique_icons,
        stats.mapped_items,
        stats.entries,
        mib(stats.tar_bytes),
        mib(stats.packed_bytes),
    );
    println!("  {iconless} named items have Icon = 0 (permanently iconless rows)");
    report_missing_icons(&missing, output.en_named_items.len());

    // Record which client the icons came out of. An offline CSV build has no
    // loaded manifest, but it still regenerated the icon pack, so update the
    // provenance there too.
    let mut manifest = Manifest::load(&manifest_path)?;
    manifest.icons = Some(manifest::IconPack {
        game_version: install.version.clone(),
    });
    manifest.save(&manifest_path)?;

    Ok(())
}

struct Extraction {
    /// `(icon id, image)` for every distinct icon that decoded.
    icons: Vec<(i32, image::RgbaImage)>,
    /// `(item id, icon id)` for every item whose icon is in `icons`.
    item_to_icon: Vec<(i32, i32)>,
    /// Item ids whose non-zero icon id is genuinely not indexed in the install.
    missing: Vec<i32>,
    /// Item ids whose icon *is* indexed but could not be read, with the reason.
    /// Distinct from `missing` on purpose: absent icons are a normal
    /// consequence of CSVs running ahead of the client, whereas an unreadable
    /// one is always a defect in this toolchain (stale ironworks, new container
    /// shape). Folding the two together previously disguised a decoder gap as a
    /// stale game client.
    unreadable: Vec<(i32, String)>,
    /// Named items with `Icon == 0` — iconless by data, not by staleness.
    iconless: usize,
}

/// Decodes every distinct icon id the named items reference.
fn extract_icons(
    install: &icon_extract::GameInstall,
    named_items: &[(i32, i32)],
) -> anyhow::Result<Extraction> {
    use icon_extract::IconRead;
    /// What a previously-seen icon id turned out to be, so each id is only
    /// read once however many items share it.
    #[derive(Clone)]
    enum Seen {
        Readable,
        Absent,
        Unreadable(String),
    }
    let mut decoded: std::collections::HashMap<i32, Seen> = std::collections::HashMap::new();
    let mut extraction = Extraction {
        icons: Vec::new(),
        item_to_icon: Vec::with_capacity(named_items.len()),
        missing: Vec::new(),
        unreadable: Vec::new(),
        iconless: 0,
    };
    for &(item_id, icon_id) in named_items {
        if icon_id == 0 {
            extraction.iconless += 1;
            continue;
        }
        let seen = match decoded.entry(icon_id) {
            std::collections::hash_map::Entry::Occupied(seen) => seen.get().clone(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                let seen = match install.icon(icon_id)? {
                    IconRead::Image(image) => {
                        extraction.icons.push((icon_id, image));
                        Seen::Readable
                    }
                    IconRead::Absent => Seen::Absent,
                    IconRead::Unreadable { path, reason } => {
                        Seen::Unreadable(format!("{path}: {reason}"))
                    }
                };
                vacant.insert(seen).clone()
            }
        };
        match seen {
            Seen::Readable => extraction.item_to_icon.push((item_id, icon_id)),
            Seen::Absent => extraction.missing.push(item_id),
            Seen::Unreadable(why) => extraction.unreadable.push((item_id, why)),
        }
    }
    Ok(extraction)
}

/// True when so many icon-bearing items came back unreadable that the problem
/// has to be systemic rather than a stale-client tail. A client one patch
/// behind the pinned CSVs measures ~1-3% missing; a wrong install path or an
/// unparseable client format measures near 100%.
fn extraction_looks_broken(missing: usize, named_items: &[(i32, i32)]) -> bool {
    let with_icons = named_items.iter().filter(|(_, icon)| *icon != 0).count();
    missing * 10 > with_icons
}

/// Rewrites every pinned SHA to whatever the upstream branch points at now.
fn update_pins(manifest: &mut Manifest, run_dir: &Path) -> anyhow::Result<()> {
    for (name, source) in manifest.sources.iter_mut() {
        let latest = fetch::latest_sha(run_dir, source)?;
        if latest == source.sha {
            println!("{name}: {} (unchanged)", source.sha);
        } else {
            println!("{name}: {} -> {latest}", source.sha);
            source.sha = latest;
        }
    }
    Ok(())
}

/// Reports named items whose icon could not be extracted; these render as the
/// fallback image.
///
/// Extraction reads the game files directly, so unlike the old universalis
/// pipeline the count should be near zero. A large count almost always means
/// the install is behind the pinned CSVs — patch the game and re-run. The
/// preview shows the *highest* ids because a new game version appends its items
/// at the top of the id range.
fn report_missing_icons(missing: &[i32], named_items: usize) {
    if missing.is_empty() {
        println!("  every named item has an icon");
        return;
    }
    let preview = newest_missing(missing, 20);
    println!(
        "  {} of {named_items} named items have no icon in the install \
         (client older than the pinned CSVs?); highest ids: {}{}",
        missing.len(),
        preview
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        if missing.len() > preview.len() {
            ", ..."
        } else {
            ""
        },
    );
}

/// The `limit` highest ids from an ascending list, highest first.
fn newest_missing(missing: &[i32], limit: usize) -> Vec<i32> {
    missing.iter().rev().take(limit).copied().collect()
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> anyhow::Result<Args> {
    let mut args = Args {
        pins: Pins::Pinned,
        offline_source: None,
        skip_icons: false,
        game_path: None,
    };
    let mut argv = argv.into_iter();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--pinned" => args.pins = Pins::Pinned,
            "--latest" => args.pins = Pins::Latest,
            "--skip-icons" => args.skip_icons = true,
            "--offline-source" => {
                let path = argv.next().context("--offline-source needs a path")?;
                if path.starts_with("--") {
                    bail!("--offline-source needs a path, got the flag {path:?}");
                }
                args.offline_source = Some(PathBuf::from(path));
            }
            "--game-path" => {
                let path = argv.next().context("--game-path needs a path")?;
                if path.starts_with("--") {
                    bail!("--game-path needs a path, got the flag {path:?}");
                }
                args.game_path = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other:?}"),
        }
    }
    if args.pins == Pins::Latest && args.offline_source.is_some() {
        bail!("--latest cannot be combined with --offline-source: an offline build never fetches");
    }
    Ok(args)
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

#[cfg(test)]
mod arg_tests {
    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Args> {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn defaults_to_a_pinned_fetching_build() {
        let args = parse(&[]).expect("no args should parse");
        assert_eq!(args.pins, Pins::Pinned);
        assert_eq!(args.offline_source, None);
        assert!(!args.skip_icons);
    }

    #[test]
    fn flags_are_parsed() {
        let args = parse(&["--latest", "--skip-icons"]).expect("should parse");
        assert_eq!(args.pins, Pins::Latest);
        assert!(args.skip_icons);

        let args = parse(&["--offline-source", "/repo", "--pinned"]).expect("should parse");
        assert_eq!(args.offline_source, Some(PathBuf::from("/repo")));
        assert_eq!(args.pins, Pins::Pinned);
    }

    #[test]
    fn rejects_unknown_and_incomplete_arguments() {
        assert!(parse(&["--wat"]).is_err());
        assert!(parse(&["--offline-source"]).is_err());
    }

    #[test]
    fn offline_source_does_not_swallow_the_next_flag() {
        assert!(parse(&["--offline-source", "--skip-icons"]).is_err());
    }

    #[test]
    fn broken_extraction_is_a_ratio_of_icon_bearing_items() {
        // 3 items carry icons; 1 missing (33%) is broken, while iconless rows
        // never count toward the denominator.
        let named = [(1, 100), (2, 0), (3, 101), (4, 102)];
        assert!(extraction_looks_broken(1, &named));
        assert!(!extraction_looks_broken(0, &named));
        // A stale-client tail (~3%) stays under the 10% tripwire.
        let many: Vec<(i32, i32)> = (0..1000).map(|i| (i, 100 + i)).collect();
        assert!(!extraction_looks_broken(30, &many));
        assert!(extraction_looks_broken(101, &many));
    }

    #[test]
    fn game_path_takes_a_path_argument() {
        let args = parse(&["--game-path", "D:/ffxiv"]).expect("should parse");
        assert_eq!(args.game_path, Some(PathBuf::from("D:/ffxiv")));
        assert!(parse(&["--game-path"]).is_err());
        assert!(parse(&["--game-path", "--skip-icons"]).is_err());
    }

    #[test]
    fn missing_preview_shows_the_highest_ids_first() {
        // New game content lands at the top of the id range, so the tail of the
        // ascending list is the part worth showing.
        let missing: Vec<i32> = (1..=50).collect();
        assert_eq!(newest_missing(&missing, 3), vec![50, 49, 48]);
        // Fewer entries than the limit is fine, and still highest-first.
        assert_eq!(newest_missing(&[7, 9], 20), vec![9, 7]);
        assert!(newest_missing(&[], 20).is_empty());
    }

    #[test]
    fn rejects_latest_with_offline_source() {
        assert!(parse(&["--latest", "--offline-source", "/repo"]).is_err());
    }
}

#[cfg(test)]
mod lfs_guard_tests {
    use super::lfs_guard::assert_not_lfs_pointer;
    use std::io::Write;

    #[test]
    fn panics_on_lfs_pointer_stub() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pointer.bin");
        let mut f = std::fs::File::create(&path).expect("create pointer file");
        write!(
            f,
            "version https://git-lfs.github.com/spec/v1\noid sha256:0000000000000000000000000000000000000000000000000000000000000000\nsize 12345\n"
        )
        .expect("write pointer contents");
        drop(f);

        let result = std::panic::catch_unwind(|| assert_not_lfs_pointer(&path));
        let err = result.expect_err("expected a panic for an lfs pointer stub");
        let message = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("panic payload should be a string");
        assert!(
            message.contains("git-lfs pointer"),
            "unexpected panic message: {message}"
        );
        assert!(
            message.contains("git lfs install && git lfs pull"),
            "unexpected panic message: {message}"
        );
    }

    #[test]
    fn passes_on_real_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("real.bin");
        std::fs::write(&path, [0xDEu8, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4]).expect("write real file");

        let result = std::panic::catch_unwind(|| assert_not_lfs_pointer(&path));
        assert!(result.is_ok(), "did not expect a panic for a real file");
    }
}
