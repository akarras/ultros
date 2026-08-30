//! The list view's unified filter row.
//!
//! One panel row with three labeled groups — exclusions (datacenter chips +
//! world select), sort, and view — replacing the three separately-styled
//! clusters the page grew over time. The component is deliberately
//! context-free apart from i18n: worlds and datacenters arrive as plain
//! `Vec`s and all state flows through `Signal`/`Callback` props, so the row
//! can be SSR-rendered in a test with nothing but an `Owner` and an i18n
//! context (see the snapshot test at the bottom).

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use icondata as i;
use leptos::prelude::*;

use crate::components::icon::Icon;
use crate::i18n::*;
use ultros_api_types::{
    ActiveListing,
    list::ListItem,
    world_helper::{AnyResult, AnySelector, WorldHelper},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortKey {
    Name,
    Price,
    Acquired,
}

/// Sort order for the list item table, encoded in the `sort` query param as
/// `name`, `name-desc`, `price`, `price-desc`, `acquired`, or `acquired-desc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SortSpec {
    pub(crate) key: SortKey,
    pub(crate) descending: bool,
}

impl FromStr for SortSpec {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (base, descending) = match s.strip_suffix("-desc") {
            Some(base) => (base, true),
            None => (s, false),
        };
        let key = match base {
            "name" => SortKey::Name,
            "price" => SortKey::Price,
            "acquired" => SortKey::Acquired,
            _ => return Err(()),
        };
        Ok(SortSpec { key, descending })
    }
}

impl fmt::Display for SortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = match self.key {
            SortKey::Name => "name",
            SortKey::Price => "price",
            SortKey::Acquired => "acquired",
        };
        write!(f, "{base}")?;
        if self.descending {
            write!(f, "-desc")?;
        }
        Ok(())
    }
}

/// The distinct worlds that appear in `items`' listings, as `(id, name)`
/// sorted by id. Names fall back to `World {id}` when the helper is absent
/// or doesn't know the id, so a stale world id still renders a removable
/// chip rather than vanishing.
pub(crate) fn worlds_in_listings(
    items: &[(ListItem, Vec<ActiveListing>)],
    helper: Option<&WorldHelper>,
) -> Vec<(i32, String)> {
    let mut worlds = std::collections::BTreeMap::new();
    for (_, listings) in items {
        for listing in listings {
            worlds.entry(listing.world_id).or_insert_with(|| {
                helper
                    .and_then(|helper| helper.lookup_selector(AnySelector::World(listing.world_id)))
                    .and_then(|result| match result {
                        AnyResult::World(world) => Some(world.name.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("World {}", listing.world_id))
            });
        }
    }
    worlds.into_iter().collect()
}

#[component]
pub(crate) fn ListFilterRow(
    /// Worlds present in the current listings, `(id, name)`.
    worlds: Vec<(i32, String)>,
    /// Datacenter names covered by the list's world/datacenter/region scope.
    datacenters: Vec<String>,
    #[prop(into)] excluded_worlds: Signal<HashSet<i32>>,
    #[prop(into)] set_excluded_worlds: Callback<HashSet<i32>>,
    #[prop(into)] excluded_datacenters: Signal<HashSet<String>>,
    #[prop(into)] set_excluded_datacenters: Callback<HashSet<String>>,
    #[prop(into)] sort_spec: Signal<Option<SortSpec>>,
    #[prop(into)] set_sort_spec: Callback<Option<SortSpec>>,
    #[prop(into)] hide_acquired: Signal<bool>,
    #[prop(into)] set_hide_acquired: Callback<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let worlds = StoredValue::new(worlds);

    let available_to_add = Memo::new(move |_| {
        let excluded = excluded_worlds.get();
        worlds.with_value(|worlds| {
            worlds
                .iter()
                .filter(|(world_id, _)| !excluded.contains(world_id))
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    view! {
        <div
            class="panel rounded-lg p-3 flex flex-wrap items-center gap-x-6 gap-y-3"
            data-testid="list-filter-row"
        >
            // ===== Exclusions =====
            <div
                class="flex flex-wrap items-center gap-2"
                aria-label=t_string!(i18n, list_view_exclude_datacenters)
            >
                <span class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                    {t!(i18n, list_view_exclusions_label)}
                </span>
                <div class="flex flex-wrap gap-2">
                    <For
                        each=move || datacenters.clone()
                        key=|name| name.clone()
                        children=move |name| {
                            let chip_label = name.clone();
                            let chip_hook = name.clone();
                            let is_excluded = Signal::derive({
                                let name = name.clone();
                                move || excluded_datacenters.with(|set| set.contains(&name))
                            });
                            let toggle = {
                                let name = name.clone();
                                move |_| {
                                    let mut set = excluded_datacenters.get_untracked();
                                    if !set.remove(&name) {
                                        set.insert(name.clone());
                                    }
                                    set_excluded_datacenters.run(set);
                                }
                            };
                            view! {
                                <button
                                    class="btn-secondary px-3 py-1 text-xs"
                                    class:bg-red-950=is_excluded
                                    class:text-red-200=is_excluded
                                    class:border-red-400=is_excluded
                                    data-datacenter=chip_hook
                                    on:click=toggle
                                >
                                    {chip_label}
                                </button>
                            }
                        }
                    />
                </div>
                <label class="sr-only" for="list-world-exclusion">
                    {t!(i18n, list_view_exclude_worlds)}
                </label>
                <select
                    id="list-world-exclusion"
                    class="input h-9 min-w-40 py-1 text-sm"
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        if let Ok(world_id) = value.parse::<i32>() {
                            let mut set = excluded_worlds.get_untracked();
                            set.insert(world_id);
                            set_excluded_worlds.run(set);
                        }
                    }
                >
                    <option value="">{move || {
                        if available_to_add.with(|worlds| worlds.is_empty()) {
                            t_string!(i18n, list_view_no_worlds_left).to_string()
                        } else {
                            t_string!(i18n, list_view_add_world).to_string()
                        }
                    }}</option>
                    <For
                        each=move || available_to_add.get()
                        key=|(world_id, _)| *world_id
                        children=move |(world_id, name)| {
                            view! {
                                <option value=world_id.to_string()>{name}</option>
                            }
                        }
                    />
                </select>
                <Show when=move || !excluded_worlds.with(|set| set.is_empty())>
                    <div class="flex flex-wrap items-center gap-1">
                        <For
                            each=move || {
                                let excluded = excluded_worlds.get();
                                worlds.with_value(|worlds| {
                                    worlds
                                        .iter()
                                        .filter(|(world_id, _)| excluded.contains(world_id))
                                        .cloned()
                                        .collect::<Vec<_>>()
                                })
                            }
                            key=|(world_id, _)| *world_id
                            children=move |(world_id, name)| {
                                let aria_label =
                                    t_string!(i18n, list_view_remove_world_exclusion_aria, name = name.clone())
                                        .to_string();
                                view! {
                                    <button
                                        type="button"
                                        class="inline-flex items-center gap-1 rounded-md border border-[color:var(--color-outline)] px-2 py-1 text-xs text-[color:var(--color-text)] hover:border-[color:var(--color-outline-strong)]"
                                        aria-label=aria_label
                                        on:click=move |_| {
                                            let mut set = excluded_worlds.get_untracked();
                                            set.remove(&world_id);
                                            set_excluded_worlds.run(set);
                                        }
                                    >
                                        <span>{name}</span>
                                        <Icon icon=i::BiXRegular />
                                    </button>
                                }
                            }
                        />
                        <button
                            type="button"
                            class="btn-ghost px-2 py-1 text-xs"
                            on:click=move |_| set_excluded_worlds.run(HashSet::new())
                        >
                            {t!(i18n, list_view_clear_world_exclusions)}
                        </button>
                    </div>
                </Show>
            </div>

            // ===== Sort =====
            <div class="flex flex-wrap items-center gap-2">
                <label
                    class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]"
                    for="list-sort-select"
                >
                    {t!(i18n, list_view_sort_label)}
                </label>
                <select
                    id="list-sort-select"
                    class="input h-9 py-1 text-sm"
                    prop:value=move || {
                        sort_spec.get().map(|s| s.to_string()).unwrap_or_default()
                    }
                    on:change=move |event| {
                        set_sort_spec.run(event_target_value(&event).parse::<SortSpec>().ok());
                    }
                >
                    <option value="">{t!(i18n, list_view_sort_default)}</option>
                    <option value="name">{t!(i18n, list_view_sort_name_asc)}</option>
                    <option value="name-desc">{t!(i18n, list_view_sort_name_desc)}</option>
                    <option value="price">{t!(i18n, list_view_sort_price_asc)}</option>
                    <option value="price-desc">{t!(i18n, list_view_sort_price_desc)}</option>
                    <option value="acquired">{t!(i18n, list_view_sort_acquired_asc)}</option>
                    <option value="acquired-desc">{t!(i18n, list_view_sort_acquired_desc)}</option>
                </select>
            </div>

            // ===== View =====
            <button
                type="button"
                class="btn-secondary px-3 py-1 text-xs"
                class:bg-brand-950=hide_acquired
                class:active=hide_acquired
                on:click=move |_| {
                    set_hide_acquired.run(!hide_acquired.get_untracked());
                }
            >
                {t!(i18n, list_view_hide_acquired)}
            </button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos_i18n::context::init_i18n_context;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};

    fn init_executor() {
        let _ = any_spawner::Executor::init_futures_executor();
    }

    fn world_helper() -> WorldHelper {
        WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".into(),
                    region_id: 1,
                    worlds: vec![World {
                        id: 100,
                        name: "Adamantoise".into(),
                        datacenter_id: 10,
                    }],
                }],
            }],
        }
        .into()
    }

    fn item_with_worlds(id: i32, world_ids: &[i32]) -> (ListItem, Vec<ActiveListing>) {
        let listings = world_ids
            .iter()
            .enumerate()
            .map(|(index, world_id)| ActiveListing {
                id: id * 100 + index as i32,
                world_id: *world_id,
                item_id: id,
                retainer_id: 1,
                price_per_unit: 100,
                quantity: 1,
                hq: false,
                timestamp: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            })
            .collect();
        (
            ListItem {
                id,
                list_id: 1,
                item_id: id,
                ..Default::default()
            },
            listings,
        )
    }

    #[test]
    fn worlds_in_listings_dedupes_and_names() {
        let helper = world_helper();
        let items = vec![
            item_with_worlds(1, &[100, 999]),
            item_with_worlds(2, &[100]),
        ];

        let worlds = worlds_in_listings(&items, Some(&helper));

        assert_eq!(
            worlds,
            vec![
                (100, "Adamantoise".to_string()),
                (999, "World 999".to_string()),
            ]
        );
        assert!(
            worlds_in_listings(&items, None)
                .iter()
                .all(|(_, name)| name.starts_with("World "))
        );
    }

    #[test]
    fn sort_spec_round_trips_through_query_param_encoding() {
        for encoded in [
            "name",
            "name-desc",
            "price",
            "price-desc",
            "acquired",
            "acquired-desc",
        ] {
            let spec: SortSpec = encoded.parse().unwrap();
            assert_eq!(spec.to_string(), encoded);
        }
        assert!("bogus".parse::<SortSpec>().is_err());
        assert!("".parse::<SortSpec>().is_err());
    }

    /// The full SSR markup of the row, pinned. A layout regression — a group
    /// losing its label, the DC chips losing their `data-datacenter` hook
    /// (which `integration/list-flow.cjs` clicks), an excluded chip losing
    /// its red state — shows up as a snapshot diff here without standing up
    /// a browser.
    #[test]
    fn filter_row_renders_all_groups() {
        init_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let html = view! {
                <ListFilterRow
                    worlds=vec![(100, "Adamantoise".to_string()), (110, "Behemoth".to_string())]
                    datacenters=vec!["Aether".to_string(), "Primal".to_string()]
                    excluded_worlds=Signal::derive(|| HashSet::from([110]))
                    set_excluded_worlds=Callback::new(|_| {})
                    excluded_datacenters=Signal::derive(|| {
                        HashSet::from(["Primal".to_string()])
                    })
                    set_excluded_datacenters=Callback::new(|_| {})
                    sort_spec=Signal::derive(|| {
                        Some(SortSpec {
                            key: SortKey::Price,
                            descending: true,
                        })
                    })
                    set_sort_spec=Callback::new(|_| {})
                    hide_acquired=Signal::derive(|| false)
                    set_hide_acquired=Callback::new(|_| {})
                />
            }
            .to_html();
            insta::assert_snapshot!(html);
        });
    }
}
