//! Decode a few icons per pixel format to PNG, so each decoder can be
//! eyeballed rather than trusted.
//!
//! cargo run --release -p icon-extract --example dump -- <outdir>

use ironworks::file::tex::Texture;
use std::collections::BTreeMap;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&out).unwrap();
    let root = std::env::var("FFXIV_PATH").ok();
    let install = icon_extract::GameInstall::discover(root.as_deref().map(std::path::Path::new))
        .expect("no FFXIV install found; set FFXIV_PATH");
    let ironworks = install.ironworks();

    const PER_FORMAT: usize = 2;
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();

    for id in 1..250_000 {
        for hr in [true, false] {
            let path = icon_extract::icon_sqpack_path(id, hr);
            let Ok(tex) = ironworks.file::<Texture>(&path) else {
                continue;
            };
            let fmt = format!("{:?}", tex.format());
            let n = taken.entry(fmt.clone()).or_default();
            if *n >= PER_FORMAT {
                break;
            }
            // Skip the 4096x4096 atlas sheets; they are not item icons and the
            // PNGs are unwieldy.
            if tex.width() > 256 {
                break;
            }
            match icon_extract::decode_tex(&tex) {
                Ok(img) => {
                    let path = format!("{out}/{fmt}_{id}_{}x{}.png", img.width(), img.height());
                    img.save(&path).unwrap();
                    println!("{fmt:14} icon {id:>6} {}x{}", img.width(), img.height());
                    *n += 1;
                }
                Err(e) => println!("{fmt:14} icon {id:>6} DECODE FAILED: {e}"),
            }
            break;
        }
    }
    println!("\nformats sampled: {taken:?}");
}
