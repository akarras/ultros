//! One travel limit for both Build estimates and candidate Shop routes.
//! State belongs to the editor: changing this control never adopts a trip.
use crate::components::icon::Icon;
use crate::i18n::*;
use leptos::prelude::*;
use std::collections::BTreeSet;
use ultros_calc::list_travel::{TravelBlocked, TravelLimit};

#[component]
pub fn ListTravelControls(
    limit: Signal<TravelLimit>,
    on_change: Callback<TravelLimit>,
    home_world: Signal<Option<String>>,
    home_datacenter: Signal<Option<String>>,
    #[prop(optional)] blocked: Option<Signal<Option<TravelBlocked>>>,
    #[prop(optional)] on_clear_unresolved: Option<Callback<()>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let unavailable = move || {
        if let Some(blocked) = blocked {
            return blocked.get().map(|reason| match reason {
                TravelBlocked::WorldData => {
                    t_string!(i18n, list_travel_missing_metadata).to_string()
                }
                TravelBlocked::UnknownDatacenter => {
                    t_string!(i18n, list_travel_unknown_datacenter).to_string()
                }
                TravelBlocked::HomeWorld => t_string!(i18n, list_travel_missing_world).to_string(),
                TravelBlocked::HomeDatacenter => {
                    t_string!(i18n, list_travel_missing_datacenter).to_string()
                }
            });
        }
        match limit.get() {
            TravelLimit::Scope => None,
            _ if home_world.get().is_none() => {
                Some(t_string!(i18n, list_travel_missing_world).to_string())
            }
            TravelLimit::Datacenter if home_datacenter.get().is_none() => {
                Some(t_string!(i18n, list_travel_missing_datacenter).to_string())
            }
            _ => None,
        }
    };
    view! {
        <div class="flex min-w-0 flex-col gap-1" data-testid="list-travel-controls">
            <label class="flex flex-col gap-1 text-sm">
                <span class="block text-[color:var(--color-text-muted)]">{move || t_string!(i18n, list_travel_label)}</span>
                <select class="input h-[2.625rem] min-w-0 max-w-full" data-testid="list-travel-limit"
                    prop:value=move || limit.get().to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<TravelLimit>() {
                            on_change.run(value);
                        }
                    }>
                    <option value="scope">{move || t_string!(i18n, list_travel_scope)}</option>
                    <option value="world" disabled=move || home_world.get().is_none()>{move || home_world.get().map(|name| t_string!(i18n, list_travel_world, name = name).to_string()).unwrap_or_else(|| t_string!(i18n, list_travel_world_unset).to_string())}</option>
                    <option value="dc" disabled=move || home_world.get().is_none() || home_datacenter.get().is_none()>{move || home_datacenter.get().map(|name| t_string!(i18n, list_travel_datacenter, name = name).to_string()).unwrap_or_else(|| t_string!(i18n, list_travel_datacenter_unset).to_string())}</option>
                </select>
            </label>
            <p role="status" class="text-sm text-[color:var(--color-text-muted)]" data-testid="list-travel-unavailable" class:hidden=move || unavailable().is_none()>{unavailable}</p>
            <Show when=move || on_clear_unresolved.is_some() && blocked.is_some_and(|reason| reason.get() == Some(TravelBlocked::UnknownDatacenter))>
                <button type="button" class="btn-secondary min-h-11" data-testid="list-travel-clear-unresolved" on:click=move |_| { if let Some(clear) = on_clear_unresolved { clear.run(()); } }>{move || t_string!(i18n, list_travel_clear_unknown)}</button>
            </Show>
        </div>
    }
}

/// Chip names for the datacenter group: the catalog in order, then any
/// excluded name the catalog no longer knows (a renamed datacenter from an
/// old bookmark), so a stale exclusion stays visible and clickable.
fn datacenter_chips(catalog: &[String], excluded: &BTreeSet<String>) -> Vec<String> {
    let mut chips = catalog.to_vec();
    chips.extend(
        excluded
            .iter()
            .filter(|name| !catalog.contains(name))
            .cloned(),
    );
    chips
}

/// Display name for an excluded world. An id the catalog does not know
/// falls back to `World {id}` so its chip still renders and can be removed.
fn world_exclusion_label(worlds: &[(i32, String)], world_id: i32) -> String {
    worlds
        .iter()
        .find(|(id, _)| *id == world_id)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| format!("World {world_id}"))
}

fn toggle_excluded_name(set: &BTreeSet<String>, name: &str) -> BTreeSet<String> {
    let mut next = set.clone();
    if !next.remove(name) {
        next.insert(name.to_string());
    }
    next
}

/// Visible, removable world/datacenter exclusions for the list editors.
///
/// The `excluded-worlds` / `excluded-datacenters` URL params are honoured by
/// both Build and Shop pricing, so they must never be invisible state: when
/// a link arrives with exclusions the section starts open, the header keeps
/// a count while it is collapsed, and Clear is always one click away.
#[component]
pub fn ListExclusionControls(
    #[prop(into)] excluded_worlds: Signal<BTreeSet<i32>>,
    #[prop(into)] set_excluded_worlds: Callback<BTreeSet<i32>>,
    #[prop(into)] excluded_datacenters: Signal<BTreeSet<String>>,
    #[prop(into)] set_excluded_datacenters: Callback<BTreeSet<String>>,
    /// Every datacenter name in the catalog.
    datacenters: Vec<String>,
    /// Every world as `(id, name)`.
    worlds: Vec<(i32, String)>,
) -> impl IntoView {
    let i18n = use_i18n();
    let datacenters = StoredValue::new(datacenters);
    let worlds = StoredValue::new(worlds);
    let open = RwSignal::new(
        !excluded_worlds.get_untracked().is_empty()
            || !excluded_datacenters.get_untracked().is_empty(),
    );
    let count = Signal::derive(move || {
        excluded_worlds.with(BTreeSet::len) + excluded_datacenters.with(BTreeSet::len)
    });
    let has_exclusions = Signal::derive(move || count.get() > 0);
    let clear_all = move |_| {
        set_excluded_worlds.run(BTreeSet::new());
        set_excluded_datacenters.run(BTreeSet::new());
    };
    let available_worlds = Memo::new(move |_| {
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
        <div class="flex min-w-0 flex-col gap-2" data-testid="list-exclusions">
            <div class="flex flex-wrap items-center gap-2">
                <button
                    type="button"
                    class="btn-secondary px-3 py-1 text-xs"
                    data-testid="list-exclusions-toggle"
                    aria-expanded=move || open.get().to_string()
                    on:click=move |_| open.update(|open| *open = !*open)
                >
                    {t!(i18n, list_view_exclusions_label)}
                    <Show when=move || has_exclusions.get()>
                        <span data-testid="list-exclusions-count">" ("{move || count.get()}")"</span>
                    </Show>
                </button>
                <Show when=move || has_exclusions.get()>
                    <button
                        type="button"
                        class="btn-ghost px-2 py-1 text-xs"
                        data-testid="list-exclusions-clear"
                        on:click=clear_all
                    >
                        {t!(i18n, list_view_clear_world_exclusions)}
                    </button>
                </Show>
            </div>
            <Show when=move || open.get()>
                <div class="flex flex-col gap-2">
                    <div
                        class="flex flex-wrap items-center gap-2"
                        role="group"
                        aria-label=move || t_string!(i18n, list_view_exclude_datacenters).to_string()
                    >
                        <span class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, list_view_exclude_datacenters)}</span>
                        <For
                            each=move || datacenters.with_value(|catalog| datacenter_chips(catalog, &excluded_datacenters.get()))
                            key=|name| name.clone()
                            children=move |name| {
                                let chip_label = name.clone();
                                let chip_hook = name.clone();
                                let is_excluded = Signal::derive({
                                    let name = name.clone();
                                    move || excluded_datacenters.with(|set| set.contains(&name))
                                });
                                let toggle = move |_| {
                                    set_excluded_datacenters.run(toggle_excluded_name(
                                        &excluded_datacenters.get_untracked(),
                                        &name,
                                    ));
                                };
                                view! {
                                    <button
                                        type="button"
                                        class="btn-secondary px-3 py-1 text-xs"
                                        class:bg-red-950=is_excluded
                                        class:text-red-200=is_excluded
                                        class:border-red-400=is_excluded
                                        aria-pressed=move || is_excluded.get().to_string()
                                        data-datacenter=chip_hook
                                        on:click=toggle
                                    >
                                        {chip_label}
                                    </button>
                                }
                            }
                        />
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                        <label class="text-sm text-[color:var(--color-text-muted)]" for="list-world-exclusion">
                            {t!(i18n, list_view_exclude_worlds)}
                        </label>
                        <select
                            id="list-world-exclusion"
                            class="input h-9 min-w-40 py-1 text-sm"
                            data-testid="list-exclude-world-select"
                            on:change=move |event| {
                                if let Ok(world_id) = event_target_value(&event).parse::<i32>() {
                                    let mut set = excluded_worlds.get_untracked();
                                    set.insert(world_id);
                                    set_excluded_worlds.run(set);
                                }
                            }
                        >
                            <option value="">{move || {
                                if available_worlds.with(|worlds| worlds.is_empty()) {
                                    t_string!(i18n, list_view_no_worlds_left).to_string()
                                } else {
                                    t_string!(i18n, list_view_add_world).to_string()
                                }
                            }}</option>
                            <For
                                each=move || available_worlds.get()
                                key=|(world_id, _)| *world_id
                                children=move |(world_id, name)| {
                                    view! { <option value=world_id.to_string()>{name}</option> }
                                }
                            />
                        </select>
                        <For
                            each=move || {
                                excluded_worlds.get().into_iter().map(|world_id| {
                                    (world_id, worlds.with_value(|worlds| world_exclusion_label(worlds, world_id)))
                                }).collect::<Vec<_>>()
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
                                        data-excluded-world=world_id.to_string()
                                        on:click=move |_| {
                                            let mut set = excluded_worlds.get_untracked();
                                            set.remove(&world_id);
                                            set_excluded_worlds.run(set);
                                        }
                                    >
                                        <span>{name}</span>
                                        <Icon icon=icondata::BiXRegular aria_hidden=true />
                                    </button>
                                }
                            }
                        />
                    </div>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos_i18n::context::init_i18n_context;

    #[test]
    fn stale_exclusions_stay_visible_and_toggle_off() {
        let catalog = vec!["Aether".to_string()];
        let excluded = BTreeSet::from(["Renamed DC".to_string()]);
        assert_eq!(
            datacenter_chips(&catalog, &excluded),
            vec!["Aether", "Renamed DC"]
        );
        assert!(toggle_excluded_name(&excluded, "Renamed DC").is_empty());
        assert_eq!(
            toggle_excluded_name(&BTreeSet::new(), "Aether"),
            BTreeSet::from(["Aether".to_string()])
        );
        let worlds = vec![(100, "Adamantoise".to_string())];
        assert_eq!(world_exclusion_label(&worlds, 100), "Adamantoise");
        assert_eq!(world_exclusion_label(&worlds, 999), "World 999");
    }

    fn render(excluded_worlds: BTreeSet<i32>, excluded_datacenters: BTreeSet<String>) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            view! {
                <ListExclusionControls
                    excluded_worlds=Signal::derive(move || excluded_worlds.clone())
                    set_excluded_worlds=Callback::new(|_| {})
                    excluded_datacenters=Signal::derive(move || excluded_datacenters.clone())
                    set_excluded_datacenters=Callback::new(|_| {})
                    datacenters=vec!["Aether".to_string(), "Primal".to_string()]
                    worlds=vec![
                        (100, "Adamantoise".to_string()),
                        (110, "Behemoth".to_string()),
                    ]
                />
            }
            .to_html()
        })
    }

    /// A legacy bookmark's exclusions must be visible without any click:
    /// the section starts open, the datacenter chip reads pressed, the
    /// world chip is removable, and Clear is offered.
    #[test]
    fn bookmarked_exclusions_render_visible_and_removable() {
        let html = render(
            BTreeSet::from([100, 999]),
            BTreeSet::from(["Primal".to_string(), "Renamed DC".to_string()]),
        );
        assert!(html.contains("list-exclusions-count"), "{html}");
        assert!(html.contains("list-exclusions-clear"), "{html}");
        assert!(html.contains("data-datacenter=\"Primal\""), "{html}");
        assert!(html.contains("aria-pressed=\"true\""), "{html}");
        assert!(
            html.contains("aria-pressed=\"false\""),
            "an untouched datacenter chip reads not-pressed, not a bare attribute: {html}"
        );
        assert!(html.contains("aria-expanded=\"true\""), "{html}");
        assert!(
            html.contains("data-datacenter=\"Renamed DC\""),
            "a stale datacenter exclusion stays removable: {html}"
        );
        assert!(html.contains("data-excluded-world=\"100\""), "{html}");
        assert!(html.contains("Adamantoise"), "{html}");
        assert!(
            html.contains("World 999"),
            "an unknown world id still renders a chip: {html}"
        );
        assert!(
            !html.contains("<option value=\"100\">"),
            "an excluded world is not offered again: {html}"
        );
        assert!(html.contains("<option value=\"110\">"), "{html}");
    }

    #[test]
    fn no_exclusions_render_a_collapsed_empty_section() {
        let html = render(BTreeSet::new(), BTreeSet::new());
        assert!(html.contains("list-exclusions-toggle"), "{html}");
        assert!(!html.contains("list-exclusions-count"), "{html}");
        assert!(!html.contains("list-exclusions-clear"), "{html}");
        assert!(!html.contains("data-datacenter="), "{html}");
        assert!(!html.contains("data-excluded-world="), "{html}");
    }
}
