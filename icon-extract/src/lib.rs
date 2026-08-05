//! Extracts item icons from a local FFXIV install via ironworks.
//!
//! Replaces the old universalis-assets dependency (Lodestone-crawled PNGs that
//! only covered marketable items) with direct SqPack reads, so currencies and
//! untradable exchange rewards get icons too. The game ships icons as `.tex`
//! at `ui/icon/<group>/<id>.tex` (40px) with a `_hr1` 2x variant (80px); we
//! prefer the 2x and fall back to the base.
//!
//! `ui/icon` uses six pixel formats in practice — BC1, BC3, **BC7**, Bgra8,
//! Bgr5a1 and Bgra4 — and all are decoded here (BC2/BC4/BC5 too, for the cost
//! of a line each). An earlier census reported only BC1 and Bgra8; it
//! undercounted because it could only inspect icons that *parsed*, and BC7 in
//! particular could not be parsed at all by the ironworks release then in use.
//! A format outside the handled set is a hard error naming the format, never a
//! corrupt image.
//!
//! Reads distinguish three outcomes, which matters because conflating the last
//! two is what made a decoder gap look like a stale game client: the icon is
//! present and decodable, genuinely absent from the install, or **present but
//! unreadable**. See [`IconRead`].

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use image::RgbaImage;
use ironworks::{
    file::tex::{Format, Texture},
    sqpack::{Install, SqPack},
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
        let ironworks = Ironworks::new().with_resource(SqPack::new(Install::at(&root)));
        Ok(GameInstall { ironworks, version })
    }

    /// The underlying reader, for diagnostics that need to inspect raw
    /// textures (see `examples/probe.rs`).
    pub fn ironworks(&self) -> &Ironworks {
        &self.ironworks
    }

    /// Read and decode icon `icon_id`, preferring the 2x `_hr1` variant.
    ///
    /// Never collapses "absent" into "unreadable". A `.tex` that is indexed but
    /// fails to parse is a *defect* — either a stale ironworks or a genuinely
    /// new container shape — and reporting it as merely missing is what
    /// previously disguised ~11% of `ui/icon` as content the client did not
    /// have. Decode errors on a successfully read texture still fail loudly:
    /// an unknown pixel format must never degrade into a blank icon.
    pub fn icon(&self, icon_id: i32) -> anyhow::Result<IconRead> {
        let mut failure: Option<(String, ironworks::Error)> = None;
        for hr in [true, false] {
            let path = icon_sqpack_path(icon_id, hr);
            match self.ironworks.file::<Texture>(&path) {
                Ok(tex) => {
                    let image = decode_tex(&tex).with_context(|| format!("decoding {path}"))?;
                    return Ok(IconRead::Image(image));
                }
                // Genuinely not in the install — try the other resolution.
                Err(ironworks::Error::NotFound(_)) => continue,
                // Indexed but unparseable. Keep the first such error: the
                // `_hr1` one is the more informative of the pair.
                Err(e) => {
                    if failure.is_none() {
                        failure = Some((path, e));
                    }
                }
            }
        }
        Ok(match failure {
            Some((path, e)) => IconRead::Unreadable {
                path,
                reason: e.to_string(),
            },
            None => IconRead::Absent,
        })
    }
}

/// Outcome of reading one icon out of the install.
#[derive(Debug)]
pub enum IconRead {
    /// Decoded successfully.
    Image(RgbaImage),
    /// Neither the `_hr1` nor the base `.tex` is indexed. Expected: the Item
    /// sheet references icon ids the client legitimately does not ship.
    Absent,
    /// The `.tex` *is* indexed but could not be read. Never expected — surface
    /// it rather than counting it as absent.
    Unreadable { path: String, reason: String },
}

impl IconRead {
    /// The decoded image, if there is one.
    pub fn image(self) -> Option<RgbaImage> {
        match self {
            IconRead::Image(image) => Some(image),
            _ => None,
        }
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

/// A texture2ddecoder block decoder: compressed bytes and dimensions in,
/// ARGB words out.
type BlockDecoder = fn(&[u8], usize, usize, &mut [u32]) -> Result<(), &'static str>;

/// Run one of texture2ddecoder's block decoders and swizzle its output to
/// RGBA8. They all share this shape and all emit ARGB words, i.e. B,G,R,A
/// bytes on a little-endian host.
fn decode_bc(
    data: &[u8],
    w: usize,
    h: usize,
    decode: BlockDecoder,
    name: &str,
) -> anyhow::Result<Vec<u8>> {
    let mut pixels = vec![0u32; w * h];
    decode(data, w, h, &mut pixels).map_err(|e| anyhow!("{name} decode failed: {e}"))?;
    Ok(pixels
        .iter()
        .flat_map(|px| {
            let [b, g, r, a] = px.to_le_bytes();
            [r, g, b, a]
        })
        .collect())
}

/// Mip 0 of a 16-bit-per-pixel texture, as little-endian `u16`s.
fn packed16(data: &[u8], w: usize, h: usize, name: &str) -> anyhow::Result<Vec<u16>> {
    let mip0 = data
        .get(..w * h * 2)
        .with_context(|| format!("{name} data too short for {w}x{h}"))?;
    Ok(mip0
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect())
}

/// Decode mip 0 of `tex` to RGBA8.
pub fn decode_tex(tex: &Texture) -> anyhow::Result<RgbaImage> {
    let (w, h) = (tex.width() as usize, tex.height() as usize);
    let data = tex.data();
    let rgba: Vec<u8> = match tex.format() {
        Format::Bgra8Unorm => {
            let mip0 = data
                .get(..w * h * 4)
                .with_context(|| format!("Bgra8 data too short for {w}x{h}"))?;
            // Stored B,G,R,A per pixel.
            mip0.chunks_exact(4)
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect()
        }
        Format::Bc1Unorm => decode_bc(data, w, h, texture2ddecoder::decode_bc1, "bc1")?,
        Format::Bc2Unorm => decode_bc(data, w, h, texture2ddecoder::decode_bc2, "bc2")?,
        Format::Bc3Unorm => decode_bc(data, w, h, texture2ddecoder::decode_bc3, "bc3")?,
        // Bcn2. BC7 is the one that matters: ~9% of ui/icon, and the reason
        // ironworks 0.4.1 could not read those entries at all — its `Format`
        // enum predates the whole Bcn2 group, so the read failed before any
        // decoding was attempted.
        Format::Bc4Unorm => decode_bc(data, w, h, texture2ddecoder::decode_bc4, "bc4")?,
        Format::Bc5Unorm => decode_bc(data, w, h, texture2ddecoder::decode_bc5, "bc5")?,
        Format::Bc7Unorm => decode_bc(data, w, h, texture2ddecoder::decode_bc7, "bc7")?,
        // 16-bit packed. Despite the ironworks names these are the D3D9
        // orderings — A1R5G5B5 and A4R4G4B4 — so alpha is the high bits and
        // blue the low. Channels are widened by multiplying up rather than
        // shifting, so full-scale input maps to 255 instead of 248/240.
        Format::Bgr5a1Unorm => {
            let mip0 = packed16(data, w, h, "Bgr5a1")?;
            mip0.iter()
                .flat_map(|&v| {
                    let r = ((v >> 10) & 0x1f) as u32;
                    let g = ((v >> 5) & 0x1f) as u32;
                    let b = (v & 0x1f) as u32;
                    let a = (v >> 15) & 0x1;
                    [
                        (r * 255 / 31) as u8,
                        (g * 255 / 31) as u8,
                        (b * 255 / 31) as u8,
                        if a == 1 { 255 } else { 0 },
                    ]
                })
                .collect()
        }
        Format::Bgra4Unorm => {
            let mip0 = packed16(data, w, h, "Bgra4")?;
            mip0.iter()
                .flat_map(|&v| {
                    let r = ((v >> 8) & 0xf) as u8;
                    let g = ((v >> 4) & 0xf) as u8;
                    let b = (v & 0xf) as u8;
                    let a = ((v >> 12) & 0xf) as u8;
                    // 0x0..0xf -> 0x00..0xff exactly.
                    [r * 17, g * 17, b * 17, a * 17]
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
        let tex = Texture::read(std::io::Cursor::new(tex_bytes(0x1450, 1, 1, &[1, 2, 3, 4])))
            .expect("parse tex");
        let img = decode_tex(&tex).expect("decode");
        assert_eq!(img.dimensions(), (1, 1));
        assert_eq!(img.get_pixel(0, 0).0, [3, 2, 1, 4]);
    }

    #[test]
    fn dxt1_decodes_a_solid_color_block() {
        // BC1 block: color0=color1=pure red in RGB565, all indices 0 -> 4x4 red.
        let block = [0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0];
        let tex = Texture::read(std::io::Cursor::new(tex_bytes(0x3420, 4, 4, &block)))
            .expect("parse tex");
        let img = decode_tex(&tex).expect("decode");
        assert_eq!(img.dimensions(), (4, 4));
        for pixel in img.pixels() {
            assert_eq!(pixel.0, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn unknown_formats_error_instead_of_corrupting() {
        // L8 (0x1130) is a real format no icon uses; decode must refuse it.
        let tex =
            Texture::read(std::io::Cursor::new(tex_bytes(0x1130, 1, 1, &[0]))).expect("parse tex");
        let error = decode_tex(&tex).expect_err("L8 should be rejected");
        assert!(error.to_string().contains("unhandled texture format"));
    }

    #[test]
    fn truncated_argb8_data_errors() {
        let tex = Texture::read(std::io::Cursor::new(tex_bytes(0x1450, 2, 2, &[0; 4])))
            .expect("parse tex");
        assert!(decode_tex(&tex).is_err());
    }
}
