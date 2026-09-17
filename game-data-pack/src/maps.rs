//! Builds `data/maps/maps.tar.zst`: one WebP per in-game map the NPC pages
//! can show a pin on, decoded out of the client's `ui/map/*.tex` textures.
//!
//! Only maps that carry a packed NPC placement are included — the client has
//! several hundred maps, most of them dungeon floors nobody buys anything on.
//! Textures are 2048px; they are packed at [`MAP_PX`] because the page shows
//! them in a panel a few hundred pixels wide and zooms with CSS, and a full
//! 2048px WebP per map would quadruple the archive for no visible gain.
//!
//! Entry names are `<map id>.webp`, parsed back by `ultros-xiv-icons`.

use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::{Context, anyhow};
use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;
use tar::{Builder, Header};

/// Packed edge length in pixels.
pub const MAP_PX: u32 = 1024;
/// Lossy WebP quality. Map art is soft-edged painting; q80 is visually clean.
const WEBP_QUALITY: f32 = 80.0;
const ZSTD_LEVEL: i32 = 19;

pub struct MapStats {
    pub maps: usize,
    pub tar_bytes: usize,
    pub packed_bytes: usize,
}

/// Name of one map inside the tar; `ultros-xiv-icons` parses the stem back.
pub fn entry_name(map_id: i32) -> String {
    format!("{map_id}.webp")
}

/// Encodes every `(map id, image)` and writes the archive to `out_path`.
/// Inputs may arrive in any order; the archive is sorted by map id so a
/// re-run with unchanged inputs does not churn LFS.
pub fn build_pack(mut maps: Vec<(i32, RgbaImage)>, out_path: &Path) -> anyhow::Result<MapStats> {
    maps.sort_by_key(|(id, _)| *id);
    let encoded: Vec<(String, Vec<u8>)> = maps
        .par_iter()
        .map(|(id, image)| encode_map(*id, image))
        .collect::<anyhow::Result<_>>()?;

    let mut tar = Builder::new(Cursor::new(Vec::new()));
    for (name, data) in &encoded {
        let mut header = Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        tar.append_data(&mut header, name, Cursor::new(data.as_slice()))
            .with_context(|| format!("appending {name} to the map archive"))?;
    }
    let tar_bytes = tar
        .into_inner()
        .context("finishing the map archive")?
        .into_inner();

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut encoder = zstd::Encoder::new(Vec::new(), ZSTD_LEVEL).context("starting zstd")?;
    encoder
        .write_all(&tar_bytes)
        .context("compressing the map archive")?;
    let packed = encoder.finish().context("finishing zstd")?;
    std::fs::write(out_path, &packed).with_context(|| format!("writing {}", out_path.display()))?;

    Ok(MapStats {
        maps: maps.len(),
        tar_bytes: tar_bytes.len(),
        packed_bytes: packed.len(),
    })
}

fn encode_map(map_id: i32, image: &RgbaImage) -> anyhow::Result<(String, Vec<u8>)> {
    let image = DynamicImage::ImageRgba8(image.clone());
    let resized = if image.width() > MAP_PX || image.height() > MAP_PX {
        image
            .resize(MAP_PX, MAP_PX, FilterType::Lanczos3)
            .to_rgba8()
    } else {
        image.to_rgba8()
    };
    let (width, height) = resized.dimensions();
    let webp = webp::Encoder::from_rgba(resized.as_raw(), width, height)
        .encode_simple(false, WEBP_QUALITY)
        .map_err(|e| anyhow!("encoding map {map_id} failed: {e:?}"))?
        .to_vec();
    Ok((entry_name(map_id), webp))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn test_image(px: u32) -> RgbaImage {
        let mut image = RgbaImage::new(px, px);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgba([(x / 8) as u8, (y / 8) as u8, 96, 255]);
        }
        image
    }

    #[test]
    fn entry_names_are_the_map_id() {
        assert_eq!(entry_name(2), "2.webp");
        assert_eq!(entry_name(1234), "1234.webp");
    }

    #[test]
    fn pack_downscales_sorts_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("maps").join("maps.tar.zst");
        let stats = build_pack(vec![(14, test_image(2048)), (2, test_image(64))], &out).unwrap();
        assert_eq!(stats.maps, 2);

        let packed = std::fs::read(&out).unwrap();
        let mut tar_bytes = Vec::new();
        zstd::Decoder::new(Cursor::new(packed))
            .unwrap()
            .read_to_end(&mut tar_bytes)
            .unwrap();
        let mut archive = tar::Archive::new(Cursor::new(tar_bytes));
        let mut names = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().display().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap();
            let expected = if name == "14.webp" { MAP_PX } else { 64 };
            assert_eq!(
                (decoded.width(), decoded.height()),
                (expected, expected),
                "{name}"
            );
            names.push(name);
        }
        assert_eq!(names, ["2.webp", "14.webp"]);
    }
}
