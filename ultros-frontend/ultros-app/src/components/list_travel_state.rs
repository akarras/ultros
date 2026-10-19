//! Editor-owned URL settings. Both list editors filter the successfully
//! served rows with this policy before their single coverage-aware estimate.
#[cfg(feature = "hydrate")]
use crate::components::app_link::use_query_map_or_default;
use crate::{
    components::list_travel_controls::{ListExclusionControls, ListTravelControls},
    global_state::{home_world::use_home_world, use_world_helper},
    query_defaults::filter_query_signal,
    routes::list_common::{IdList, NameList},
};
use leptos::prelude::*;
#[cfg(any(feature = "hydrate", test))]
use leptos_router::params::ParamsMap;
use std::collections::BTreeSet;
use ultros_api_types::world_helper::{AnySelector, WorldHelper};
use ultros_calc::list_travel::{TravelBlocked, TravelLimit, TravelPolicy};

#[derive(Clone, Copy)]
pub struct ListTravelState {
    pub policy: Signal<TravelPolicy>,
    pub limit: Signal<TravelLimit>,
    pub set_limit: Callback<TravelLimit>,
    pub home_world: Signal<Option<String>>,
    pub home_datacenter: Signal<Option<String>>,
    pub blocked: Signal<Option<TravelBlocked>>,
    pub clear_unresolved: Callback<()>,
    /// The `excluded-worlds` / `excluded-datacenters` URL settings, decoded.
    /// Both list editors render these through [`ListExclusionControls`], so
    /// a legacy bookmark's exclusions are visible and removable instead of
    /// silently filtering prices.
    pub excluded_worlds: Signal<BTreeSet<i32>>,
    pub excluded_datacenters: Signal<BTreeSet<String>>,
    /// Replace the whole exclusion set; an empty set clears the URL param.
    pub set_excluded_worlds: Callback<BTreeSet<i32>>,
    pub set_excluded_datacenters: Callback<BTreeSet<String>>,
    /// Every datacenter name and `(world id, name)` in the catalog, for the
    /// exclusion chips and the add-world select.
    pub datacenter_options: StoredValue<Vec<String>>,
    pub world_options: StoredValue<Vec<(i32, String)>>,
    // Only the browser-side handoff links read the live query map.
    #[cfg(feature = "hydrate")]
    query: Memo<ParamsMap>,
}

/// Called once in the editor owner, outside Suspense and mode views.
pub fn use_list_travel() -> ListTravelState {
    #[cfg(feature = "hydrate")]
    let query = use_query_map_or_default();
    let (limit, set_limit) = filter_query_signal::<TravelLimit>("travel");
    let (excluded_worlds_param, set_excluded_worlds_param) =
        filter_query_signal::<IdList>("excluded-worlds");
    let (excluded_datacenters_param, set_excluded_datacenters_param) =
        filter_query_signal::<NameList>("excluded-datacenters");
    let excluded_worlds = Signal::derive(move || {
        excluded_worlds_param
            .get()
            .map(|ids| ids.0.into_iter().collect())
            .unwrap_or_default()
    });
    let excluded_datacenters = Signal::derive(move || {
        excluded_datacenters_param
            .get()
            .map(|names| names.0.into_iter().collect())
            .unwrap_or_default()
    });
    let (home, _) = use_home_world();
    let worlds = StoredValue::new(use_world_helper().ok());
    let (datacenter_options, world_options) =
        worlds.with_value(|worlds| exclusion_catalog(worlds.as_deref()));
    let limit = Signal::derive(move || limit.get().unwrap_or_default());
    let policy = Signal::derive(move || {
        let limit = limit.get();
        let home = home.get().map(|world| world.id);
        let excluded_worlds = excluded_worlds.get();
        let excluded_names = excluded_datacenters.get();
        worlds.with_value(|worlds| match worlds {
            Some(worlds) => TravelPolicy::from_world_helper(
                limit,
                home,
                worlds,
                excluded_worlds,
                &excluded_names,
            ),
            None => TravelPolicy {
                limit,
                home_world: home,
                excluded_worlds,
                metadata_missing: true,
                unresolved_datacenters: excluded_names,
                ..Default::default()
            },
        })
    });
    ListTravelState {
        policy,
        limit,
        set_limit: Callback::new(move |limit| {
            set_limit.set((limit != TravelLimit::Scope).then_some(limit))
        }),
        home_world: Signal::derive(move || home.get().map(|world| world.name)),
        home_datacenter: Signal::derive(move || {
            let home = home.get()?;
            worlds.with_value(|worlds| {
                worlds
                    .as_ref()?
                    .lookup_selector(AnySelector::Datacenter(home.datacenter_id))
                    .map(|dc| dc.get_name().to_string())
            })
        }),
        blocked: Signal::derive(move || policy.with(TravelPolicy::blocked)),
        clear_unresolved: Callback::new(move |()| {
            let unresolved = policy.get_untracked().unresolved_datacenters;
            set_excluded_datacenters_param.set(clear_unresolved_names(
                excluded_datacenters_param.get_untracked(),
                &unresolved,
            ));
        }),
        excluded_worlds,
        excluded_datacenters,
        set_excluded_worlds: Callback::new(move |worlds| {
            set_excluded_worlds_param.set(id_list_param(worlds))
        }),
        set_excluded_datacenters: Callback::new(move |names| {
            set_excluded_datacenters_param.set(name_list_param(names))
        }),
        datacenter_options: StoredValue::new(datacenter_options),
        world_options: StoredValue::new(world_options),
        #[cfg(feature = "hydrate")]
        query,
    }
}

fn clear_unresolved_names(
    names: Option<NameList>,
    unresolved: &BTreeSet<String>,
) -> Option<NameList> {
    let retained: Vec<_> = names
        .map(|names| names.0)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !unresolved.contains(name))
        .collect();
    (!retained.is_empty()).then_some(NameList(retained))
}

/// Encode an exclusion set for its URL param. An empty set clears the param
/// entirely, so a cleared exclusion leaves no `excluded-worlds=` residue a
/// later reader could mistake for an active filter.
fn id_list_param(worlds: BTreeSet<i32>) -> Option<IdList> {
    (!worlds.is_empty()).then(|| IdList(worlds.into_iter().collect()))
}

fn name_list_param(names: BTreeSet<String>) -> Option<NameList> {
    (!names.is_empty()).then(|| NameList(names.into_iter().collect()))
}

/// Every datacenter name (catalog order) and world `(id, name)` (by name,
/// so the add-world select reads alphabetically). Without world data both
/// are empty; already-excluded worlds and datacenters still render as
/// removable chips from the URL values themselves.
fn exclusion_catalog(worlds: Option<&WorldHelper>) -> (Vec<String>, Vec<(i32, String)>) {
    let Some(worlds) = worlds else {
        return (Vec::new(), Vec::new());
    };
    let datacenters = worlds
        .iter()
        .filter_map(|value| value.as_datacenter().map(|dc| dc.name.clone()))
        .collect();
    let mut world_list: Vec<(i32, String)> = worlds
        .iter()
        .filter_map(|value| value.as_world().map(|world| (world.id, world.name.clone())))
        .collect();
    world_list.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    (datacenters, world_list)
}

#[cfg(feature = "hydrate")]
impl ListTravelState {
    pub fn online_href(&self, list_id: i32, shop: bool) -> String {
        online_href(list_id, shop, &self.query.get_untracked())
    }

    pub fn device_continue_href(&self, device_id: &str, shop: bool) -> String {
        device_continue_href(device_id, shop, &self.query.get())
    }
}

#[cfg(any(feature = "hydrate", test))]
fn view_query(shop: bool, source: &ParamsMap) -> ParamsMap {
    let mut query = ParamsMap::new();
    if shop {
        query.replace("buy", "true".to_string());
    }
    // Carry only list-view settings. Device recovery/promotion flags must not
    // be replayed on the account route after the same-list handoff.
    for key in ["travel", "excluded-worlds", "excluded-datacenters"] {
        if let Some(value) = source.get(key) {
            query.replace(key, value);
        }
    }
    query
}

#[cfg(any(feature = "hydrate", test))]
fn online_href(list_id: i32, shop: bool, source: &ParamsMap) -> String {
    format!(
        "/list/{list_id}{}",
        view_query(shop, source).to_query_string()
    )
}

#[cfg(any(feature = "hydrate", test))]
fn device_continue_href(device_id: &str, shop: bool, source: &ParamsMap) -> String {
    let mut query = view_query(shop, source);
    query.replace("make_online", "1".to_string());
    format!("/list/device/{device_id}{}", query.to_query_string())
}

#[component]
pub fn ListTravelPanel(state: ListTravelState) -> impl IntoView {
    view! {
        <div class="flex min-w-0 flex-col gap-2">
            <ListTravelControls limit=state.limit on_change=state.set_limit home_world=state.home_world home_datacenter=state.home_datacenter blocked=state.blocked on_clear_unresolved=state.clear_unresolved />
            <ListExclusionControls
                excluded_worlds=state.excluded_worlds
                set_excluded_worlds=state.set_excluded_worlds
                excluded_datacenters=state.excluded_datacenters
                set_excluded_datacenters=state.set_excluded_datacenters
                datacenters=state.datacenter_options.get_value()
                worlds=state.world_options.get_value()
            />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_stale_names_preserves_every_resolved_exclusion() {
        let names = Some(NameList(vec!["Known DC".into(), "Renamed DC".into()]));
        let unknown = BTreeSet::from(["Renamed DC".into()]);
        assert_eq!(
            clear_unresolved_names(names, &unknown),
            Some(NameList(vec!["Known DC".into()]))
        );
        assert_eq!(
            clear_unresolved_names(Some(NameList(vec!["Renamed DC".into()])), &unknown),
            None
        );
    }

    /// The server-side encoder percent-escapes `-` and `_` in keys
    /// (`excluded%2Dworlds`, `make%5Fonline`); the browser encoder keeps
    /// them. Both decode to the same key, so accept either spelling.
    fn has_param(href: &str, key: &str, value: &str) -> bool {
        let escaped = key.replace('-', "%2D").replace('_', "%5F");
        href.contains(&format!("{key}={value}")) || href.contains(&format!("{escaped}={value}"))
    }

    #[test]
    fn online_handoff_keeps_travel_and_exclusions_without_device_flags() {
        let mut source = ParamsMap::new();
        source.replace("travel", "dc".to_string());
        source.replace("excluded-worlds", "1,9".to_string());
        source.replace("excluded-datacenters", "Other & DC".to_string());
        source.replace("recovery", "1".to_string());
        source.replace("make_online", "1".to_string());
        let href = online_href(42, true, &source);
        assert!(href.starts_with("/list/42?"));
        assert!(href.contains("travel=dc"));
        assert!(href.contains("buy=true"));
        assert!(has_param(&href, "excluded-worlds", ""));
        assert!(has_param(&href, "excluded-datacenters", ""));
        assert!(
            href.contains("%26"),
            "DC names remain one encoded query value"
        );
        assert!(!href.contains("recovery"));
        assert!(!has_param(&href, "make_online", ""));
        assert!(!online_href(42, false, &source).contains("buy="));
        let continuation = device_continue_href("local-id", true, &source);
        assert!(continuation.starts_with("/list/device/local-id?"));
        assert!(has_param(&continuation, "make_online", "1"));
        assert!(continuation.contains("travel=dc"));
        assert!(continuation.contains("buy=true"));
        assert!(has_param(&continuation, "excluded-worlds", ""));
        assert!(continuation.contains("%26"));
        assert!(!continuation.contains("recovery"));
    }

    #[test]
    fn exclusion_params_clear_when_the_set_empties() {
        // The regression this guards: an exclusion a bookmark carried in
        // could be read but never written, so removing it in the UI must
        // drop the whole param, not leave an empty `excluded-worlds=`.
        assert_eq!(id_list_param(BTreeSet::new()), None);
        assert_eq!(
            id_list_param(BTreeSet::from([9, 1])),
            Some(IdList(vec![1, 9]))
        );
        assert_eq!(name_list_param(BTreeSet::new()), None);
        assert_eq!(
            name_list_param(BTreeSet::from(["Primal".into(), "Aether".into()])),
            Some(NameList(vec!["Aether".into(), "Primal".into()]))
        );
    }

    #[test]
    fn exclusion_catalog_lists_datacenters_and_worlds_by_name() {
        use ultros_api_types::world::{Datacenter, Region, World, WorldData};
        let helper = WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![
                    Datacenter {
                        id: 10,
                        name: "Aether".into(),
                        region_id: 1,
                        worlds: vec![
                            World {
                                id: 101,
                                name: "Gilgamesh".into(),
                                datacenter_id: 10,
                            },
                            World {
                                id: 100,
                                name: "Adamantoise".into(),
                                datacenter_id: 10,
                            },
                        ],
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
        });
        let (datacenters, worlds) = exclusion_catalog(Some(&helper));
        assert_eq!(datacenters, vec!["Aether", "Primal"]);
        assert_eq!(
            worlds,
            vec![
                (100, "Adamantoise".to_string()),
                (110, "Behemoth".to_string()),
                (101, "Gilgamesh".to_string()),
            ]
        );
        assert_eq!(exclusion_catalog(None), (Vec::new(), Vec::new()));
    }
}
