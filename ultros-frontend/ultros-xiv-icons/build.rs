use futures::{StreamExt, stream};
use image::{ImageFormat, ImageReader, imageops::FilterType};
use std::{
    env,
    ffi::OsStr,
    fs::{DirEntry, read_dir},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Instant,
};
use tar::{Builder, Header};
use tempfile::TempDir;
use ultros_api_types::icon_size::IconSize;

// Worktree-fallback helpers (main-worktree discovery, pin-drift detection)
// shared with `xiv-gen/src/csv_to_rkyv.rs` — a single source of truth so the
// two build paths can't drift. Its unit tests run through the xiv-gen side:
// `cargo test -p xiv-gen --features csv_to_rkyv`.
include!("../../xiv-gen/src/worktree_fallback.rs");

/// Locate the `universalis-assets` checkout the icons are read from.
///
/// Resolution order: `UNIVERSALIS_ASSETS_DIR` env override, then the submodule
/// next to this crate, then the main git worktree's copy — linked worktrees
/// rarely have submodules initialized, but the main checkout usually does.
/// Mirrors `resolve_datamining_dir` in `xiv-gen/src/csv_to_rkyv.rs`; the
/// shared mechanics live in the include!d `worktree_fallback.rs`.
fn universalis_assets_dir(manifest_dir: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("UNIVERSALIS_ASSETS_DIR") {
        let dir = PathBuf::from(dir);
        assert!(
            assets_populated(&dir),
            "UNIVERSALIS_ASSETS_DIR is set to {} but icon2x/ is missing or incomplete there \
             (fewer than {MIN_ICON_COUNT} files)",
            dir.display()
        );
        return dir;
    }
    let local = manifest_dir.join("universalis-assets");
    if assets_populated(&local) {
        return local;
    }
    if let Some(main) = main_worktree(manifest_dir) {
        let candidate = main
            .join("ultros-frontend")
            .join("ultros-xiv-icons")
            .join("universalis-assets");
        if candidate != local && assets_populated(&candidate) {
            if let Some(drift) = pin_drift_warning(manifest_dir, "universalis-assets", &candidate) {
                println!("cargo:warning={drift}");
            }
            println!(
                "cargo:warning=universalis-assets submodule not populated in this checkout; \
                 falling back to {}",
                candidate.display()
            );
            return candidate;
        }
    }
    panic!(
        "could not find a populated universalis-assets checkout. Either initialize the \
         submodule (see CLAUDE.md), or set UNIVERSALIS_ASSETS_DIR to an existing checkout. \
         Looked at {} and the main worktree.",
        local.display()
    )
}

/// Minimum number of entries `icon2x/` must contain to count as populated.
/// The full asset set is ~17.2k PNGs (17,208 as of 2026-07, data version
/// 7.55); an interrupted shallow fetch typically leaves the dir empty or with
/// a small partial set. 10k leaves headroom in both directions.
const MIN_ICON_COUNT: usize = 10_000;

/// A failed shallow submodule fetch can leave `universalis-assets` present
/// but empty or partially fetched, so require icon2x to contain a plausible
/// number of files — not just any single entry — before trusting it. An
/// incomplete local copy then falls back instead of shipping a truncated
/// icon tarball.
fn assets_populated(dir: &Path) -> bool {
    std::fs::read_dir(dir.join("icon2x"))
        .map(|entries| entries.take(MIN_ICON_COUNT).count() >= MIN_ICON_COUNT)
        .unwrap_or(false)
}

/// Resizes all xiv-icons and bundles them
async fn resize_all_images(assets: &Path, out_dir: &Path) {
    let path = std::fs::canonicalize(assets)
        .unwrap_or_else(|error| panic!("{error}\n{}", assets.display()));
    println!("opening {path:?}");
    let mut paths = vec![];
    for file in read_dir(&path).unwrap_or_else(|error| panic!("{error}\n{path:?}")) {
        let entry = file.expect("Unable to read file");
        paths.push(entry);
    }
    let len = paths.len();
    let progress_bar = indicatif::ProgressBar::new(len as u64);
    let bar = &progress_bar;
    stream::iter(paths)
        .for_each_concurrent(Some(50), |path| async move {
            let out_dir = out_dir.to_path_buf();
            let handle = tokio::spawn(async move {
                resize_image(path, &out_dir).await;
                // bar.inc(1);
            });
            handle.await.unwrap();
            bar.inc(1);
        })
        .await;
}

async fn resize_image(entry: DirEntry, out_dir: &PathBuf) -> Option<()> {
    // create three sizes of images
    let file = entry.file_name();
    let file = file.to_str()?;
    let (file_name, _) = file.split_once('.')?;
    let path = entry.path();
    let extension = path.extension().and_then(OsStr::to_str)?;
    if extension != "png" {
        return None;
    }
    let data = tokio::fs::read(entry.path())
        .await
        .unwrap_or_else(|error| panic!("{error:?} {entry:?}"));
    let image = ImageReader::new(Cursor::new(&data))
        .with_guessed_format()
        .unwrap_or_else(|error| panic!("{error:?} {entry:?}"));
    let image = image
        .decode()
        .unwrap_or_else(|error| panic!("{error:?}\n{entry:?}"));
    let image = &image;

    // let out_dir = env::var("OUT_DIR").unwrap();
    // let out_dir = out_dir.as_str();
    let resize = async move |icon_size: IconSize| {
        let size = icon_size.get_px_size();
        let resized = image.resize(size, size, FilterType::CatmullRom);

        let file = vec![];
        let mut cursor = Cursor::new(file);
        resized.write_to(&mut cursor, ImageFormat::WebP).unwrap();
        let path = format!("{file_name}_{icon_size}.webp");
        let path = out_dir.join(path);
        tokio::fs::write(path, cursor.into_inner())
            .await
            .unwrap_or_else(|e| panic!("{e}\n{out_dir:?}"));

        // resized.save(format!("{out_dir}/{file_name}{icon_size:?}.webp")).unwrap_or_else(|_| panic!("Error saving file {entry:?}"));
    };
    resize(IconSize::Large).await;
    resize(IconSize::Medium).await;
    resize(IconSize::Small).await;
    Some(())
}

async fn compress(path: &PathBuf) {
    let dir = std::fs::read_dir(path).unwrap();
    let mut entries = vec![];
    for entry in dir {
        entries.push(entry.unwrap());
    }
    let values: Vec<_> = stream::iter(entries)
        .map(|entry| async move {
            let file = tokio::fs::read(entry.path()).await.unwrap();
            let file_name = entry.file_name().to_str().unwrap().to_string();
            (file_name, file)
        })
        .buffered(50)
        .collect()
        .await;
    let archive = vec![];
    let archive = Cursor::new(archive);
    let mut tar = Builder::new(archive);
    for (file, data) in values {
        let mut header = Header::new_gnu();
        header.set_path(&file).unwrap();
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, file, Cursor::new(data))
            .unwrap();
    }
    tar.finish().unwrap();
    let cursor = tar.into_inner().unwrap();
    // Write tar to a zstd-compressed file. Level 19 is the highest "normal"
    // level (20-22 are --ultra and need much more memory for diminishing
    // returns on already-entropy-coded webp content). This build script runs
    // once and its output is cached, so we can afford the slowest reasonable
    // level for the best ratio.
    let mut compressed = zstd::Encoder::new(Vec::new(), 19).unwrap();
    compressed
        .write_all(cursor.into_inner().as_slice())
        .unwrap();
    let compress = compressed.finish().unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out_dir}/images.tar.zst"), compress).unwrap();
}

#[tokio::main]
async fn main() {
    println!("cargo:rerun-if-changed=./build.rs");
    // build.rs textually include!s the shared worktree-fallback helpers, so
    // watch that file too (the path is relative to this package's root).
    println!("cargo:rerun-if-changed=../../xiv-gen/src/worktree_fallback.rs");
    println!("cargo:rerun-if-env-changed=UNIVERSALIS_ASSETS_DIR");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let assets = universalis_assets_dir(Path::new(&manifest_dir)).join("icon2x");
    // Register the *resolved* dir: a fixed `./universalis-assets/icon2x` would
    // not exist in a worktree using the fallback, and cargo re-runs the build
    // script on every build when a rerun-if-changed path is missing.
    println!("cargo:rerun-if-changed={}", assets.display());
    let instant = Instant::now();
    let temp_dir = TempDir::new().unwrap();
    resize_all_images(&assets, temp_dir.path()).await;
    compress(&temp_dir.path().to_path_buf()).await;
    println!(
        "Finished resizing {}ms",
        Instant::now().duration_since(instant).as_millis()
    );
}
