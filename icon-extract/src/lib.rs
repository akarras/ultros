//! Extracts item icons from a local FFXIV install via ironworks.
//!
//! Replaces the old universalis-assets dependency (Lodestone-crawled PNGs that
//! only covered marketable items) with direct SqPack reads, so currencies and
//! untradable exchange rewards get icons too. The game ships icons as `.tex`
//! at `ui/icon/<group>/<id>.tex` (40px) with a `_hr1` 2x variant (80px); we
//! prefer the 2x and fall back to the base.
//!
//! A 7.55-era census of every icon referenced by the Item sheet found exactly
//! two pixel formats in use — Dxt1 (BC1) and Argb8 — so those are the only
//! decoders implemented. A new format shows up as a hard error naming the
//! format, not as a corrupt image.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use image::RgbaImage;
use ironworks::{
    ffxiv::FsResource,
    file::tex::{Format, Texture},
    sqpack::SqPack,
    Ironworks,
};

/// SqPack path of an icon's `.tex` file. Icons are grouped in
/// thousands-aligned directories: icon 65002 lives in `ui/icon/065000/`.
pub fn icon_sqpack_path(icon_id: i32, hr: bool) -> String {
    let group = (icon_id / 1000) * 1000;
    let suffix = if hr { "_hr1" } else { "" };
    format!("ui/icon/{group:06}/{icon_id:06}{suffix}.tex")
}

/// An opened FFXIV install we can read icons out of.
pub struct GameInstall {
    ironworks: Ironworks,
    /// Contents of `game/ffxivgame.ver`, e.g. `2026.07.16.0001.0000`. This is
    /// the provenance the icon pack records in `data/manifest.toml`.
    pub version: String,
}

impl GameInstall {
    /// Open the install at `path` (the directory containing `game/`), or search
    /// the standard install locations when `path` is `None`.
    pub fn discover(path: Option<&Path>) -> anyhow::Result<GameInstall> {
        let root = match path {
            Some(explicit) => {
                if !explicit.join("game").join("sqpack").is_dir() {
                    bail!(
                        "{} does not look like an FFXIV install (no game/sqpack inside)",
                        explicit.display()
                    );
                }
                explicit.to_path_buf()
            }
            None => find_install().context(
                "no FFXIV install found in the standard locations; pass --game-path <install root>",
            )?,
        };
        let ver_path = root.join("game").join("ffxivgame.ver");
        let version = std::fs::read_to_string(&ver_path)
            .with_context(|| format!("reading {}", ver_path.display()))?
            .trim()
            .to_string();
        let ironworks = Ironworks::new().with_resource(SqPack::new(FsResource::at(&root)));
        Ok(GameInstall { ironworks, version })
    }

    /// Read and decode icon `icon_id`, preferring the 2x `_hr1` variant.
    /// `Ok(None)` means the install yields no usable icon at either resolution
    /// — including the rare entry whose SqPack record doesn't parse (a handful
    /// of ids index a `.tex` that ironworks cannot read; treat them like the
    /// absent ones and let the caller's missing-count guard catch anything
    /// systemic). Decode errors on a successfully *read* texture still fail
    /// loudly: an unknown pixel format must never degrade into a blank icon.
    pub fn icon(&self, icon_id: i32) -> anyhow::Result<Option<RgbaImage>> {
        for hr in [true, false] {
            let path = icon_sqpack_path(icon_id, hr);
            match self.ironworks.file::<Texture>(&path) {
                Ok(tex) => {
                    return decode_tex(&tex)
                        .with_context(|| format!("decoding {path}"))
                        .map(Some);
                }
                Err(_) => continue,
            }
        }
        Ok(None)
    }
}

/// The install locations `GameInstall::discover` searches, matching ironworks'
/// own `FsResource::search` list.
const TRY_PATHS: &[&str] = &[
    r"C:\SquareEnix\FINAL FANTASY XIV - A Realm Reborn",
    r"C:\Program Files (x86)\Steam\steamapps\common\FINAL FANTASY XIV Online",
    r"C:\Program Files (x86)\Steam\steamapps\common\FINAL FANTASY XIV - A Realm Reborn",
    r"C:\Program Files (x86)\FINAL FANTASY XIV - A Realm Reborn",
    r"C:\Program Files (x86)\SquareEnix\FINAL FANTASY XIV - A Realm Reborn",
];

fn find_install() -> Option<PathBuf> {
    TRY_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.join("game").join("sqpack").is_dir())
}

/// Decode mip 0 of `tex` to RGBA8.
pub fn decode_tex(tex: &Texture) -> anyhow::Result<RgbaImage> {
    let (w, h) = (tex.width() as usize, tex.height() as usize);
    let data = tex.data();
    let rgba: Vec<u8> = match tex.format() {
        Format::Argb8 => {
            let mip0 = data
                .get(..w * h * 4)
                .with_context(|| format!("Argb8 data too short for {w}x{h}"))?;
            // Stored B,G,R,A per pixel.
            mip0.chunks_exact(4)
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect()
        }
        Format::Dxt1 => {
            let mut pixels = vec![0u32; w * h];
            texture2ddecoder::decode_bc1(data, w, h, &mut pixels)
                .map_err(|e| anyhow!("bc1 decode failed: {e}"))?;
            // texture2ddecoder emits ARGB words, i.e. B,G,R,A bytes on LE.
            pixels
                .iter()
                .flat_map(|px| {
                    let [b, g, r, a] = px.to_le_bytes();
                    [r, g, b, a]
                })
                .collect()
        }
        other => bail!("icon uses unhandled texture format {other:?}"),
    };
    RgbaImage::from_raw(w as u32, h as u32, rgba).context("pixel buffer does not match dimensions")
}

#[cfg(test)]
mod tests {
    use ironworks::file::File;

    use super::*;

    /// Build the 80-byte `.tex` header + payload that `Texture`'s binread
    /// parser expects: attributes, format, dims, mips, LoD + surface tables.
    fn tex_bytes(format: u32, width: u16, height: u16, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x00800000u32.to_le_bytes()); // attributes: 2D
        bytes.extend_from_slice(&format.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // depth
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mip levels
        bytes.extend_from_slice(&[0u8; 12]); // lod surfaces
        let mut offsets = [0u32; 13];
        offsets[0] = 80;
        for offset in offsets {
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        assert_eq!(bytes.len(), 80);
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn icon_paths_group_by_thousands() {
        assert_eq!(
            icon_sqpack_path(65002, true),
            "ui/icon/065000/065002_hr1.tex"
        );
        assert_eq!(icon_sqpack_path(20801, false), "ui/icon/020000/020801.tex");
        assert_eq!(icon_sqpack_path(999, true), "ui/icon/000000/000999_hr1.tex");
    }

    #[test]
    fn argb8_decodes_as_bgra_storage() {
        // One pixel stored B=1, G=2, R=3, A=4 must come out R=3, G=2, B=1, A=4.
        let tex = Texture::read(tex_bytes(0x1450, 1, 1, &[1, 2, 3, 4])).expect("parse tex");
        let img = decode_tex(&tex).expect("decode");
        assert_eq!(img.dimensions(), (1, 1));
        assert_eq!(img.get_pixel(0, 0).0, [3, 2, 1, 4]);
    }

    #[test]
    fn dxt1_decodes_a_solid_color_block() {
        // BC1 block: color0=color1=pure red in RGB565, all indices 0 -> 4x4 red.
        let block = [0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0];
        let tex = Texture::read(tex_bytes(0x3420, 4, 4, &block)).expect("parse tex");
        let img = decode_tex(&tex).expect("decode");
        assert_eq!(img.dimensions(), (4, 4));
        for pixel in img.pixels() {
            assert_eq!(pixel.0, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn unknown_formats_error_instead_of_corrupting() {
        // L8 (0x1130) is a real format no icon uses; decode must refuse it.
        let tex = Texture::read(tex_bytes(0x1130, 1, 1, &[0])).expect("parse tex");
        let error = decode_tex(&tex).expect_err("L8 should be rejected");
        assert!(error.to_string().contains("unhandled texture format"));
    }

    #[test]
    fn truncated_argb8_data_errors() {
        let tex = Texture::read(tex_bytes(0x1450, 2, 2, &[0; 4])).expect("parse tex");
        assert!(decode_tex(&tex).is_err());
    }
}
