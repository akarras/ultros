//! Inline accordion for the item explorer's subcategory navigation.
//! Sections have optional headers (role groups) and hold the chip links
//! the toolbar builds for the selected group.

use leptos::prelude::*;
use leptos_router::components::A;
use xiv_gen::{ClassJobId, ItemSearchCategoryId};

use crate::components::fonts::{ClassJobIcon, ItemSearchCategoryIcon};
use crate::components::icon::Icon;
use icondata as i;

/// Icon shown on a nav chip. Kept as plain data (not a view) so the link
/// lists stay `Clone` and can live inside signals.
#[derive(Clone, Copy, PartialEq)]
pub enum NavIcon {
    Job(ClassJobId),
    Category(ItemSearchCategoryId),
}

#[derive(Clone, PartialEq)]
pub struct NavLink {
    pub label: String,
    pub href: String,
    pub icon: NavIcon,
}

/// Accordion of navigation chips in labeled sections.
///
/// The panel's children are always rendered; only the wrapper's
/// `grid-template-rows` track changes between collapsed and expanded. That
/// keeps the SSR and hydration view trees identical in shape — the same
/// discipline the popover this replaced followed by toggling a class rather
/// than mounting and unmounting — and unlike a `hidden` toggle it animates.
///
/// `open` is owned by the caller. The item explorer toolbar forces the
/// accordion open when a group pill is clicked and collapses it on
/// navigation, so the state cannot live in here.
#[component]
pub fn GroupedNavAccordion(
    /// Header label — the active subcategory or a "browse" prompt.
    #[prop(into)]
    button_label: Signal<String>,
    /// Sections: optional header + chip links.
    #[prop(into)]
    groups: Signal<Vec<(Option<String>, Vec<NavLink>)>>,
    /// Expanded state, owned by the caller.
    open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="item-explorer-accordion">
            <button
                type="button"
                class="item-explorer-accordion-header"
                aria-expanded=move || open.get().to_string()
                aria-controls="item-explorer-subcategories"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <span>{move || button_label.get()}</span>
                <Icon
                    icon=i::BiChevronDownRegular
                    aria_hidden=true
                    attr:class="item-explorer-accordion-chevron"
                />
            </button>
            <div
                id="item-explorer-subcategories"
                class="item-explorer-accordion-panel"
                data-open=move || open.get().to_string()
            >
                <div class="item-explorer-accordion-inner">
                    {move || {
                        groups
                            .get()
                            .into_iter()
                            .map(|(header, links)| {
                                view! {
                                    <div class="flex flex-col gap-2">
                                        {header
                                            .map(|header| {
                                                view! {
                                                    <div class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                                                        {header}
                                                    </div>
                                                }
                                            })}
                                        <div class="flex flex-wrap gap-2">
                                            {links
                                                .into_iter()
                                                .map(|link| {
                                                    view! {
                                                        <A href=link.href attr:class="item-explorer-chip">
                                                            {match link.icon {
                                                                NavIcon::Job(id) => {
                                                                    view! { <ClassJobIcon id=id /> }.into_any()
                                                                }
                                                                NavIcon::Category(id) => {
                                                                    view! { <ItemSearchCategoryIcon id=id /> }.into_any()
                                                                }
                                                            }}
                                                            <span>{link.label}</span>
                                                        </A>
                                                    }
                                                })
                                                .collect::<Vec<_>>()}
                                        </div>
                                    </div>
                                }
                            })
                            .collect::<Vec<_>>()
                    }}
                </div>
            </div>
        </div>
    }
    .into_any()
}
