//! Toolbar for `/items/*`: group pill selector over a subcategory chip
//! strip. Replaces the page-local sidebar that pre-dated the AppShell.

use crate::components::grouped_nav_accordion::{GroupedNavAccordion, NavIcon, NavLink};
use crate::components::toolbar::{Toolbar, ToolbarField, ToolbarPills, ToolbarSpacer};
use crate::components::world_picker::WorldPicker;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
use crate::routes::item_explorer::{
    canonical_job_acronym, resolve_category_param, resolve_jobset_param,
};
use crate::routes::item_explorer_roles::{RoleGroup, role_for_job, role_for_weapon_category};
use crate::routes::item_explorer_scope::{ExplorerPriceScope, href_with_world};
use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_params_map};
use xiv_gen::{ClassJob, ItemSearchCategoryId};

/// Resolve the active top-level category group (1=Weapons, 2=Armor,
/// 3=Items, 4=Housing, 5=Job Sets) from route params. Both args come
/// directly from `ParamsMap::get(...).as_deref()` at the call site, so
/// this helper has no router dependency and is trivial to unit-test.
pub(crate) fn active_group_from_route(jobset: Option<&str>, category: Option<&str>) -> Option<u8> {
    if jobset.is_some() {
        return Some(5);
    }
    let data = xiv_gen_db::data();
    resolve_category_param(data, category?).map(|cat| cat.category)
}

/// Whether the subcategory accordion starts expanded for a route.
///
/// Open only when the URL names no subcategory — the bare `/items` landing,
/// where the picker is the entire content of the page. A deep link that
/// already names a category or job set starts collapsed so the item list
/// gets the viewport. An unresolvable category counts as "names nothing"
/// and opens, so a dead link lands on a usable picker.
///
/// Both args come straight from `ParamsMap::get(...).as_deref()` at the call
/// site, so this stays router-free and unit-testable like its neighbour.
pub(crate) fn accordion_open_for_route(jobset: Option<&str>, category: Option<&str>) -> bool {
    active_group_from_route(jobset, category).is_none()
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
    job_chips_sorted_in(xiv_gen_db::data())
}

/// `job_chips_sorted` against an explicit dataset, so the locale-independence
/// tests can run the real filter over each shipped locale pack rather than a
/// reimplementation of it.
pub(crate) fn job_chips_sorted_in(data: &'static xiv_gen::Data) -> Vec<&'static ClassJob> {
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

/// Bucket `(role, link)` pairs into ordered, labeled sections, dropping the
/// roles that got no links. Both role-grouped tabs — Weapons (via the
/// category's `class_job`) and Job Sets (via the job's id, through
/// `role_for_job`) — build their sections through this.
fn role_buckets(
    items: impl Iterator<Item = (RoleGroup, NavLink)>,
    role_label: impl Fn(RoleGroup) -> String,
) -> Vec<(Option<String>, Vec<NavLink>)> {
    let mut buckets: Vec<(RoleGroup, Vec<NavLink>)> = RoleGroup::ORDERED
        .iter()
        .map(|role| (*role, Vec::new()))
        .collect();
    for (role, link) in items {
        if let Some((_, links)) = buckets.iter_mut().find(|(r, _)| *r == role) {
            links.push(link);
        }
    }
    buckets
        .into_iter()
        .filter(|(_, links)| !links.is_empty())
        .map(|(role, links)| (Some(role_label(role)), links))
        .collect()
}

/// Path segment for a job's `/items/jobset/:jobset` link.
///
/// The canonical English acronym, *not* `job_chip_label`: the label is
/// localized, and a localized slug is a link that resolves under no locale at
/// all — `job_category_lookup` only knows the English acronyms, so a German
/// client's own "FST" chip navigates to an empty page. Falls back to the
/// (escaped) label only for a job id outside the acronym table.
pub(crate) fn job_chip_slug(job: &ClassJob) -> String {
    canonical_job_acronym(job.key_id)
        .map(|acronym| acronym.to_string())
        .unwrap_or_else(|| job_chip_label(job).replace('/', "%2F"))
}

/// Localized display label for a `/items/jobset/:jobset` param.
///
/// The param is the canonical English acronym, so showing it verbatim would
/// label a German player's active chip "PGL" where every other chip reads
/// "FST". Resolve it back through the job the same way `CategoryItems` turns
/// its numeric category id back into a localized name; falls through to the
/// raw param when it names no known job.
pub(crate) fn jobset_display_label(data: &xiv_gen::Data, raw_param: &str) -> Option<String> {
    let canonical = resolve_jobset_param(data, raw_param)?;
    data.class_jobs
        .iter()
        .find(|(id, _)| canonical_job_acronym(**id) == Some(canonical.as_str()))
        .map(|(_, job)| job_chip_label(job).to_string())
}

#[component]
pub fn ItemExplorerToolbar() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let location = use_location();
    let scope = use_context::<ExplorerPriceScope>().expect(
        "ItemExplorerToolbar is always rendered inside ItemExplorer, which provides the scope",
    );

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

    // Expanded state, owned here rather than in the accordion: a group pill
    // click has to force it open.
    let open = RwSignal::new({
        let p = params.get_untracked();
        accordion_open_for_route(p.get("jobset").as_deref(), p.get("category").as_deref())
    });

    // Collapse onto any navigation; re-open on the bare /items route.
    //
    // Choosing any subcategory means the user is done picking, so every
    // navigation should collapse the accordion — including between two
    // categories in the same group (/items/category/24 ->
    // /items/category/25, both Armor). `active_group` is a `Memo` that only
    // notifies when its *value* changes, and that same-group case leaves it
    // at `Some(2)` on both sides, so tracking it alone would miss the nav.
    // Tracking the pathname directly makes every navigation re-run this.
    // On its first run it writes the same value `open` was already
    // initialised to, so there is no hydration flicker. Pill clicks don't
    // navigate, so they can't be undone by this effect.
    Effect::new(move |_| {
        location.pathname.track();
        open.set(active_group.get().is_none());
    });

    let pill = move |group: u8, label_view: AnyView| {
        view! {
            <button
                type="button"
                aria-pressed=move || (selected_group.get() == group).to_string()
                aria-controls="item-explorer-subcategories"
                on:click=move |_| {
                    selected_group.set(group);
                    open.set(true);
                }
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
                <ToolbarSpacer />
                <ToolbarField label=t_string!(i18n, item_explorer_world_picker_label).to_string()>
                    <WorldPicker
                        current_world=scope.picker_value
                        set_current_world=scope.picker_setter
                    />
                </ToolbarField>
            </Toolbar>

            <div
                role="navigation"
                aria-label=t_string!(i18n, item_explorer_categories).to_string()
            >
                {move || {
                    // Track the locale-swap revision so category/job names
                    // re-render after `reload_xiv_data`.
                    let data = tracked_data();
                    let group = selected_group.get();
                    let world = scope.query_world.get();
                    let role_label = |role: RoleGroup| -> String {
                        match role {
                            RoleGroup::Tank => t_string!(i18n, item_explorer_role_tank).to_string(),
                            RoleGroup::Healer => t_string!(i18n, item_explorer_role_healer).to_string(),
                            RoleGroup::Melee => t_string!(i18n, item_explorer_role_melee).to_string(),
                            RoleGroup::PhysRanged => {
                                t_string!(i18n, item_explorer_role_phys_ranged).to_string()
                            }
                            RoleGroup::Caster => t_string!(i18n, item_explorer_role_caster).to_string(),
                            RoleGroup::Hand => t_string!(i18n, item_explorer_role_hand).to_string(),
                            RoleGroup::Land => t_string!(i18n, item_explorer_role_land).to_string(),
                            RoleGroup::Other => t_string!(i18n, item_explorer_role_other).to_string(),
                        }
                    };
                    // Header label: the active subcategory when it belongs to
                    // the selected group, else a browse prompt.
                    let button_label = if active_group.get() == Some(group) {
                        let p = params.get();
                        match p.get("jobset") {
                            // The jobset param is a canonical English acronym,
                            // so resolve it back to the localized label rather
                            // than labelling a German player's trigger "PGL".
                            Some(jobset) => jobset_display_label(data, &jobset).or_else(|| {
                                percent_encoding::percent_decode_str(&jobset)
                                    .decode_utf8()
                                    .ok()
                                    .map(|s| s.to_string())
                            }),
                            // The category param is an id, so resolve it back
                            // to the localized name instead of labelling the
                            // header with a bare number.
                            None => p.get("category").and_then(|cat| {
                                resolve_category_param(data, &cat)
                                    .map(|category| category.name.clone())
                            }),
                        }
                    } else {
                        None
                    }
                    .unwrap_or_else(|| {
                        t_string!(i18n, item_explorer_browse_subcategories).to_string()
                    });

                    let groups: Vec<(Option<String>, Vec<NavLink>)> = if group == 5 {
                        // Job sets: jobs bucketed by role.
                        role_buckets(
                            job_chips_sorted()
                                .into_iter()
                                .map(|job| {
                                    // Label stays localized (a German player
                                    // expects "FST"); the href uses the
                                    // canonical English acronym so the route
                                    // resolves on both the English SSR pass
                                    // and the client's locale. Role likewise
                                    // buckets on the job's id, not on the
                                    // localized abbreviation.
                                    (
                                        role_for_job(job),
                                        NavLink {
                                            label: job_chip_label(job).to_string(),
                                            href: href_with_world(
                                                format!(
                                                    "/items/jobset/{}",
                                                    job_chip_slug(job),
                                                ),
                                                world.as_deref(),
                                            ),
                                            icon: NavIcon::Job(job.key_id),
                                        },
                                    )
                                }),
                            role_label,
                        )
                    } else if group == 1 {
                        // Weapons: categories bucketed by the role of their
                        // associated job.
                        role_buckets(
                            category_chips_for_group(1)
                                .into_iter()
                                .map(|(name, id)| {
                                    let role = data
                                        .item_search_categorys
                                        .get(&id)
                                        .map(|cat| role_for_weapon_category(cat, data))
                                        .unwrap_or(RoleGroup::Other);
                                    (
                                        role,
                                        NavLink {
                                            label: name.to_string(),
                                            href: href_with_world(
                                                format!("/items/category/{}", id.0),
                                                world.as_deref(),
                                            ),
                                            icon: NavIcon::Category(id),
                                        },
                                    )
                                }),
                            role_label,
                        )
                    } else {
                        // Armor, Items, Housing: one headerless section.
                        let links: Vec<NavLink> = category_chips_for_group(group)
                            .into_iter()
                            .map(|(name, id)| NavLink {
                                label: name.to_string(),
                                href: href_with_world(
                                    format!("/items/category/{}", id.0),
                                    world.as_deref(),
                                ),
                                icon: NavIcon::Category(id),
                            })
                            .collect();
                        vec![(None, links)]
                    };

                    view! {
                        <GroupedNavAccordion
                            button_label=button_label
                            groups=groups
                            open=open
                        />
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
    use xiv_gen::ClassJobId;

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

    /// Category ids are the canonical route key — resolving them is what
    /// keeps the toolbar's selected group identical between the English SSR
    /// render and a localized client.
    #[test]
    fn active_group_resolves_a_numeric_category_id() {
        let data = xiv_gen_db::data();
        let weapon = data
            .item_search_categorys
            .values()
            .filter(|cat| cat.category == 1)
            .min_by_key(|cat| cat.key_id.0)
            .expect("weapons group must have categories");
        assert_eq!(
            active_group_from_route(None, Some(&weapon.key_id.0.to_string())),
            Some(1),
            "a numeric category id must select the category's own group",
        );
    }

    #[test]
    fn active_group_for_unknown_category_id_is_none() {
        // Well past the end of the sheet; must not select a group.
        assert_eq!(active_group_from_route(None, Some("999999")), None);
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

    #[test]
    fn accordion_starts_open_on_bare_items_route() {
        // Nothing is selected yet, so the picker is the whole point of the
        // page — show it expanded.
        assert!(accordion_open_for_route(None, None));
    }

    #[test]
    fn accordion_starts_closed_for_a_jobset_deep_link() {
        assert!(!accordion_open_for_route(Some("PLD"), None));
    }

    #[test]
    fn accordion_starts_closed_for_a_category_id_deep_link() {
        // The canonical route key is the numeric id.
        let data = xiv_gen_db::data();
        let weapon = data
            .item_search_categorys
            .values()
            .filter(|cat| cat.category == 1)
            .min_by_key(|cat| cat.key_id.0)
            .expect("weapons group must have categories");
        assert!(!accordion_open_for_route(
            None,
            Some(&weapon.key_id.0.to_string()),
        ));
    }

    #[test]
    fn accordion_starts_closed_for_a_legacy_name_param() {
        // Links minted before the switch to ids are percent-encoded names
        // and still resolve; they name a category, so they collapse too.
        assert!(!accordion_open_for_route(None, Some("Pugilist%27s%20Arms"),));
    }

    #[test]
    fn accordion_starts_open_for_an_unresolvable_category() {
        // A dead link selects nothing, so falling back to the expanded
        // picker is the useful answer rather than a collapsed header
        // labelled with a category that does not exist.
        assert!(accordion_open_for_route(None, Some("999999")));
    }

    #[test]
    fn every_group_produces_at_least_one_subcategory_link() {
        // All five groups now render through one accordion instead of the
        // old popover/chip-strip fork, so an empty group would silently
        // produce an accordion that expands to nothing.
        for group in 1..=4u8 {
            assert!(
                !category_chips_for_group(group).is_empty(),
                "group {group} must have at least one category chip",
            );
        }
        assert!(
            !job_chips_sorted().is_empty(),
            "the job sets group must have at least one chip",
        );
    }

    #[test]
    fn role_buckets_orders_sections_and_drops_empty_roles() {
        let link = |label: &str| NavLink {
            label: label.to_string(),
            href: String::new(),
            icon: NavIcon::Job(ClassJobId(0)),
        };
        let sections = role_buckets(
            [
                (RoleGroup::Caster, link("BLM")),
                (RoleGroup::Tank, link("PLD")),
                (RoleGroup::Tank, link("WAR")),
            ]
            .into_iter(),
            |role| format!("{role:?}"),
        );
        // Sections follow RoleGroup::ORDERED (Tank before Caster) no matter
        // what order the links arrive in, and the six roles that got no
        // links produce no section header at all.
        assert_eq!(sections.len(), 2, "empty roles must not render a section");
        assert_eq!(sections[0].0.as_deref(), Some("Tank"));
        assert_eq!(
            sections[0]
                .1
                .iter()
                .map(|l| l.label.as_str())
                .collect::<Vec<_>>(),
            vec!["PLD", "WAR"],
            "links keep their input order within a bucket",
        );
        assert_eq!(sections[1].0.as_deref(), Some("Caster"));
    }
}
