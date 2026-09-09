use icondata as i;
use leptos::{
    either::Either,
    prelude::*,
    reactive::wrappers::write::{IntoSignalSetter, SignalSetter},
};

use crate::{
    components::{icon::Icon, select::Select},
    global_state::{LocalWorldData, home_world::locale_preferred_region},
};
use ultros_api_types::{world::World, world_helper::AnySelector};

/// Marks whether an entry is a world, datacenter or region.
///
/// The icons zoom out as the scope widens - a pin on the map for a single
/// world, the server rack that hosts a datacenter's worlds, the globe for a
/// region - so the three tiers stay distinguishable at a glance without a text
/// badge crowding the name. The kind is still exposed as text via `title` and
/// an `sr-only` label, so nothing is icon-only.
#[component]
pub fn SelectorKind(selector: AnySelector) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let icon = match selector {
        AnySelector::World(_) => i::BsGeoAltFill,
        AnySelector::Datacenter(_) => i::FaServerSolid,
        AnySelector::Region(_) => i::FaEarthAmericasSolid,
    };
    let label = move || match selector {
        AnySelector::World(_) => crate::i18n::t_string!(i18n, world).to_string(),
        AnySelector::Datacenter(_) => crate::i18n::t_string!(i18n, datacenter).to_string(),
        AnySelector::Region(_) => crate::i18n::t_string!(i18n, region).to_string(),
    };
    view! {
        <span
            class="shrink-0 flex items-center text-[color:var(--color-text-muted)]"
            title=label
        >
            <Icon icon=icon aria_hidden=true />
            <span class="sr-only">{label}</span>
        </span>
    }
}

#[component]
pub fn WorldOnlyPicker(
    current_world: Signal<Option<World>>,
    set_current_world: SignalSetter<Option<World>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>()
        .expect("Local world data should always be present")
        .0;
    let i18n = crate::i18n::use_i18n();
    match local_worlds {
        Ok(worlds) => {
            let data = Memo::new(move |_| {
                let preferred = locale_preferred_region(i18n.get_locale());
                worlds
                    .iter_with_region_priority(preferred)
                    .filter_map(|w| w.as_world())
                    .cloned()
                    .collect::<Vec<_>>()
            });
            let left = view! {
                <div class="relative">
                    <Select
                        items=data.into()
                        as_label=move |w| w.name.clone()
                        choice=current_world
                        set_choice=set_current_world
                        children=move |_w, label| {
                            view! { <div class="flex items-center min-w-0 truncate">{label}</div> }
                        }
                    />
                </div>
            };
            Either::Left(left)
        }
        Err(e) => Either::Right(view! {
            <div class="relative">
                <div class="text-red-400 p-2 rounded-lg bg-red-950/50 border border-red-800/30">
                    <span>{crate::i18n::t!(i18n, world_picker_no_worlds_prefix)}</span>
                    <span>{e.to_string()}</span>
                </div>
            </div>
        }),
    }
}

#[component]
pub fn WorldPicker(
    current_world: Signal<Option<AnySelector>>,
    set_current_world: SignalSetter<Option<AnySelector>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>()
        .expect("Local world data should always be present")
        .0;
    let i18n = crate::i18n::use_i18n();

    match local_worlds {
        Ok(worlds) => {
            let worlds_1 = worlds.clone();
            let data = Memo::new(move |_| {
                let preferred = locale_preferred_region(i18n.get_locale());
                worlds
                    .iter_with_region_priority(preferred)
                    .map(|l| (l.get_name().to_string(), AnySelector::from(&l)))
                    .collect::<Vec<_>>()
            });
            let choice = Memo::new(move |_| {
                current_world().and_then(|world| {
                    worlds_1
                        .lookup_selector(world)
                        .map(|r| (r.get_name().to_string(), world))
                })
            })
            .into();
            let set_choice = move |option: Option<(String, AnySelector)>| {
                set_current_world(option.map(|(_, s)| s));
            };
            let set_choice = set_choice.into_signal_setter();
            Either::Left(view! {
                <div class="relative">
                    <Select
                        items=data.into()
                        choice=choice
                        set_choice=set_choice
                        as_label=move |(d, _)| d.clone()
                        selected_prefix=move |(_, s)| {
                            view! { <SelectorKind selector=s /> }.into_any()
                        }
                        children=move |(_, s), view| {
                            view! {
                                <div class="flex w-full min-w-0 items-center gap-2.5">
                                    <SelectorKind selector=s />
                                    <div class="truncate">{view}</div>
                                </div>
                            }
                        }
                    />
                </div>
            })
        }
        Err(e) => Either::Right(view! {
            <div class="relative z-[150]">
                <div class="text-red-400 p-2 rounded-lg bg-red-950/50 border border-red-800/30">
                    <span>{crate::i18n::t!(i18n, world_picker_no_worlds_prefix)}</span>
                    <span>{e.to_string()}</span>
                </div>
            </div>
        }),
    }
    .into_any()
}
