//! Builds `data/icons/images.tar.zst` from icons decoded out of the game data.
//!
//! Inputs are `(item id, RGBA image)` pairs produced by `icon-extract` reading
//! the local FFXIV install. Deliberate choices:
//! - Two packed sizes only: Large (80px — the native `_hr1` resolution, so
//!   hidpi screens finally get a sharp icon) and Medium (40px). Small requests
//!   are served the Medium bytes by `ultros-xiv-icons`; a dedicated 25px encode
//!   saved almost nothing over WebP'd 40px and tripled the entry count.
//! - Lossy WebP q85 keeps the icons visually clean at display sizes while
//!   cutting the archive to roughly a third of the lossless encode.
//! - Inputs are sorted and gnu headers keep mtime/uid/gid at 0, so the archive
//!   is reproducible and a re-run with unchanged inputs does not churn LFS.

use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::{Context, anyhow};
use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;
use tar::{Builder, Header};
use ultros_api_types::icon_size::IconSize;

/// Packed sizes and their pixel dimensions, largest first. The pixel counts are
/// a pack concern, not a display concern — the CSS boxes in
/// `IconSize::get_size_px` stay 60/40/25.
pub const PACK_SIZES: [(IconSize, u32); 2] = [(IconSize::Large, 80), (IconSize::Medium, 40)];

/// Lossy WebP quality. See the module docs.
const WEBP_QUALITY: f32 = 85.0;

/// zstd level 19 is the highest "normal" level (20-22 are --ultra and need much
/// more memory for diminishing returns on already-entropy-coded webp content).
/// This runs once per data bump, so the slowest reasonable level is affordable.
const ZSTD_LEVEL: i32 = 19;

/// What the icon pack cost.
pub struct IconStats {
    pub unique_icons: usize,
    pub mapped_items: usize,
    pub entries: usize,
    pub tar_bytes: usize,
    pub packed_bytes: usize,
}

/// The tar entry mapping item ids to icon ids: ascii `<item id> <icon id>`
/// lines sorted by item id. Icons are stored once per *icon* id — thousands of
/// items share an icon, and duplicating the WebP per item doubled the pack and
/// the server's decompressed in-memory copy.
pub const ITEM_MAP_ENTRY: &str = "items.map";

/// Serialized `items.map` contents.
pub fn item_map_contents(item_to_icon: &[(i32, i32)]) -> String {
    let mut pairs: Vec<(i32, i32)> = item_to_icon.to_vec();
    pairs.sort_unstable();
    pairs
        .iter()
        .map(|(item, icon)| format!("{item} {icon}\n"))
        .collect()
}

/// Name of a single icon inside the tar.
///
/// `ultros-xiv-icons` parses these back at runtime by splitting on `.` then `_`
/// (see `ultros-frontend/ultros-xiv-icons/src/lib.rs`), so the stem must stay
/// the numeric icon id and the suffix must stay `IconSize`'s `Display`.
pub fn entry_name(icon_id: i32, size: IconSize) -> String {
    format!("{icon_id}_{size}.webp")
}

/// Resizes every unique icon to the packed sizes and writes the
/// zstd-compressed tar of WebPs (plus the `items.map` entry translating item
/// ids to icon ids) to `out_path`. `icons` may arrive in any order; the
/// archive is sorted by icon id.
pub fn build_pack(
    mut icons: Vec<(i32, RgbaImage)>,
    item_to_icon: &[(i32, i32)],
    out_path: &Path,
) -> anyhow::Result<IconStats> {
    // Archive order is part of the committed artifact; keep it stable so a
    // re-run with unchanged inputs does not churn the LFS object.
    icons.sort_by_key(|(icon_id, _)| *icon_id);

    let encoded: Vec<Vec<(String, Vec<u8>)>> = icons
        .par_iter()
        .map(|(icon_id, image)| encode_icon(*icon_id, image))
        .collect::<anyhow::Result<_>>()?;

    let map = item_map_contents(item_to_icon);
    let mut tar = Builder::new(Cursor::new(Vec::new()));
    let mut entries = 0;
    let map_entry = [(ITEM_MAP_ENTRY.to_string(), map.into_bytes())];
    for (name, data) in map_entry.iter().chain(encoded.iter().flatten()) {
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
        unique_icons: icons.len(),
        mapped_items: item_to_icon.len(),
        entries,
        tar_bytes: tar_bytes.len(),
        packed_bytes: packed.len(),
    })
}

fn encode_icon(icon_id: i32, image: &RgbaImage) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let image = DynamicImage::ImageRgba8(image.clone());
    PACK_SIZES
        .iter()
        .map(|&(size, px)| {
            let resized = image.resize(px, px, FilterType::CatmullRom).to_rgba8();
            let (width, height) = resized.dimensions();
            // `Encoder::encode` unwraps internally; go through `encode_simple` so
            // an encoder failure surfaces as an error instead of a panic.
            let webp = webp::Encoder::from_rgba(resized.as_raw(), width, height)
                .encode_simple(false, WEBP_QUALITY)
                .map_err(|e| anyhow!("encoding icon {icon_id} at {size} failed: {e:?}"))?
                .to_vec();
            Ok((entry_name(icon_id, size), webp))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    /// Hand-rolled `^\d+_(Large|Medium)\.webp$` — what the runtime can serve.
    fn matches_runtime_pattern(name: &str) -> bool {
        let Some((stem, size)) = name
            .strip_suffix(".webp")
            .and_then(|base| base.split_once('_'))
        else {
            return false;
        };
        !stem.is_empty()
            && stem.bytes().all(|b| b.is_ascii_digit())
            && matches!(size, "Large" | "Medium")
    }

    fn test_image(px: u32) -> RgbaImage {
        let mut image = RgbaImage::new(px, px);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgba([x as u8, y as u8, 128, 255]);
        }
        image
    }

    #[test]
    fn entry_names_match_the_runtime_parser() {
        for (size, _) in PACK_SIZES {
            let name = entry_name(18355, size);
            assert!(
                matches_runtime_pattern(&name),
                "entry name {name:?} does not match ^\\d+_(Large|Medium)\\.webp$"
            );
            assert!(name.contains(&size.to_string()));
        }
    }

    #[test]
    fn pack_stores_large_and_medium_only() {
        let names: Vec<_> = PACK_SIZES.map(|(s, _)| entry_name(10, s)).to_vec();
        assert_eq!(names, ["10_Large.webp", "10_Medium.webp"]);
    }

    #[test]
    fn large_is_the_native_hr1_resolution() {
        // 80px is what the game's `_hr1` icons actually are; storing less would
        // resample twice (once here, once in the browser).
        assert_eq!(PACK_SIZES[0], (IconSize::Large, 80));
        assert_eq!(PACK_SIZES[1], (IconSize::Medium, 40));
    }

    #[test]
    fn item_map_is_sorted_ascii_pairs() {
        assert_eq!(
            item_map_contents(&[(28, 65023), (1, 65002), (30, 65023)]),
            "1 65002\n28 65023\n30 65023\n"
        );
    }

    #[test]
    fn pack_round_trips_through_zstd_and_tar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out").join("images.tar.zst");
        // Two unique icons; icon 20801 is shared by two items.
        let stats = build_pack(
            vec![(20801, test_image(80)), (65002, test_image(72))],
            &[(5057, 20801), (1, 65002), (5058, 20801)],
            &out,
        )
        .expect("build the icon pack");
        assert_eq!(stats.unique_icons, 2);
        assert_eq!(stats.mapped_items, 3);
        assert_eq!(stats.entries, 5);
        assert!(stats.packed_bytes > 0);

        let packed = std::fs::read(&out).expect("read the pack");
        let mut tar_bytes = Vec::new();
        zstd::Decoder::new(Cursor::new(packed))
            .expect("zstd decoder")
            .read_to_end(&mut tar_bytes)
            .expect("decompress");

        let mut archive = tar::Archive::new(Cursor::new(tar_bytes));
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for entry in archive.entries().expect("entries") {
            let mut entry = entry.expect("entry");
            let name = entry.path().expect("path").display().to_string();
            assert!(
                name == ITEM_MAP_ENTRY || matches_runtime_pattern(&name),
                "packed entry {name:?} is not parseable at runtime"
            );
            let mut data = Vec::new();
            entry.read_to_end(&mut data).expect("entry data");
            entries.push((name, data));
        }
        let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "items.map",
                "20801_Large.webp",
                "20801_Medium.webp",
                "65002_Large.webp",
                "65002_Medium.webp",
            ]
        );
        assert_eq!(entries[0].1, b"1 65002\n5057 20801\n5058 20801\n");

        // The Large entry really is 80px, even when the source was the odd
        // 72px currency resolution (upscaled once here, not in the browser).
        let (_, large) = &entries[1];
        let decoded = webp::Decoder::new(large).decode().expect("decode webp");
        let image = decoded.to_image();
        assert_eq!((image.width(), image.height()), (80, 80));
    }
}
