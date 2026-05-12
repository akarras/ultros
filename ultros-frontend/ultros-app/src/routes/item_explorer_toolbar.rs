//! Toolbar for `/items/*`: group pill selector over a subcategory chip
//! strip. Replaces the page-local sidebar that pre-dated the AppShell.

use xiv_gen::{ClassJob, ItemSearchCategoryId};

/// Resolve the active top-level category group (1=Weapons, 2=Armor,
/// 3=Items, 4=Housing, 5=Job Sets) from route params. Both args come
/// directly from `ParamsMap::get(...).as_deref()` at the call site, so
/// this helper has no router dependency and is trivial to unit-test.
pub(crate) fn active_group_from_route(jobset: Option<&str>, category: Option<&str>) -> Option<u8> {
    if jobset.is_some() {
        return Some(5);
    }
    let cat_raw = category?;
    let cat_name = percent_encoding::percent_decode_str(cat_raw)
        .decode_utf8()
        .ok()?;
    let data = xiv_gen_db::data();
    data.item_search_categorys
        .values()
        .find(|cat| cat.name == cat_name)
        .map(|cat| cat.category)
}

/// Return the search categories that belong to a non-job group
/// (1..=4), sorted by `cat.order`. Each entry is
/// `(display_name, ItemSearchCategoryId)`. Group 5 returns empty —
/// jobs use `job_chips_sorted` instead.
pub(crate) fn category_chips_for_group(group: u8) -> Vec<(&'static str, ItemSearchCategoryId)> {
    if group == 5 || group == 0 {
        return Vec::new();
    }
    let data = xiv_gen_db::data();
    let mut rows: Vec<(u8, &'static str, ItemSearchCategoryId)> = data
        .item_search_categorys
        .iter()
        .filter(|(_, cat)| cat.category == group)
        .map(|(id, cat)| (cat.order, cat.name.as_str(), *id))
        .collect();
    rows.sort_by_key(|(order, _, _)| *order);
    rows.into_iter().map(|(_, name, id)| (name, id)).collect()
}

/// Return the visible class jobs sorted by `ui_priority`. Mirrors the
/// filter used by `routes::item_explorer::JobsList` and the existing
/// `test_job_filtering` test: only jobs with `job_index > 0` or
/// `doh_dol_job_index >= 0`, and with a non-empty abbreviation or name.
pub(crate) fn job_chips_sorted() -> Vec<&'static ClassJob> {
    let data = xiv_gen_db::data();
    let mut jobs: Vec<&'static ClassJob> = data
        .class_jobs
        .iter()
        .filter(|(_, job)| job.job_index > 0 || job.doh_dol_job_index >= 0)
        .filter(|(_, job)| !job.abbreviation.is_empty() || !job.name.is_empty())
        .map(|(_, job)| job)
        .collect();
    jobs.sort_by_key(|job| job.ui_priority);
    jobs
}

/// Segment label shown on a job chip: prefer the abbreviation, fall
/// back to the full name. Matches the path-segment logic that
/// `routes::item_explorer::JobsList` uses for the `href`.
pub(crate) fn job_chip_label(job: &ClassJob) -> &str {
    if job.abbreviation.is_empty() {
        job.name.as_str()
    } else {
        job.abbreviation.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_group_is_none_on_bare_items_route() {
        assert_eq!(active_group_from_route(None, None), None);
    }

    #[test]
    fn active_group_for_jobset_route_is_five() {
        assert_eq!(active_group_from_route(Some("PLD"), None), Some(5));
    }

    #[test]
    fn active_group_for_weapon_category_is_one() {
        // "Pugilist's Arms" is a weapon (category = 1) in the xiv data.
        // Percent-encoded as it would arrive from the router.
        assert_eq!(
            active_group_from_route(None, Some("Pugilist%27s%20Arms")),
            Some(1),
        );
    }

    #[test]
    fn active_group_for_unknown_category_is_none() {
        assert_eq!(
            active_group_from_route(None, Some("Not%20A%20Real%20Category")),
            None,
        );
    }

    #[test]
    fn jobset_wins_over_category_when_both_present() {
        // Defensive — if the router ever produces both, Job Sets takes
        // precedence (matches the original `active_category_group` order).
        assert_eq!(active_group_from_route(Some("PLD"), Some("Sword")), Some(5),);
    }

    #[test]
    fn weapon_chips_are_sorted_by_order_and_non_empty() {
        let chips = category_chips_for_group(1);
        assert!(!chips.is_empty(), "weapons group must have chips");

        // Re-fetch the source-of-truth order from xiv data to assert sort.
        let data = xiv_gen_db::data();
        let mut expected: Vec<_> = data
            .item_search_categorys
            .iter()
            .filter(|(_, c)| c.category == 1)
            .map(|(id, c)| (c.order, c.name.as_str(), *id))
            .collect();
        expected.sort_by_key(|(order, _, _)| *order);
        let expected_names: Vec<&str> = expected.iter().map(|(_, name, _)| *name).collect();
        let actual_names: Vec<&str> = chips.iter().map(|(name, _)| *name).collect();
        assert_eq!(actual_names, expected_names);
    }

    #[test]
    fn job_sets_group_returns_no_category_chips() {
        // Group 5 is rendered as job chips, not category chips.
        assert!(category_chips_for_group(5).is_empty());
    }

    #[test]
    fn job_chips_contain_samurai_and_carpenter_but_not_marauder() {
        let chips = job_chips_sorted();
        let names: Vec<&str> = chips.iter().map(|j| j.name.as_str()).collect();
        assert!(names.contains(&"samurai"), "samurai should be in job chips");
        assert!(
            names.contains(&"carpenter"),
            "carpenter should be in job chips"
        );
        assert!(
            !names.contains(&"marauder"),
            "marauder must not be in job chips"
        );
    }

    #[test]
    fn job_chip_label_prefers_abbreviation() {
        let data = xiv_gen_db::data();
        let pld = data
            .class_jobs
            .iter()
            .find(|(_, j)| j.name == "paladin")
            .map(|(_, j)| j)
            .expect("paladin job must exist");
        assert_eq!(job_chip_label(pld), pld.abbreviation.as_str());
    }
}
