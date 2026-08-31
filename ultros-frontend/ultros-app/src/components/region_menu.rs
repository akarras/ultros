//! Price-zone row + drop-up for the sidebar, above the account row.
//!
//! Surfaces the global price zone (the `PRICE_ZONE` cookie behind
//! [`get_price_zone`]) so it can be seen and changed from any page (#1179)
//! instead of only from `/settings`. The panel is a region → datacenter →
//! world accordion rather than the searchable `WorldPicker`: `Select`'s
//! dropdown portals to `document.body`, so `use_dismissable`'s outside-click
//! handler would unmount the panel on the very pointerdown that picks a
//! world — and an `<input>`-based combobox pops the soft keyboard inside the
//! mobile drawer. The full searchable picker remains available in settings.

use crate::components::dismissable::use_dismissable;
use crate::components::icon::Icon;
use crate::components::world_picker::SelectorKind;
use crate::global_state::home_world::{
    get_price_zone, locale_preferred_region, result_to_selector_read, selector_to_setter_signal,
};
use crate::global_state::use_world_helper;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::either::Either;
use leptos::html;
use leptos::prelude::*;
use ultros_api_types::world::Region;
use ultros_api_types::world_helper::AnySelector;

/// A select-row + optional expand-chevron pair. Two sibling buttons, never
/// nested — a button inside a button is invalid HTML — so "click the name to
/// select this scope" and "click the chevron to drill in" stay unambiguous
/// and separately tappable.
#[component]
fn MenuRow(
    selector: AnySelector,
    name: String,
    current: Signal<Option<AnySelector>>,
    on_select: Callback<AnySelector>,
    /// `Some` renders the expand chevron; the bool signal is whether this
    /// row's children are currently expanded.
    #[prop(optional, into)]
    expand: Option<(Signal<bool>, Callback<()>)>,
) -> impl IntoView {
    let i18n = use_i18n();
    let selected = Memo::new(move |_| current.get() == Some(selector));
    let select_name = name.clone();
    let expand_label = format!("{} {}", t_string!(i18n, region_menu_expand), name);
    view! {
        <div class="menu-row">
            <button
                type="button"
                class=move || {
                    if selected.get() { "menu-item menu-item-selected" } else { "menu-item" }
                }
                on:click=move |_| on_select.run(selector)
            >
                <SelectorKind selector=selector />
                <span class="ml-2 truncate">{select_name}</span>
                <Show when=move || selected.get()>
                    <span class="menu-item-trailing">
                        <Icon icon=i::BsCheckCircleFill width="0.9em" height="0.9em" />
                    </span>
                </Show>
            </button>
            {expand
                .map(|(expanded, toggle)| {
                    view! {
                        <button
                            type="button"
                            class="menu-expand"
                            aria-label=expand_label
                            aria-expanded=move || if expanded.get() { "true" } else { "false" }
                            on:click=move |_| toggle.run(())
                        >
                            <Icon icon=i::BiChevronDownSolid width="1em" height="1em" />
                        </button>
                    }
                })}
        </div>
    }
}

/// The region → datacenter → world accordion inside the panel. One branch
/// open per level, seeded from the current selection's ancestors so the
/// panel opens showing where you already are.
///
/// Shared between the price-zone drop-up (any scope selectable) and the
/// home-world drop-up (`worlds_only`, where region/datacenter rows only
/// drill in — a home world is always a world).
#[component]
pub(crate) fn ZoneAccordion(
    regions: Signal<Vec<Region>>,
    current: Signal<Option<AnySelector>>,
    on_select: Callback<AnySelector>,
    /// When true, only world rows fire `on_select`; clicking a region or
    /// datacenter name toggles its branch open instead, same as the chevron.
    #[prop(optional)]
    worlds_only: bool,
) -> impl IntoView {
    let ancestors = move || {
        regions.with_untracked(|regions| match current.get_untracked() {
            Some(AnySelector::Region(id)) => (Some(id), None),
            Some(AnySelector::Datacenter(id)) => (
                regions
                    .iter()
                    .find(|r| r.datacenters.iter().any(|d| d.id == id))
                    .map(|r| r.id),
                Some(id),
            ),
            Some(AnySelector::World(id)) => regions
                .iter()
                .find_map(|r| {
                    r.datacenters
                        .iter()
                        .find(|d| d.worlds.iter().any(|w| w.id == id))
                        .map(|d| (Some(r.id), Some(d.id)))
                })
                .unwrap_or((None, None)),
            None => (None, None),
        })
    };
    let (initial_region, initial_dc) = ancestors();
    let (expanded_region, set_expanded_region) = signal(initial_region);
    let (expanded_dc, set_expanded_dc) = signal(initial_dc);

    view! {
        {move || {
            regions
                .get()
                .into_iter()
                .map(|region| {
                    let region_id = region.id;
                    let region_open = Signal::derive(move || {
                        expanded_region.get() == Some(region_id)
                    });
                    let toggle_region = Callback::new(move |_| {
                        set_expanded_region
                            .update(|r| {
                                *r = (*r != Some(region_id)).then_some(region_id);
                            });
                    });
                    let datacenters = region.datacenters;
                    let region_select = if worlds_only {
                        Callback::new(move |_| toggle_region.run(()))
                    } else {
                        on_select
                    };
                    view! {
                        <MenuRow
                            selector=AnySelector::Region(region_id)
                            name=region.name
                            current=current
                            on_select=region_select
                            expand=(region_open, toggle_region)
                        />
                        <Show when=move || region_open.get()>
                            <div class="menu-accordion">
                                {datacenters
                                    .clone()
                                    .into_iter()
                                    .map(|dc| {
                                        let dc_id = dc.id;
                                        let dc_open = Signal::derive(move || {
                                            expanded_dc.get() == Some(dc_id)
                                        });
                                        let toggle_dc = Callback::new(move |_| {
                                            set_expanded_dc
                                                .update(|d| {
                                                    *d = (*d != Some(dc_id)).then_some(dc_id);
                                                });
                                        });
                                        let worlds = dc.worlds;
                                        let dc_select = if worlds_only {
                                            Callback::new(move |_| toggle_dc.run(()))
                                        } else {
                                            on_select
                                        };
                                        view! {
                                            <MenuRow
                                                selector=AnySelector::Datacenter(dc_id)
                                                name=dc.name
                                                current=current
                                                on_select=dc_select
                                                expand=(dc_open, toggle_dc)
                                            />
                                            <Show when=move || dc_open.get()>
                                                <div class="menu-accordion">
                                                    {worlds
                                                        .clone()
                                                        .into_iter()
                                                        .map(|world| {
                                                            view! {
                                                                <MenuRow
                                                                    selector=AnySelector::World(world.id)
                                                                    name=world.name
                                                                    current=current
                                                                    on_select=on_select
                                                                />
                                                            }
                                                        })
                                                        .collect_view()}
                                                </div>
                                            </Show>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </Show>
                    }
                })
                .collect_view()
        }}
    }
}

#[component]
pub fn RegionMenu() -> impl IntoView {
    let i18n = use_i18n();
    let (open, set_open) = signal(false);
    let root_ref = NodeRef::<html::Div>::new();

    // Route change, click outside, Escape — the shared idiom.
    use_dismissable(root_ref, move || set_open(false));

    let (zone, set_zone) = get_price_zone();
    let current = result_to_selector_read(zone);
    let set_selector = selector_to_setter_signal(set_zone);
    let on_select = Callback::new(move |selector: AnySelector| {
        set_selector.set(Some(selector));
        set_open(false);
    });

    // NOT `use_context().expect(..)`: the sidebar renders on every page, and
    // an absent `LocalWorldData` context is a live production state (GlitchTip
    // #7120/#7187). `use_world_helper` collapses "never provided" and "holds an
    // Err" into the one error branch this panel already renders.
    let local_worlds = use_world_helper();

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
            Either::Left(
                move || view! { <ZoneAccordion regions=regions.into() current=current on_select=on_select /> },
            )
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
        <div class="side-nav-region" node_ref=root_ref>
            <button
                class="side-nav-account-trigger"
                aria-haspopup="true"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                // See `HomeWorldMenu`: the visible overline is hidden in the
                // collapsed rail and the price-tag icon is `aria_hidden`, so
                // the accessible name has to come from the attribute.
                aria-label=move || {
                    format!(
                        "{}: {}",
                        t_string!(i18n, region_menu_label),
                        match zone.get() {
                            Some(zone) => zone.get_name().to_string(),
                            None => t_string!(i18n, region_menu_no_zone).to_string(),
                        },
                    )
                }
                on:click=move |_| set_open.update(|v| *v = !*v)
            >
                // A price tag, not `SelectorKind`: the kind icon made this
                // row indistinguishable from the home-world row whenever the
                // zone was a world (#1235). "Where prices come from" is the
                // row's identity; the selected scope's kind still shows on
                // the rows inside the panel.
                <span class="shrink-0 flex items-center text-[color:var(--color-text-muted)]">
                    <Icon icon=i::ImPriceTag aria_hidden=true />
                </span>
                <span class="side-nav-label side-nav-zone-text ml-2">
                    <span class="side-nav-zone-overline">{t!(i18n, region_menu_label)}</span>
                    <span class="side-nav-zone-value">
                        {move || match zone.get() {
                            Some(zone) => zone.get_name().to_string(),
                            None => t_string!(i18n, region_menu_no_zone).to_string(),
                        }}
                    </span>
                </span>
                <Icon
                    icon=i::BiChevronUpSolid
                    width="1em"
                    height="1em"
                    attr:class="side-nav-account-caret"
                />
            </button>

            <Show when=move || open.get()>
                <div class="side-nav-account-panel side-nav-region-panel" tabindex="-1">
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
