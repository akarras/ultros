//! Pure helpers for collapsing a per-job item list into named gear
//! sets. See module tests for the contract.

use xiv_gen::ItemId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupableItem {
    pub id: ItemId,
    pub name: String,
    pub ilvl: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSetGroup {
    pub stem: String,
    pub ilvl: i32,
    pub items: Vec<GroupableItem>,
}

/// Strip a leading English `Ornate ` (case-insensitive) marker —
/// FFXIV labels dyeable glamour variants this way, and the underlying
/// piece is otherwise identical to its plain-set counterpart.
/// Returns the input unchanged when the marker is absent.
///
/// V1 only handles the English token. The grouping algorithm itself
/// is language-agnostic (pure prefix comparison), so a localised
/// "Ornate" variant just falls through to the ungrouped fallback
/// rather than joining its set — a graceful degradation.
fn strip_ornate_prefix(name: &str) -> &str {
    const ORNATE: &str = "Ornate ";
    let bytes = name.as_bytes();
    if bytes.len() >= ORNATE.len() && bytes[..ORNATE.len()].eq_ignore_ascii_case(ORNATE.as_bytes())
    {
        &name[ORNATE.len()..]
    } else {
        name
    }
}

/// Longest common prefix of two `&str` slices, expressed as a UTF-8
/// byte length clamped to the previous character boundary so it can
/// always be sliced safely.
fn lcp_bytes(a: &str, b: &str) -> usize {
    let max = a.len().min(b.len());
    let bytes_a = a.as_bytes();
    let bytes_b = b.as_bytes();
    let mut i = 0;
    while i < max && bytes_a[i] == bytes_b[i] {
        i += 1;
    }
    while i > 0 && !a.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Trim a candidate stem to its last natural delimiter so e.g.
/// `"Courtly Lover's S"` collapses to `"Courtly Lover's"`. Handles
/// ASCII space, hyphen, JP middle dot (`・`), and the ideographic
/// space (`\u{3000}`).
fn trim_to_delimiter(stem: &str) -> &str {
    const DELIMS: &[char] = &[' ', '-', '・', '\u{3000}'];
    match stem.rfind(DELIMS) {
        Some(idx) => stem[..idx].trim_end(),
        None => stem.trim_end(),
    }
}

pub fn group_into_sets(items: Vec<GroupableItem>) -> (Vec<JobSetGroup>, Vec<GroupableItem>) {
    use std::collections::BTreeMap;

    // Bucket by ilvl. BTreeMap iterates in ascending order; we walk
    // it in reverse so higher-iLvl sets surface first in the UI.
    let mut buckets: BTreeMap<i32, Vec<GroupableItem>> = BTreeMap::new();
    for item in items {
        buckets.entry(item.ilvl).or_default().push(item);
    }

    let mut groups: Vec<JobSetGroup> = Vec::new();
    let mut ungrouped: Vec<GroupableItem> = Vec::new();

    for (ilvl, bucket) in buckets.into_iter().rev() {
        if bucket.len() < 2 {
            ungrouped.extend(bucket);
            continue;
        }

        // Fold LCP across all names in the bucket, comparing the
        // glamour-stripped form so "Ornate Courtly Lover's …" can
        // join "Courtly Lover's …" instead of forcing the bucket
        // apart at the first character.
        let mut reference: &str = strip_ornate_prefix(&bucket[0].name);
        let mut prefix_len = reference.len();
        for item in &bucket[1..] {
            let candidate = strip_ornate_prefix(&item.name);
            let common = lcp_bytes(&reference[..prefix_len], candidate);
            prefix_len = common;
            if prefix_len == 0 {
                break;
            }
            if candidate.len() < reference.len() {
                reference = candidate;
                prefix_len = prefix_len.min(reference.len());
            }
        }

        let stem = trim_to_delimiter(&reference[..prefix_len]);

        if stem.chars().any(|c| !c.is_whitespace()) {
            groups.push(JobSetGroup {
                stem: stem.to_string(),
                ilvl,
                items: bucket,
            });
        } else {
            ungrouped.extend(bucket);
        }
    }

    (groups, ungrouped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(id: u32, name: &str, ilvl: i32) -> GroupableItem {
        GroupableItem {
            id: ItemId(id as i32),
            name: name.to_string(),
            ilvl,
        }
    }

    /// Augmented Ironworks (PLD/Fending) at iLvl 130 — a Heavensward-
    /// era set that exists in every locale, including KR/CN which lag
    /// behind global on the most recent patches.  Each tuple is
    /// `(item_id, localised_name)`; the IDs are identical across
    /// locales because they're the FFXIV `ItemId` keys.
    ///
    /// Names sourced directly from the ffxiv-datamining CSVs at the
    /// pinned submodule commits.
    const AUGMENTED_IRONWORKS_EN: &[(u32, &str)] = &[
        (8876, "Augmented Ironworks Helm of Fending"),
        (8877, "Augmented Ironworks Armor of Fending"),
        (8878, "Augmented Ironworks Gauntlets of Fending"),
        (8879, "Augmented Ironworks Trousers of Fending"),
        (8880, "Augmented Ironworks Sabatons of Fending"),
        (8882, "Augmented Ironworks Bracelet of Fending"),
        (8883, "Augmented Ironworks Earrings of Fending"),
        (8884, "Augmented Ironworks Choker of Fending"),
        (8885, "Augmented Ironworks Ring of Fending"),
    ];
    const AUGMENTED_IRONWORKS_JA: &[(u32, &str)] = &[
        (8876, "ガーロンド・ディフェンダーヘルムRE"),
        (8877, "ガーロンド・ディフェンダーアーマーRE"),
        (8878, "ガーロンド・ディフェンダーガントレットRE"),
        (8879, "ガーロンド・ディフェンダートラウザーRE"),
        (8880, "ガーロンド・ディフェンダーサバトンRE"),
        (8882, "ガーロンド・ディフェンダーブレスレットRE"),
        (8883, "ガーロンド・ディフェンダーイヤリングRE"),
        (8884, "ガーロンド・ディフェンダーチョーカーRE"),
        (8885, "ガーロンド・ディフェンダーリングRE"),
    ];
    const AUGMENTED_IRONWORKS_KO: &[(u32, &str)] = &[
        (8876, "보강된 갈론드 수호자 투구"),
        (8877, "보강된 갈론드 수호자 갑옷"),
        (8878, "보강된 갈론드 수호자 건틀릿"),
        (8879, "보강된 갈론드 수호자 긴바지"),
        (8880, "보강된 갈론드 수호자 판금장화"),
        (8882, "보강된 갈론드 수호자 팔찌"),
        (8883, "보강된 갈론드 수호자 귀걸이"),
        (8884, "보강된 갈론드 수호자 목장식"),
        (8885, "보강된 갈론드 수호자 반지"),
    ];
    const AUGMENTED_IRONWORKS_CN: &[(u32, &str)] = &[
        (8876, "改良型加隆德御敌头盔"),
        (8877, "改良型加隆德御敌战甲"),
        (8878, "改良型加隆德御敌手铠"),
        (8879, "改良型加隆德御敌软甲裤"),
        (8880, "改良型加隆德御敌铠靴"),
        (8882, "改良型加隆德御敌手镯"),
        (8883, "改良型加隆德御敌耳坠"),
        (8884, "改良型加隆德御敌项环"),
        (8885, "改良型加隆德御敌指环"),
    ];
    const AUGMENTED_IRONWORKS_TC: &[(u32, &str)] = &[
        (8876, "改良型加隆德禦敵頭盔"),
        (8877, "改良型加隆德禦敵戰甲"),
        (8878, "改良型加隆德禦敵手鎧"),
        (8879, "改良型加隆德禦敵軟甲褲"),
        (8880, "改良型加隆德禦敵鎧靴"),
        (8882, "改良型加隆德禦敵手鐲"),
        (8883, "改良型加隆德禦敵耳墜"),
        (8884, "改良型加隆德禦敵項環"),
        (8885, "改良型加隆德禦敵指環"),
    ];

    fn fixture(rows: &[(u32, &str)]) -> Vec<GroupableItem> {
        rows.iter().map(|(id, n)| it(*id, n, 130)).collect()
    }

    fn assert_single_group_with_ids(
        locale_label: &str,
        rows: &[(u32, &str)],
        expected_ids: &[u32],
    ) {
        let (groups, ungrouped) = group_into_sets(fixture(rows));
        assert!(
            ungrouped.is_empty(),
            "{locale_label}: nothing should fall through to ungrouped, got: {ungrouped:?}",
        );
        assert_eq!(groups.len(), 1, "{locale_label}: expected exactly one set");
        let group = &groups[0];
        assert_eq!(group.ilvl, 130, "{locale_label}: iLvl preserved");
        let mut got_ids: Vec<i32> = group.items.iter().map(|i| i.id.0).collect();
        got_ids.sort();
        let mut want_ids: Vec<i32> = expected_ids.iter().map(|i| *i as i32).collect();
        want_ids.sort();
        assert_eq!(
            got_ids, want_ids,
            "{locale_label}: set membership must match across locales (same IDs in the group)",
        );
        assert!(
            group.stem.chars().any(|c| !c.is_whitespace()),
            "{locale_label}: stem must be non-empty for display, got {:?}",
            group.stem,
        );
    }

    #[test]
    fn cross_language_stability_old_set_en_ja_ko_cn_tc() {
        // The same nine item IDs must group together regardless of the
        // active display language. The displayed stem will differ —
        // that's fine; what matters for the user-facing detail page
        // is that set membership (the IDs) is identical so links and
        // price lookups behave the same.
        let expected_ids: Vec<u32> = AUGMENTED_IRONWORKS_EN.iter().map(|(id, _)| *id).collect();

        assert_single_group_with_ids("EN", AUGMENTED_IRONWORKS_EN, &expected_ids);
        assert_single_group_with_ids("JA", AUGMENTED_IRONWORKS_JA, &expected_ids);
        assert_single_group_with_ids("KO", AUGMENTED_IRONWORKS_KO, &expected_ids);
        assert_single_group_with_ids("CN", AUGMENTED_IRONWORKS_CN, &expected_ids);
        assert_single_group_with_ids("TC", AUGMENTED_IRONWORKS_TC, &expected_ids);
    }

    #[test]
    fn cross_language_stems_pick_up_recognisable_set_names() {
        // Spot-check the display stems. We don't want the algorithm
        // to drift into showing nonsense titles (e.g. a single
        // character from a kanji compound).
        let (groups, _) = group_into_sets(fixture(AUGMENTED_IRONWORKS_EN));
        assert_eq!(groups[0].stem, "Augmented Ironworks");

        let (groups, _) = group_into_sets(fixture(AUGMENTED_IRONWORKS_KO));
        assert_eq!(groups[0].stem, "보강된 갈론드 수호자");

        // CJK names without latin delimiters keep their full prefix.
        let (groups, _) = group_into_sets(fixture(AUGMENTED_IRONWORKS_CN));
        assert_eq!(groups[0].stem, "改良型加隆德御敌");

        let (groups, _) = group_into_sets(fixture(AUGMENTED_IRONWORKS_TC));
        assert_eq!(groups[0].stem, "改良型加隆德禦敵");
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let (groups, ungrouped) = group_into_sets(Vec::new());
        assert!(groups.is_empty());
        assert!(ungrouped.is_empty());
    }

    #[test]
    fn partition_is_complete() {
        // Every input item must end up in exactly one output bucket.
        let items = vec![
            it(1, "Augmented Ironworks Helm of Fending", 130),
            it(2, "Augmented Ironworks Armor of Fending", 130),
            it(3, "Bahamut's Talon", 600), // singleton -> ungrouped
            it(4, "Adamantite Pickaxe", 130),
            it(5, "Holy Cermet Hatchet", 130),
        ];
        let total = items.len();

        let (groups, ungrouped) = group_into_sets(items);

        let grouped_count: usize = groups.iter().map(|g| g.items.len()).sum();
        assert_eq!(
            grouped_count + ungrouped.len(),
            total,
            "every input item must appear once in either groups or ungrouped",
        );
    }

    #[test]
    fn ornate_variant_joins_base_set() {
        // FFXIV dyeable glamours ship as "Ornate <Set Name> <Slot>",
        // sharing iLvl with the plain set. They should land in the
        // same group, with the stem still reading as the plain set
        // name so the UI doesn't display "Ornate" twice.
        let items = vec![
            it(1, "Courtly Lover's Surcoat of Fending", 770),
            it(2, "Courtly Lover's Breeches of Fending", 770),
            it(3, "Courtly Lover's Boots of Fending", 770),
            it(4, "Ornate Courtly Lover's Surcoat of Fending", 770),
            it(5, "Ornate Courtly Lover's Breeches of Fending", 770),
        ];

        let (groups, ungrouped) = group_into_sets(items);

        assert!(
            ungrouped.is_empty(),
            "Ornate variants should not fall through to ungrouped, got: {ungrouped:?}",
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].stem, "Courtly Lover's");
        assert_eq!(groups[0].items.len(), 5);
    }

    #[test]
    fn singleton_bucket_falls_to_ungrouped() {
        // A relic or one-off accessory at a unique iLvl shouldn't
        // pretend to be a "set" on its own.
        let items = vec![
            it(1, "Bahamut's Talon", 600),
            it(2, "Courtly Lover's Sword", 770),
            it(3, "Courtly Lover's Shield", 770),
        ];

        let (groups, ungrouped) = group_into_sets(items);

        assert_eq!(groups.len(), 1, "only the 770 pair forms a set");
        assert_eq!(groups[0].stem, "Courtly Lover's");
        assert_eq!(ungrouped.len(), 1);
        assert_eq!(ungrouped[0].name, "Bahamut's Talon");
    }

    #[test]
    fn ungrouped_when_bucket_has_no_meaningful_prefix() {
        // Two items at the same iLvl that share no name prefix
        // (mixed-set leftovers) should both fall to ungrouped.
        let items = vec![
            it(1, "Adamantite Pickaxe", 130),
            it(2, "Holy Cermet Hatchet", 130),
        ];

        let (groups, ungrouped) = group_into_sets(items);

        assert!(groups.is_empty(), "no shared prefix => no set");
        assert_eq!(ungrouped.len(), 2);
    }

    #[test]
    fn does_not_cross_ilvl_buckets() {
        // Two PLD sets that happen to share a prefix but at
        // different iLvls — they must NOT merge into one group.
        let items = vec![
            it(1, "Ironworks Sword", 110),
            it(2, "Ironworks Shield", 110),
            it(3, "Ironworks Helm of Fending", 110),
            it(4, "Augmented Ironworks Sword", 130),
            it(5, "Augmented Ironworks Shield", 130),
            it(6, "Augmented Ironworks Helm of Fending", 130),
        ];

        let (groups, ungrouped) = group_into_sets(items);

        assert!(ungrouped.is_empty(), "no item should be ungrouped");
        assert_eq!(groups.len(), 2, "exactly one group per iLvl");
        // Higher iLvl first.
        assert_eq!(groups[0].ilvl, 130);
        assert_eq!(groups[0].stem, "Augmented Ironworks");
        assert_eq!(groups[1].ilvl, 110);
        assert_eq!(groups[1].stem, "Ironworks");
    }

    #[test]
    fn groups_courtly_lovers_set_in_english() {
        // Real PLD job-set members at iLvl 770 (Dawntrail patch series).
        let items = vec![
            it(1, "Courtly Lover's Surcoat of Fending", 770),
            it(2, "Courtly Lover's Ring of Fending", 770),
            it(3, "Courtly Lover's Hairpin of Fending", 770),
            it(4, "Courtly Lover's Shield", 770),
            it(5, "Courtly Lover's Sword", 770),
        ];

        let (groups, ungrouped) = group_into_sets(items);

        assert_eq!(ungrouped, Vec::<GroupableItem>::new());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].stem, "Courtly Lover's");
        assert_eq!(groups[0].ilvl, 770);
        assert_eq!(groups[0].items.len(), 5);
    }
}
