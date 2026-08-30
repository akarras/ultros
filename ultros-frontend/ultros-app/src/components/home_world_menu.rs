//! Home-world row + drop-up for the sidebar, above the price-zone row.
//!
//! Surfaces the home world (the `HOME_WORLD` cookie behind [`use_home_world`])
//! next to the price zone so it can be seen and changed from any page instead
//! of only from `/settings`. Reuses the same region → datacenter → world
//! accordion as [`RegionMenu`](crate::components::region_menu::RegionMenu) —
//! see that module for why an accordion rather than the searchable
//! `WorldPicker` — but in `worlds_only` mode: a home world is always a world,
//! so region/datacenter rows only drill in.

use crate::components::dismissable::use_dismissable;
use crate::components::icon::Icon;
use crate::components::region_menu::ZoneAccordion;
use crate::components::world_picker::SelectorKind;
use crate::global_state::home_world::{locale_preferred_region, use_home_world};
use crate::global_state::use_world_helper;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::either::Either;
use leptos::html;
use leptos::prelude::*;
use ultros_api_types::world_helper::AnySelector;

#[component]
pub fn HomeWorldMenu() -> impl IntoView {
    let i18n = use_i18n();
    let (open, set_open) = signal(false);
    let root_ref = NodeRef::<html::Div>::new();

    // Route change, click outside, Escape — the shared idiom.
    use_dismissable(root_ref, move || set_open(false));

    let (homeworld, set_homeworld) = use_home_world();
    let current = Signal::derive(move || homeworld.get().map(|w| AnySelector::World(w.id)));

    // See RegionMenu for why this is not `use_context().expect(..)`.
    let local_worlds = use_world_helper();

    let on_select = Callback::new(move |selector: AnySelector| {
        // `worlds_only` means only world rows reach here, but resolve through
        // the helper anyway rather than trusting that invariant with a panic.
        if let AnySelector::World(_) = selector
            && let Ok(worlds) = use_world_helper()
            && let Some(world) = worlds
                .lookup_selector(selector)
                .and_then(|w| w.as_world().map(|w| w.to_owned()))
        {
            set_homeworld.set(Some(world));
            set_open(false);
        }
    });

    let panel_body = match local_worlds {
        Ok(worlds) => {
            let regions = Memo::new(move |_| {
                let preferred = locale_preferred_region(i18n.get_locale());
                worlds
                    .regions_ordered(preferred)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>()
            });
            Either::Left(move || {
                view! {
                    <ZoneAccordion
                        regions=regions.into()
                        current=current
                        on_select=on_select
                        worlds_only=true
                    />
                }
            })
        }
        Err(e) => Either::Right(move || {
            view! {
                <div class="text-red-400 p-2 rounded-lg bg-red-950/50 border border-red-800/30">
                    <span>{t!(i18n, world_picker_no_worlds_prefix)}</span>
                    <span>{e.to_string()}</span>
                </div>
            }
        }),
    };

    view! {
        <div class="side-nav-region side-nav-home-world" node_ref=root_ref>
            <button
                class="side-nav-account-trigger"
                aria-haspopup="true"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                on:click=move |_| set_open.update(|v| *v = !*v)
            >
                <span class="sr-only">{t!(i18n, home_world)}</span>
                {move || match current.get() {
                    Some(selector) => view! { <SelectorKind selector=selector /> }.into_any(),
                    None => {
                        view! {
                            <span class="shrink-0 flex items-center text-[color:var(--color-text-muted)]">
                                <Icon icon=i::BsGeoAltFill aria_hidden=true />
                            </span>
                        }
                            .into_any()
                    }
                }}
                <span class="side-nav-label ml-2">
                    {move || match homeworld.get() {
                        Some(world) => world.name,
                        None => t_string!(i18n, set_home_world).to_string(),
                    }}
                </span>
                <Icon
                    icon=i::BiChevronUpSolid
                    width="1em"
                    height="1em"
                    attr:class="side-nav-account-caret"
                />
            </button>

            <Show when=move || open.get()>
                <div
                    class="side-nav-account-panel side-nav-region-panel side-nav-home-world-panel"
                    tabindex="-1"
                >
                    {match &panel_body {
                        Either::Left(body) => body().into_any(),
                        Either::Right(body) => body().into_any(),
                    }}
                </div>
            </Show>
        </div>
    }
    .into_any()
}
