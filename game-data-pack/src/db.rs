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
    /// Ascending ids of the *named* items in the `en` data — the missing-icon
    /// report checks these against the icons that exist upstream.
    pub en_named_item_ids: Vec<i32>,
}

/// Item ids worth checking for a missing icon: the rows that actually have a
/// name, sorted ascending.
///
/// Measured against 7.55: 52,801 item rows of which 50,773 are named. Dropping
/// the 2,028 unnamed placeholder rows keeps rows that can never have an icon out
/// of the report, but it does not make the report small — see
/// `main::report_missing_icons` for why the count is inherently large.
fn named_item_ids<'a>(items: impl IntoIterator<Item = (i32, &'a str)>) -> Vec<i32> {
    let mut ids: Vec<i32> = items
        .into_iter()
        .filter(|(_, name)| !name.trim().is_empty())
        .map(|(id, _)| id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Reads every language out of `datamining_root` and writes one pack per
/// language into `out_dir`.
pub fn build_packs(datamining_root: &Path, out_dir: &Path) -> anyhow::Result<DbOutput> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    let mut packs = Vec::with_capacity(LANGUAGES.len());
    let mut en_named_item_ids = Vec::new();
    for lang in LANGUAGES {
        let data = read_data_from(datamining_root, lang);
        if lang == Language::En {
            en_named_item_ids = named_item_ids(
                data.items
                    .iter()
                    .map(|(id, item)| (id.0, item.name.as_str())),
            );
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
        en_named_item_ids,
    })
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
    fn named_item_ids_drops_unnamed_rows_and_sorts() {
        let rows = vec![
            (30, "Iron Ingot"),
            (10, ""),
            (44_000, "Grade 2 Gemdraught of Mind"),
            (20, "   "),
            (5, "Cotton"),
        ];
        assert_eq!(named_item_ids(rows), vec![5, 30, 44_000]);
    }

    #[test]
    fn named_item_ids_is_empty_when_nothing_is_named() {
        assert!(named_item_ids(vec![(1, ""), (2, " ")]).is_empty());
    }
}
