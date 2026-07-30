//! Builds `data/xiv-db/<lang>.rkyv` from an ffxiv-datamining CSV tree.
//!
//! The container format (rkyv 0.7 + zlib `Compression::best()`) has to stay
//! exactly what `xiv-gen-db` decodes at runtime — do not "improve" it here.

use std::path::Path;

use anyhow::{Context, anyhow, ensure};
use flate2::{Compression, FlushCompress};
use xiv_gen::Language;
use xiv_gen::csv_to_rkyv::read_data_from;

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
    /// Item ids present in the `en` data, sorted — the missing-icon report
    /// checks these against the icons that exist upstream.
    pub en_item_ids: Vec<i32>,
}

/// Reads every language out of `datamining_root` and writes one pack per
/// language into `out_dir`.
pub fn build_packs(datamining_root: &Path, out_dir: &Path) -> anyhow::Result<DbOutput> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    let mut packs = Vec::with_capacity(LANGUAGES.len());
    let mut en_item_ids = Vec::new();
    for lang in LANGUAGES {
        let data = read_data_from(datamining_root, lang);
        if lang == Language::En {
            en_item_ids = data.items.keys().map(|id| id.0).collect();
            en_item_ids.sort_unstable();
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

        let dest = out_dir.join(format!("{}.rkyv", lang.to_path_part()));
        std::fs::write(&dest, &packed).with_context(|| format!("writing {}", dest.display()))?;

        packs.push(PackStats {
            lang,
            items,
            raw_bytes: raw.len(),
            packed_bytes: packed.len(),
        });
    }
    Ok(DbOutput { packs, en_item_ids })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_is_packed_exactly_once() {
        let mut paths: Vec<_> = LANGUAGES.iter().map(|l| l.to_path_part()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["cn", "de", "en", "fr", "ja", "ko", "tc"]);
    }
}
