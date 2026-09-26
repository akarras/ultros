//! Shared list data helpers and activity feed. The legacy REST page is retired.
use crate::components::{
    listing_filters::filter_active_listings,
    skeleton::{SkeletonCell, SkeletonColumn},
};
use crate::i18n::*;
use leptos::prelude::*;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use ultros_api_types::{
    ActiveListing,
    list::{ListActivity, ListItem, ListWithPermission},
    world_helper::WorldHelper,
};

/// The `(list, rows with listings)` shape every list surface consumes, named
/// once so `list_view_sync.rs` can spell it in its own loader's signature.
pub(crate) type ListViewResult =
    Result<(ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>), crate::error::AppError>;

/// Drop every listing whose world *or* datacenter is excluded, for every
/// item. This is the one place exclusion is applied to the list view's data —
/// the table rows, summary, price sort, and buying view all consume its
/// output, so a DC exclusion can't be honored by one surface and ignored by
/// another (the pre-redesign bug: only `BuyingView` and `PriceViewer` looked
/// at `excluded-datacenters`, so the sort order and row listings never did).
pub(crate) fn filter_excluded(
    items: &[(ListItem, Vec<ActiveListing>)],
    excluded_worlds: &HashSet<i32>,
    excluded_datacenters: &HashSet<String>,
    world_helper: Option<&WorldHelper>,
) -> Vec<(ListItem, Vec<ActiveListing>)> {
    if excluded_worlds.is_empty() && excluded_datacenters.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .map(|(item, listings)| {
            (
                item.clone(),
                filter_active_listings(
                    listings.clone(),
                    world_helper,
                    excluded_worlds,
                    excluded_datacenters,
                ),
            )
        })
        .collect()
}

/// Comma-separated list of world ids for a query param. Held sorted and
/// deduped so encoding is canonical and a URL round-trips exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IdList(pub(crate) Vec<i32>);

impl FromStr for IdList {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut ids: Vec<i32> = s
            .split(',')
            .filter_map(|part| part.trim().parse().ok())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(Self(ids))
    }
}

impl fmt::Display for IdList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, id) in self.0.iter().enumerate() {
            if index > 0 {
                write!(f, ",")?;
            }
            write!(f, "{id}")?;
        }
        Ok(())
    }
}

/// Comma-separated list of datacenter names for a query param. Sorted and
/// deduped for the same canonical-encoding reason as [`IdList`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NameList(pub(crate) Vec<String>);

impl FromStr for NameList {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut names: Vec<String> = s
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
        names.sort_unstable();
        names.dedup();
        Ok(Self(names))
    }
}

impl fmt::Display for NameList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, name) in self.0.iter().enumerate() {
            if index > 0 {
                write!(f, ",")?;
            }
            write!(f, "{name}")?;
        }
        Ok(())
    }
}

pub(crate) fn remaining_quantity(item: &ListItem) -> i32 {
    let quantity = item.quantity.unwrap_or(1).max(1);
    quantity.saturating_sub(item.acquired.unwrap_or(0).clamp(0, quantity))
}

/// Skeleton columns for the list-item table, in the same order as
/// HQ, item, quantity, price, options — so the
/// loading state has the real table's rhythm. The select column is left out:
/// it only shows in bulk-edit mode, which nobody is in while a list is still
/// loading.
pub(crate) fn list_item_table_skeleton_columns() -> Vec<SkeletonColumn> {
    vec![
        SkeletonColumn::new("w-16 px-3 py-3", SkeletonCell::Badge),
        SkeletonColumn::new("flex-1 min-w-40 px-3 py-3", SkeletonCell::IconText),
        SkeletonColumn::new("w-40 px-3 py-3", SkeletonCell::Number),
        SkeletonColumn::new("flex-1 px-3 py-3", SkeletonCell::Text),
        SkeletonColumn::new("w-44 px-3 py-3", SkeletonCell::Number),
    ]
}

#[component]
pub(crate) fn ActivityFeed(
    activity: Resource<Result<Vec<ListActivity>, crate::error::AppError>>,
    /// The Lists 2.0 page wraps the feed in a `<details>` whose summary is
    /// the heading, so it asks for none here.
    #[prop(default = true)]
    show_heading: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <section class="flex flex-col gap-3">
            <Show when=move || show_heading>
                <h2 class="text-lg font-bold text-[color:var(--brand-fg)]">{t!(i18n, list_view_activity_heading)}</h2>
            </Show>
            <Suspense fallback=move || {
                view! { <div class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, list_view_loading_activity)}</div> }
            }>
                {move || {
                    activity
                        .get()
                        .map(|result| match result {
                            Ok(rows) if rows.is_empty() => {
                                view! {
                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-4 text-sm text-[color:var(--color-text-muted)]">
                                        {t!(i18n, list_view_no_activity)}
                                    </div>
                                }
                                    .into_any()
                            }
                            Ok(rows) => {
                                view! {
                                    <ol class="flex flex-col gap-2">
                                        // ⚡ Bolt Optimization: Using collect_view() instead of <For> to prevent unnecessary cloning of rows inside a conditional block that completely recreates the view.
                                        {rows.into_iter().map(|activity| {
                                                view! {
                                                    <li class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                        <div class="text-sm font-semibold text-[color:var(--color-text)]">{if activity.kind == ultros_api_types::list::ListActivityKind::ListCreated && activity.payload.get("source").and_then(|v| v.as_str()) == Some("device") {
                                                            t_string!(i18n, adoption_activity_created, name = activity.actor_username.clone(), list = activity.payload.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string()).to_string()
                                                        } else { activity.message }}</div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)]">
                                                            {activity.created_at.format("%Y-%m-%d %H:%M UTC").to_string()}
                                                        </div>
                                                    </li>
                                                }
                                        }).collect_view()}
                                    </ol>
                                }
                                    .into_any()
                            }
                            Err(e) => {
                                view! {
                                    <div class="rounded-lg border border-red-400/40 p-4 text-sm text-red-200">{format!("{e}")}</div>
                                }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn list_item(id: i32) -> ListItem {
        ListItem {
            id,
            list_id: 1,
            item_id: id,
            quantity: Some(1),
            acquired: Some(0),
            hq: None,
            target_price: None,
        }
    }

    fn listing(id: i32, world_id: i32) -> ActiveListing {
        ActiveListing {
            id,
            world_id,
            item_id: 1,
            retainer_id: 1,
            price_per_unit: 100,
            quantity: 1,
            hq: false,
            timestamp: NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        }
    }

    /// Aether (dc 10) holds world 100 Adamantoise; Primal (dc 11) holds
    /// world 110 Behemoth — the same fixture `components/listing_filters.rs`
    /// uses, so exclusion semantics are asserted against identical data.
    fn world_helper() -> ultros_api_types::world_helper::WorldHelper {
        use ultros_api_types::world::{Datacenter, Region, World, WorldData};
        WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![
                    Datacenter {
                        id: 10,
                        name: "Aether".into(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 100,
                            name: "Adamantoise".into(),
                            datacenter_id: 10,
                        }],
                    },
                    Datacenter {
                        id: 11,
                        name: "Primal".into(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 110,
                            name: "Behemoth".into(),
                            datacenter_id: 11,
                        }],
                    },
                ],
            }],
        }
        .into()
    }

    #[test]
    fn filter_excluded_with_empty_sets_is_identity() {
        let helper = world_helper();
        let items = vec![(
            list_item(1),
            vec![listing(1, 100), listing(2, 110), listing(3, 102)],
        )];

        let filtered = filter_excluded(&items, &HashSet::new(), &HashSet::new(), Some(&helper));

        assert_eq!(filtered, items);
    }

    #[test]
    fn filter_excluded_removes_datacenter_listings_from_every_item() {
        let helper = world_helper();
        let items = vec![
            (list_item(1), vec![listing(1, 100), listing(2, 110)]),
            (list_item(2), vec![listing(3, 110)]),
        ];
        let excluded_dcs = HashSet::from(["Primal".to_string()]);

        let filtered = filter_excluded(&items, &HashSet::new(), &excluded_dcs, Some(&helper));

        // Behemoth (world 110, on Primal) listings vanish; the items stay.
        assert_eq!(
            filtered
                .iter()
                .flat_map(|(_, listings)| listings.iter().map(|listing| listing.world_id))
                .collect::<Vec<_>>(),
            vec![100]
        );
        assert_eq!(
            filtered.iter().map(|(item, _)| item.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn filter_excluded_applies_worlds_and_datacenters_together() {
        let helper = world_helper();
        let items = vec![(list_item(1), vec![listing(1, 100), listing(2, 110)])];
        let excluded_worlds = HashSet::from([100]);
        let excluded_dcs = HashSet::from(["Primal".to_string()]);

        let filtered = filter_excluded(&items, &excluded_worlds, &excluded_dcs, Some(&helper));

        assert!(filtered[0].1.is_empty());
        assert_eq!(filtered[0].0.id, 1);
    }

    #[test]
    fn filter_excluded_without_world_data_still_applies_world_exclusions() {
        let items = vec![(list_item(1), vec![listing(1, 100), listing(2, 110)])];
        let excluded_worlds = HashSet::from([100]);
        // DC exclusions can't resolve without world data — they must degrade
        // to a no-op rather than dropping everything or panicking.
        let excluded_dcs = HashSet::from(["Primal".to_string()]);

        let filtered = filter_excluded(&items, &excluded_worlds, &excluded_dcs, None);

        assert_eq!(
            filtered[0]
                .1
                .iter()
                .map(|listing| listing.world_id)
                .collect::<Vec<_>>(),
            vec![110]
        );
    }

    #[test]
    fn id_list_round_trips_through_query_param_encoding() {
        let parsed: IdList = "35,33,34".parse().unwrap();
        assert_eq!(parsed, IdList(vec![33, 34, 35]));
        assert_eq!(parsed.to_string(), "33,34,35");
        assert_eq!(parsed.to_string().parse::<IdList>().unwrap(), parsed);
    }

    #[test]
    fn id_list_skips_junk_and_dedupes() {
        let parsed: IdList = "33,,junk,34, 33 ".parse().unwrap();
        assert_eq!(parsed, IdList(vec![33, 34]));
    }

    #[test]
    fn name_list_round_trips_through_query_param_encoding() {
        let parsed: NameList = "Primal, Aether,".parse().unwrap();
        assert_eq!(
            parsed,
            NameList(vec!["Aether".to_string(), "Primal".to_string()])
        );
        assert_eq!(parsed.to_string(), "Aether,Primal");
        assert_eq!(parsed.to_string().parse::<NameList>().unwrap(), parsed);
    }
}
