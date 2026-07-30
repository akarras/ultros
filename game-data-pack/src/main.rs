//! Generates the LFS-tracked game-data packs under `data/`.
//!
//! Fetches the pinned upstream sources (sparse + blobless), runs the CSV→rkyv
//! pipeline for every language and the PNG→WebP pipeline for the item icons.

mod db;
mod fetch;
mod icons;
mod manifest;

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
  --offline-source <path>  Build from an already-populated ultros checkout
                           instead of fetching anything
  --skip-icons             Skip the icon pack (and the universalis-assets fetch)
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

    let layout = match &args.offline_source {
        Some(checkout) => {
            println!("building from {} (no fetching)", checkout.display());
            Layout::offline(checkout, args.skip_icons)
        }
        None => {
            let manifest_path = data_dir.join("manifest.toml");
            let mut manifest = Manifest::load(&manifest_path)?;
            let cache_root = repo_root.join(".game-data-cache");
            if args.pins == Pins::Latest {
                std::fs::create_dir_all(&cache_root)
                    .with_context(|| format!("creating cache root {}", cache_root.display()))?;
                update_pins(&mut manifest, &cache_root, args.skip_icons)?;
                manifest.save(&manifest_path)?;
                println!("updated {}", manifest_path.display());
            }
            fetch::fetch_all(&cache_root, &manifest, args.skip_icons)?
        }
    };

    let xiv_db_dir = data_dir.join("xiv-db");
    let output = db::build_packs(&layout.datamining, &xiv_db_dir)?;
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

    match &layout.icons {
        None => println!("\nicons skipped"),
        Some(icon_dir) => {
            let out_path = data_dir.join("icons").join("images.tar.zst");
            let stats = icons::build_pack(icon_dir, &out_path)?;
            println!("\nicons -> {}", out_path.display());
            println!(
                "  {} source PNGs, {} entries, {:.2}MB tar -> {:.2}MB zst",
                stats.source_pngs,
                stats.entries,
                mib(stats.tar_bytes),
                mib(stats.packed_bytes),
            );
            report_missing_icons(icon_dir, &output.en_named_item_ids)?;
        }
    }

    Ok(())
}

/// Rewrites every pinned SHA to whatever the upstream branch points at now.
fn update_pins(manifest: &mut Manifest, run_dir: &Path, skip_icons: bool) -> anyhow::Result<()> {
    for (name, source) in manifest.sources.iter_mut() {
        if skip_icons && name == fetch::ICONS_SOURCE {
            // Bumping this pin without regenerating the icon pack would make the
            // manifest claim data/icons was built from a SHA it never saw.
            println!("{name}: pin left at {} (--skip-icons)", source.sha);
            continue;
        }
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

/// Reports named items the upstream icon repo has no PNG for; these render as a
/// blank icon.
///
/// The absolute count is inherently large — measured on 7.55 the icon repo ships
/// 17,209 files against 50,773 named items, so ~33.5k named items legitimately
/// have no icon of their own. What is actionable is (a) how the count moves
/// between runs and (b) the preview, which shows the *highest* ids because a new
/// game version appends its items at the top of the id range. Previewing the low
/// end would just reprint the same permanently-iconless legacy rows every run.
fn report_missing_icons(icon_dir: &Path, named_item_ids: &[i32]) -> anyhow::Result<()> {
    let missing = icons::missing_icon_ids(icon_dir, named_item_ids)?;
    if missing.is_empty() {
        println!("  every named item has an upstream icon");
        return Ok(());
    }
    let preview = newest_missing(&missing, 20);
    println!(
        "  {} of {} named items have no upstream icon; highest ids: {}{}",
        missing.len(),
        named_item_ids.len(),
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
    Ok(())
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
