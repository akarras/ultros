//! Wide popover panel for navigating grouped subcategory links, used by
//! the item explorer toolbar. Sections have optional headers (role
//! groups) and hold the same chip links the old inline strip used.

use leptos::html::Div;
use leptos::prelude::*;
use leptos_router::components::A;
use xiv_gen::{ClassJobId, ItemSearchCategoryId};

use crate::components::dismissable::use_dismissable;
use crate::components::fonts::{ClassJobIcon, ItemSearchCategoryIcon};
use crate::components::icon::Icon;
use icondata as i;

/// Icon shown on a popover chip. Kept as plain data (not a view) so the
/// link lists stay `Clone` and can live inside signals.
#[derive(Clone, Copy, PartialEq)]
pub enum PopoverIcon {
    Job(ClassJobId),
    Category(ItemSearchCategoryId),
}

#[derive(Clone, PartialEq)]
pub struct PopoverLink {
    pub label: String,
    pub href: String,
    pub icon: PopoverIcon,
}

/// Popover of navigation chips in labeled sections. The panel is always
/// present in the DOM and toggled with a `hidden` class so the SSR and
/// hydration view trees keep the same shape; only the class attribute
/// changes on open/close.
#[component]
pub fn GroupedNavPopover(
    /// Trigger label — the active subcategory or a "browse" prompt.
    #[prop(into)]
    button_label: Signal<String>,
    /// Sections: optional header + chip links.
    #[prop(into)]
    groups: Signal<Vec<(Option<String>, Vec<PopoverLink>)>>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let container = NodeRef::<Div>::new();

    // Route change, click outside, Escape — the shared idiom.
    use_dismissable(container, move || open.set(false));

    view! {
        <div class="relative" node_ref=container>
            <button
                class="btn-secondary flex items-center gap-2"
                aria-haspopup="true"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <span>{move || button_label.get()}</span>
                <Icon icon=i::BiChevronDownRegular />
            </button>
            <div class=move || {
                if open.get() {
                    "absolute left-0 top-full mt-2 z-[100] panel rounded-xl shadow-lg border border-white/5 w-[min(90vw,44rem)] max-h-[70vh] overflow-y-auto p-4 flex flex-col gap-4"
                } else {
                    "hidden"
                }
            }>
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
                                                            PopoverIcon::Job(id) => {
                                                                view! { <ClassJobIcon id=id /> }.into_any()
                                                            }
                                                            PopoverIcon::Category(id) => {
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
    }
    .into_any()
}
