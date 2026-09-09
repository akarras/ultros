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
    let Some(data) = decode_en_pack("en_pack_decodes_and_contains_the_probe_item") else {
        return;
    };

    data.items
        .iter()
        .find(|(_, item)| item.name == PROBE_ITEM)
        .unwrap_or_else(|| panic!("expected to find an item named {PROBE_ITEM:?}"));
}

/// The pack carries fields that exist in no CSV column — they are resolved
/// during generation from sheets that are then thrown away. Nothing at runtime
/// can rebuild them, so a generator that silently stopped populating one would
/// only show up as an empty vendor panel or a resale suggestion for an item
/// nobody can buy. Assert they survived the round trip.
#[test]
fn en_pack_carries_the_derived_vendor_fields() {
    let Some(data) = decode_en_pack("en_pack_carries_the_derived_vendor_fields") else {
        return;
    };

    assert!(
        !data.gil_shop_npcs.is_empty(),
        "pack has no shop -> NPC index"
    );
    assert!(
        data.gil_shop_npcs.values().all(|npcs| !npcs.is_empty()),
        "every indexed shop should name at least one NPC"
    );

    // Usagi Kabuto (#1362): sold only by a Heavensturn festival vendor and by
    // the Calamity Salvager to players holding a 2013 event achievement.
    let usagi = data
        .items
        .values()
        .find(|i| i.name == "Usagi Kabuto")
        .expect("Usagi Kabuto missing from the pack")
        .key_id;
    let rows: Vec<_> = data
        .gil_shop_items
        .values()
        .flatten()
        .filter(|r| r.item == usagi.0)
        .collect();
    assert!(!rows.is_empty(), "Usagi Kabuto is sold by no shop");
    assert!(
        rows.iter().all(|r| !r.availability.is_obtainable()),
        "no Usagi Kabuto row should be obtainable, got {:?}",
        rows.iter().map(|r| r.availability).collect::<Vec<_>>()
    );

    // The classifier must not collapse to a single verdict: ordinary vendor
    // stock has to stay `Open`, or every resale candidate would be filtered.
    let open = data
        .gil_shop_items
        .values()
        .flatten()
        .filter(|r| r.availability == xiv_gen::VendorAvailability::Open)
        .count();
    assert!(
        open > 10_000,
        "expected most shop rows to be ungated, got {open}"
    );
}

/// Decodes the committed pack, or returns `None` after explaining why it could
/// not (missing file or un-pulled LFS stub) so a fresh clone stays green.
fn decode_en_pack(test_name: &str) -> Option<xiv_gen::Data> {
    let path = repo_root().join("data").join("xiv-db").join("en.rkyv");

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            // A fresh clone without `git lfs pull` (or before Task 4 has run)
            // has no real pack file here. Don't fail `cargo test` for that —
            // just say so and move on.
            eprintln!(
                "skipping {test_name}: could not read {}: {error}",
                path.display()
            );
            return None;
        }
    };

    if is_lfs_pointer_stub(&bytes) {
        eprintln!(
            "skipping {test_name}: {} is an LFS pointer stub (run `git lfs pull`)",
            path.display()
        );
        return None;
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

    Some(
        rkyv::from_bytes::<xiv_gen::Data>(&aligned)
            .expect("failed to deserialize data/xiv-db/en.rkyv"),
    )
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
