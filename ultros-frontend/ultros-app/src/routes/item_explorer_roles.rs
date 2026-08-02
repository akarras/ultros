//! Role grouping for the item explorer's subcategory accordion.
//!
//! FFXIV's game data has no per-job "role" field in the sheets we ship
//! (`ClassJob` carries only indices/priorities), so the grouping is a
//! hand-written table keyed on job abbreviation. Weapon search
//! categories resolve to a role through their `class_job` field, so one
//! table serves both the Weapons tab and the Job Sets tab — and weapon
//! categories added by future expansions group automatically as long as
//! their job's abbreviation is in the table.

use xiv_gen::{ClassJobId, ItemSearchCategory};

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
    /// Display order of the accordion's role sections.
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

/// Map a weapon search category (group 1) to a role via its `class_job`.
pub(crate) fn role_for_weapon_category(
    cat: &ItemSearchCategory,
    data: &xiv_gen::Data,
) -> RoleGroup {
    data.class_jobs
        .get(&ClassJobId(cat.class_job as i32))
        .map(|job| role_for_job_abbr(&job.abbreviation))
        .unwrap_or(RoleGroup::Other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::item_explorer_toolbar::job_chips_sorted;

    #[test]
    fn every_visible_job_maps_to_a_role() {
        for job in job_chips_sorted() {
            let role = role_for_job_abbr(&job.abbreviation);
            assert_ne!(
                role,
                RoleGroup::Other,
                "job {:?} ({}) has no role mapping",
                job.abbreviation,
                job.name,
            );
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

    #[test]
    fn role_lookup_is_case_insensitive() {
        assert_eq!(role_for_job_abbr("pld"), RoleGroup::Tank);
        assert_eq!(role_for_job_abbr("Whm"), RoleGroup::Healer);
        assert_eq!(role_for_job_abbr("FSH"), RoleGroup::Land);
    }
}
