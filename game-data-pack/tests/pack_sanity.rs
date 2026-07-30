//! Sanity-decodes the committed `data/xiv-db/en.rkyv` pack the same way
//! `xiv-gen-db::decompress_data` does at runtime: zlib `ZlibDecoder::read_to_end`,
//! then a copy into `rkyv::AlignedVec` before `rkyv::from_bytes` (plain
//! `Vec<u8>` is only byte-aligned, and rkyv needs `FixedIsize` alignment —
//! this bites on Windows in particular).
//!
//! Skips gracefully (rather than failing `cargo test`) when the pack is
//! absent or still an LFS pointer stub, so a fresh clone that hasn't run
//! `git lfs pull` stays green.

use std::io::Read;
use std::path::PathBuf;

/// The same probe `xiv-gen-db`'s `test_embed` uses.
const PROBE_ITEM: &str = "Grade 2 Gemdraught of Mind";

#[test]
fn en_pack_decodes_and_contains_the_probe_item() {
    let path = repo_root().join("data").join("xiv-db").join("en.rkyv");

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            // A fresh clone without `git lfs pull` (or before Task 4 has run)
            // has no real pack file here. Don't fail `cargo test` for that —
            // just say so and move on.
            eprintln!(
                "skipping en_pack_decodes_and_contains_the_probe_item: could not read {}: {error}",
                path.display()
            );
            return;
        }
    };

    if is_lfs_pointer_stub(&bytes) {
        eprintln!(
            "skipping en_pack_decodes_and_contains_the_probe_item: {} is an LFS pointer stub \
             (run `git lfs pull`)",
            path.display()
        );
        return;
    }

    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(bytes.as_slice())
        .read_to_end(&mut decoded)
        .expect("failed to zlib-decompress data/xiv-db/en.rkyv");

    // rkyv requires the byte buffer to be aligned to `FixedIsize`; a plain
    // `Vec<u8>` only guarantees byte alignment, so copy into an `AlignedVec`
    // first. Skipping this fails with an "unaligned pointer" error on Windows.
    let mut aligned = rkyv::AlignedVec::with_capacity(decoded.len());
    aligned.extend_from_slice(&decoded);

    let data = rkyv::from_bytes::<xiv_gen::Data>(&aligned)
        .expect("failed to deserialize data/xiv-db/en.rkyv");

    data.items
        .iter()
        .find(|(_, item)| item.name == PROBE_ITEM)
        .unwrap_or_else(|| panic!("expected to find an item named {PROBE_ITEM:?}"));
}

/// LFS pointer stubs are small text files starting with this line; real pack
/// bytes never do (they start with a zlib header byte).
fn is_lfs_pointer_stub(bytes: &[u8]) -> bool {
    bytes.starts_with(b"version https://git-lfs.github.com/spec/v1")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("game-data-pack has a parent directory")
        .to_path_buf()
}
