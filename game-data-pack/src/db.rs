//! Builds `data/xiv-db/<lang>.rkyv` from an ffxiv-datamining CSV tree.
//!
//! The container format (rkyv 0.7 + zlib `Compression::best()`) has to stay
//! exactly what `xiv-gen-db` decodes at runtime — do not "improve" it here.

use std::path::Path;

use std::collections::HashMap;

use anyhow::{Context, anyhow, ensure};
use flate2::{Compression, FlushCompress};
use xiv_gen::csv_to_rkyv::{Supplements, read_data_with};
use xiv_gen::{ENpcResidentId, Language, MapId, NpcPlacement};

/// Every language packed.
pub const LANGUAGES: [Language; 7] = [
    Language::En,
    Language::Ja,
    Language::De,
    Language::Fr,
    Language::Cn,
    Language::Ko,
    Language::Tc,
];

/// What one language's pack cost.
pub struct PackStats {
    pub lang: Language,
    pub items: usize,
    pub raw_bytes: usize,
    pub packed_bytes: usize,
}

pub struct DbOutput {
    pub packs: Vec<PackStats>,
    /// `(item id, icon id)` of the *named* items in the `en` data, ascending by
    /// item id — the icon extraction reads exactly these out of the game files.
    pub en_named_items: Vec<(i32, i32)>,
    /// The placements that made it into the `en` pack (shop and exchange NPCs
    /// and leve issuers only) — what `data/npc-placements.json` records and
    /// what the map pack has to cover.
    pub en_npc_placements: HashMap<ENpcResidentId, Vec<NpcPlacement>>,
    /// `(map id, Map.Id texture stem)` for every map a packed placement sits
    /// on, ascending by map id.
    pub en_maps_used: Vec<(i32, String)>,
    /// Distinct gil-shop vendor NPCs in the `en` data, for the coverage report.
    pub en_vendor_npcs: usize,
    /// Distinct special-shop (exchange) NPCs in the `en` data, likewise.
    pub en_exchange_npcs: usize,
}

/// Items worth extracting an icon for: the rows that actually have a name, as
/// `(item id, icon id)` sorted ascending by item id.
///
/// Measured against 7.55: 52,801 item rows of which 50,773 are named. The
/// 2,028 unnamed placeholder rows are dropped because they can never render
/// anywhere an icon is wanted.
fn named_items<'a>(items: impl IntoIterator<Item = (i32, &'a str, i32)>) -> Vec<(i32, i32)> {
    let mut named: Vec<(i32, i32)> = items
        .into_iter()
        .filter(|(_, name, _)| !name.trim().is_empty())
        .map(|(id, _, icon)| (id, icon))
        .collect();
    named.sort_unstable();
    named
}

/// Reads every language out of `datamining_root`, folds `supplements` in, and
/// writes one pack per language into `out_dir`.
pub fn build_packs(
    datamining_root: &Path,
    out_dir: &Path,
    supplements: &Supplements,
) -> anyhow::Result<DbOutput> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    let mut packs = Vec::with_capacity(LANGUAGES.len());
    let mut en_named_items = Vec::new();
    let mut en_npc_placements = HashMap::new();
    let mut en_maps_used = Vec::new();
    let mut en_vendor_npcs = 0;
    let mut en_exchange_npcs = 0;
    for lang in LANGUAGES {
        let data = read_data_with(datamining_root, lang, supplements);
        if lang == Language::En {
            en_named_items = named_items(
                data.items
                    .iter()
                    .map(|(id, item)| (id.0, item.name.as_str(), item.icon)),
            );
            en_npc_placements = data.npc_placements.clone();
            en_maps_used = maps_used(&data.npc_placements, &data.maps);
            en_vendor_npcs = data
                .gil_shop_npcs
                .values()
                .flatten()
                .collect::<std::collections::HashSet<_>>()
                .len();
            en_exchange_npcs = data
                .special_shop_npcs
                .values()
                .flatten()
                .collect::<std::collections::HashSet<_>>()
                .len();
        }
        let items = data.items.len();

        // The dataset is large, so use a generous scratch size up front. rkyv's
        // `AllocSerializer` falls back to heap-allocated scratch when needed, but
        // a larger inline buffer avoids extra allocations during serialization.
        let raw = rkyv::to_bytes::<_, 1_048_576>(&data)
            .map_err(|e| anyhow!("serializing the {lang:?} data with rkyv: {e:?}"))?;

        let mut flate = flate2::Compress::new(Compression::best(), true);
        let mut packed = Vec::with_capacity(raw.len());
        flate
            .compress_vec(raw.as_slice(), &mut packed, FlushCompress::Full)
            .with_context(|| format!("deflating the {lang:?} pack"))?;
        ensure!(
            !packed.is_empty(),
            "the {lang:?} pack compressed to nothing"
        );
        // `compress_vec` writes into the Vec's spare capacity and never grows it,
        // so a pack that failed to compress below 1.0 would silently be truncated
        // rather than error. Confirm the whole input was actually consumed.
        ensure!(
            flate.total_in() as usize == raw.len(),
            "the {lang:?} pack only deflated {} of {} bytes; the output buffer was too small",
            flate.total_in(),
            raw.len()
        );

        let dest = out_dir.join(format!("{}.rkyv", lang.to_path_part()));
        std::fs::write(&dest, &packed).with_context(|| format!("writing {}", dest.display()))?;

        packs.push(PackStats {
            lang,
            items,
            raw_bytes: raw.len(),
            packed_bytes: packed.len(),
        });
    }
    Ok(DbOutput {
        packs,
        en_named_items,
        en_npc_placements,
        en_maps_used,
        en_vendor_npcs,
        en_exchange_npcs,
    })
}

/// Every map some placement refers to, with its texture stem, ascending.
fn maps_used(
    placements: &HashMap<ENpcResidentId, Vec<NpcPlacement>>,
    maps: &HashMap<MapId, xiv_gen::Map>,
) -> Vec<(i32, String)> {
    let mut used: Vec<(i32, String)> = placements
        .values()
        .flatten()
        .map(|p| p.map)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter_map(|id| maps.get(&id).map(|m| (id.0, m.id.clone())))
        .collect();
    used.sort_unstable();
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_path_parts_are_the_seven_expected() {
        let mut paths: Vec<_> = LANGUAGES.iter().map(|l| l.to_path_part()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["cn", "de", "en", "fr", "ja", "ko", "tc"]);
    }

    #[test]
    fn named_items_drops_unnamed_rows_and_sorts_by_id() {
        let rows = vec![
            (30, "Iron Ingot", 20801),
            (10, "", 999),
            (44_000, "Grade 2 Gemdraught of Mind", 20872),
            (20, "   ", 998),
            (5, "Cotton", 21_001),
        ];
        assert_eq!(
            named_items(rows),
            vec![(5, 21_001), (30, 20801), (44_000, 20872)]
        );
    }

    #[test]
    fn named_items_is_empty_when_nothing_is_named() {
        assert!(named_items(vec![(1, "", 5), (2, " ", 6)]).is_empty());
    }
}
