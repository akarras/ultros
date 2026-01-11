use crate::global_state::LocalWorldData;
use crate::global_state::home_world::use_home_world;
use icondata;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::location::Url;
use ultros_api_types::world_helper::{AnyResult, OwnedResult};

use crate::components::icon::Icon;

#[component]
pub fn WorldButton(
    current_world: Memo<String>,
    #[prop(into)] world: OwnedResult,
    item_id: i32,
) -> impl IntoView {
    let (home_world, _) = use_home_world();
    let world_name = world.get_name().to_string();
    let world_2 = world_name.clone();
    let world_3 = world_name.clone();
    let is_home_world = Signal::derive({
        move || {
            home_world
                .with(|w| w.as_ref().map(|w| w.name == world_2))
                .unwrap_or_default()
        }
    });
    let (bg_color, other_styles) = match world {
        OwnedResult::Region(_) => (
            "bg-brand-500/10",
            "text-lg font-bold text-brand-200 px-4 py-2",
        ),
        OwnedResult::Datacenter(_) => (
            "bg-brand-500/15",
            "text-base font-semibold text-brand-300 px-3 py-1.5",
        ),
        OwnedResult::World(_) => ("bg-transparent", "text-sm px-2 py-1"),
    };
    let is_selected = move || current_world.with(|w| w == world_3.as_str());
    let home_world_emphasis = move || {
        is_home_world.with(|w| {
            if *w {
                "border-2 border-brand-400 shadow-lg"
            } else {
                ""
            }
        })
    };
    view! {
        <A
            attr:class=move || {
                [
                    "rounded-md text-[color:var(--color-text)] flex items-center gap-2 transition-all duration-200",
                    bg_color,
                    other_styles,
                    "hover:scale-105 hover:shadow-lg shadow-brand-900/20",
                    if is_selected() { "bg-brand-500/25 font-bold" } else { "" },
                    home_world_emphasis(),
                ]
                    .join(" ")
            }
                href=format!("/item/{}/{item_id}", Url::escape(&world_name))
            >
                {move || {
                    is_home_world
                        .get()
                        .then(|| {
                            view! {
                                <Icon icon=icondata::AiHomeFilled attr:class="text-brand-200" />
                                <div class="w-1"></div>
                            }
                        })
                }}
                {world_name}
            </A>
    }.into_any()
}

#[component]
pub fn HomeWorldButton(current_world: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    let (home_world, _) = use_home_world();
    home_world
        .get_untracked()
        .map(move |world| {
            view! { <WorldButton current_world world=AnyResult::World(&world) item_id=item_id() /> }
        })
        .into_any()
}

#[component]
pub fn WorldGrouping(
    region: OwnedResult,
    active_datacenter: Option<ultros_api_types::world::Datacenter>,
    current_world: Memo<String>,
    item_id: i32,
) -> impl IntoView {
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let datacenters = world_data.get_datacenters(&region.as_ref());
    view! {
        <div class="flex flex-col gap-2 rounded-lg bg-brand-900/20 p-2">
            <h2 class="text-lg font-bold text-brand-200 px-2 py-1">
                "Datacenter"
            </h2>
            <div class="flex flex-wrap gap-1">
                {datacenters
                    .iter()
                    .map(|dc| {
                        view! {
                            <WorldButton
                                current_world=current_world
                                world=AnyResult::Datacenter(dc)
                                item_id=item_id
                            />
                        }
                    })
                    .collect_view()}
            </div>
            {active_datacenter
                .map(|dc| {
                    view! {
                        <h2 class="text-lg font-bold text-brand-200 px-2 py-1">
                            "Worlds"
                        </h2>
                        <div class="flex flex-wrap gap-1">
                            {dc
                                .worlds
                                .iter()
                                .map(|w| {
                                    view! {
                                        <WorldButton
                                            current_world=current_world
                                            world=AnyResult::World(w)
                                            item_id=item_id
                                        />
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                })}
        </div>
    }
}

#[component]
pub fn WorldMenu(world_name: Memo<String>, item_id: Memo<i32>) -> impl IntoView {
    let current_world = world_name;
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let (home_world, _) = use_home_world();

    view! {
        <div class="sticky top-0 z-10">
            <div class="container mx-auto px-4">
                <div class="panel">
                    <div class="flex flex-col gap-2 py-3">
                        {move || {
                            let world = world_name();
                            let world_name = Url::unescape(&world);
                            let all_regions = world_data.get_inner_data().regions.iter().map(|r| {
                                view! {
                                    <WorldButton
                                        current_world=current_world
                                        world=AnyResult::Region(r)
                                        item_id=item_id()
                                    />
                                }
                            });
                            let selected_any_result = world_data.lookup_world_by_name(&world_name);
                            let region = if let Some(world) = selected_any_result {
                                world_data.get_region(world)
                            } else {
                                let region_result = world_data
                                    .lookup_world_by_name("North-America")
                                    .unwrap();
                                world_data.get_region(region_result)
                            };

                            let active_datacenter = if let Some(any_result) = selected_any_result {
                                match any_result {
                                    AnyResult::World(world) => world_data
                                        .get_datacenters(&AnyResult::World(world))
                                        .first()
                                        .map(|dc| (*dc).clone()),
                                    AnyResult::Datacenter(dc) => Some((*dc).clone()),
                                    AnyResult::Region(_) => None,
                                }
                            } else {
                                None
                            };

                            let home_world_in_region = home_world
                                .with_untracked(|home| {
                                    home
                                        .as_ref()
                                        .map(|home| {
                                            region
                                                .datacenters
                                                .iter()
                                                .any(|dc| dc.worlds.iter().any(|w| w.id == home.id))
                                        })
                                        .unwrap_or(true)
                                });

                            view! {
                                <div class="flex flex-wrap items-center gap-1">
                                    {all_regions.collect_view()}
                                    {(!home_world_in_region)
                                        .then(|| {
                                            view! { <HomeWorldButton current_world item_id /> }
                                        })}
                                </div>
                                <div class="w-full h-px bg-brand-700/50 my-1"></div>
                                <WorldGrouping
                                    region=OwnedResult::Region(region.clone())
                                    active_datacenter
                                    current_world
                                    item_id=item_id()
                                />
                            }
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
    .into_any()
}
