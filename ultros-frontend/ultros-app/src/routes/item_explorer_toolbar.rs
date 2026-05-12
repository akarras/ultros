//! Toolbar for `/items/*`: group pill selector over a subcategory chip
//! strip. Replaces the page-local sidebar that pre-dated the AppShell.

use crate::components::fonts::{ClassJobIcon, ItemSearchCategoryIcon};
use crate::components::toolbar::{Toolbar, ToolbarPills};
use crate::i18n::{t, t_string, use_i18n};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
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
/// filter used by the original sidebar `JobsList` and the existing
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
/// back to the full name. Matches the path-segment logic that the
/// original sidebar `JobsList` used for the `href`.
pub(crate) fn job_chip_label(job: &ClassJob) -> &str {
    if job.abbreviation.is_empty() {
        job.name.as_str()
    } else {
        job.abbreviation.as_str()
    }
}

#[component]
pub fn ItemExplorerToolbar() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();

    let active_group = Memo::new(move |_| {
        let p = params();
        active_group_from_route(p.get("jobset").as_deref(), p.get("category").as_deref())
    });

    // Default selection: whatever the route says, else Weapons (1).
    let selected_group = RwSignal::new(active_group.get_untracked().unwrap_or(1));

    // When the route changes (e.g. browser back), follow it.
    Effect::new(move |_| {
        selected_group.set(active_group.get().unwrap_or(1));
    });

    let pill = move |group: u8, label_view: AnyView| {
        view! {
            <button
                aria-pressed=move || (selected_group.get() == group).to_string()
                on:click=move |_| selected_group.set(group)
            >
                {label_view}
            </button>
        }
    };

    view! {
        <div class="flex flex-col gap-3 mb-4">
            <Toolbar>
                <ToolbarPills>
                    {pill(1, view! { {t!(i18n, item_explorer_weapons)} }.into_any())}
                    {pill(2, view! { {t!(i18n, item_explorer_armor)} }.into_any())}
                    {pill(3, view! { {t!(i18n, item_explorer_items)} }.into_any())}
                    {pill(4, view! { {t!(i18n, item_explorer_housing)} }.into_any())}
                    {pill(5, view! { {t!(i18n, item_explorer_job_sets)} }.into_any())}
                </ToolbarPills>
            </Toolbar>

            <div
                class="item-explorer-chip-row"
                role="navigation"
                aria-label=t_string!(i18n, item_explorer_categories).to_string()
            >
                {move || {
                    let group = selected_group.get();
                    if group == 5 {
                        job_chips_sorted()
                            .into_iter()
                            .map(|job| {
                                let label = job_chip_label(job).to_string();
                                let href = format!(
                                    "/items/jobset/{}",
                                    label.replace('/', "%2F")
                                );
                                let job_id = job.key_id;
                                view! {
                                    <A href=href attr:class="item-explorer-chip">
                                        <ClassJobIcon id=job_id />
                                        <span>{label}</span>
                                    </A>
                                }.into_any()
                            })
                            .collect::<Vec<_>>()
                    } else {
                        category_chips_for_group(group)
                            .into_iter()
                            .map(|(name, id)| {
                                let href = format!(
                                    "/items/category/{}",
                                    name.replace('/', "%2F")
                                );
                                view! {
                                    <A href=href attr:class="item-explorer-chip">
                                        <ItemSearchCategoryIcon id=id />
                                        <span>{name}</span>
                                    </A>
                                }.into_any()
                            })
                            .collect::<Vec<_>>()
                    }
                }}
            </div>
        </div>
    }
    .into_any()
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
        // Compare (name, id) pairs, not just names, so a tie on `order`
        // between two categories can't silently mask an ID mismatch.
        let data = xiv_gen_db::data();
        let mut expected: Vec<_> = data
            .item_search_categorys
            .iter()
            .filter(|(_, c)| c.category == 1)
            .map(|(id, c)| (c.order, c.name.as_str(), *id))
            .collect();
        expected.sort_by_key(|(order, _, _)| *order);
        let expected_pairs: Vec<(&str, ItemSearchCategoryId)> =
            expected.iter().map(|(_, name, id)| (*name, *id)).collect();
        assert_eq!(chips, expected_pairs);
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
    fn job_chips_are_sorted_by_ui_priority_ascending() {
        let chips = job_chips_sorted();
        let priorities: Vec<u32> = chips.iter().map(|j| j.ui_priority).collect();
        let mut sorted = priorities.clone();
        sorted.sort();
        assert_eq!(
            priorities, sorted,
            "job chips must be sorted by ui_priority ascending"
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
