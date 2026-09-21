//! Reproject existing full packs without fetching CSVs or a game installation.
#[path = "../browser.rs"]
mod browser;
use std::io::Read;

fn main() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
    for lang in ["en", "ja", "de", "fr", "cn", "ko", "tc"] {
        let bytes = std::fs::read(root.join(format!("xiv-db/{lang}.rkyv")))?;
        let mut decoded = Vec::new();
        brotli::Decompressor::new(bytes.as_slice(), 65536).read_to_end(&mut decoded)?;
        let mut aligned = rkyv::AlignedVec::new();
        aligned.extend_from_slice(&decoded);
        let data = rkyv::from_bytes::<xiv_gen::Data>(&aligned)
            .map_err(|e| anyhow::anyhow!("{lang}: {e}"))?;
        let size = browser::write_startup(&data, &root.join(format!("xiv-startup/{lang}.rkyv")))?;
        println!(
            "{lang}: {} -> {size} bytes (saved {})",
            bytes.len(),
            bytes.len() - size
        );
    }
    Ok(())
}
