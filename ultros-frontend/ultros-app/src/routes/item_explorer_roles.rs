//! Role grouping for the item explorer's subcategory popovers.
//!
//! FFXIV's game data has no per-job "role" field in the sheets we ship
//! (`ClassJob` carries only indices/priorities), so the grouping is a
//! hand-written table keyed on job abbreviation. Weapon search
//! categories resolve to a role through their `class_job` field, so one
//! table serves both the Weapons tab and the Job Sets tab — and weapon
//! categories added by future expansions group automatically as long as
//! their job's abbreviation is in the table.

use crate::routes::item_explorer::canonical_job_acronym;
use xiv_gen::{ClassJob, ClassJobId, ItemSearchCategory};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub(crate) enum RoleGroup {
    Tank,
    Healer,
    Melee,
    PhysRanged,
    Caster,
    Hand,
    Land,
    Other,
}

impl RoleGroup {
    /// Display order of the popover sections.
    pub(crate) const ORDERED: [RoleGroup; 8] = [
        RoleGroup::Tank,
        RoleGroup::Healer,
        RoleGroup::Melee,
        RoleGroup::PhysRanged,
        RoleGroup::Caster,
        RoleGroup::Hand,
        RoleGroup::Land,
        RoleGroup::Other,
    ];
}

/// Map a job abbreviation (base class or job, any case) to its role.
pub(crate) fn role_for_job_abbr(abbr: &str) -> RoleGroup {
    match abbr.to_ascii_uppercase().as_str() {
        "GLA" | "PLD" | "MRD" | "WAR" | "DRK" | "GNB" => RoleGroup::Tank,
        "CNJ" | "WHM" | "SCH" | "AST" | "SGE" => RoleGroup::Healer,
        "PGL" | "MNK" | "LNC" | "DRG" | "ROG" | "NIN" | "SAM" | "RPR" | "VPR" | "BST" => {
            RoleGroup::Melee
        }
        "ARC" | "BRD" | "MCH" | "DNC" => RoleGroup::PhysRanged,
        "THM" | "BLM" | "ACN" | "SMN" | "RDM" | "BLU" | "PCT" => RoleGroup::Caster,
        "CRP" | "BSM" | "ARM" | "GSM" | "LTW" | "WVR" | "ALC" | "CUL" => RoleGroup::Hand,
        "MIN" | "BTN" | "FSH" => RoleGroup::Land,
        _ => {
            tracing::warn!(abbr, "Unknown job abbreviation for role grouping");
            RoleGroup::Other
        }
    }
}

/// Map a [`ClassJob`] to its role.
///
/// Keyed on the job's **id**, not its `abbreviation`: that field is a
/// localized display string ("FST" in German for the job English calls "PGL"),
/// so matching it against the English table below buckets 23 of 36 German and
/// 22 of 36 French jobs into [`RoleGroup::Other`]. Ids are locale-independent,
/// so this resolves identically for every visitor.
pub(crate) fn role_for_job(job: &ClassJob) -> RoleGroup {
    match canonical_job_acronym(job.key_id) {
        Some(acronym) => role_for_job_abbr(acronym),
        None => {
            tracing::warn!(id = job.key_id.0, "Class job id outside the acronym table");
            RoleGroup::Other
        }
    }
}

/// Map a weapon search category (group 1) to a role via its `class_job`.
pub(crate) fn role_for_weapon_category(
    cat: &ItemSearchCategory,
    data: &xiv_gen::Data,
) -> RoleGroup {
    data.class_jobs
        .get(&ClassJobId(cat.class_job as i32))
        .map(role_for_job)
        .unwrap_or(RoleGroup::Other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::item_explorer_toolbar::{
        category_chips_for_group, job_chips_sorted, job_chips_sorted_in,
    };
    use xiv_gen::Language;

    #[test]
    fn every_visible_job_maps_to_a_role() {
        for job in job_chips_sorted() {
            let role = role_for_job(job);
            assert_ne!(
                role,
                RoleGroup::Other,
                "job {:?} ({}) has no role mapping",
                job.abbreviation,
                job.name,
            );
        }
    }

    /// `every_visible_job_maps_to_a_role` passed even while the grouping was
    /// broken, because the test process loads English data. Keying the lookup
    /// on the localized `abbreviation` dropped 23 of 36 German and 22 of 36
    /// French jobs into `Other`, collapsing the role sections of the Job Sets
    /// popover into one undifferentiated list.
    #[test]
    fn role_grouping_is_identical_in_every_locale() {
        let en = xiv_gen_db::data_for(Language::En);
        for lang in [
            Language::En,
            Language::Ja,
            Language::De,
            Language::Fr,
            Language::Cn,
            Language::Ko,
            Language::Tc,
        ] {
            let data = xiv_gen_db::data_for(lang);
            for job in job_chips_sorted_in(data) {
                let role = role_for_job(job);
                assert_ne!(
                    role,
                    RoleGroup::Other,
                    "{lang:?}: job {:?} ({}) has no role mapping",
                    job.abbreviation,
                    job.name,
                );
                let en_role = en
                    .class_jobs
                    .get(&job.key_id)
                    .map(role_for_job)
                    .expect("the same job id exists in every locale");
                assert_eq!(
                    role, en_role,
                    "{lang:?}: job id {} buckets differently than under English data",
                    job.key_id.0,
                );
            }
        }
    }

    #[test]
    fn every_weapon_category_maps_to_a_role() {
        let data = xiv_gen_db::data();
        for (id, cat) in data
            .item_search_categorys
            .iter()
            .filter(|(_, c)| c.category == 1)
        {
            let role = role_for_weapon_category(cat, data);
            assert_ne!(
                role,
                RoleGroup::Other,
                "weapon category {:?} (id {}, class_job {}) has no role mapping",
                cat.name,
                id.0,
                cat.class_job,
            );
        }
    }

    /// Documents the chips-vs-dropdown decision for the non-grouped tabs:
    /// with >8 subcategories a tab renders the popover, with <=8 it keeps
    /// the inline chip strip. If game data ever changes these counts the
    /// toolbar adapts automatically — this test just makes the change
    /// visible in review.
    #[test]
    fn non_weapon_groups_have_expected_sizes() {
        for group in 2..=4u8 {
            let count = category_chips_for_group(group).len();
            assert!(
                count > 8,
                "group {group} has {count} subcategories; expected >8 (popover branch)",
            );
        }
    }

    #[test]
    fn role_lookup_is_case_insensitive() {
        assert_eq!(role_for_job_abbr("pld"), RoleGroup::Tank);
        assert_eq!(role_for_job_abbr("Whm"), RoleGroup::Healer);
        assert_eq!(role_for_job_abbr("FSH"), RoleGroup::Land);
    }
}
