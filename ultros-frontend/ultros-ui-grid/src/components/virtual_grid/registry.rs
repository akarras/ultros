//! Shared filter definitions for a grid, its toolbar and its column menus.
//! Provide once in the owner containing both ControlBar and QueryGrid. Controls
//! keep their URL keys and run before calculations; aliases become metric queries.
use super::{ColumnFilter, GridColumn, metrics::*};
use crate::components::app_link::use_location_or_default;
use leptos::prelude::*;
use leptos_router::params::ParamsMap;

#[derive(Clone, Copy, Debug)]
pub struct FilterAlias {
    pub key: &'static str,
    pub column: &'static str,
    pub op: FilterOp,
    pub convert: fn(&str) -> Option<String>,
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

pub fn clear_key(query: &mut ParamsMap, key: &str) {
    query.remove(key);
    if matches!(key, "next-sale" | "last-sold" | "min-sales") {
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

#[derive(Clone, Copy)]
pub struct FilterRegistry {
    aliases: StoredValue<Vec<FilterAlias>>,
    sort_aliases: StoredValue<Vec<SortAlias>>,
    default_sort: StoredValue<Option<&'static str>>,
    controls: Signal<Vec<ColumnFilter>>,
    columns: RwSignal<Option<Signal<Vec<GridColumn>>>>,
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
            editing: RwSignal::new(None),
            count: RwSignal::new(None),
        };
        provide_context(registry);
        registry
    }

    pub fn register_count(self, count: Signal<usize>) {
        self.count.set(Some(count));
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

    pub fn filters(self, query: &ParamsMap) -> MetricFilters {
        self.aliases
            .with_value(|aliases| resolve_filters(query, aliases))
    }

    pub fn canonical(self, query: &ParamsMap) -> ParamsMap {
        self.aliases
            .with_value(|aliases| canonical_query(query, aliases))
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
            .or_else(|| self.default_sort.get_value().map(str::to_owned))
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
            if entry.filter.metric.is_none() && entry.filter.clear_with_filters {
                clear_key(&mut next, entry.filter.key);
            }
        }
        next
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
        let _next = super::filter::cleared_query(&query, &[filter]);
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
            .filter(|e| !registry.active(&e.filter, &query))
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
            <div class="sticky-bar-popover p-3 w-[min(92vw,24rem)]" data-registered-editor>
                <super::filter::ColumnFilterEditor filter/>
                <button type="button" class="mt-2 text-sm" aria-label=t_string!(i18n, grid_close)
                    on:click=move |_| registry.editing.set(None)>"×"</button>
            </div>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
