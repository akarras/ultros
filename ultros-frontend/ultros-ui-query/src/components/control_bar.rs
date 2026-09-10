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
//! Registered grid hosts provide `FilterRegistry` in the common owner. Their
//! native and shared metric definitions populate the menu and editable chips,
//! and their optional columns populate the Columns picker (#1330), which
//! flips `?cols=` through the grid's own visibility command so the toolbar,
//! the header menu and saved views never disagree; the bar grows as chips
//! wrap because their grids own the scrolling. Legacy hosts can continue
//! passing their own options, picker and children during migration.
//!
//! ## The height lock for legacy hosts
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
use ultros_ui_grid::components::virtual_grid::GridColumn;
use ultros_ui_grid::components::virtual_grid::registry::{
    FilterRegistry, RegisteredFilterChips, RegisteredFilterEditor, RegisteredFilterMenu,
};

use leptos::prelude::*;

use crate::components::dismissable::{provide_popover_group, use_dismissable};
use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;

/// Height of the two-row control bar, independent of grid header positioning.
pub const STICKY_BAR_HEIGHT: f64 = 76.0;

/// A group heading in the columns picker. Options carrying the same
/// heading (by label) are rendered under one heading.
#[derive(Clone, Debug, PartialEq)]
pub struct PickerHeading {
    pub label: String,
    /// Hover text on the heading ("Shows sale history for Aether (loads once)").
    pub title: Option<String>,
}

/// One column the picker can turn on or off.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnOption {
    /// Stable token, as persisted in `?cols=`.
    pub id: &'static str,
    pub label: String,
    /// `None` = the flat, ungrouped picker every page renders today.
    pub group: Option<PickerHeading>,
    /// Greyed out and not toggleable; `hint` says why.
    pub disabled: bool,
    pub hint: Option<String>,
}

impl ColumnOption {
    pub fn new(id: &'static str, label: String) -> Self {
        Self {
            id,
            label,
            group: None,
            disabled: false,
            hint: None,
        }
    }
}

/// The picker's options for a registered grid's optional columns.
///
/// The page's own columns carry no picker group and come first, as the
/// flat list a page with only native columns always had; grouped (shared)
/// columns follow, gathered under one heading per group in order of first
/// appearance, so a shared column a page placed early in its table does not
/// strand the native entries after it under its heading. The list renders a
/// heading wherever it differs from the previous entry's, so the gathering
/// is what keeps each heading to a single occurrence.
pub fn picker_options_from(columns: &[GridColumn]) -> Vec<ColumnOption> {
    let mut groups: Vec<&str> = Vec::new();
    for col in columns.iter().filter(|col| col.optional) {
        if let Some(group) = col.picker_group.as_deref()
            && !groups.contains(&group)
        {
            groups.push(group);
        }
    }
    let mut entries: Vec<(usize, ColumnOption)> = columns
        .iter()
        .filter(|col| col.optional)
        .map(|col| {
            let rank = col
                .picker_group
                .as_deref()
                .and_then(|group| groups.iter().position(|g| *g == group))
                .map_or(0, |i| i + 1);
            let option = ColumnOption {
                group: col
                    .picker_group
                    .clone()
                    .map(|label| PickerHeading { label, title: None }),
                ..ColumnOption::new(col.id, col.label.clone())
            };
            (rank, option)
        })
        .collect();
    entries.sort_by_key(|(rank, _)| *rank);
    entries.into_iter().map(|(_, option)| option).collect()
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

/// Parse a `?cols=` value into the visible-column set.
///
/// `None` (param absent) yields `default`; an explicit value — even the
/// empty string — is respected verbatim, filtered to ids in `all` so a
/// stale token from an old bookmark drops instead of lingering unrendered.
pub fn parse_visible_cols(
    raw: Option<&str>,
    all: &'static [&'static str],
    default: &'static [&'static str],
) -> HashSet<&'static str> {
    match raw {
        None => default.iter().copied().collect(),
        Some(s) => s
            .split(',')
            .filter_map(|tok| all.iter().find(|c| **c == tok).copied())
            .collect(),
    }
}

/// Serialize the visible set back to the `?cols=` value, in `all`'s order
/// so the URL is stable regardless of toggle order.
pub fn serialize_visible_cols(
    visible: &HashSet<&'static str>,
    all: &'static [&'static str],
) -> String {
    all.iter()
        .filter(|c| visible.contains(*c))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
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

/// The picker's option list. An option's `group` heading is rendered once,
/// where it first differs from the previous option's, so a page that passes
/// ungrouped options gets the flat list it always had. Options are a `Vec`
/// in the page's order — nothing here iterates a map.
#[component]
pub fn ColumnsPickerList(
    #[prop(into)] columns: Signal<Vec<ColumnOption>>,
    #[prop(into)] visible_columns: Signal<HashSet<&'static str>>,
    // `optional_no_strip`: `optional` on an `Option<T>` field strips the
    // Option from the builder setter (leptos_macro `component.rs:1033`),
    // which would reject both the bar's pass-through and the test's `None`.
    #[prop(optional_no_strip)] on_toggle_column: Option<Callback<&'static str>>,
) -> impl IntoView {
    move || {
        let mut out: Vec<AnyView> = Vec::new();
        let mut last_heading: Option<String> = None;
        for col in columns.get() {
            if let Some(heading) = &col.group
                && last_heading.as_deref() != Some(heading.label.as_str())
            {
                last_heading = Some(heading.label.clone());
                let label = heading.label.clone();
                out.push(match heading.title.clone() {
                    Some(title) => view! {
                        <span class="basis-full text-xs uppercase tracking-wide text-[color:var(--color-text-muted)] mt-1" title=title>{label}</span>
                    }
                    .into_any(),
                    None => view! {
                        <span class="basis-full text-xs uppercase tracking-wide text-[color:var(--color-text-muted)] mt-1">{label}</span>
                    }
                    .into_any(),
                });
            }
            let id = col.id;
            let toggle = move |_| {
                if let Some(toggle) = on_toggle_column {
                    toggle.run(id);
                }
            };
            // A ticked column is never locked: the cap greys an unchecked
            // capped entry, and only hints a checked one.
            let disabled = col.disabled && !visible_columns.get().contains(id);
            out.push(if disabled || col.hint.is_some() {
                let hint = col.hint.clone().unwrap_or_default();
                // The grey and the cursor follow `disabled`, not `hint`. A
                // hint on a toggleable entry says why the column may look
                // empty; rendering it as unavailable would be a lie, and
                // the ticked-capped case above already relies on the entry
                // staying usable.
                let class = if disabled {
                    "inline-flex items-center gap-2 cursor-not-allowed opacity-60 text-[color:var(--color-text)]"
                } else {
                    "inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]"
                };
                view! {
                    <label class=class title=hint>
                        <input
                            type="checkbox"
                            class="accent-brand-300"
                            disabled=disabled
                            prop:checked=move || visible_columns.get().contains(id)
                            on:change=toggle
                        />
                        <span>{col.label.clone()}</span>
                    </label>
                }
                .into_any()
            } else {
                view! {
                    <label class="inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]">
                        <input
                            type="checkbox"
                            class="accent-brand-300"
                            prop:checked=move || visible_columns.get().contains(id)
                            on:change=toggle
                        />
                        <span>{col.label.clone()}</span>
                    </label>
                }
                .into_any()
            });
        }
        out
    }
}

/// The sticky control bar.
///
/// Owns the two popovers (Columns, `+ Filter`) and their dismissal wiring;
/// everything route-specific arrives as a view prop or a callback.
#[component]
pub fn ControlBar(
    /// Grid tools scroll their rows internally, so their toolbar stays in
    /// normal page flow rather than covering the grid when the page scrolls.
    #[prop(default = true)]
    sticky: bool,
    /// Row 1, left: the result count and any data-transparency note. This is
    /// the one thing allowed to give up space, so it truncates first.
    #[prop(into)]
    summary: ViewFn,
    /// Row 1, between the summary and the Columns button: status pills, a
    /// saved-views menu — anything that must not shrink.
    #[prop(optional, into)]
    actions: ViewFn,
    /// Columns the picker offers. Empty (the default) hands the picker to
    /// the registered grid's optional columns when a [`FilterRegistry`] is
    /// in context, and otherwise hides the Columns button entirely, for
    /// tools with a fixed column set.
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
    #[prop(into, default = Signal::derive(Vec::new))]
    available_filters: Signal<Vec<FilterOption>>,
    /// Add one filter, seeded with something to show.
    #[prop(default = Callback::new(|_| ()))]
    on_add_filter: Callback<&'static str>,
    /// Extra controls below the filter list — a picker whose chip is
    /// read-only has to live here, since there is nothing to type into.
    #[prop(optional, into)]
    filter_menu_extra: ViewFn,
    /// Clear every filter at once.
    #[prop(default = Callback::new(|_| ()))]
    on_clear_all: Callback<()>,
    /// Shown in the chip row when nothing is filtered.
    #[prop(into)]
    empty_label: Signal<String>,
    /// True when no chip is rendered — drives `empty_label`. Kept separate
    /// from `children` because only the caller knows what its chips do.
    #[prop(into, default = Signal::derive(|| true))]
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
    #[prop(optional)]
    children: Option<ChildrenFn>,
) -> impl IntoView {
    let i18n = use_i18n();
    let registry = use_context::<FilterRegistry>();
    let location = crate::components::app_link::use_location_or_default();
    #[cfg(feature = "hydrate")]
    let nav = leptos_router::hooks::use_navigate();
    // Grid filters share the URL with each tool's chips. Use the same queued
    // query setter as those chips so Clear all merges every removal without
    // replacing the user's column layout, visibility, or sort.
    let (grid_filters, set_grid_filters) =
        crate::query_defaults::filter_query_signal::<String>("gf");
    let all_filters_empty = Signal::derive(move || {
        if let Some(registry) = registry {
            !registry
                .entries()
                .iter()
                .any(|e| registry.active(&e.filter, &location.query.get()))
        } else {
            is_empty()
                && ultros_grid_core::metrics::parse_filters(grid_filters.get().as_deref())
                    .is_empty()
        }
    });
    let popovers = popovers.unwrap_or_default();
    let ControlBarPopovers {
        filter_menu: show_filter_menu,
        columns_picker: show_columns_picker,
    } = popovers;

    // Menus mounted in `actions` (the saved-views menu, the recipe
    // analyzer's Market menu) live *inside* the bar, so a tap on one of
    // their buttons is not an outside click here and would leave the bar's
    // own popovers open underneath. The group makes the exclusion explicit:
    // the bar joins it below, those menus join it as they render, and
    // whichever opens closes the rest.
    provide_popover_group();

    // Both of the bar's own popovers are anchored inside it, so one
    // container dismisses both: tap-away, route change, Escape.
    let bar_ref = NodeRef::<leptos::html::Div>::new();
    let popover_token = use_dismissable(bar_ref, move || {
        popovers.close();
        if let Some(r) = registry {
            r.editing.set(None);
        }
    });

    // `ViewFn` is not `Copy`, and each of these is read from inside a nested
    // reactive closure — stored so those closures stay `FnMut`.
    let summary = StoredValue::new(summary);
    let actions = StoredValue::new(actions);
    let columns_extra = StoredValue::new(columns_extra);
    let filter_menu_extra = StoredValue::new(filter_menu_extra);

    // A page that passes its own picker keeps it whole — options, checked
    // state, toggle and reset are one contract. Everything else gets the
    // registered grid's: the same resolved columns, `?cols=` state and
    // visibility commands its header menu uses, so the two never disagree.
    let registry_picker =
        Signal::derive(move || registry.filter(|_| columns.with(|explicit| explicit.is_empty())));
    let picker_columns = Signal::derive(move || match registry_picker.get() {
        Some(registry) => picker_options_from(&registry.optional_columns()),
        None => columns.get(),
    });
    let picker_visible = Signal::derive(move || match registry_picker.get() {
        Some(registry) => registry.visible_columns(),
        None => visible_columns.get(),
    });
    let toggle_column =
        Callback::new(
            move |id: &'static str| match registry_picker.get_untracked() {
                Some(registry) => registry.toggle_column(id),
                None => {
                    if let Some(toggle) = on_toggle_column {
                        toggle.run(id);
                    }
                }
            },
        );
    let reset_columns = Signal::derive(move || match registry_picker.get() {
        Some(registry) => Some(Callback::new(move |_| registry.reset_columns())),
        None => on_reset_columns,
    });
    let has_columns = Signal::derive(move || !picker_columns.get().is_empty());

    view! {
        <div class="sticky-bar px-2 py-1 flex flex-col gap-1" class:registered-filter-bar=registry.is_some() style=format!("{} position: {};", if registry.is_some() { format!("min-height: {STICKY_BAR_HEIGHT}px;") } else { format!("height: {STICKY_BAR_HEIGHT}px;") }, if sticky { "sticky" } else { "relative" }) node_ref=bar_ref>
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
                                        let opening = !show_columns_picker.get_untracked();
                                        if opening {
                                            popover_token.opening();
                                        }
                                        show_columns_picker.set(opening);
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
                    on:click=move |_| {
                        if let Some(registry) = registry {
                            registry.editing.set(None);
                            let _q = registry.clear_all(&location.query.get_untracked());
                            #[cfg(feature = "hydrate")]
                            nav(&format!("{}{}", location.pathname.get_untracked(), _q.to_query_string()), leptos_router::NavigateOptions { replace: true, scroll: false, ..Default::default() });
                        } else {
                            on_clear_all.run(());
                            set_grid_filters.set(None);
                        }
                    }
                >
                    <Icon icon=icondata::MdiFilterRemove />
                    <span class="hidden md:inline sticky-bar-button-label">
                        {t!(i18n, analyzer_clear_all)}
                    </span>
                </button>
            </div>

            // Row 2 — the filters themselves. One chip per active filter, and
            // nothing at all for the ones that are not in use.
            <div class=if registry.is_some() { "min-h-8 flex items-start gap-2 min-w-0" } else { "h-8 flex items-center gap-2 min-w-0" }>
                <div class="filter-chip-row" style=if registry.is_some() { "flex-wrap: wrap; overflow: visible; height: auto;" } else { "" } node_ref=chip_row>
                    {move || {
                        all_filters_empty()
                            .then(|| {
                                view! {
                                    <span class="text-sm text-[color:var(--color-text-muted)] whitespace-nowrap">
                                        {empty_label()}
                                    </span>
                                }
                            })
                    }}
                    {registry.map(|registry| view! { <RegisteredFilterChips registry/> })}
                    {children.map(|children| children())}
                </div>
                <button
                    class="sticky-bar-button"
                    data-add-filter-menu
                    aria-expanded=move || show_filter_menu.get().to_string()
                    on:click=move |_| {
                        show_columns_picker.set(false);
                        let opening = !show_filter_menu.get_untracked();
                        if opening {
                            popover_token.opening();
                        }
                        show_filter_menu.set(opening);
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
                                {registry.map(|registry| view! { <RegisteredFilterMenu registry on_select=Callback::new(move |_| show_filter_menu.set(false))/> })}
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

            {registry.map(|registry| view! { <RegisteredFilterEditor registry/> })}

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
                                <ColumnsPickerList
                                    columns=picker_columns
                                    visible_columns=picker_visible
                                    on_toggle_column=Some(toggle_column)
                                />
                                {move || {
                                    reset_columns
                                        .get()
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

#[cfg(test)]
mod tests {
    use super::*;
    use leptos_i18n::context::init_i18n_context;

    fn render_list(cols: Vec<ColumnOption>) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            view! {
                <ColumnsPickerList
                    columns=Signal::derive(move || cols.clone())
                    visible_columns=Signal::derive(HashSet::new)
                    on_toggle_column=None
                />
            }
            .to_html()
        })
    }

    /// Native (ungrouped) columns lead in table order; shared columns are
    /// gathered under one heading per group in order of first appearance,
    /// even when a page placed one of them ahead of its native columns.
    /// Required columns never reach the picker.
    #[test]
    fn registered_picker_leads_with_native_columns_and_gathers_groups() {
        let shared = |id, group: &str| {
            let mut col = GridColumn::new(id, id.to_uppercase(), 100.0, true, false);
            col.picker_group = Some(group.into());
            col
        };
        let columns = vec![
            GridColumn::new("item", "Item".into(), 300.0, false, true),
            shared("market-sale-median", "Sale history (selected window)"),
            GridColumn::new("profit", "Profit".into(), 100.0, true, true),
            shared("market-units-7", "Sale history (7d)"),
            GridColumn::new("level", "Level".into(), 100.0, true, false),
            shared("market-units", "Sale history (selected window)"),
        ];
        let options = picker_options_from(&columns);
        let ids: Vec<_> = options.iter().map(|o| o.id).collect();
        assert_eq!(
            ids,
            [
                "profit",
                "level",
                "market-sale-median",
                "market-units",
                "market-units-7"
            ]
        );
        assert!(options[0].group.is_none() && options[1].group.is_none());
        assert_eq!(
            options[2].group.as_ref().map(|g| g.label.as_str()),
            Some("Sale history (selected window)")
        );
        assert_eq!(options[3].group, options[2].group);
        assert_eq!(
            options[4].group.as_ref().map(|g| g.label.as_str()),
            Some("Sale history (7d)")
        );
        assert_eq!(options[2].label, "MARKET-SALE-MEDIAN");
        assert!(options.iter().all(|o| !o.disabled && o.hint.is_none()));
    }

    /// Ungrouped options render the flat list every page renders today:
    /// no headings, no disabled inputs, no titles.
    #[test]
    fn picker_list_without_groups_is_the_flat_list() {
        let html = render_list(vec![
            ColumnOption::new("tax", "Tax".into()),
            ColumnOption::new("vwap", "VWAP (7d)".into()),
        ]);
        assert_eq!(html.matches("<label").count(), 2, "{html}");
        assert_eq!(
            html.matches("<label class=\"inline-flex items-center gap-2 cursor-pointer text-[color:var(--color-text)]\"><input type=\"checkbox\" class=\"accent-brand-300\"").count(),
            2,
            "{html}"
        );
        assert!(html.contains("<span>Tax</span>"), "{html}");
        assert!(!html.contains("basis-full"), "{html}");
        assert!(!html.contains("disabled"), "{html}");
        assert!(!html.contains("title="), "{html}");
    }

    #[test]
    fn picker_list_renders_group_headings_once_and_disables_capped_options() {
        let rev = PickerHeading {
            label: "Revenue · Gilgamesh".into(),
            title: None,
        };
        let cost = PickerHeading {
            label: "Cost · Aether".into(),
            title: Some("loads once".into()),
        };
        let html = render_list(vec![
            ColumnOption {
                group: Some(rev.clone()),
                ..ColumnOption::new("rev-sale-min", "Sale minimum (7d)".into())
            },
            ColumnOption {
                group: Some(rev),
                ..ColumnOption::new("rev-sale-avg", "Sale average (7d)".into())
            },
            ColumnOption {
                group: Some(cost.clone()),
                ..ColumnOption::new("cost-sale-min", "Sale minimum (7d)".into())
            },
            ColumnOption {
                group: Some(cost),
                disabled: true,
                hint: Some("capped".into()),
                ..ColumnOption::new("cost-sale-avg", "Sale average (7d)".into())
            },
            // Hinted but perfectly toggleable — the recipe analyzer's
            // "Needs a wider sell scope". It gets the title and NOT the
            // lock, and the ticked-capped entry above relies on the same
            // split (`disabled` is recomputed against the visible set, so
            // a ticked capped column keeps its hint and loses its lock).
            ColumnOption {
                hint: Some("needs a wider scope".into()),
                ..ColumnOption::new("scope-vs-home", "Scope vs home".into())
            },
        ]);
        assert_eq!(html.matches("Revenue · Gilgamesh").count(), 1, "{html}");
        assert_eq!(html.matches("Cost · Aether").count(), 1, "{html}");
        assert!(html.contains("title=\"loads once\""), "{html}");
        assert_eq!(html.matches("basis-full").count(), 2, "{html}");
        assert_eq!(html.matches("disabled").count(), 1, "{html}");
        assert!(html.contains("title=\"capped\""), "{html}");
        // The grey and the cursor follow `disabled`, never `hint`: exactly
        // one entry here is unavailable, so exactly one is drawn that way.
        assert!(html.contains("title=\"needs a wider scope\""), "{html}");
        assert_eq!(
            html.matches("cursor-not-allowed").count(),
            1,
            "a hint explains an entry; it does not disable it: {html}"
        );
        // Headings precede their options.
        let rev_at = html.find("Revenue · Gilgamesh").unwrap();
        let first_opt = html.find("Sale minimum (7d)").unwrap();
        assert!(rev_at < first_opt, "{html}");
    }
}
