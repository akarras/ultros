//! Sticky control bar: the filter surface every tool page shares.
//!
//! Grew up inline in the Flip Finder and is now the standard filter surface
//! across the tools (#1133, #1127). It replaced the older `Toolbar` idiom —
//! a stack of labelled fields that rendered every filter whether it was in
//! use or not, and then echoed the active ones in a second hand-rolled chip
//! row — which is deleted.
//!
//! The shape here is the opposite: **only active filters take space.** Row 1
//! is the result count plus view-level controls; row 2 is one [`FilterChip`]
//! per active filter and a `+ Filter` menu holding everything unset. The bar's
//! height therefore tracks the filters in use rather than the filters that
//! exist.
//!
//! ## The height lock
//!
//! The bar is pinned to exactly [`STICKY_BAR_HEIGHT`] because the table header
//! sticks directly beneath it at that offset — a bar that grew with its
//! content would cover its own column headers. So the rows can neither wrap
//! nor scroll, and every control has to *fit*, at every width and in every
//! locale. Three things keep row 1 inside, in the order they give up space:
//! the summary is `flex-1` and truncates first, button labels are hidden below
//! `md` and ellipsize above it, and icons never shrink. A breakpoint alone
//! would not do it — the side nav takes 240px at `lg`, so the row is no wider
//! at 1024px than at 768px (#1055).
//!
//! Anything added to row 1 needs to be able to yield too.

use std::collections::HashSet;

use leptos::prelude::*;

use crate::components::dismissable::use_dismissable;
use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;

/// Height reserved for the sticky control bar. Feeds
/// `ScrollSource::Window { sticky_offset }` so rows hidden behind the bar are
/// not counted as visible, and is pinned by the bar's own `h-[76px]`.
pub const STICKY_BAR_HEIGHT: f64 = 76.0;

/// One column the picker can turn on or off.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnOption {
    /// Stable token, as persisted in `?cols=`.
    pub id: &'static str,
    pub label: String,
}

/// Handle on the bar's two popovers.
///
/// [`ControlBar`] makes its own when the caller doesn't pass one. Pass one
/// when the page's `filter_menu_extra` / `columns_extra` content needs to
/// dismiss the popover it lives in — a picker that commits on `change` has to
/// close its own menu, or it sits open over the page it just filtered.
#[derive(Copy, Clone)]
pub struct ControlBarPopovers {
    pub filter_menu: RwSignal<bool>,
    pub columns_picker: RwSignal<bool>,
}

impl Default for ControlBarPopovers {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlBarPopovers {
    pub fn new() -> Self {
        Self {
            filter_menu: RwSignal::new(false),
            columns_picker: RwSignal::new(false),
        }
    }

    pub fn close(&self) {
        self.filter_menu.set(false);
        self.columns_picker.set(false);
    }
}

/// One filter the `+ Filter` menu can add.
///
/// The label is the long, explanatory one — the menu is where a filter has to
/// be *recognized*, not just recalled, so it does not reuse the terser chip
/// label.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterOption {
    pub id: &'static str,
    pub label: String,
}

/// The sticky control bar.
///
/// Owns the two popovers (Columns, `+ Filter`) and their dismissal wiring;
/// everything route-specific arrives as a view prop or a callback.
#[component]
pub fn ControlBar(
    /// Row 1, left: the result count and any data-transparency note. This is
    /// the one thing allowed to give up space, so it truncates first.
    #[prop(into)]
    summary: ViewFn,
    /// Row 1, between the summary and the Columns button: status pills, a
    /// saved-views menu — anything that must not shrink.
    #[prop(optional, into)]
    actions: ViewFn,
    /// Columns the picker offers. Empty (the default) hides the Columns
    /// button entirely, for tools with a fixed column set.
    #[prop(optional, into)]
    columns: Signal<Vec<ColumnOption>>,
    /// Which of `columns` are currently on.
    #[prop(optional, into)]
    visible_columns: Signal<HashSet<&'static str>>,
    /// Flip one column. Required whenever `columns` is non-empty.
    #[prop(optional)]
    on_toggle_column: Option<Callback<&'static str>>,
    /// Restore the default column set.
    #[prop(optional)]
    on_reset_columns: Option<Callback<()>>,
    /// Extra controls below the column checkboxes, on their own row.
    #[prop(optional, into)]
    columns_extra: ViewFn,
    /// Filters the `+ Filter` menu offers — already narrowed to the ones not
    /// on screen as a chip.
    #[prop(into)]
    available_filters: Signal<Vec<FilterOption>>,
    /// Add one filter, seeded with something to show.
    on_add_filter: Callback<&'static str>,
    /// Extra controls below the filter list — a picker whose chip is
    /// read-only has to live here, since there is nothing to type into.
    #[prop(optional, into)]
    filter_menu_extra: ViewFn,
    /// Clear every filter at once.
    on_clear_all: Callback<()>,
    /// Shown in the chip row when nothing is filtered.
    #[prop(into)]
    empty_label: Signal<String>,
    /// True when no chip is rendered — drives `empty_label`. Kept separate
    /// from `children` because only the caller knows what its chips do.
    #[prop(into)]
    is_empty: Signal<bool>,
    /// Pass one when the page drives the popovers from its own extra content.
    #[prop(optional)]
    popovers: Option<ControlBarPopovers>,
    /// The chip strip's element, for a caller that decorates it from its own
    /// scroll geometry — the flip finder's edge fades (#1057) read
    /// `scrollLeft`/`scrollWidth` off this. Same "caller owns it" shape as
    /// `popovers`. Left unattached when nobody asks for it.
    #[prop(optional)]
    chip_row: NodeRef<leptos::html::Div>,
    /// One [`FilterChip`](crate::components::filter_chip::FilterChip) per
    /// active filter.
    children: ChildrenFn,
) -> impl IntoView {
    let i18n = use_i18n();
    let popovers = popovers.unwrap_or_default();
    let ControlBarPopovers {
        filter_menu: show_filter_menu,
        columns_picker: show_columns_picker,
    } = popovers;

    // Both popovers are anchored inside the bar, so one container dismisses
    // both: tap-away, route change, Escape.
    let bar_ref = NodeRef::<leptos::html::Div>::new();
    use_dismissable(bar_ref, move || popovers.close());

    // `ViewFn` is not `Copy`, and each of these is read from inside a nested
    // reactive closure — stored so those closures stay `FnMut`.
    let summary = StoredValue::new(summary);
    let actions = StoredValue::new(actions);
    let columns_extra = StoredValue::new(columns_extra);
    let filter_menu_extra = StoredValue::new(filter_menu_extra);

    let has_columns = Signal::derive(move || !columns.get().is_empty());

    view! {
        <div class="sticky-bar h-[76px] px-2 py-1 flex flex-col gap-1" node_ref=bar_ref>
            // Row 1 — result count and view-level controls.
            <div class="h-8 flex items-center gap-2 md:gap-3 min-w-0">
                // The one item allowed to give up space. `overflow-hidden` is
                // safe on this wrapper specifically: it holds text and nothing
                // sticky or absolutely positioned, so it does not become a
                // scrollport for anything that matters.
                <div class="flex-1 min-w-0 flex items-baseline gap-2 overflow-hidden">
                    {move || summary.with_value(|f| f.run())}
                </div>
                {move || actions.with_value(|f| f.run())}
                {move || {
                    has_columns()
                        .then(|| {
                            view! {
                                <button
                                    class="sticky-bar-button sticky-bar-button-shrink"
                                    aria-label=t_string!(i18n, analyzer_columns_button)
                                    aria-expanded=move || show_columns_picker.get().to_string()
                                    on:click=move |_| {
                                        show_filter_menu.set(false);
                                        show_columns_picker.update(|v| *v = !*v);
                                    }
                                >
                                    <Icon icon=i::FaTableColumnsSolid />
                                    <span class="hidden md:inline sticky-bar-button-label">
                                        {t!(i18n, analyzer_columns_button)}
                                    </span>
                                </button>
                            }
                        })
                }}
                <button
                    class="sticky-bar-button sticky-bar-button-shrink"
                    aria-label=t_string!(i18n, aria_clear_all_filters)
                    on:click=move |_| on_clear_all.run(())
                >
                    <Icon icon=icondata::MdiFilterRemove />
                    <span class="hidden md:inline sticky-bar-button-label">
                        {t!(i18n, analyzer_clear_all)}
                    </span>
                </button>
            </div>

            // Row 2 — the filters themselves. One chip per active filter, and
            // nothing at all for the ones that are not in use.
            <div class="h-8 flex items-center gap-2 min-w-0">
                <div class="filter-chip-row" node_ref=chip_row>
                    {move || {
                        is_empty()
                            .then(|| {
                                view! {
                                    <span class="text-sm text-[color:var(--color-text-muted)] whitespace-nowrap">
                                        {empty_label()}
                                    </span>
                                }
                            })
                    }}
                    {children()}
                </div>
                <button
                    class="sticky-bar-button"
                    aria-expanded=move || show_filter_menu.get().to_string()
                    on:click=move |_| {
                        show_columns_picker.set(false);
                        show_filter_menu.update(|v| *v = !*v);
                    }
                >
                    <Icon icon=i::FaFilterSolid />
                    {t!(i18n, analyzer_add_filter)}
                </button>
            </div>

            // `+ Filter` menu. Unset filters live here, so the bar's height
            // tracks the filters in use rather than the filters that exist.
            {move || {
                show_filter_menu
                    .get()
                    .then(|| {
                        view! {
                            <div class="sticky-bar-popover p-3 w-[min(92vw,20rem)] flex flex-col gap-2 text-sm">
                                {move || {
                                    available_filters
                                        .get()
                                        .into_iter()
                                        .map(|filter| {
                                            view! {
                                                <button
                                                    class="text-left px-2 py-1 rounded-sm text-[color:var(--color-text)] hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]"
                                                    on:click=move |_| {
                                                        on_add_filter.run(filter.id);
                                                        show_filter_menu.set(false);
                                                    }
                                                >
                                                    {filter.label.clone()}
                                                </button>
                                            }
                                        })
                                        .collect_view()
                                }}
                                {move || filter_menu_extra.with_value(|f| f.run())}
                            </div>
                        }
                    })
            }}

            // Columns picker. A popover rather than a panel so opening it
            // cannot change the bar's height.
            {move || {
                (show_columns_picker.get() && has_columns())
                    .then(|| {
                        view! {
                            <div class="sticky-bar-popover p-3 w-[min(92vw,32rem)] flex flex-row flex-wrap items-center gap-x-5 gap-y-2 text-sm">
                                <span class="font-semibold text-[color:var(--brand-fg)]">
                                    {t!(i18n, analyzer_columns_picker_label)}
                                </span>
                                {move || {
                                    columns
                                        .get()
                                        .into_iter()
                                        .map(|col| {
                                            let id = col.id;
                                            view! {
                                                <label class="inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]">
                                                    <input
                                                        type="checkbox"
                                                        class="accent-brand-300"
                                                        prop:checked=move || visible_columns.get().contains(id)
                                                        on:change=move |_| {
                                                            if let Some(toggle) = on_toggle_column {
                                                                toggle.run(id);
                                                            }
                                                        }
                                                    />
                                                    <span>{col.label.clone()}</span>
                                                </label>
                                            }
                                        })
                                        .collect_view()
                                }}
                                {move || {
                                    on_reset_columns
                                        .map(|reset| {
                                            view! {
                                                <button
                                                    class="ml-auto text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                                    on:click=move |_| reset.run(())
                                                >
                                                    {t!(i18n, analyzer_columns_picker_reset)}
                                                </button>
                                            }
                                        })
                                }}
                                {move || columns_extra.with_value(|f| f.run())}
                            </div>
                        }
                    })
            }}
        </div>
    }
}
