//! Sanity-decodes the committed `data/xiv-db/en.rkyv` pack the same way
//! `xiv-gen-db::decompress_data` does at runtime: a brotli
//! `Decompressor::read_to_end`, then a copy into `rkyv::AlignedVec` before
//! `rkyv::from_bytes` (plain
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
fn all_startup_packs_match_projection_and_preserve_references() {
    fn decode(bytes: &[u8]) -> xiv_gen::Data {
        let mut raw = Vec::new();
        brotli_decompressor::Decompressor::new(bytes, 65536)
            .read_to_end(&mut raw)
            .unwrap();
        let mut aligned = rkyv::AlignedVec::new();
        aligned.extend_from_slice(&raw);
        rkyv::from_bytes(&aligned).unwrap()
    }
    for lang in ["en", "ja", "de", "fr", "cn", "ko", "tc"] {
        let full_bytes =
            std::fs::read(repo_root().join(format!("data/xiv-db/{lang}.rkyv"))).unwrap();
        let startup_bytes =
            std::fs::read(repo_root().join(format!("data/xiv-startup/{lang}.rkyv"))).unwrap();
        assert!(
            !is_lfs_pointer_stub(&full_bytes) && !is_lfs_pointer_stub(&startup_bytes),
            "git lfs pull is required"
        );
        let full = decode(&full_bytes);
        let startup = decode(&startup_bytes);
        assert!(
            startup_bytes.len() * 100 < full_bytes.len() * 75,
            "{lang}: startup byte budget"
        );
        assert_eq!(
            serde_json::to_value(&startup).unwrap(),
            serde_json::to_value(xiv_gen::browser::startup_data(&full)).unwrap(),
            "{lang}: stale generated startup pack"
        );
        assert_eq!(startup.items.len(), full.items.len());
        assert!(
            startup
                .items
                .values()
                .all(|item| item.description.is_empty())
        );
        assert!(full.items.values().any(|item| !item.description.is_empty()));
        assert!(full.e_npc_residents.len() > startup.e_npc_residents.len());
        if lang == "en" {
            // Keep the browser regression's direct-URL fixture outside the
            // startup pack, so it cannot pass by reading a retained vendor.
            let fixture = xiv_gen::ENpcResidentId(1000063);
            assert!(!full.e_npc_residents[&fixture].singular.is_empty());
            assert!(!startup.e_npc_residents.contains_key(&fixture));
        }
        for npc in full
            .gil_shop_npcs
            .values()
            .chain(full.special_shop_npcs.values())
            .chain(full.collectables_shop_npcs.values())
            .chain(full.leve_issuers.values())
            .flatten()
            .chain(full.npc_placements.keys())
        {
            if let Some(original) = full.e_npc_residents.get(npc) {
                assert_eq!(
                    startup.e_npc_residents.get(npc).map(|row| &row.singular),
                    Some(&original.singular),
                    "{lang}: missing referenced NPC {npc:?}"
                );
            }
        }
    }
}

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

/// The exchange indexes and the tomestone/scrip cost rewrite come from sheets
/// read only at generation (`InclusionShop*`, `CustomTalk*`, `TomestonesItem`
/// and `SpecialShop.CostType`). Losing any of them is silent at runtime: the
/// scrip exchange NPC pages go empty, or a Purple Scrip item goes back to
/// costing "250 x Fire Shard".
#[test]
fn en_pack_carries_the_exchange_indexes_and_resolved_costs() {
    let Some(data) = decode_en_pack("en_pack_carries_the_exchange_indexes_and_resolved_costs")
    else {
        return;
    };
    let npcs_of = |shop: i32| {
        data.special_shop_npcs
            .get(&xiv_gen::SpecialShopId(shop))
            .cloned()
            .unwrap_or_default()
    };
    // Purple Scrip Exchange (Lv. 90 Materials): behind the InclusionShop
    // window of every scrip exchange NPC, and the cost row of item 39595.
    let scrip_exchange = xiv_gen::ENpcResidentId(1001617);
    assert!(
        npcs_of(1770488).contains(&scrip_exchange),
        "{:?}",
        npcs_of(1770488)
    );
    let scrip_shops = data
        .special_shop_npcs
        .iter()
        .filter(|(_, npcs)| npcs.contains(&scrip_exchange))
        .count();
    assert!(
        scrip_shops > 40,
        "scrip exchange offers {scrip_shops} shops"
    );
    let purple = &data.special_shops[&xiv_gen::SpecialShopId(1770488)];
    let slot = purple
        .item_receive_0
        .iter()
        .position(|i| *i == 39595)
        .expect("item 39595 left the Purple Scrip Exchange");
    assert_eq!(
        (purple.item_cost_0[slot], purple.count_cost_0[slot]),
        (33913, 250),
        "Purple Crafters' Scrip cost"
    );
    // Allagan Tomestones of Poetics (DoW, IL 630): a tomestone index cost.
    let poetics = &data.special_shops[&xiv_gen::SpecialShopId(1770606)];
    assert!(
        poetics.item_cost_0.contains(&28) && !poetics.item_cost_0.contains(&1),
        "{:?}",
        &poetics.item_cost_0[..5]
    );
    // Seika (Kugane) reaches her reoutfitting shops only through a CustomTalk
    // argument; the Grand Company quartermasters only through nested handlers.
    assert!(npcs_of(1769572).contains(&xiv_gen::ENpcResidentId(1013747)));
    assert!(npcs_of(1770340).contains(&xiv_gen::ENpcResidentId(1000200)));
    // Every Collectable Appraiser runs one script; the scrip turn-in shops are
    // attributed by rule, the material exchanges by their own slot.
    let appraiser = xiv_gen::ENpcResidentId(1001616);
    assert!(
        data.collectables_shop_npcs[&xiv_gen::CollectablesShopId(3866626)].contains(&appraiser)
    );
    assert_eq!(
        data.collectables_shop_npcs[&xiv_gen::CollectablesShopId(3866630)],
        vec![xiv_gen::ENpcResidentId(1027566)],
        "Limbeth, Resplendent Materials Exchange"
    );
    let lists = data
        .special_shop_npcs
        .values()
        .map(|npcs| ("special", npcs))
        .chain(
            data.collectables_shop_npcs
                .values()
                .map(|npcs| ("collectables", npcs)),
        );
    for (name, npcs) in lists {
        assert!(!npcs.is_empty(), "{name}: an indexed shop names no NPC");
        assert!(
            npcs.windows(2).all(|w| w[0].0 < w[1].0),
            "{name}: NPC list not sorted+deduped: {npcs:?}"
        );
    }
    // The placements walk must have widened with the indexes.
    assert!(
        data.npc_placements.contains_key(&scrip_exchange),
        "the scrip exchange NPC has no placement"
    );
}

/// The client-derived placements and the sheets the map pins need are folded
/// in at generation; a generator that dropped them would only show up as an
/// item page with every vendor "Location unavailable".
#[test]
fn en_pack_carries_npc_placements_and_map_sheets() {
    let Some(data) = decode_en_pack("en_pack_carries_npc_placements_and_map_sheets") else {
        return;
    };
    // Ilorie, Old Gridania: the Level sheet never placed her.
    let ilorie = &data.npc_placements[&xiv_gen::ENpcResidentId(1000216)];
    assert!(
        ilorie
            .iter()
            .any(|p| p.territory.0 == 133 && (p.x - 14.4).abs() < 0.1 && (p.y - 9.7).abs() < 0.1),
        "{ilorie:?}"
    );
    // Well over the Level sheet's 193; a big drop means the layout walk broke.
    assert!(
        data.npc_placements.len() > 400,
        "{}",
        data.npc_placements.len()
    );
    for placements in data.npc_placements.values() {
        assert!(!placements.is_empty());
        for p in placements {
            assert!(
                data.maps.contains_key(&p.map),
                "placement on unknown map {p:?}"
            );
            assert!(data.territory_types.contains_key(&p.territory));
        }
    }
    let gridania = &data.maps[&xiv_gen::MapId(2)];
    assert_eq!(
        (gridania.id.as_str(), gridania.size_factor),
        ("f1t1/00", 200)
    );
    assert_eq!(
        data.place_names[&xiv_gen::PlaceNameId(52)].name,
        "New Gridania"
    );
    assert_eq!(
        data.leve_issuers[&xiv_gen::LeveId(21)],
        vec![xiv_gen::ENpcResidentId(1000101)]
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
    brotli_decompressor::Decompressor::new(bytes.as_slice(), 64 * 1024)
        .read_to_end(&mut decoded)
        .expect("failed to brotli-decompress data/xiv-db/en.rkyv");

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
