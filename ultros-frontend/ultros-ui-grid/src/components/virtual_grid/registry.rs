//! Shared filter definitions for a grid, its toolbar and its column menus.
//! Provide once in the owner containing both ControlBar and QueryGrid. Controls
//! keep their URL keys and run before calculations; aliases become metric queries.
use super::{ColumnFilter, GridColumn, metrics::*};
use crate::components::app_link::use_location_or_default;
use leptos::prelude::*;
use leptos_router::params::ParamsMap;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug)]
pub struct FilterAlias {
    pub key: &'static str,
    pub column: &'static str,
    pub op: FilterOp,
    pub convert: fn(&str) -> Option<String>,
    /// The inverse of `convert`: how a stored threshold reads under `key`.
    /// [`readable_query`] only writes the alias when `convert` maps this
    /// straight back to the stored value, so a lossy spelling stays in `gf`.
    pub display: fn(&str) -> Option<String>,
}

impl FilterAlias {
    pub fn new(key: &'static str, column: &'static str, op: FilterOp) -> Self {
        Self {
            key,
            column,
            op,
            convert: |raw| {
                let raw = raw.trim();
                (!raw.is_empty()).then(|| raw.to_string())
            },
            display: |value| Some(value.to_string()),
        }
    }
    /// Preserve the parsing contract of a legacy signed-integer parameter.
    pub fn integer(key: &'static str, column: &'static str, op: FilterOp) -> Self {
        Self {
            convert: |raw| raw.parse::<i32>().ok().map(|v| v.to_string()),
            ..Self::new(key, column, op)
        }
    }

    /// Preserve the finite f32 thresholds used by existing daily-rate filters.
    pub fn decimal(key: &'static str, column: &'static str, op: FilterOp) -> Self {
        Self {
            convert: |raw| {
                raw.parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(|v| (v as f64).to_string())
            },
            // `0.1` is stored as the widened `0.10000000149011612`; the
            // shortest f32 spelling converts back to exactly that.
            display: |value| value.parse::<f64>().ok().map(|v| (v as f32).to_string()),
            ..Self::new(key, column, op)
        }
    }
}

/// A retired native `?sort=` token that now names a metric column. Old links
/// keep their order; the first header click writes the canonical `grid:` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortAlias {
    pub key: &'static str,
    pub column: &'static str,
}

impl SortAlias {
    pub const fn new(key: &'static str, column: &'static str) -> Self {
        Self { key, column }
    }
}

/// The metric column `?sort=` selects: an explicit `grid:<id>` first, then a
/// registered alias. A native token without an alias sorts nothing here.
pub fn resolve_sort(sort: Option<&str>, aliases: &[SortAlias]) -> Option<String> {
    let sort = sort?;
    if let Some(column) = sort.strip_prefix("grid:") {
        return Some(column.to_string());
    }
    aliases
        .iter()
        .find(|alias| alias.key == sort)
        .map(|alias| alias.column.to_string())
}

/// [`resolve_sort`] against the registry in context, or none when the host
/// registered nothing.
pub fn effective_sort(query: &ParamsMap) -> Option<String> {
    let sort = query.get("sort");
    match use_context::<FilterRegistry>() {
        Some(registry) => registry.sort_column(sort.as_deref()),
        None => resolve_sort(sort.as_deref(), &[]),
    }
}

/// Read aliases only for columns absent from explicit gf, including invalid
/// explicit values. An edit canonicalizes all aliases in one URL replacement.
pub fn resolve_filters(query: &ParamsMap, aliases: &[FilterAlias]) -> MetricFilters {
    let mut filters = parse_filters(query.get("gf").as_deref());
    let explicit = filters.clone();
    for alias in aliases {
        if explicit.contains_key(alias.column) {
            continue;
        }
        let Some(value) = query.get(alias.key).and_then(|v| (alias.convert)(&v)) else {
            continue;
        };
        let next = MetricFilter {
            op: alias.op,
            value,
        };
        if let Some(previous) = filters.get(alias.column) {
            let bounds = match (previous.op, next.op) {
                (FilterOp::Gte, FilterOp::Lte) => Some((&previous.value, &next.value)),
                (FilterOp::Lte, FilterOp::Gte) => Some((&next.value, &previous.value)),
                _ => None,
            };
            if let Some((low, high)) = bounds {
                filters.insert(
                    alias.column.into(),
                    MetricFilter {
                        op: FilterOp::Between,
                        value: format!("{low},{high}"),
                    },
                );
            }
        } else {
            filters.insert(alias.column.into(), next);
        }
    }
    filters
}

/// `seconds` as the humantime duration a person would type (`86400` → `1d`),
/// for aliases whose `convert` parses durations into seconds.
pub fn seconds_as_duration(seconds: &str) -> Option<String> {
    let seconds = seconds.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 || seconds.fract() != 0.0 {
        return None;
    }
    let seconds = seconds as u64;
    let (unit, size) = [("d", 86_400), ("h", 3_600), ("m", 60)]
        .into_iter()
        .find(|(_, size)| seconds != 0 && seconds.is_multiple_of(*size))
        .unwrap_or(("s", 1));
    Some(format!("{}{unit}", seconds / size))
}

pub fn clear_key(query: &mut ParamsMap, key: &str) {
    query.remove(key);
    // The empty value keeps a seeded default from returning on reload. A `v`
    // marker already stops every seed, so there it would only be URL noise.
    if matches!(key, "next-sale" | "last-sold" | "min-sales") && query.get("v").is_none() {
        query.insert(key.to_string(), String::new());
    }
}

pub fn canonical_query(query: &ParamsMap, aliases: &[FilterAlias]) -> ParamsMap {
    let mut next = query.clone();
    let filters = resolve_filters(query, aliases);
    for alias in aliases {
        clear_key(&mut next, alias.key);
    }
    write_filters(&mut next, &filters);
    next
}

/// `query` as a person should read it: every filter an alias states exactly
/// moves out of the `gf` JSON into its alias key (`min-buy=5000`,
/// `last-sold=1d`), and only the rest stay packed in `gf`.
///
/// The inverse of [`canonical_query`]: editors work on the canonical form,
/// where every filter is in `gf`, and hand the result through this before it
/// reaches the address bar. [`resolve_filters`] reads both forms the same.
pub fn readable_query(query: &ParamsMap, aliases: &[FilterAlias]) -> ParamsMap {
    let mut next = query.clone();
    for alias in aliases {
        next.remove(alias.key);
    }
    let mut packed = MetricFilters::new();
    for (column, filter) in resolve_filters(query, aliases) {
        match alias_values(&column, &filter, aliases) {
            Some(values) => {
                for (key, value) in values {
                    // `insert` decodes its input.
                    next.insert(key, leptos_router::location::Url::escape(&value));
                }
            }
            None => {
                packed.insert(column, filter);
            }
        }
    }
    for alias in aliases {
        if next.get(alias.key).is_none() && query.get(alias.key).is_some() {
            clear_key(&mut next, alias.key);
        }
    }
    write_filters(&mut next, &packed);
    next
}

/// The alias keys and values that state `filter` exactly, or `None` when any
/// part of it has no alias (it then stays in `gf` whole). A two-sided range
/// needs both a `Gte` and an `Lte` alias; the reader merges them back.
fn alias_values(
    column: &str,
    filter: &MetricFilter,
    aliases: &[FilterAlias],
) -> Option<Vec<(&'static str, String)>> {
    let find = |op| aliases.iter().find(|a| a.column == column && a.op == op);
    let spell = |alias: &FilterAlias, value: &str| {
        let shown = (alias.display)(value)?;
        ((alias.convert)(&shown).as_deref() == Some(value)).then_some((alias.key, shown))
    };
    match filter.op {
        FilterOp::Between | FilterOp::Range => {
            let (low, high) = filter.range_sides()?;
            let mut values = Vec::new();
            if let Some(low) = low {
                values.push(spell(find(FilterOp::Gte)?, low)?);
            }
            if let Some(high) = high {
                values.push(spell(find(FilterOp::Lte)?, high)?);
            }
            (!values.is_empty()).then_some(values)
        }
        op => Some(vec![spell(find(op)?, &filter.value)?]),
    }
}

pub fn write_filters(query: &mut ParamsMap, filters: &MetricFilters) {
    query.remove("gf");
    if !filters.is_empty() {
        query.insert("gf", serde_json::to_string(filters).unwrap_or_default());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredFilter {
    pub filter: ColumnFilter,
    pub group: Option<String>,
}

/// The grid's column-visibility commands, registered beside its resolved
/// columns so a toolbar picker flips the same `?cols=` the header menu does.
#[derive(Clone, Copy)]
pub struct ColumnVisibility {
    /// Show or hide one optional column, leaving the layout delta untouched.
    pub set_visible: Callback<(&'static str, bool)>,
    /// Hide the first column and show the second in one URL write, the
    /// second taking over the first's dragged position and width.
    pub swap: Callback<(&'static str, &'static str)>,
    /// Drop the column keys so every optional column returns to its page
    /// default.
    pub reset: Callback<()>,
}

/// Replace the column keys of `query` with where `columns` departs from its
/// defaults (`show-cols`/`hide-cols`, see [`ultros_grid_core::columns`]), in
/// definition order so the URL is stable regardless of toggle order. Shared
/// by the header menu and the toolbar picker.
pub fn write_columns(query: &mut ParamsMap, columns: &[GridColumn]) {
    use ultros_grid_core::columns::{COLUMN_KEYS, HIDE_COLS, SHOW_COLS, column_departures};
    for key in COLUMN_KEYS {
        query.remove(key);
    }
    let (show, hide) = column_departures(
        columns
            .iter()
            .filter(|c| c.optional)
            .map(|c| (c.id, c.default_visible, c.visible)),
    );
    if let Some(show) = show {
        query.insert(SHOW_COLS, show);
    }
    if let Some(hide) = hide {
        query.insert(HIDE_COLS, hide);
    }
}

/// The column keys of `query`.
pub fn column_query(query: &ParamsMap) -> ultros_grid_core::columns::ColumnQuery<'_> {
    use ultros_grid_core::columns::{HIDE_COLS, LEGACY_COLS, SHOW_COLS};
    ultros_grid_core::columns::ColumnQuery {
        cols: query.get_str(LEGACY_COLS),
        show: query.get_str(SHOW_COLS),
        hide: query.get_str(HIDE_COLS),
    }
}

/// Only column identity and ordering policy belong in query resolution.
/// Labels, picker hints and widths can change with a market window without
/// invalidating the row query that reads that window's resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SortColumn {
    id: &'static str,
    native_sort: Option<super::layout::NativeSort>,
    active: bool,
}

#[derive(Clone, Copy)]
pub struct FilterRegistry {
    aliases: StoredValue<Vec<FilterAlias>>,
    sort_aliases: StoredValue<Vec<SortAlias>>,
    default_sort: StoredValue<Option<&'static str>>,
    controls: Signal<Vec<ColumnFilter>>,
    columns: RwSignal<Option<Signal<Vec<GridColumn>>>>,
    sort_columns: RwSignal<Option<Memo<Vec<SortColumn>>>>,
    sortable_columns: StoredValue<HashSet<&'static str>>,
    visibility: RwSignal<Option<ColumnVisibility>>,
    layout: RwSignal<Option<Signal<Option<String>>>>,
    pub editing: RwSignal<Option<ColumnFilter>>,
    count: RwSignal<Option<Signal<usize>>>,
}

impl FilterRegistry {
    pub fn provide(aliases: Vec<FilterAlias>, controls: Signal<Vec<ColumnFilter>>) -> Self {
        let registry = Self {
            aliases: StoredValue::new(aliases),
            sort_aliases: StoredValue::new(Vec::new()),
            default_sort: StoredValue::new(None),
            controls,
            columns: RwSignal::new(None),
            sort_columns: RwSignal::new(None),
            sortable_columns: StoredValue::new(HashSet::new()),
            visibility: RwSignal::new(None),
            layout: RwSignal::new(None),
            editing: RwSignal::new(None),
            count: RwSignal::new(None),
        };
        provide_context(registry);
        registry
    }

    pub fn register_count(self, count: Signal<usize>) {
        self.count.set(Some(count));
    }

    /// Route inputs independent of the grid's resolved columns. Formula
    /// headings may read these while those columns are being computed.
    pub fn controls(self) -> Vec<ColumnFilter> {
        self.controls.get()
    }

    pub fn row_count(self) -> usize {
        self.count
            .get()
            .map(|count| count.get())
            .unwrap_or_default()
    }

    pub fn register(self, columns: Signal<Vec<GridColumn>>) {
        self.columns.set(Some(columns));
    }

    /// Register query metadata before resolving query state. The projection
    /// shields queries from presentation-only changes to complete definitions;
    /// using the resolved columns here would create a dependency cycle.
    pub fn register_sort_columns(
        self,
        columns: Signal<Vec<GridColumn>>,
        sortable: HashSet<&'static str>,
    ) {
        self.sortable_columns.set_value(sortable);
        let schema = Memo::new(move |_| {
            columns.with(|columns| {
                columns
                    .iter()
                    .map(|column| SortColumn {
                        id: column.id,
                        native_sort: column.native_sort,
                        active: column.aria_sort != "none",
                    })
                    .collect()
            })
        });
        self.sort_columns.set(Some(schema));
    }

    pub fn native_column(self, token: &str) -> Option<&'static str> {
        self.sort_columns.get()?.with(|columns| {
            columns
                .iter()
                .find(|column| {
                    column.native_sort.is_some_and(|sort| sort.token == token)
                        && self
                            .sortable_columns
                            .with_value(|ids| ids.contains(column.id))
                })
                .map(|column| column.id)
        })
    }

    pub fn default_ascending(self, id: &str) -> bool {
        self.sort_columns.get().is_some_and(|columns| {
            columns.with(|columns| {
                columns
                    .iter()
                    .find(|column| column.id == id)
                    .and_then(|column| column.native_sort)
                    .is_some_and(|sort| sort.default_ascending)
            })
        })
    }

    /// An omitted definition is unavailable in this data set; a hidden
    /// definition is still a valid sort target. Before a grid registers,
    /// keep resolving URLs so page-owned loaders can discover their needs.
    fn column_defined(self, id: &str) -> bool {
        self.sort_columns.get().is_none_or(|columns| {
            columns.with(|columns| columns.iter().any(|column| column.id == id))
        })
    }

    /// Old native links omit their column's default direction. Existing
    /// `grid:` links instead default to descending; newly written links are explicit.
    pub fn sort_ascending(self, query: &ParamsMap) -> bool {
        match query.get("dir").as_deref() {
            Some("asc") => true,
            Some("desc") => false,
            _ if query
                .get("sort")
                .and_then(|sort| sort.strip_prefix("grid:").map(str::to_owned))
                .is_some_and(|column| self.column_defined(&column)) =>
            {
                false
            }
            _ => self
                .sort_column(query.get("sort").as_deref())
                .is_some_and(|column| self.default_ascending(&column)),
        }
    }

    pub fn register_visibility(self, visibility: ColumnVisibility) {
        self.visibility.set(Some(visibility));
    }

    /// Every column the grid lets the user turn on or off, in the grid's
    /// own order and carrying the labels and picker groups it resolved.
    pub fn optional_columns(self) -> Vec<GridColumn> {
        self.columns
            .get()
            .map(|columns| {
                columns.with(|defs| defs.iter().filter(|c| c.optional).cloned().collect())
            })
            .unwrap_or_default()
    }

    /// The optional columns currently on, after `?cols=` is applied.
    pub fn visible_columns(self) -> HashSet<&'static str> {
        self.columns
            .get()
            .map(|columns| {
                columns.with(|defs| {
                    defs.iter()
                        .filter(|c| c.optional && c.visible)
                        .map(|c| c.id)
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// Flip one optional column through the grid's own visibility command.
    /// A no-op until a grid has registered, or for an id it does not own.
    pub fn toggle_column(self, id: &'static str) {
        let Some(visibility) = self.visibility.get_untracked() else {
            return;
        };
        let Some(columns) = self.columns.get_untracked() else {
            return;
        };
        let current = columns.with_untracked(|defs| {
            defs.iter()
                .find(|c| c.id == id && c.optional)
                .map(|c| c.visible)
        });
        if let Some(visible) = current {
            visibility.set_visible.run((id, !visible));
        }
    }

    /// Replace a shown optional column with a hidden one in its place — a
    /// statistic moving to another window. A no-op for any other pair.
    pub fn swap_columns(self, shown: &'static str, hidden: &'static str) {
        let Some(visibility) = self.visibility.get_untracked() else {
            return;
        };
        let Some(columns) = self.columns.get_untracked() else {
            return;
        };
        let state = |id: &str| {
            columns.with_untracked(|defs| {
                defs.iter()
                    .find(|c| c.id == id && c.optional)
                    .map(|c| c.visible)
            })
        };
        if state(shown) == Some(true) && state(hidden) == Some(false) {
            visibility.swap.run((shown, hidden));
        }
    }

    /// The grid's layout string (`GridLayout::parse`), so the picker can list
    /// columns in the order they are drawn.
    pub fn register_layout(self, layout: Signal<Option<String>>) {
        self.layout.set(Some(layout));
    }

    /// Every column on screen, required ones included, in display order.
    pub fn displayed_columns(self) -> Vec<GridColumn> {
        let Some(columns) = self.columns.get() else {
            return Vec::new();
        };
        let layout = self.layout.get().and_then(|layout| layout.get());
        columns.with(|defs| {
            super::GridLayout::parse(layout.as_deref(), defs)
                .order
                .iter()
                .filter_map(|id| defs.iter().find(|c| c.id == id && c.visible).cloned())
                .collect()
        })
    }

    pub fn reset_columns(self) {
        if let Some(visibility) = self.visibility.get_untracked() {
            visibility.reset.run(());
        }
    }

    pub fn filters(self, query: &ParamsMap) -> MetricFilters {
        self.aliases
            .with_value(|aliases| resolve_filters(query, aliases))
    }

    pub fn canonical(self, query: &ParamsMap) -> ParamsMap {
        self.aliases
            .with_value(|aliases| canonical_query(query, aliases))
    }

    /// See [`readable_query`]. Apply to an edited canonical query right
    /// before it is navigated to.
    pub fn readable(self, query: &ParamsMap) -> ParamsMap {
        self.aliases
            .with_value(|aliases| readable_query(query, aliases))
    }

    /// Register retired native sort tokens once at setup, before any grid
    /// or header reads the URL.
    pub fn register_sort_aliases(self, aliases: Vec<SortAlias>) {
        self.sort_aliases.set_value(aliases);
    }

    /// Preserve a host's original ordering when no recognized sort is present.
    pub fn register_default_sort(self, column: &'static str) {
        self.default_sort.set_value(Some(column));
    }

    /// The metric column a raw `?sort=` value selects, aliases included.
    pub fn sort_column(self, sort: Option<&str>) -> Option<String> {
        self.sort_aliases
            .with_value(|aliases| resolve_sort(sort, aliases))
            .filter(|column| {
                !sort.is_some_and(|sort| sort.starts_with("grid:")) || self.column_defined(column)
            })
            .or_else(|| {
                sort.and_then(|token| self.native_column(token))
                    .map(str::to_owned)
            })
            .or_else(|| self.default_sort.get_value().map(str::to_owned))
            .or_else(|| {
                self.sort_columns.get().and_then(|columns| {
                    columns.with(|columns| {
                        columns
                            .iter()
                            .find(|column| {
                                column.active
                                    && column.native_sort.is_some()
                                    && self
                                        .sortable_columns
                                        .with_value(|ids| ids.contains(column.id))
                            })
                            .map(|column| column.id.to_owned())
                    })
                })
            })
    }

    pub fn is_alias(self, key: &str) -> bool {
        self.aliases
            .with_value(|aliases| aliases.iter().any(|a| a.key == key))
    }

    pub fn entries(self) -> Vec<RegisteredFilter> {
        let mut result = Vec::new();
        if let Some(columns) = self.columns.get() {
            for col in columns.get() {
                for filter in col.filters {
                    if filter.metric.is_none() && self.is_alias(filter.key) {
                        continue;
                    }
                    if !result.iter().any(|f: &RegisteredFilter| {
                        f.filter.key == filter.key && f.filter.metric == filter.metric
                    }) {
                        result.push(RegisteredFilter {
                            filter,
                            group: col.picker_group.clone(),
                        });
                    }
                }
            }
        }
        for filter in self.controls.get() {
            if !result
                .iter()
                .any(|f| f.filter.key == filter.key && f.filter.metric == filter.metric)
            {
                result.push(RegisteredFilter {
                    filter,
                    group: None,
                });
            }
        }
        result
    }

    pub fn active(self, filter: &ColumnFilter, query: &ParamsMap) -> bool {
        if filter.calculation {
            return false;
        }
        if let Some(kind) = filter.metric {
            self.filters(query)
                .get(filter.key)
                .is_some_and(|f| f.valid(kind))
        } else {
            query
                .get(filter.key)
                .or_else(|| filter.default_value.clone())
                .is_some_and(|v| !v.is_empty())
        }
    }

    pub fn clear_all(self, query: &ParamsMap) -> ParamsMap {
        let mut next = self.canonical(query);
        next.remove("gf");
        for entry in self.entries() {
            if entry.filter.metric.is_none()
                && entry.filter.clear_with_filters
                && !entry.filter.calculation
            {
                clear_key(&mut next, entry.filter.key);
            }
        }
        self.readable(&next)
    }
}

pub fn effective_filters(query: &ParamsMap) -> MetricFilters {
    use_context::<FilterRegistry>()
        .map(|r| r.filters(query))
        .unwrap_or_else(|| parse_filters(query.get("gf").as_deref()))
}

fn choice_label(filter: &ColumnFilter, token: &str) -> String {
    filter
        .choices
        .iter()
        .find(|(key, _)| key == token)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| {
            crate::components::filter_chip::option_label(Some(&filter.options), token.to_string())
        })
}

#[component]
pub fn RegisteredFilterChips(registry: FilterRegistry) -> impl IntoView {
    let location = use_location_or_default();
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    use crate::i18n::*;
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let remove = Callback::new(move |filter: ColumnFilter| {
        let query = registry.canonical(&location.query.get_untracked());
        let _next = registry.readable(&super::filter::cleared_query(&query, &[filter]));
        registry.editing.set(None);
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}{}",
                location.pathname.get_untracked(),
                _next.to_query_string(),
                location.hash.get_untracked()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });
    move || {
        let query = location.query.get();
        registry
            .entries()
            .into_iter()
            .filter(|entry| registry.active(&entry.filter, &query))
            .map(|entry| {
                let filter = entry.filter;
                let value = if filter.metric.is_some() {
                    registry.filters(&query).get(filter.key).map(|f| {
                        if let Some(range) = super::filter::range_chip(f, filter.unit) {
                            return range;
                        }
                        let value = if matches!(f.op, FilterOp::Eq | FilterOp::Ne) {
                            choice_label(&filter, &f.value)
                        } else {
                            f.value.clone()
                        };
                        format!("{} {}", super::filter::operator_label(f.op), value)
                    }).unwrap_or_default()
                } else {
                    let raw = query.get(filter.key)
                        .or_else(|| filter.default_value.clone()).unwrap_or_default();
                    let label = |token: &str| choice_label(&filter, token);
                    if filter.multiple {
                        raw.split(',').map(label).collect::<Vec<_>>().join(", ")
                    } else {
                        label(&raw)
                    }
                };
                let edit = filter.clone();
                let key = filter.key;
                let label = filter.label.clone();
                let removable = filter.default_value.is_none() || query.get(filter.key).is_some();
                view! {
                    <span class="filter-chip" data-registered-filter=key>
                        <button class="filter-chip-value"
                            on:click=move |_| registry.editing.set(Some(edit.clone()))>
                            {label} " " {value}
                        </button>
                        {removable.then(|| view! {
                            <button class="filter-chip-x" aria-label=t_string!(i18n, aria_remove_filter)
                                on:click=move |_| remove.run(filter.clone())>"×"</button>
                        })}
                    </span>
                }
            }).collect_view()
    }
}

#[component]
pub fn RegisteredFilterMenu(registry: FilterRegistry, on_select: Callback<()>) -> impl IntoView {
    let query = use_location_or_default().query;
    move || {
        let query = query.get();
        let mut previous_group = None;
        let mut entries: Vec<_> = registry
            .entries()
            .into_iter()
            .filter(|e| !e.filter.calculation && !registry.active(&e.filter, &query))
            .collect();
        // A host's column table may interleave groups; the picker shows each
        // heading once, so the menu keeps each group together too. Ungrouped
        // controls lead, then groups in first-appearance order (stable sort).
        let mut order: Vec<Option<String>> = vec![None];
        for entry in &entries {
            if !order.contains(&entry.group) {
                order.push(entry.group.clone());
            }
        }
        entries.sort_by_key(|entry| order.iter().position(|g| *g == entry.group).unwrap_or(0));
        entries
            .into_iter()
            .map(|entry| {
                let heading = if entry.group != previous_group {
                    entry.group.clone()
                } else {
                    None
                };
                previous_group = entry.group;
                let filter = entry.filter;
                let label = filter.label.clone();
                view! {
                    {heading.map(|heading| view! { <strong class="pt-2">{heading}</strong> })}
                    <button class="text-left px-2 py-1 rounded-sm hover:bg-white/10"
                        data-add-filter=filter.key
                        on:click=move |_| {
                            registry.editing.set(Some(filter.clone()));
                            on_select.run(());
                        }>{label}</button>
                }
            })
            .collect_view()
    }
}

#[component]
pub fn RegisteredFilterEditor(registry: FilterRegistry) -> impl IntoView {
    use crate::i18n::*;
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    move || {
        registry.editing.get().map(|filter| view! {
            <div class="sticky-bar-popover grid-menu-popover p-3 w-[min(92vw,20rem)]" data-registered-editor>
                <div class="grid-menu-header">
                    <strong>{filter.label.clone()}</strong>
                    <button type="button" class="grid-icon-btn" title=t_string!(i18n, grid_close) aria-label=t_string!(i18n, grid_close)
                        on:click=move |_| registry.editing.set(None)>
                        <crate::components::icon::Icon icon=icondata::MdiClose aria_hidden=true/>
                    </button>
                </div>
                <super::filter::ColumnFilterEditor filter=filter.clone()/>
            </div>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_updates_do_not_invalidate_sort_resolution() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            let columns = RwSignal::new(vec![
                GridColumn::new("units", "Units (30d)".into(), 120.0, true, true)
                    .native_sort("units", false)
                    .sorted(true, false),
                GridColumn::new("price", "Price".into(), 100.0, true, false)
                    .native_sort("price", true),
            ]);
            registry.register_sort_columns(columns.into(), HashSet::from(["units", "price"]));
            let reads = Arc::new(AtomicUsize::new(0));
            let sort_reads = reads.clone();
            let resolved = Memo::new(move |_| {
                sort_reads.fetch_add(1, Ordering::Relaxed);
                (
                    registry.sort_column(None),
                    registry.sort_ascending(&ParamsMap::new()),
                )
            });
            assert_eq!(resolved.get(), (Some("units".into()), false));
            assert_eq!(reads.load(Ordering::Relaxed), 1);

            // Switching windows updates copy and geometry, not sort policy.
            columns.update(|columns| {
                columns[0].label = "Units (7d)".into();
                columns[0].picker_hint = Some("Seven-day activity".into());
                columns[0].width = 160.0;
                columns[0].visible = false;
            });
            assert_eq!(resolved.get(), (Some("units".into()), false));
            assert_eq!(reads.load(Ordering::Relaxed), 1);
            assert_eq!(registry.native_column("units"), Some("units"));

            // Availability and fallback changes still invalidate the policy.
            columns.update(|columns| {
                columns.remove(0);
                columns[0].aria_sort = "ascending";
            });
            assert_eq!(resolved.get(), (Some("price".into()), true));
            assert_eq!(reads.load(Ordering::Relaxed), 2);
            assert_eq!(registry.native_column("units"), None);
            assert_eq!(
                registry.sort_column(Some("grid:units")),
                Some("price".into())
            );
        });
    }

    #[test]
    fn hidden_native_columns_and_menu_links_share_query_order_and_direction() {
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            // The cheap-first column is hidden: no header can register it.
            let columns = vec![
                GridColumn::new("profit", "Profit".into(), 100.0, true, true)
                    .native_sort("profit", false)
                    .sorted(true, false),
                GridColumn::new("buy-price", "Cost".into(), 100.0, true, false)
                    .native_sort("cost", true),
            ];
            registry.register_sort_columns(
                Signal::derive(move || columns.clone()),
                HashSet::from(["profit", "buy-price"]),
            );
            assert_eq!(registry.native_column("cost"), Some("buy-price"));
            assert_eq!(registry.sort_column(None).as_deref(), Some("profit"));
            assert_eq!(
                registry.sort_column(Some("invalid")).as_deref(),
                Some("profit")
            );
            let legacy = params(&[("sort", "cost")]);
            let menu = params(&[("sort", "grid:buy-price"), ("dir", "asc")]);
            let rows = vec![30, 10, 20];
            let metric = vec![GridMetric::number("buy-price", |value: &i32| {
                GridValue::Number(*value as f64)
            })];
            for query in [&legacy, &menu] {
                assert!(registry.sort_ascending(query));
                let result = query_rows(
                    &rows,
                    &metric,
                    &MetricFilters::new(),
                    registry
                        .sort_column(query.get("sort").as_deref())
                        .as_deref(),
                    registry.sort_ascending(query),
                );
                assert_eq!(result.rows.unwrap(), [10, 20, 30]);
            }
            // Existing metric URLs without a direction keep their old meaning.
            assert!(!registry.sort_ascending(&params(&[("sort", "grid:buy-price")])));
            assert!(!registry.sort_ascending(&params(&[("sort", "cost"), ("dir", "desc")])));
        });
    }

    #[test]
    fn partial_native_descriptors_cannot_enable_global_sorting() {
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            registry.register_sort_columns(
                Signal::derive(|| {
                    vec![
                        GridColumn::new("trend", "Trend".into(), 100.0, true, false)
                            .native_sort("trend", false),
                    ]
                }),
                HashSet::new(),
            );
            assert_eq!(registry.native_column("trend"), None);
            assert_eq!(registry.sort_column(Some("trend")), None);
            // An explicit partial metric is still resolved here. QueryGrid
            // excludes it from sorting rather than silently choosing another.
            assert_eq!(
                registry.sort_column(Some("grid:trend")).as_deref(),
                Some("trend")
            );
        });
    }

    #[test]
    fn unavailable_grid_columns_fall_back_but_hidden_columns_remain_valid() {
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            registry.register_sort_columns(
                Signal::derive(|| {
                    vec![
                        GridColumn::new("item", "Name".into(), 300.0, false, true)
                            .native_sort("name", true)
                            .sorted(true, true),
                        GridColumn::new("price", "Price".into(), 100.0, true, false)
                            .native_sort("price", true),
                    ]
                }),
                HashSet::from(["item", "price", "ilvl"]),
            );
            assert_eq!(
                registry.sort_column(Some("grid:price")).as_deref(),
                Some("price")
            );
            assert_eq!(
                registry.sort_column(Some("grid:ilvl")).as_deref(),
                Some("item")
            );
            assert!(registry.sort_ascending(&params(&[("sort", "grid:ilvl")])));
            // Existing valid grid URLs still retain descending as their
            // implicit direction, even if the native column prefers ascending.
            assert!(!registry.sort_ascending(&params(&[("sort", "grid:price")])));
            assert!(!registry.sort_ascending(&params(&[("sort", "grid:ilvl"), ("dir", "desc")])));
        });
    }
    #[test]
    fn calculation_inputs_never_become_row_filters_or_get_cleared_with_them() {
        let owner = Owner::new();
        owner.with(|| {
            let mut basis = ColumnFilter::new("revenue", "Sale estimate".into(), false);
            basis.default_value = Some("listing-min".into());
            basis.calculation = true;
            let control = basis.clone();
            let registry =
                FilterRegistry::provide(aliases(), Signal::derive(move || vec![control.clone()]));
            let query = params(&[
                ("revenue", "sale-median"),
                ("min-buy", "100"),
                ("window", "30"),
            ]);
            assert!(!registry.active(&basis, &ParamsMap::new()));
            assert!(!registry.active(&basis, &query));
            assert_eq!(registry.controls(), vec![basis.clone()]);
            let cleared = registry.clear_all(&query);
            assert!(registry.filters(&cleared).is_empty());
            assert_eq!(cleared.get("revenue").as_deref(), Some("sale-median"));
            assert_eq!(cleared.get("window").as_deref(), Some("30"));
            assert_eq!(super::super::filter::cleared_query(&query, &[basis]), query);
        });
    }
    fn params(pairs: &[(&str, &str)]) -> ParamsMap {
        let mut query = ParamsMap::new();
        for (key, value) in pairs {
            query.insert(key.to_string(), value.to_string());
        }
        query
    }
    fn aliases() -> Vec<FilterAlias> {
        vec![
            FilterAlias::new("min-buy", "buy_price", FilterOp::Gte),
            FilterAlias::new("max-price", "buy_price", FilterOp::Lte),
            FilterAlias::new("min-sales", "daily-sales", FilterOp::Gte),
        ]
    }
    #[test]
    fn visible_calculation_defaults_and_clear_policy_preserve_price_basis() {
        let owner = Owner::new();
        owner.with(|| {
            let mut basis = ColumnFilter::new("revenue", "Sale price".into(), false);
            basis.default_value = Some("listing-min".into());
            basis.clear_with_filters = false;
            let control = basis.clone();
            let registry =
                FilterRegistry::provide(aliases(), Signal::derive(move || vec![control.clone()]));
            assert!(registry.active(&basis, &ParamsMap::new()));
            let query = params(&[
                ("revenue", "sale-median"),
                ("min-buy", "100"),
                ("window", "30"),
            ]);
            let clear = registry.clear_all(&query);
            assert_eq!(clear.get("revenue").as_deref(), Some("sale-median"));
            assert_eq!(clear.get("window").as_deref(), Some("30"));
            assert!(registry.filters(&clear).is_empty());
            let reset = super::super::filter::cleared_query(&clear, &[basis.clone()]);
            assert!(reset.get("revenue").is_none());
            assert!(
                registry.active(&basis, &reset),
                "resetting an override restores the visible calculation default"
            );
        });
    }

    #[test]
    fn aliases_keep_legacy_numeric_parsing_and_do_not_reinterpret_invalid_values() {
        let aliases = [
            FilterAlias::integer("profit", "profit", FilterOp::Gte),
            FilterAlias::decimal("min-sales", "daily-sales", FilterOp::Gte),
        ];
        for raw in ["1.5", "2147483648", "invalid"] {
            assert!(!resolve_filters(&params(&[("profit", raw)]), &aliases).contains_key("profit"));
        }
        for raw in ["NaN", "inf", "invalid"] {
            assert!(
                !resolve_filters(&params(&[("min-sales", raw)]), &aliases)
                    .contains_key("daily-sales")
            );
        }
        let filters = resolve_filters(&params(&[("min-sales", "0.7")]), &aliases);
        assert_eq!(
            filters["daily-sales"].matches(&GridValue::Number(0.7_f32 as f64), false),
            Some(true)
        );
    }

    #[test]
    fn legacy_bounds_are_simultaneous_and_explicit_grid_state_wins() {
        let query = params(&[("min-buy", "10"), ("max-price", "20")]);
        let filters = resolve_filters(&query, &aliases());
        let bound = &filters["buy_price"];
        assert_eq!(bound.op, FilterOp::Between);
        for (price, pass) in [
            (9.0, false),
            (10.0, true),
            (15.0, true),
            (20.0, true),
            (21.0, false),
        ] {
            assert_eq!(bound.matches(&GridValue::Number(price), false), Some(pass));
        }
        let mut explicit = query;
        explicit.insert(
            "gf",
            r#"{"buy_price":{"op":"lte","value":"5"}}"#.to_string(),
        );
        assert_eq!(
            resolve_filters(&explicit, &aliases())["buy_price"].value,
            "5"
        );
        assert_eq!(
            resolve_filters(&explicit, &aliases())["buy_price"].op,
            FilterOp::Lte
        );
    }
    #[test]
    fn contradictory_legacy_bounds_still_exclude_known_rows() {
        let filters = resolve_filters(
            &params(&[("min-buy", "20"), ("max-price", "10")]),
            &aliases(),
        );
        let filter = &filters["buy_price"];
        assert!(filter.valid(ValueKind::Number));
        assert_eq!(filter.matches(&GridValue::Number(15.0), false), Some(false));
        assert_eq!(filter.matches(&GridValue::Pending, false), None);
    }

    #[test]
    fn editing_then_clearing_and_reloading_cannot_resurrect_aliases_or_defaults() {
        let query = params(&[
            ("min-buy", "10"),
            ("max-price", "20"),
            ("min-sales", "1"),
            ("tax", "false"),
            ("window", "30"),
            ("l", "2~item~"),
            ("sort", "grid:buy_price"),
        ]);
        let canonical = canonical_query(&query, &aliases());
        assert_eq!(canonical.get("min-buy"), None);
        assert_eq!(canonical.get("max-price"), None);
        assert_eq!(canonical.get("min-sales").as_deref(), Some(""));
        let cleared = super::super::filter::cleared_query(
            &canonical,
            &[
                ColumnFilter::metric("buy_price", "Price".into(), ValueKind::Number),
                ColumnFilter::metric("daily-sales", "Sales".into(), ValueKind::Number),
            ],
        );
        assert!(resolve_filters(&cleared, &aliases()).is_empty());
        assert_eq!(canonical_query(&cleared, &aliases()), cleared);
        for key in ["tax", "window", "l", "sort"] {
            assert_eq!(cleared.get(key), query.get(key));
        }
    }
    fn flip_aliases() -> Vec<FilterAlias> {
        let mut aliases = aliases();
        aliases.push(FilterAlias::integer("roi", "roi", FilterOp::Gte));
        aliases.push(FilterAlias {
            convert: |raw| {
                raw.strip_suffix('d')
                    .and_then(|days| days.parse::<u64>().ok())
                    .map(|days| (days * 86_400).to_string())
            },
            display: seconds_as_duration,
            ..FilterAlias::new("last-sold", "last_sold", FilterOp::Lte)
        });
        aliases
    }

    /// The link that prompted this: three filters packed as percent-encoded
    /// JSON, plus the empty `last-sold=` a clear left behind.
    #[test]
    fn readable_query_moves_aliased_filters_out_of_gf() {
        let query = params(&[
            ("v", "1"),
            ("last-sold", ""),
            (
                "gf",
                r#"{"buy_price":{"op":"gte","value":"5000"},"last_sold":{"op":"lte","value":"86400"},"roi":{"op":"gte","value":"30"}}"#,
            ),
        ]);
        let readable = readable_query(&query, &flip_aliases());
        assert_eq!(readable.get("gf"), None);
        assert_eq!(readable.get("min-buy").as_deref(), Some("5000"));
        assert_eq!(readable.get("last-sold").as_deref(), Some("1d"));
        assert_eq!(readable.get("roi").as_deref(), Some("30"));
        assert_eq!(readable.get("v").as_deref(), Some("1"));
        assert_eq!(
            resolve_filters(&readable, &flip_aliases()),
            resolve_filters(&query, &flip_aliases())
        );
    }

    #[test]
    fn readable_query_splits_ranges_and_keeps_what_no_alias_states() {
        let query = params(&[(
            "gf",
            r#"{"buy_price":{"op":"range","value":"10,20"},"roi":{"op":"range","value":"30,"},"daily-sales":{"op":"range","value":",5"},"profit":{"op":"gte","value":"1"}}"#,
        )]);
        let readable = readable_query(&query, &flip_aliases());
        assert_eq!(readable.get("min-buy").as_deref(), Some("10"));
        assert_eq!(readable.get("max-price").as_deref(), Some("20"));
        assert_eq!(readable.get("roi").as_deref(), Some("30"));
        // `min-sales` is a floor; an upper bound on daily sales has no alias,
        // and `profit` has none at all in this set.
        let packed = parse_filters(readable.get("gf").as_deref());
        assert_eq!(
            packed.keys().map(String::as_str).collect::<Vec<_>>(),
            ["daily-sales", "profit"]
        );
        let filters = resolve_filters(&readable, &flip_aliases());
        for (price, pass) in [(9.0, false), (10.0, true), (20.0, true), (21.0, false)] {
            assert_eq!(
                filters["buy_price"].matches(&GridValue::Number(price), false),
                Some(pass)
            );
        }
    }

    #[test]
    fn readable_query_leaves_lossy_values_in_gf() {
        // An integer alias cannot state 1.5, and a day-granular one cannot
        // state 90 seconds; both must survive exactly.
        let gf = r#"{"last_sold":{"op":"lte","value":"90"},"roi":{"op":"gte","value":"1.5"}}"#;
        let readable = readable_query(&params(&[("gf", gf)]), &flip_aliases());
        assert_eq!(readable.get("roi"), None);
        assert_eq!(readable.get("last-sold"), None);
        assert_eq!(
            parse_filters(readable.get("gf").as_deref()),
            parse_filters(Some(gf))
        );
    }

    #[test]
    fn clearing_keeps_the_seed_guard_only_without_an_explicit_view() {
        let cleared = |pairs: &[(&str, &str)]| {
            let mut query = canonical_query(&params(pairs), &aliases());
            query.remove("gf");
            readable_query(&query, &aliases())
        };
        let bare = cleared(&[("min-sales", "1")]);
        assert_eq!(bare.get("min-sales").as_deref(), Some(""));
        let explicit = cleared(&[("min-sales", "1"), ("v", "1")]);
        assert_eq!(explicit.get("min-sales"), None);
    }

    #[test]
    fn seconds_read_as_the_largest_whole_unit() {
        for (seconds, duration) in [
            ("86400", Some("1d")),
            ("172800", Some("2d")),
            ("7200", Some("2h")),
            ("300", Some("5m")),
            ("90", Some("90s")),
            ("0", Some("0s")),
            ("1.5", None),
            ("-60", None),
            ("soon", None),
        ] {
            assert_eq!(
                seconds_as_duration(seconds).as_deref(),
                duration,
                "{seconds}"
            );
        }
    }

    #[test]
    fn invalid_explicit_values_do_not_fall_back_to_a_legacy_threshold() {
        let query = params(&[
            ("min-buy", "10"),
            ("gf", r#"{"buy_price":{"op":"gte","value":"invalid"}}"#),
        ]);
        let filters = resolve_filters(&query, &aliases());
        assert!(!filters["buy_price"].valid(ValueKind::Number));
        assert_eq!(filters["buy_price"].value, "invalid");
    }
    /// The toolbar picker reads the grid's resolved columns and flips them
    /// through the grid's own command: required columns are never offered,
    /// the URL names only departures from the defaults, and an unknown id
    /// is ignored rather than written.
    #[test]
    fn registry_offers_optional_columns_and_toggles_through_the_grid() {
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(aliases(), Signal::derive(Vec::new));
            assert!(registry.optional_columns().is_empty());
            registry.toggle_column("profit");
            let defs = vec![
                GridColumn::new("item", "Item".into(), 300.0, false, true),
                GridColumn::new("profit", "Profit".into(), 100.0, true, true),
                GridColumn::new("level", "Level".into(), 100.0, true, false),
            ];
            let mut written = params(&[("cols", "profit,level")]);
            write_columns(&mut written, &defs);
            // Both columns are at their defaults: nothing to say.
            assert_eq!(written, ParamsMap::new());
            let mut flipped = defs.clone();
            flipped[1].visible = false;
            flipped[2].visible = true;
            write_columns(&mut written, &flipped);
            assert_eq!(written.get("show-cols").as_deref(), Some("level"));
            assert_eq!(written.get("hide-cols").as_deref(), Some("profit"));
            registry.register(Signal::derive(move || defs.clone()));
            let writes = RwSignal::new(Vec::<(&'static str, bool)>::new());
            let resets = RwSignal::new(0usize);
            let swaps = RwSignal::new(Vec::<(&'static str, &'static str)>::new());
            registry.register_visibility(ColumnVisibility {
                set_visible: Callback::new(move |change| writes.update(|w| w.push(change))),
                swap: Callback::new(move |change| swaps.update(|s| s.push(change))),
                reset: Callback::new(move |_| resets.update(|n| *n += 1)),
            });
            // Only a shown optional column can be swapped, and only for a
            // hidden optional one.
            registry.swap_columns("profit", "level");
            registry.swap_columns("level", "profit");
            registry.swap_columns("item", "level");
            registry.swap_columns("profit", "missing");
            assert_eq!(swaps.get(), vec![("profit", "level")]);
            let ids: Vec<_> = registry.optional_columns().iter().map(|c| c.id).collect();
            assert_eq!(ids, ["profit", "level"]);
            assert_eq!(registry.visible_columns(), HashSet::from(["profit"]));
            registry.toggle_column("profit");
            registry.toggle_column("level");
            registry.toggle_column("item");
            registry.toggle_column("missing");
            assert_eq!(writes.get(), vec![("profit", false), ("level", true)]);
            registry.reset_columns();
            assert_eq!(resets.get(), 1);
        });
    }

    /// The picker's "Showing" list reads the columns on screen, in the order
    /// the user dragged them into, required ones included.
    #[test]
    fn displayed_columns_follow_the_layout_and_skip_hidden_ones() {
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            assert!(registry.displayed_columns().is_empty());
            let defs = vec![
                GridColumn::new("item", "Item".into(), 300.0, false, true),
                GridColumn::new("profit", "Profit".into(), 100.0, true, true),
                GridColumn::new("level", "Level".into(), 100.0, true, false),
                GridColumn::new("roi", "ROI".into(), 100.0, true, true),
            ];
            registry.register(Signal::derive(move || defs.clone()));
            let ids = || -> Vec<_> { registry.displayed_columns().iter().map(|c| c.id).collect() };
            assert_eq!(ids(), ["item", "profit", "roi"]);
            let layout = RwSignal::new(Some("3~roi~".to_string()));
            registry.register_layout(layout.into());
            assert_eq!(ids(), ["roi", "item", "profit"]);
            layout.set(None);
            assert_eq!(ids(), ["item", "profit", "roi"]);
        });
    }

    #[test]
    fn legacy_sort_tokens_resolve_to_metric_columns_until_a_header_rewrites_them() {
        let aliases = [
            SortAlias::new("units", "units"),
            SortAlias::new("price", "market-listing"),
        ];
        assert_eq!(
            resolve_sort(Some("units"), &aliases).as_deref(),
            Some("units")
        );
        assert_eq!(
            resolve_sort(Some("price"), &aliases).as_deref(),
            Some("market-listing")
        );
        // Explicit grid state wins over an alias with the same token.
        assert_eq!(
            resolve_sort(Some("grid:price"), &aliases).as_deref(),
            Some("price")
        );
        assert_eq!(resolve_sort(Some("profit"), &aliases), None);
        assert_eq!(resolve_sort(None, &aliases), None);
        assert_eq!(resolve_sort(Some("units"), &[]), None);
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            assert_eq!(registry.sort_column(Some("units")), None);
            registry.register_sort_aliases(aliases.to_vec());
            assert_eq!(
                registry.sort_column(Some("units")).as_deref(),
                Some("units")
            );
            assert_eq!(
                effective_sort(&params(&[("sort", "price")])).as_deref(),
                Some("market-listing")
            );
            registry.register_default_sort("units");
            assert_eq!(registry.sort_column(None).as_deref(), Some("units"));
            assert_eq!(
                registry.sort_column(Some("invalid")).as_deref(),
                Some("units")
            );
            assert_eq!(
                registry.sort_column(Some("grid:market-listing")).as_deref(),
                Some("market-listing")
            );
            assert_eq!(
                effective_sort(&params(&[("dir", "asc")])).as_deref(),
                Some("units")
            );
        });
    }

    #[test]
    fn the_menu_keeps_each_group_together_in_first_appearance_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            let registry = FilterRegistry::provide(Vec::new(), Signal::derive(Vec::new));
            let column = |id: &'static str, group: Option<&str>| {
                let mut col = GridColumn::new(id, id.to_string(), 100.0, true, false);
                col.picker_group = group.map(str::to_string);
                col.filters = vec![ColumnFilter::metric(id, id.to_string(), ValueKind::Number)];
                col
            };
            // A column table that interleaves its groups, as Recipe's does.
            let columns = vec![
                column("profit", None),
                column("rev-a", Some("Revenue")),
                column("volume", Some("Market")),
                column("rev-b", Some("Revenue")),
                column("roi", None),
            ];
            registry.register(Signal::derive(move || columns.clone()));
            let html = view! {
                <RegisteredFilterMenu registry on_select=Callback::new(|_| ())/>
            }
            .to_html();
            let at = |needle: &str| {
                html.find(needle)
                    .unwrap_or_else(|| panic!("{needle}: {html}"))
            };
            assert_eq!(html.matches("<strong").count(), 2, "{html}");
            assert!(at("data-add-filter=\"profit\"") < at("data-add-filter=\"roi\""));
            assert!(at("data-add-filter=\"roi\"") < at(">Revenue<"));
            assert!(at(">Revenue<") < at("data-add-filter=\"rev-a\""));
            assert!(at("data-add-filter=\"rev-a\"") < at("data-add-filter=\"rev-b\""));
            assert!(at("data-add-filter=\"rev-b\"") < at(">Market<"));
            assert!(at(">Market<") < at("data-add-filter=\"volume\""));
        });
    }

    #[test]
    fn registry_includes_hidden_metrics_and_deduplicates_controls() {
        let owner = Owner::new();
        owner.with(|| {
            let tax = ColumnFilter::new("tax", "Tax".into(), false);
            let registry =
                FilterRegistry::provide(aliases(), Signal::derive(move || vec![tax.clone()]));
            let mut col = GridColumn::new("buy_price", "Buy".into(), 100.0, true, false);
            col.picker_group = Some("Market · 30d".into());
            col.filters = vec![
                ColumnFilter::new("min-buy", "Minimum".into(), true),
                ColumnFilter::metric("buy_price", "Buy".into(), ValueKind::Number),
                ColumnFilter::new("tax", "Tax".into(), false),
            ];
            registry.register(Signal::derive(move || vec![col.clone()]));
            let entries = registry.entries();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].filter.key, "buy_price");
            assert_eq!(entries[0].group.as_deref(), Some("Market · 30d"));
            assert_eq!(entries[1].filter.key, "tax");
            assert!(registry.active(&entries[0].filter, &params(&[("min-buy", "10")])));
            assert!(
                registry
                    .clear_all(&params(&[("min-buy", "10"), ("tax", "false")]))
                    .get("tax")
                    .is_none()
            );
        });
    }
}
