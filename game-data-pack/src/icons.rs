//! Builds `data/icons/images.tar.zst` from the upstream `icon2x` PNGs.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use image::ImageReader;
use image::imageops::FilterType;
use rayon::prelude::*;
use tar::{Builder, Header};
use ultros_api_types::icon_size::IconSize;

/// Sizes packed for every icon, largest first.
pub const ICON_SIZES: [IconSize; 3] = [IconSize::Large, IconSize::Medium, IconSize::Small];

/// Lossy WebP quality. 85 keeps the icons visually clean at 60px and under
/// while cutting the archive to roughly a third of the lossless encode.
const WEBP_QUALITY: f32 = 85.0;

/// zstd level 19 is the highest "normal" level (20-22 are --ultra and need much
/// more memory for diminishing returns on already-entropy-coded webp content).
/// This runs once per data bump, so the slowest reasonable level is affordable.
const ZSTD_LEVEL: i32 = 19;

/// What the icon pack cost.
pub struct IconStats {
    pub source_pngs: usize,
    pub entries: usize,
    pub tar_bytes: usize,
    pub packed_bytes: usize,
}

/// Name of a single icon inside the tar.
///
/// `ultros-xiv-icons` parses these back at runtime by splitting on `.` then `_`
/// (see `ultros-frontend/ultros-xiv-icons/src/lib.rs::parse_url`), so the stem
/// must stay the numeric item id and the suffix must stay `IconSize`'s `Display`.
pub fn entry_name(file_stem: &str, size: IconSize) -> String {
    format!("{file_stem}_{size}.webp")
}

/// Resizes every PNG in `icon_dir` to the three icon sizes and writes the
/// zstd-compressed tar of WebPs to `out_path`.
pub fn build_pack(icon_dir: &Path, out_path: &Path) -> anyhow::Result<IconStats> {
    let mut sources = png_paths(icon_dir)?;
    // Archive order is part of the committed artifact; keep it stable so a
    // re-run with unchanged inputs does not churn the LFS object.
    sources.sort();

    let encoded: Vec<Vec<(String, Vec<u8>)>> = sources
        .par_iter()
        .map(|path| encode_icon(path))
        .collect::<anyhow::Result<_>>()?;

    let mut tar = Builder::new(Cursor::new(Vec::new()));
    let mut entries = 0;
    for (name, data) in encoded.iter().flatten() {
        let mut header = Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        // `append_data` fills in the path and the checksum; everything else in a
        // fresh gnu header (mtime, uid, gid) is zero, which keeps this reproducible.
        tar.append_data(&mut header, name, Cursor::new(data.as_slice()))
            .with_context(|| format!("appending {name} to the icon archive"))?;
        entries += 1;
    }
    let tar_bytes = tar
        .into_inner()
        .context("finishing the icon archive")?
        .into_inner();

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut encoder = zstd::Encoder::new(Vec::new(), ZSTD_LEVEL).context("starting zstd")?;
    encoder
        .write_all(&tar_bytes)
        .context("compressing the icon archive")?;
    let packed = encoder.finish().context("finishing zstd")?;
    std::fs::write(out_path, &packed).with_context(|| format!("writing {}", out_path.display()))?;

    Ok(IconStats {
        source_pngs: sources.len(),
        entries,
        tar_bytes: tar_bytes.len(),
        packed_bytes: packed.len(),
    })
}

/// Item ids from the game data that have no `<id>.png` upstream, in the order
/// they were given.
pub fn missing_icon_ids(icon_dir: &Path, item_ids: &[i32]) -> anyhow::Result<Vec<i32>> {
    let available = available_icon_ids(icon_dir)?;
    Ok(item_ids
        .iter()
        .copied()
        .filter(|id| !available.contains(id))
        .collect())
}

fn available_icon_ids(icon_dir: &Path) -> anyhow::Result<BTreeSet<i32>> {
    Ok(png_paths(icon_dir)?
        .iter()
        .filter_map(|path| path.file_stem()?.to_str()?.parse().ok())
        .collect())
}

fn encode_icon(path: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("icon {} has a non-utf8 name", path.display()))?;
    let image = ImageReader::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("sniffing the format of {}", path.display()))?
        .decode()
        .with_context(|| format!("decoding {}", path.display()))?;

    ICON_SIZES
        .iter()
        .map(|&size| {
            let px = size.get_px_size();
            let resized = image.resize(px, px, FilterType::CatmullRom).to_rgba8();
            let (width, height) = resized.dimensions();
            let webp = webp::Encoder::from_rgba(resized.as_raw(), width, height)
                .encode(WEBP_QUALITY)
                .to_vec();
            Ok((entry_name(stem, size), webp))
        })
        .collect()
}

fn png_paths(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("reading icon dir {}", dir.display()))?
    {
        let path = entry
            .with_context(|| format!("reading icon dir {}", dir.display()))?
            .path();
        if path.extension().and_then(OsStr::to_str) == Some("png") {
            paths.push(path);
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    /// Hand-rolled `^\d+_(Large|Medium|Small)\.webp$`.
    fn matches_runtime_pattern(name: &str) -> bool {
        let Some((stem, size)) = name
            .strip_suffix(".webp")
            .and_then(|base| base.split_once('_'))
        else {
            return false;
        };
        !stem.is_empty()
            && stem.bytes().all(|b| b.is_ascii_digit())
            && matches!(size, "Large" | "Medium" | "Small")
    }

    fn write_test_png(path: &Path) {
        let mut image = image::RgbaImage::new(80, 80);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgba([x as u8, y as u8, 128, 255]);
        }
        image.save(path).expect("write test png");
    }

    #[test]
    fn entry_names_match_the_runtime_parser() {
        for size in ICON_SIZES {
            let name = entry_name("18355", size);
            assert!(
                matches_runtime_pattern(&name),
                "entry name {name:?} does not match ^\\d+_(Large|Medium|Small)\\.webp$"
            );
            assert!(name.contains(&size.to_string()));
        }
    }

    #[test]
    fn entry_names_are_unique_per_size() {
        let names: Vec<_> = ICON_SIZES.map(|s| entry_name("10", s)).to_vec();
        assert_eq!(names, ["10_Large.webp", "10_Medium.webp", "10_Small.webp"]);
    }

    #[test]
    fn pattern_helper_rejects_bad_names() {
        assert!(!matches_runtime_pattern("10_Huge.webp"));
        assert!(!matches_runtime_pattern("abc_Large.webp"));
        assert!(!matches_runtime_pattern("10_Large.png"));
    }

    #[test]
    fn missing_ids_are_the_ones_without_a_png() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("10.png"), b"not really a png").expect("write");
        std::fs::write(dir.path().join("20.png"), b"not really a png").expect("write");
        std::fs::write(dir.path().join("notes.txt"), b"ignored").expect("write");

        assert_eq!(
            missing_icon_ids(dir.path(), &[10, 20, 30, 40]).expect("scan"),
            vec![30, 40]
        );
    }

    #[test]
    fn pack_round_trips_through_zstd_and_tar() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_test_png(&dir.path().join("18355.png"));
        write_test_png(&dir.path().join("10.png"));
        std::fs::write(dir.path().join("README.md"), b"ignored").expect("write");

        let out = dir.path().join("out").join("images.tar.zst");
        let stats = build_pack(dir.path(), &out).expect("build the icon pack");
        assert_eq!(stats.source_pngs, 2);
        assert_eq!(stats.entries, 6);
        assert!(stats.packed_bytes > 0);

        let packed = std::fs::read(&out).expect("read the pack");
        let mut tar_bytes = Vec::new();
        zstd::Decoder::new(Cursor::new(packed))
            .expect("zstd decoder")
            .read_to_end(&mut tar_bytes)
            .expect("decompress");

        let mut archive = tar::Archive::new(Cursor::new(tar_bytes));
        let mut names: Vec<String> = Vec::new();
        for entry in archive.entries().expect("entries") {
            let entry = entry.expect("entry");
            let name = entry.path().expect("path").display().to_string();
            assert!(
                matches_runtime_pattern(&name),
                "packed entry {name:?} is not parseable at runtime"
            );
            names.push(name);
        }
        names.sort();
        assert_eq!(
            names,
            [
                "10_Large.webp",
                "10_Medium.webp",
                "10_Small.webp",
                "18355_Large.webp",
                "18355_Medium.webp",
                "18355_Small.webp",
            ]
        );
    }
}
