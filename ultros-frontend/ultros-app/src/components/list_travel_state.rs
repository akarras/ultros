//! Editor-owned URL settings. Both list editors filter the successfully
//! served rows with this policy before their single coverage-aware estimate.
use crate::{
    components::{app_link::use_query_map_or_default, list_travel_controls::ListTravelControls},
    global_state::{home_world::use_home_world, use_world_helper},
    query_defaults::filter_query_signal,
    routes::list_view::{IdList, NameList},
};
use leptos::prelude::*;
use leptos_router::params::ParamsMap;
use std::collections::BTreeSet;
use ultros_api_types::world_helper::AnySelector;
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
    query: Memo<ParamsMap>,
}

/// Called once in the editor owner, outside Suspense and mode views.
pub fn use_list_travel() -> ListTravelState {
    let query = use_query_map_or_default();
    let (limit, set_limit) = filter_query_signal::<TravelLimit>("travel");
    let (excluded_worlds, _) = filter_query_signal::<IdList>("excluded-worlds");
    let (excluded_datacenters, set_excluded_datacenters) =
        filter_query_signal::<NameList>("excluded-datacenters");
    let (home, _) = use_home_world();
    let worlds = StoredValue::new(use_world_helper().ok());
    let limit = Signal::derive(move || limit.get().unwrap_or_default());
    let policy = Signal::derive(move || {
        let limit = limit.get();
        let home = home.get().map(|world| world.id);
        let excluded_worlds = excluded_worlds
            .get()
            .map(|ids| ids.0.into_iter().collect())
            .unwrap_or_default();
        let excluded_names: BTreeSet<_> = excluded_datacenters
            .get()
            .map(|names| names.0.into_iter().collect())
            .unwrap_or_default();
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
            set_excluded_datacenters.set(clear_unresolved_names(
                excluded_datacenters.get_untracked(),
                &unresolved,
            ));
        }),
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

impl ListTravelState {
    pub fn online_href(&self, list_id: i32, shop: bool) -> String {
        online_href(list_id, shop, &self.query.get_untracked())
    }

    pub fn device_continue_href(&self, device_id: &str, shop: bool) -> String {
        device_continue_href(device_id, shop, &self.query.get())
    }
}

fn view_query(shop: bool, source: &ParamsMap) -> ParamsMap {
    let mut query = ParamsMap::new();
    query.replace("labs", "lists-sync");
    if shop {
        query.replace("buy", "true");
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

fn online_href(list_id: i32, shop: bool, source: &ParamsMap) -> String {
    format!(
        "/list/{list_id}{}",
        view_query(shop, source).to_query_string()
    )
}

fn device_continue_href(device_id: &str, shop: bool, source: &ParamsMap) -> String {
    let mut query = view_query(shop, source);
    query.replace("make_online", "1");
    format!("/list/device/{device_id}{}", query.to_query_string())
}

#[component]
pub fn ListTravelPanel(state: ListTravelState) -> impl IntoView {
    view! { <ListTravelControls limit=state.limit on_change=state.set_limit home_world=state.home_world home_datacenter=state.home_datacenter blocked=state.blocked on_clear_unresolved=state.clear_unresolved /> }
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

    #[test]
    fn online_handoff_keeps_travel_and_exclusions_without_device_flags() {
        let mut source = ParamsMap::new();
        source.replace("travel", "dc");
        source.replace("excluded-worlds", "1,9");
        source.replace("excluded-datacenters", "Other & DC");
        source.replace("recovery", "1");
        source.replace("make_online", "1");
        let href = online_href(42, true, &source);
        assert!(href.starts_with("/list/42?"));
        assert!(href.contains("travel=dc"));
        assert!(href.contains("buy=true"));
        assert!(href.contains("excluded-worlds="));
        assert!(href.contains("excluded-datacenters="));
        assert!(
            href.contains("%26"),
            "DC names remain one encoded query value"
        );
        assert!(!href.contains("recovery"));
        assert!(!href.contains("make_online"));
        assert!(!online_href(42, false, &source).contains("buy="));
        let continuation = device_continue_href("local-id", true, &source);
        assert!(continuation.starts_with("/list/device/local-id?"));
        assert!(continuation.contains("make_online=1"));
        assert!(continuation.contains("travel=dc"));
        assert!(continuation.contains("buy=true"));
        assert!(continuation.contains("excluded-worlds="));
        assert!(continuation.contains("%26"));
        assert!(!continuation.contains("recovery"));
    }
}
