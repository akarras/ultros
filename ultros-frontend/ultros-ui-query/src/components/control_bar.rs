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

use std::collections::{HashMap, HashSet};
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
    /// The statistic this column is one window of. Options sharing a family
    /// (and group) are offered as one row with a window choice.
    pub family: Option<String>,
    /// This option's window within its family ("7d").
    pub variant: Option<String>,
    /// The heading over the family's row; `group` names one window.
    pub family_group: Option<PickerHeading>,
}

impl ColumnOption {
    pub fn new(id: &'static str, label: String) -> Self {
        Self {
            id,
            label,
            group: None,
            disabled: false,
            hint: None,
            family: None,
            variant: None,
            family_group: None,
        }
    }

    fn same_family(&self, other: &ColumnOption) -> bool {
        self.family.is_some()
            && self.family == other.family
            && self.family_group == other.family_group
    }
}

/// A row of the picker's "Add a column" list.
#[derive(Clone, Debug, PartialEq)]
pub enum AddRow {
    /// A column with no window to choose.
    Single(ColumnOption),
    /// One statistic, offered in each of its windows.
    Family(Vec<ColumnOption>),
}

impl AddRow {
    fn group(&self) -> Option<&PickerHeading> {
        match self {
            AddRow::Single(column) => column.group.as_ref(),
            AddRow::Family(variants) => variants[0].family_group.as_ref(),
        }
    }
}

/// The "Add a column" rows: every option not on screen, the windows of one
/// statistic folded into a single row, in the options' order. A family row
/// stays while any of its windows is hidden and lists all of them, so a
/// search for "median 90" still offers the other windows beside the match.
pub fn add_rows(
    columns: &[ColumnOption],
    visible: &HashSet<&'static str>,
    query: &str,
) -> Vec<AddRow> {
    let matched: HashSet<&str> = search_column_options(columns, query)
        .iter()
        .map(|column| column.id)
        .collect();
    let mut rows: Vec<AddRow> = Vec::new();
    for column in columns {
        let family = rows.iter_mut().find_map(|row| match row {
            AddRow::Family(variants) if variants[0].same_family(column) => Some(variants),
            _ => None,
        });
        match (family, column.family.is_some()) {
            (Some(variants), _) => variants.push(column.clone()),
            (None, true) => rows.push(AddRow::Family(vec![column.clone()])),
            (None, false) => rows.push(AddRow::Single(column.clone())),
        }
    }
    rows.retain(|row| match row {
        AddRow::Single(column) => matched.contains(column.id) && !visible.contains(column.id),
        AddRow::Family(variants) => {
            variants.iter().any(|v| matched.contains(v.id))
                && variants.iter().any(|v| !visible.contains(v.id))
        }
    });
    rows
}

/// One column on screen, as the picker's "Showing" list names it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownColumn {
    pub id: &'static str,
    pub label: String,
    /// Required columns are listed but cannot be removed.
    pub removable: bool,
}

/// The "Showing" rows for a registered grid's displayed columns. A window
/// of a statistic is named by its statistic; the window is the row's pill.
pub fn shown_columns_from(columns: &[GridColumn]) -> Vec<ShownColumn> {
    columns
        .iter()
        .map(|col| ShownColumn {
            id: col.id,
            label: col
                .picker_family
                .clone()
                .or_else(|| col.picker_label.clone())
                .unwrap_or_else(|| col.label.clone()),
            removable: col.optional,
        })
        .collect()
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
                group: col.picker_group.clone().map(|label| PickerHeading {
                    label,
                    title: col.picker_group_title.clone(),
                }),
                disabled: col.picker_disabled,
                hint: col.picker_hint.clone(),
                family: col.picker_family.clone(),
                variant: col.picker_variant.clone(),
                family_group: col.picker_family_group.clone().map(|label| PickerHeading {
                    label,
                    title: col.picker_group_title.clone(),
                }),
                ..ColumnOption::new(
                    col.id,
                    col.picker_label
                        .clone()
                        .unwrap_or_else(|| col.label.clone()),
                )
            };
            (rank, option)
        })
        .collect();
    entries.sort_by_key(|(rank, _)| *rank);
    entries.into_iter().map(|(_, option)| option).collect()
}

/// Search labels and their explanations without changing order, identity or
/// availability. Every word must match, so "median fixed" finds the pinned
/// history columns even when the words live in different metadata fields.
pub fn search_column_options(columns: &[ColumnOption], query: &str) -> Vec<ColumnOption> {
    let query = query.to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    columns
        .iter()
        .filter(|column| {
            let mut text = column.label.to_lowercase();
            if let Some(family) = &column.family {
                text.push_str(&format!(" {}", family.to_lowercase()));
            }
            if let Some(group) = &column.family_group {
                text.push_str(&format!(" {}", group.label.to_lowercase()));
            }
            if let Some(group) = &column.group {
                text.push_str(&format!(" {}", group.label.to_lowercase()));
                if let Some(title) = &group.title {
                    text.push_str(&format!(" {}", title.to_lowercase()));
                }
            }
            if let Some(hint) = &column.hint {
                text.push_str(&format!(" {}", hint.to_lowercase()));
            }
            terms.iter().all(|term| text.contains(term))
        })
        .cloned()
        .collect()
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

/// The visible-column set a URL's column keys state (see
/// [`ultros_grid_core::columns`]).
///
/// No column keys yields `default`; an explicit `cols` list — even an empty
/// one — is respected verbatim. Either way the result is filtered to ids in
/// `all`, so a stale token from an old bookmark drops instead of lingering
/// unrendered.
pub fn parse_visible_cols(
    columns: ultros_grid_core::columns::ColumnQuery<'_>,
    all: &'static [&'static str],
    default: &'static [&'static str],
) -> HashSet<&'static str> {
    all.iter()
        .copied()
        .filter(|id| columns.visible(id, default.contains(id)))
        .collect()
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

/// A group heading in the "Add a column" list.
fn picker_heading(heading: &PickerHeading) -> AnyView {
    let label = heading.label.clone();
    match heading.title.clone() {
        Some(title) => {
            view! { <li class="columns-picker-heading" title=title>{label}</li> }.into_any()
        }
        None => view! { <li class="columns-picker-heading">{label}</li> }.into_any(),
    }
}

/// The Columns popover's body: what is on screen, then what can be added.
///
/// "Showing" lists the displayed columns in grid order, each removable one
/// with an ×, and a statistic's window as a pill that opens the other windows
/// to swap to in place. "Add a column" folds the windows of one statistic
/// into a single row ([`add_rows`]), so a page offering nine statistics in
/// five windows lists nine rows rather than forty-five checkboxes.
///
/// A sibling of [`ControlBar`] so a render test can reach it without the
/// popover's open gate.
#[component]
pub fn ColumnsPicker(
    #[prop(into)] columns: Signal<Vec<ColumnOption>>,
    #[prop(into)] shown: Signal<Vec<ShownColumn>>,
    #[prop(into)] visible_columns: Signal<HashSet<&'static str>>,
    search: RwSignal<String>,
    // `optional_no_strip`: `optional` on an `Option<T>` field strips the
    // Option from the builder setter (leptos_macro `component.rs:1033`),
    // which would reject both the bar's pass-through and the test's `None`.
    #[prop(optional_no_strip)] on_toggle_column: Option<Callback<&'static str>>,
    /// Replace a shown column with another window of its statistic.
    #[prop(optional_no_strip)]
    on_swap_column: Option<Callback<(&'static str, &'static str)>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let toggle = move |id: &'static str| {
        if let Some(toggle) = on_toggle_column {
            toggle.run(id);
        }
    };
    // The shown row whose window choices are open.
    let choosing = RwSignal::new(None::<&'static str>);
    // The window picked on each Add row, keyed by the row's first window.
    let picked = RwSignal::new(HashMap::<&'static str, &'static str>::new());

    let shown_rows = move || {
        let visible = visible_columns.get();
        let options = columns.get();
        shown
            .get()
            .into_iter()
            .map(|row| {
                let id = row.id;
                let label = row.label.clone();
                let window = options.iter().find(|o| o.id == id).and_then(|current| {
                    let variant = current.variant.clone()?;
                    let variants: Vec<_> = options
                        .iter()
                        .filter(|o| o.same_family(current))
                        .map(|o| {
                            let taken = o.id != id && (visible.contains(o.id) || o.disabled);
                            (o.id, o.variant.clone().unwrap_or_default(), taken, o.hint.clone())
                        })
                        .collect();
                    let change = t_string!(i18n, analyzer_columns_change_window, column = label.clone()).to_string();
                    Some(if choosing.get() == Some(id) {
                        view! {
                            <span class="column-windows" role="group" aria-label=change>
                                {variants
                                    .into_iter()
                                    .map(|(target, variant, taken, hint)| {
                                        view! {
                                            <button
                                                type="button"
                                                class="column-window"
                                                aria-pressed=(target == id).to_string()
                                                disabled=taken
                                                title=hint
                                                on:click=move |_| {
                                                    choosing.set(None);
                                                    if target != id
                                                        && let Some(swap) = on_swap_column
                                                    {
                                                        swap.run((id, target));
                                                    }
                                                }
                                            >
                                                {variant}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </span>
                        }
                        .into_any()
                    } else {
                        view! {
                            <button
                                type="button"
                                class="column-window"
                                aria-pressed="true"
                                aria-expanded="false"
                                aria-label=change.clone()
                                title=change
                                on:click=move |_| choosing.set(Some(id))
                            >
                                {variant}
                                <span aria-hidden="true">" ▾"</span>
                            </button>
                        }
                        .into_any()
                    })
                });
                let remove = row.removable.then(|| {
                    view! {
                        <button
                            type="button"
                            class="columns-picker-icon"
                            aria-label=t_string!(i18n, analyzer_columns_remove, column = label.clone()).to_string()
                            on:click=move |_| toggle(id)
                        >
                            <Icon icon=i::MdiClose />
                        </button>
                    }
                });
                let title = label.clone();
                view! {
                    <li class="columns-picker-row" data-shown-column=id>
                        <span class="columns-picker-name" title=title>{label}</span>
                        {window}
                        {remove}
                    </li>
                }
            })
            .collect_view()
    };

    let add_list = move || {
        let visible = visible_columns.get();
        let query = search.get();
        let rows = add_rows(&columns.get(), &visible, &query);
        if rows.is_empty() {
            let empty = if query.trim().is_empty() {
                t_string!(i18n, analyzer_columns_all_shown).to_string()
            } else {
                t_string!(i18n, analyzer_columns_empty).to_string()
            };
            return view! { <li class="columns-picker-empty" role="status">{empty}</li> }
                .into_any();
        }
        let mut out: Vec<AnyView> = Vec::new();
        let mut last_heading: Option<String> = None;
        for row in rows {
            if let Some(heading) = row.group()
                && last_heading.as_deref() != Some(heading.label.as_str())
            {
                last_heading = Some(heading.label.clone());
                out.push(picker_heading(heading));
            }
            out.push(match row {
                AddRow::Single(column) => {
                    let id = column.id;
                    let add = t_string!(i18n, analyzer_columns_add_one, column = column.label.clone()).to_string();
                    view! {
                        <li class="columns-picker-row" data-add-column=id title=column.hint.clone()>
                            <span class="columns-picker-name" class:opacity-60=column.disabled>{column.label}</span>
                            <button
                                type="button"
                                class="columns-picker-icon"
                                aria-label=add
                                disabled=column.disabled
                                on:click=move |_| toggle(id)
                            >
                                <Icon icon=i::MdiPlus />
                            </button>
                        </li>
                    }
                    .into_any()
                }
                AddRow::Family(variants) => {
                    let key = variants[0].id;
                    let family = variants[0].family.clone().unwrap_or_default();
                    let open: Vec<&'static str> = variants
                        .iter()
                        .filter(|v| !visible.contains(v.id) && !v.disabled)
                        .map(|v| v.id)
                        .collect();
                    let first_open = open.first().copied();
                    let open = StoredValue::new(open);
                    // The pick survives until its window goes on screen;
                    // then the row falls back to the first one still hidden.
                    let chosen = move || {
                        picked
                            .get()
                            .get(key)
                            .copied()
                            .filter(|id| open.with_value(|open| open.contains(id)))
                            .or(first_open)
                    };
                    let add = t_string!(i18n, analyzer_columns_add_one, column = family.clone()).to_string();
                    let windows = variants
                        .into_iter()
                        .map(|v| {
                            let id = v.id;
                            let taken = !open.with_value(|open| open.contains(&id));
                            view! {
                                <button
                                    type="button"
                                    class="column-window"
                                    aria-pressed=move || (chosen() == Some(id)).to_string()
                                    disabled=taken
                                    title=v.hint.clone()
                                    on:click=move |_| picked.update(|picked| {
                                        picked.insert(key, id);
                                    })
                                >
                                    {v.variant.clone().unwrap_or(v.label)}
                                </button>
                            }
                        })
                        .collect_view();
                    view! {
                        <li class="columns-picker-row" data-add-family=key>
                            <span class="columns-picker-name" title=family.clone()>{family.clone()}</span>
                            <span class="column-windows" role="group" aria-label=family>{windows}</span>
                            <button
                                type="button"
                                class="columns-picker-icon"
                                aria-label=add
                                disabled=move || chosen().is_none()
                                on:click=move |_| {
                                    if let Some(id) = chosen() {
                                        toggle(id);
                                    }
                                }
                            >
                                <Icon icon=i::MdiPlus />
                            </button>
                        </li>
                    }
                    .into_any()
                }
            });
        }
        out.into_any()
    };

    view! {
        <h3 class="columns-picker-section">{t!(i18n, analyzer_columns_showing)}</h3>
        <ul class="columns-picker-list" data-columns-shown>{shown_rows}</ul>
        <h3 class="columns-picker-section">{t!(i18n, analyzer_columns_add)}</h3>
        <input
            type="search"
            class="input min-w-0 px-2 py-1"
            aria-label=t_string!(i18n, analyzer_columns_search)
            placeholder=t_string!(i18n, analyzer_columns_search)
            prop:value=move || search.get()
            on:input=move |event| search.set(event_target_value(&event))
        />
        <ul class="columns-picker-list" data-columns-add>{add_list}</ul>
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
    // A page with a registered grid (explicit options or not) lists what that
    // grid draws, in its order; one without lists its ticked options.
    let shown_columns = Signal::derive(move || {
        let displayed = registry
            .map(|registry| registry.displayed_columns())
            .unwrap_or_default();
        if !displayed.is_empty() {
            return shown_columns_from(&displayed);
        }
        let visible = picker_visible.get();
        picker_columns
            .get()
            .into_iter()
            .filter(|option| visible.contains(option.id))
            .map(|option| ShownColumn {
                id: option.id,
                label: option.family.unwrap_or(option.label),
                removable: true,
            })
            .collect()
    });
    let swap_column =
        Callback::new(
            move |(shown, hidden): (&'static str, &'static str)| match registry {
                Some(registry) => registry.swap_columns(shown, hidden),
                None => {
                    toggle_column.run(hidden);
                    toggle_column.run(shown);
                }
            },
        );
    let has_columns = Signal::derive(move || !picker_columns.get().is_empty());
    let column_search = RwSignal::new(String::new());

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
                                            column_search.set(String::new());
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
                            nav(&format!("{}{}{}", location.pathname.get_untracked(), _q.to_query_string(), location.hash.get_untracked()), leptos_router::NavigateOptions { replace: true, scroll: false, ..Default::default() });
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
                            <div class="sticky-bar-popover columns-picker p-3 w-[min(92vw,32rem)] max-h-[70vh] overflow-y-auto flex flex-col gap-2 text-sm">
                                <ColumnsPicker
                                    columns=picker_columns
                                    shown=shown_columns
                                    visible_columns=picker_visible
                                    search=column_search
                                    on_toggle_column=Some(toggle_column)
                                    on_swap_column=Some(swap_column)
                                />
                                {move || {
                                    reset_columns
                                        .get()
                                        .map(|reset| {
                                            view! {
                                                <button
                                                    class="self-end text-xs text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
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
    use ultros_grid_core::columns::ColumnQuery;

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

    #[test]
    fn registered_picker_preserves_availability_and_explanations() {
        let mut column = GridColumn::new("median", "Median".into(), 100.0, true, false);
        column.picker_label = Some("Sale median (7d) · follows window".into());
        column.picker_group = Some("Sale history".into());
        column.picker_group_title = Some("Gilgamesh sale history".into());
        column.picker_hint = Some("Unavailable for this market".into());
        column.picker_disabled = true;
        let options = picker_options_from(&[column]);
        assert_eq!(options[0].id, "median");
        assert_eq!(options[0].label, "Sale median (7d) · follows window");
        assert!(options[0].disabled);
        assert_eq!(
            options[0].hint.as_deref(),
            Some("Unavailable for this market")
        );
        assert_eq!(
            options[0].group.as_ref().unwrap().title.as_deref(),
            Some("Gilgamesh sale history")
        );
    }

    #[test]
    fn search_matches_all_words_across_metadata_and_keeps_identity_and_order() {
        let columns = vec![
            ColumnOption::new("profit", "Profit".into()),
            ColumnOption {
                group: Some(PickerHeading {
                    label: "Sale history (7d)".into(),
                    title: Some("Gilgamesh".into()),
                }),
                hint: Some("Always uses a fixed window".into()),
                disabled: true,
                ..ColumnOption::new("market-sale-median-7", "Sale median (7d)".into())
            },
            ColumnOption {
                hint: Some("Follows the selected window".into()),
                ..ColumnOption::new("market-sale-median", "Sale median (7d)".into())
            },
        ];
        assert_eq!(search_column_options(&columns, " \t "), columns);
        assert_eq!(
            search_column_options(&columns, "MEDIAN fixed"),
            vec![columns[1].clone()]
        );
        assert_eq!(
            search_column_options(&columns, "gilgamesh history"),
            vec![columns[1].clone()]
        );
        assert_eq!(search_column_options(&columns, "median"), columns[1..]);
        assert!(search_column_options(&columns, "profit median").is_empty());
    }

    fn heading(label: &str) -> Option<PickerHeading> {
        Some(PickerHeading {
            label: label.into(),
            title: None,
        })
    }

    fn stat(id: &'static str, family: &str, variant: &str) -> ColumnOption {
        ColumnOption {
            group: heading(&format!("Sale history ({variant})")),
            family_group: heading("Sale history"),
            family: Some(family.into()),
            variant: Some(variant.into()),
            ..ColumnOption::new(id, format!("{family} ({variant})"))
        }
    }

    fn picker_options() -> Vec<ColumnOption> {
        vec![
            ColumnOption::new("profit", "Profit".into()),
            ColumnOption::new("level", "Level".into()),
            stat("median", "Sale median", "Selected"),
            stat("median-7", "Sale median", "7d"),
            stat("median-30", "Sale median", "30d"),
            stat("spd", "Sales/day", "Selected"),
            stat("spd-7", "Sales/day", "7d"),
            ColumnOption {
                group: Some(PickerHeading {
                    label: "Listings".into(),
                    title: None,
                }),
                ..ColumnOption::new("alive", "Active listings".into())
            },
        ]
    }

    fn row_ids(rows: &[AddRow]) -> Vec<Vec<&'static str>> {
        rows.iter()
            .map(|row| match row {
                AddRow::Single(column) => vec![column.id],
                AddRow::Family(variants) => variants.iter().map(|v| v.id).collect(),
            })
            .collect()
    }

    /// Each statistic is one row carrying all its windows; plain columns
    /// stay single rows; anything on screen leaves the list.
    #[test]
    fn add_rows_fold_windows_into_one_row_per_statistic() {
        let options = picker_options();
        let rows = add_rows(&options, &HashSet::from(["profit"]), "");
        assert_eq!(
            row_ids(&rows),
            vec![
                vec!["level"],
                vec!["median", "median-7", "median-30"],
                vec!["spd", "spd-7"],
                vec!["alive"],
            ]
        );
        // A statistic with one window still hidden keeps its row, with every
        // window listed so the shown one reads as taken; with none hidden it
        // goes.
        let visible = HashSet::from(["median", "median-7", "spd", "spd-7"]);
        assert_eq!(
            row_ids(&add_rows(&options, &visible, "")),
            vec![
                vec!["profit"],
                vec!["level"],
                vec!["median", "median-7", "median-30"],
                vec!["alive"],
            ]
        );
    }

    /// Search matches a statistic by its name or any one window, and the
    /// row it keeps still offers every window.
    #[test]
    fn add_rows_search_keeps_the_whole_statistic() {
        let options = picker_options();
        let none = HashSet::new();
        assert_eq!(
            row_ids(&add_rows(&options, &none, "median 30d")),
            vec![vec!["median", "median-7", "median-30"]]
        );
        assert_eq!(
            row_ids(&add_rows(&options, &none, "sales/day")),
            vec![vec!["spd", "spd-7"]]
        );
        assert!(add_rows(&options, &none, "nothing like this").is_empty());
        // The same statistic under two groups (two markets) is two rows.
        let mut two = picker_options();
        two.push(ColumnOption {
            family_group: heading("Cost · Aether"),
            ..stat("cost-median-7", "Sale median", "7d")
        });
        assert_eq!(
            row_ids(&add_rows(&two, &none, "median")),
            vec![
                vec!["median", "median-7", "median-30"],
                vec!["cost-median-7"]
            ]
        );
    }

    /// The shown list names a statistic by its family and never offers to
    /// remove a required column.
    #[test]
    fn shown_columns_name_statistics_and_lock_required_columns() {
        let mut median = GridColumn::new("median-7", "Sale median (7d)".into(), 100.0, true, true);
        median.picker_family = Some("Sale median".into());
        median.picker_variant = Some("7d".into());
        let mut profit = GridColumn::new("profit", "Profit".into(), 100.0, true, true);
        profit.picker_label = Some("Profit per craft".into());
        let shown = shown_columns_from(&[
            GridColumn::new("item", "Item".into(), 300.0, false, true),
            median,
            profit,
        ]);
        assert_eq!(
            shown,
            vec![
                ShownColumn {
                    id: "item",
                    label: "Item".into(),
                    removable: false
                },
                ShownColumn {
                    id: "median-7",
                    label: "Sale median".into(),
                    removable: true
                },
                ShownColumn {
                    id: "profit",
                    label: "Profit per craft".into(),
                    removable: true
                },
            ]
        );
    }

    fn render_picker(options: Vec<ColumnOption>, shown: Vec<ShownColumn>) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let visible: HashSet<&'static str> = shown.iter().map(|s| s.id).collect();
            view! {
                <ColumnsPicker
                    columns=Signal::derive(move || options.clone())
                    shown=Signal::derive(move || shown.clone())
                    visible_columns=Signal::derive(move || visible.clone())
                    search=RwSignal::new(String::new())
                    on_toggle_column=None
                    on_swap_column=None
                />
            }
            .to_html()
        })
    }

    #[test]
    fn picker_lists_shown_columns_then_one_add_row_per_statistic() {
        let html = render_picker(
            picker_options(),
            vec![
                ShownColumn {
                    id: "item",
                    label: "Item".into(),
                    removable: false,
                },
                ShownColumn {
                    id: "median-7",
                    label: "Sale median".into(),
                    removable: true,
                },
            ],
        );
        let at = |needle: &str| {
            html.find(needle)
                .unwrap_or_else(|| panic!("{needle}: {html}"))
        };
        // Showing: the required column has no ×, the statistic has its
        // window pill and an ×.
        assert!(at("data-shown-column=\"item\"") < at("data-shown-column=\"median-7\""));
        assert_eq!(html.matches("aria-label=\"Remove").count(), 1, "{html}");
        assert!(html.contains("Remove Sale median"), "{html}");
        assert!(html.contains("Change the window for Sale median"), "{html}");
        // Add: one row per statistic, the shown window taken, the first
        // hidden one picked.
        assert_eq!(html.matches("data-add-family=").count(), 2, "{html}");
        let family = &html[at("data-add-family=\"median\"")..at("data-add-family=\"spd\"")];
        assert_eq!(
            family.matches("class=\"column-window\"").count(),
            3,
            "{family}"
        );
        assert_eq!(family.matches("disabled").count(), 1, "{family}");
        assert_eq!(
            family.matches("aria-pressed=\"true\"").count(),
            1,
            "{family}"
        );
        assert!(at("data-shown-column=\"median-7\"") < at("data-add-column=\"profit\""));
        // Group headings appear once, before their rows.
        assert_eq!(html.matches(">Sale history<").count(), 1, "{html}");
        assert!(at(">Sale history<") < at("data-add-family=\"median\""));
        assert!(at(">Listings<") < at("data-add-column=\"alive\""));
    }

    #[test]
    fn picker_says_when_everything_is_showing() {
        let options = vec![ColumnOption::new("profit", "Profit".into())];
        let html = render_picker(
            options,
            vec![ShownColumn {
                id: "profit",
                label: "Profit".into(),
                removable: true,
            }],
        );
        assert!(!html.contains("data-add-"), "{html}");
        assert!(html.contains("role=\"status\""), "{html}");
    }

    /// An unavailable column is offered greyed out with its reason, never
    /// as addable.
    #[test]
    fn picker_disables_unavailable_columns_with_their_reason() {
        let options = vec![ColumnOption {
            disabled: true,
            hint: Some("capped".into()),
            ..ColumnOption::new("cost", "Cost".into())
        }];
        let html = render_picker(options, Vec::new());
        assert!(html.contains("title=\"capped\""), "{html}");
        assert!(html.contains("disabled"), "{html}");
    }

    #[test]
    fn test_parse_visible_cols() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let all_cols: &[&str] = &["col1", "col2", "col3"];
            let default_cols: &[&str] = &["col1", "col2"];

            // None returns default
            let parsed = parse_visible_cols(ColumnQuery::default(), all_cols, default_cols);
            assert_eq!(parsed.len(), 2);
            assert!(parsed.contains("col1"));
            assert!(parsed.contains("col2"));

            // Empty string returns empty set
            let parsed = parse_visible_cols(
                ColumnQuery {
                    cols: Some(""),
                    ..Default::default()
                },
                all_cols,
                default_cols,
            );
            assert!(parsed.is_empty());

            // Valid values
            let parsed = parse_visible_cols(
                ColumnQuery {
                    cols: Some("col1,col3"),
                    ..Default::default()
                },
                all_cols,
                default_cols,
            );
            assert_eq!(parsed.len(), 2);
            assert!(parsed.contains("col1"));
            assert!(parsed.contains("col3"));

            // Invalid values are filtered out
            let parsed = parse_visible_cols(
                ColumnQuery {
                    cols: Some("col1,unknown,col3,invalid"),
                    ..Default::default()
                },
                all_cols,
                default_cols,
            );
            assert_eq!(parsed.len(), 2);
            assert!(parsed.contains("col1"));
            assert!(parsed.contains("col3"));
        });
    }

    #[test]
    fn test_serialize_visible_cols() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let all_cols: &[&str] = &["col1", "col2", "col3", "col4"];

            let mut visible = HashSet::new();
            visible.insert("col3");
            visible.insert("col1");

            // Serialized string should maintain the order from `all_cols`
            let serialized = serialize_visible_cols(&visible, all_cols);
            assert_eq!(serialized, "col1,col3");

            // Empty set
            let empty_visible = HashSet::new();
            assert_eq!(serialize_visible_cols(&empty_visible, all_cols), "");

            // All columns
            let all_visible: HashSet<&'static str> = all_cols.iter().copied().collect();
            assert_eq!(
                serialize_visible_cols(&all_visible, all_cols),
                "col1,col2,col3,col4"
            );
        });
    }
}
