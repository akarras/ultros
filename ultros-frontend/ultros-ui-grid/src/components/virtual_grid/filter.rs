use super::layout::ColumnFilter;
use crate::{components::app_link::use_location_or_default, i18n::*};
use leptos::prelude::*;
use leptos_router::params::ParamsMap;

/// `query` with every filter on one column cleared: metric filters drop out of
/// the packed `gf` map, plain filters drop their own key.
///
/// One call clears a whole column, which is what the header's clear button
/// needs — a column like Cost / unit carries four separate filters, and the
/// popover only offers them one at a time.
pub fn cleared_query(query: &ParamsMap, filters: &[ColumnFilter]) -> ParamsMap {
    let mut query = query.clone();
    let mut metrics = parse_filters(query.get("gf").as_deref());
    let mut touched_metrics = false;
    for filter in filters {
        if filter.calculation {
            continue;
        }
        if filter.metric.is_some() {
            touched_metrics |= metrics.remove(filter.key).is_some();
        } else {
            super::registry::clear_key(&mut query, filter.key);
        }
    }
    if touched_metrics {
        super::registry::write_filters(&mut query, &metrics);
    }
    query
}

#[component]
pub fn ColumnFilterEditor(filter: ColumnFilter) -> impl IntoView {
    if let Some(kind) = filter.metric {
        let mut choices = filter.choices;
        choices.extend(
            filter
                .options
                .into_iter()
                .map(|(key, label)| (key.to_string(), label)),
        );
        return view! { <MetricFilterEditor column=filter.key label=filter.label kind unit=filter.unit choices/> }
            .into_any();
    }
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let query = location.query;
    let key = filter.key;
    let registry = use_context::<super::registry::FilterRegistry>();
    let default_value = StoredValue::new(filter.default_value);
    let current = move |q: &ParamsMap| {
        q.get(key)
            .or_else(|| default_value.get_value())
            .unwrap_or_default()
    };
    let value = RwSignal::new(query.with_untracked(current));
    Effect::new(move |_| value.set(query.with(current)));
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let commit = Callback::new(move |next: Option<String>| {
        let q = query.get_untracked();
        let mut q = registry.map(|r| r.canonical(&q)).unwrap_or(q);
        super::registry::clear_key(&mut q, key);
        if let Some(next) = next {
            q.replace(key, next);
        }
        if let Some(registry) = registry {
            registry.editing.set(None);
        }
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}{}",
                location.pathname.get_untracked(),
                q.to_query_string(),
                location.hash.get_untracked()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });
    let multiple = filter.multiple;
    let mut options = filter.choices;
    options.extend(
        filter
            .options
            .into_iter()
            .map(|(key, label)| (key.to_string(), label)),
    );
    let step = if matches!(key, "min-sales" | "vel") {
        "any"
    } else {
        "1"
    };
    let min = (key == "sales").then_some("0");
    let max = (key == "sales"
        && location
            .pathname
            .get_untracked()
            .starts_with("/flip-finder"))
    .then_some("6");
    view! {
        <form class="grid-column-filter" data-filter=key on:submit=move |e| {
            e.prevent_default();
            commit.run(crate::components::filter_chip::committed_value(&value.get_untracked()));
        }>
            <fieldset class="min-w-0">
                <legend>{filter.label.clone()}</legend>
                {if multiple {
                    view! { <div class="flex flex-col gap-2">
                        {options.into_iter().map(|(token, label)| {
                            let checked_token = token.clone();
                            view! { <label class="flex items-center gap-2"><input type="checkbox"
                                prop:checked=move || value.with(|raw| raw.split(',').any(|v| v == checked_token))
                                on:change=move |event| {
                                    value.update(|raw| {
                                        let mut selected = raw.split(',').filter(|v| !v.is_empty()).map(str::to_string).collect::<std::collections::BTreeSet<_>>();
                                        if event_target_checked(&event) { selected.insert(token.clone()); } else { selected.remove(&token); }
                                        *raw = selected.into_iter().collect::<Vec<_>>().join(",");
                                    });
                                }/>{label}</label> }
                        }).collect_view()}
                    </div> }.into_any()
                } else if options.is_empty() {
                    view! {<input aria-label=filter.label.clone() type=if filter.numeric {"number"} else {"text"} step=step min=min max=max
                        prop:value=move || value.get() on:input=move |e| value.set(event_target_value(&e))/>}.into_any()
                } else {
                    view! {<select aria-label=filter.label.clone() prop:value=move || value.get() on:change=move |e| value.set(event_target_value(&e))>
                        <option value="">{t!(i18n, grid_filter_any)}</option>
                        {options.into_iter().map(|(value,label)| view! {<option value=value>{label}</option>}).collect_view()}
                    </select>}.into_any()
                }}
            </fieldset>
            <div class="grid-filter-actions">
                <button type="submit" class="grid-filter-apply">
                    <Icon icon=icondata::MdiCheck aria_hidden=true/>
                    <span>{t!(i18n, grid_filter_apply)}</span>
                </button>
                <button type="button" class="grid-icon-btn" data-clear-filter
                    title=t_string!(i18n, grid_filter_clear).to_string()
                    aria-label=t_string!(i18n, grid_filter_clear).to_string()
                    on:click=move |_| commit.run(None)>
                    <Icon icon=icondata::MdiFilterRemove aria_hidden=true/>
                </button>
            </div>
        </form>
    }.into_any()
}

use super::metrics::{FilterOp, MetricFilter, ValueKind, parse_filters};
use crate::components::icon::Icon;
use ultros_grid_core::units::{self, Unit, UnitError};

#[component]
pub fn MetricSortControls(column: &'static str) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let registry = use_context::<super::registry::FilterRegistry>();
    let href = move |dir: &str| {
        metric_sort_href(&location.pathname.get(), location.query.get(), column, dir)
    };
    // The direction this column is sorted in right now, if it is the sort.
    let current = move || {
        location.query.with(|q| {
            (super::registry::effective_sort(q).as_deref() == Some(column)).then(|| {
                registry
                    .map(|registry| registry.sort_ascending(q))
                    .unwrap_or_else(|| q.get("dir").as_deref() == Some("asc"))
            })
        })
    };
    let link = move |ascending: bool| {
        let label = if ascending {
            t_string!(i18n, grid_query_ascending).to_string()
        } else {
            t_string!(i18n, grid_query_descending).to_string()
        };
        view! {
            // A plain link, like the header's: the router's `A` would mark both
            // directions `aria-current="page"`, since only the query differs.
            <a href=move || href(if ascending { "asc" } else { "desc" }) data-noscroll=true
                class="grid-icon-btn" title=label.clone() aria-label=label
                data-active=move || (current() == Some(ascending)).to_string()>
                <Icon icon=if ascending { icondata::MdiSortAscending } else { icondata::MdiSortDescending } aria_hidden=true/>
            </a>
        }
    };
    view! { {link(true)} {link(false)} }
}

pub(crate) fn metric_sort_href(
    path: &str,
    mut query: ParamsMap,
    column: &str,
    dir: &str,
) -> String {
    query.replace("sort", format!("grid:{column}"));
    query.replace("dir", dir.to_string());
    format!("{path}{}", query.to_query_string())
}

/// Header action for a complete metric. The caller must exclude partial feeds;
/// QueryGrid owns the column's aria-sort and pending-result behavior.
#[component]
pub fn MetricSortHeader(
    column: &'static str,
    #[prop(into)] label: Signal<String>,
) -> impl IntoView {
    let location = use_location_or_default();
    let registry = use_context::<super::registry::FilterRegistry>();
    // Alias-aware, so a retired native token lights the header it now means.
    let active = move || {
        location
            .query
            .with(|q| super::registry::effective_sort(q).as_deref() == Some(column))
    };
    let ascending = move || {
        location.query.with(|q| {
            registry
                .map(|registry| registry.sort_ascending(q))
                .unwrap_or_else(|| q.get("dir").as_deref() == Some("asc"))
        })
    };
    view! {
        <a
            href=move || metric_sort_href(
                &location.pathname.get(), location.query.get(), column,
                if if active() { !ascending() } else { registry.is_some_and(|r| r.default_ascending(column)) } { "asc" } else { "desc" },
            )
            data-noscroll=true
            data-metric-sort=column
            class="flex min-h-11 min-w-0 items-center gap-2 px-1 !text-brand-300 hover:text-brand-200 focus-visible:outline focus-visible:outline-2"
        >
            <span class="truncate min-w-0">{move || label.get()}</span>
            {move || active().then(|| view! {
                <span aria-hidden="true" class="shrink-0">{if ascending() { "↑" } else { "↓" }}</span>
            })}
        </a>
    }
}

/// Whether a column filter asks for the value itself or only its presence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presence {
    Any,
    Has,
    Missing,
}

/// Which half of a timestamp editor is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimeTab {
    Within,
    Range,
}

const WITHIN_PRESETS: [&str; 4] = ["24h", "3d", "7d", "30d"];

/// One typed side of a range: its token, or why it cannot be read.
fn read_side(unit: Unit, text: &str) -> Result<Option<String>, UnitError> {
    units::parse_input(unit, text)
}

/// The unit's suffix inside a field.
fn unit_suffix(unit: Unit) -> Option<String> {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match unit {
        Unit::Gil => Some(t_string!(i18n, grid_unit_gil).to_string()),
        Unit::Percent => Some("%".into()),
        Unit::Rate => Some(t_string!(i18n, grid_unit_per_day).to_string()),
        _ => None,
    }
}

fn unit_placeholder(unit: Unit) -> &'static str {
    match unit {
        Unit::Gil => "1.2m",
        Unit::Plain => "1,000",
        Unit::Percent => "15",
        Unit::Rate => "0.5",
        Unit::Seconds | Unit::Hours => "2d 4h",
        Unit::Timestamp => "7d",
    }
}

fn invalid_message(unit: Unit) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match unit {
        Unit::Plain | Unit::Gil => t_string!(i18n, grid_filter_invalid_amount).to_string(),
        Unit::Percent | Unit::Rate => t_string!(i18n, grid_filter_invalid_number).to_string(),
        Unit::Seconds | Unit::Hours | Unit::Timestamp => {
            t_string!(i18n, grid_filter_invalid_duration).to_string()
        }
    }
}

/// The line under a field: how the input was read, or what went wrong.
fn preview_line(unit: Unit, text: &str) -> (bool, String) {
    match read_side(unit, text) {
        Ok(None) => (false, String::new()),
        Err(_) => (true, invalid_message(unit)),
        Ok(Some(token)) => {
            let shown = units::format_input(unit, &token);
            let shown = match unit_suffix(unit) {
                Some(suffix) if suffix != "%" => format!("{shown} {suffix}"),
                Some(suffix) => format!("{shown}{suffix}"),
                None => shown,
            };
            (false, format!("= {shown}"))
        }
    }
}

/// A `datetime-local` value as unix seconds, read in the viewer's zone.
fn local_input_to_unix(value: &str) -> Option<f64> {
    if value.trim().is_empty() {
        return None;
    }
    #[cfg(feature = "hydrate")]
    {
        let ms = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(value)).get_time();
        ms.is_finite().then_some((ms / 1000.0).floor())
    }
    #[cfg(not(feature = "hydrate"))]
    {
        // The server has no viewer zone; it never edits, so UTC will do.
        let (date, time) = value.split_once('T')?;
        let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
        let (y, m, day) = (d.next()??, d.next()??, d.next()??);
        let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
        let (h, min) = (t.next()??, t.next()??);
        let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * m + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some((days * 86_400 + h * 3_600 + min * 60) as f64)
    }
}

/// Unix seconds as a `datetime-local` value in the viewer's zone.
fn unix_to_local_input(unix: f64) -> String {
    #[cfg(feature = "hydrate")]
    {
        let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(unix * 1000.0));
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}",
            d.get_full_year(),
            d.get_month() + 1,
            d.get_date(),
            d.get_hours(),
            d.get_minutes()
        )
    }
    #[cfg(not(feature = "hydrate"))]
    {
        units::utc_minute(unix).replacen(' ', "T", 1)
    }
}

/// Unix seconds in the viewer's zone, for previews.
fn local_minute(unix: f64) -> String {
    unix_to_local_input(unix).replacen('T', " ", 1)
}

/// `UTC−7`, `UTC+5:30`: the viewer's zone for the date pickers' note.
fn local_zone() -> String {
    #[cfg(feature = "hydrate")]
    let minutes = -(js_sys::Date::new_0().get_timezone_offset() as i64);
    #[cfg(not(feature = "hydrate"))]
    let minutes = 0_i64;
    let sign = if minutes < 0 { '−' } else { '+' };
    let (h, m) = (minutes.abs() / 60, minutes.abs() % 60);
    match (minutes, m) {
        (0, _) => "UTC".into(),
        (_, 0) => format!("UTC{sign}{h}"),
        _ => format!("UTC{sign}{h}:{m:02}"),
    }
}

/// A number field with its unit inside and its reading underneath.
#[component]
fn RangeField(
    unit: Unit,
    label: String,
    text: RwSignal<String>,
    #[prop(into)] disabled: Signal<bool>,
) -> impl IntoView {
    let preview = Memo::new(move |_| text.with(|t| preview_line(unit, t)));
    let suffix = unit_suffix(unit);
    let has_suffix = suffix.is_some();
    view! {
        <label class="grid-range-side">
            <span class="grid-field-label">{label.clone()}</span>
            <span class="grid-field" class:has-suffix=has_suffix>
                <input type="text" inputmode=if unit.is_duration() || unit == Unit::Timestamp { "text" } else { "decimal" }
                    autocomplete="off" placeholder=unit_placeholder(unit) aria-label=label
                    aria-invalid=move || preview.with(|(bad, _)| bad.to_string())
                    disabled=move || disabled.get()
                    prop:value=move || text.get() on:input=move |e| text.set(event_target_value(&e))/>
                {suffix.map(|s| view! { <span class="grid-field-suffix" aria-hidden="true">{s}</span> })}
            </span>
            <span class="grid-field-preview" class:is-error=move || preview.with(|(bad, _)| *bad) aria-live="polite">
                {move || preview.with(|(_, line)| line.clone())}
            </span>
        </label>
    }
}

#[component]
fn MetricFilterEditor(
    column: &'static str,
    label: String,
    kind: ValueKind,
    unit: Unit,
    choices: Vec<(String, String)>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let query = location.query;
    let registry = use_context::<super::registry::FilterRegistry>();
    let read = move |q: &ParamsMap| {
        registry
            .map(|r| r.filters(q))
            .unwrap_or_else(|| parse_filters(q.get("gf").as_deref()))
    };
    // Mixed columns hold numbers or a status word; text columns only words.
    let numeric = kind != ValueKind::Text;
    let timestamp = numeric && unit == Unit::Timestamp;
    let choices = StoredValue::new(choices);
    let presence = RwSignal::new(Presence::Any);
    let min = RwSignal::new(String::new());
    let max = RwSignal::new(String::new());
    let tab = RwSignal::new(TimeTab::Within);
    let after = RwSignal::new(String::new());
    let before = RwSignal::new(String::new());
    // Text and mixed columns: which comparison, and against what.
    let text_op = RwSignal::new(if choices.with_value(Vec::is_empty) {
        FilterOp::Contains
    } else {
        FilterOp::Eq
    });
    let text_value = RwSignal::new(String::new());
    // Mixed columns choose between a numeric range and a word.
    let mixed_text = RwSignal::new(false);
    let invalid = RwSignal::new(None::<String>);
    let load = move |filter: Option<MetricFilter>| {
        presence.set(Presence::Any);
        for side in [min, max, after, before, text_value] {
            side.set(String::new());
        }
        tab.set(TimeTab::Within);
        mixed_text.set(false);
        invalid.set(None);
        let Some(filter) = filter else {
            return;
        };
        match filter.op {
            FilterOp::Missing => presence.set(Presence::Missing),
            FilterOp::Present => presence.set(Presence::Has),
            FilterOp::Eq | FilterOp::Ne | FilterOp::Contains => {
                // A number compared for equality is a one-point range.
                if kind == ValueKind::Number && filter.op == FilterOp::Eq {
                    let shown = units::format_input(unit, &filter.value);
                    min.set(shown.clone());
                    max.set(shown);
                } else {
                    mixed_text.set(kind == ValueKind::Mixed);
                    text_op.set(filter.op);
                    text_value.set(filter.value);
                }
            }
            _ => {
                let (low, high) = filter.range_sides().unwrap_or_default();
                let relative = low.is_some_and(units::is_relative) && high.is_none();
                if timestamp && !relative && (low.is_some() || high.is_some()) {
                    tab.set(TimeTab::Range);
                    let pick = |s: Option<&str>| {
                        s.and_then(|s| s.parse::<f64>().ok())
                            .map(unix_to_local_input)
                            .unwrap_or_default()
                    };
                    after.set(pick(low));
                    before.set(pick(high));
                } else {
                    let shown = |s: Option<&str>| {
                        s.map(|s| units::format_input(unit, s)).unwrap_or_default()
                    };
                    min.set(shown(low));
                    max.set(shown(high));
                }
            }
        }
    };
    load(query.with_untracked(read).remove(column));
    Effect::new(move |_| load(query.with(read).remove(column)));
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let write = Callback::new(move |next: Option<MetricFilter>| {
        let q = query.get_untracked();
        let mut q = registry.map(|r| r.canonical(&q)).unwrap_or(q);
        let mut filters = parse_filters(q.get("gf").as_deref());
        match next {
            Some(filter) => {
                filters.insert(column.to_string(), filter);
            }
            None => {
                filters.remove(column);
            }
        }
        super::registry::write_filters(&mut q, &filters);
        if let Some(registry) = registry {
            registry.editing.set(None);
        }
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}{}",
                location.pathname.get_untracked(),
                q.to_query_string(),
                location.hash.get_untracked()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });
    // The filter the fields describe; `Ok(None)` clears the column.
    let build = move || -> Result<Option<MetricFilter>, String> {
        let presence_filter = |op| {
            Ok(Some(MetricFilter {
                op,
                value: String::new(),
            }))
        };
        match presence.get_untracked() {
            Presence::Has => return presence_filter(FilterOp::Present),
            Presence::Missing => return presence_filter(FilterOp::Missing),
            Presence::Any => {}
        }
        if !numeric || mixed_text.get_untracked() {
            let value = text_value.get_untracked().trim().to_string();
            return Ok((!value.is_empty()).then_some(MetricFilter {
                op: text_op.get_untracked(),
                value,
            }));
        }
        if timestamp && tab.get_untracked() == TimeTab::Range {
            let low = local_input_to_unix(&after.get_untracked()).map(|v| v.to_string());
            let high = local_input_to_unix(&before.get_untracked()).map(|v| v.to_string());
            return Ok(match (&low, &high) {
                (None, None) => None,
                (Some(l), Some(h)) if l.parse::<f64>().ok() > h.parse::<f64>().ok() => {
                    return Err(t_string!(i18n, grid_filter_after_above_before).to_string());
                }
                _ => Some(MetricFilter::range(low, high)),
            });
        }
        let side = |text: RwSignal<String>| {
            read_side(unit, &text.get_untracked()).map_err(|_| invalid_message(unit))
        };
        let (low, high) = (side(min)?, if timestamp { None } else { side(max)? });
        if let (Some(l), Some(h)) = (&low, &high)
            && l.parse::<f64>().ok() > h.parse::<f64>().ok()
        {
            return Err(t_string!(i18n, grid_filter_min_above_max).to_string());
        }
        Ok((low.is_some() || high.is_some()).then(|| MetricFilter::range(low, high)))
    };
    let active = Memo::new(move |_| query.with(read).contains_key(column));
    let presence_off = Signal::derive(move || presence.get() != Presence::Any);
    let toggle_presence = move |to: Presence| {
        presence.update(|p| *p = if *p == to { Presence::Any } else { to });
    };
    let actions = move || {
        view! {
            {move || invalid.get().map(|message| view! { <span class="grid-filter-error" role="alert">{message}</span> })}
            <div class="grid-filter-actions">
                <button type="submit" class="grid-filter-apply">
                    <Icon icon=icondata::MdiCheck aria_hidden=true/>
                    <span>{t!(i18n, grid_filter_apply)}</span>
                </button>
                {move || active.get().then(|| view! {
                    <button type="button" class="grid-icon-btn" data-clear-filter
                        title=t_string!(i18n, grid_filter_clear).to_string()
                        aria-label=t_string!(i18n, grid_filter_clear).to_string()
                        on:click=move |_| write.run(None)>
                        <Icon icon=icondata::MdiFilterRemove aria_hidden=true/>
                    </button>
                })}
            </div>
        }
    };
    let presence_row = move || {
        view! {
            <div class="grid-filter-presence" role="group" aria-label=label.clone()>
                <button type="button" class="grid-chip" aria-pressed=move || (presence.get() == Presence::Has).to_string()
                    on:click=move |_| toggle_presence(Presence::Has)>{t!(i18n, grid_filter_has_value)}</button>
                <button type="button" class="grid-chip" aria-pressed=move || (presence.get() == Presence::Missing).to_string()
                    on:click=move |_| toggle_presence(Presence::Missing)>{t!(i18n, grid_filter_no_value)}</button>
            </div>
        }
    };
    let text_fields = move || {
        let ops = [
            (FilterOp::Eq, t_string!(i18n, grid_query_eq).to_string()),
            (FilterOp::Ne, t_string!(i18n, grid_query_ne).to_string()),
            (
                FilterOp::Contains,
                t_string!(i18n, grid_query_contains).to_string(),
            ),
        ];
        view! {
            <div class="grid-filter-text">
                <select aria-label=t_string!(i18n, grid_query_filter).to_string()
                    disabled=move || presence_off.get()
                    prop:value=move || op_token(text_op.get())
                    on:change=move |e| if let Ok(op) = serde_json::from_value(serde_json::Value::String(event_target_value(&e))) { text_op.set(op) }>
                    {ops.into_iter().map(|(op, label)| view! { <option value=op_token(op)>{label}</option> }).collect_view()}
                </select>
                {move || if text_op.get() != FilterOp::Contains && !choices.with_value(Vec::is_empty) {
                    view! {
                        <select aria-label=t_string!(i18n, grid_query_value).to_string() disabled=move || presence_off.get()
                            prop:value=move || text_value.get() on:change=move |e| text_value.set(event_target_value(&e))>
                            <option value="">{t!(i18n, grid_filter_any)}</option>
                            {choices.get_value().into_iter().map(|(key, label)| view! { <option value=key>{label}</option> }).collect_view()}
                        </select>
                    }.into_any()
                } else {
                    view! {
                        <input type="text" autocomplete="off" aria-label=t_string!(i18n, grid_query_value).to_string()
                            disabled=move || presence_off.get()
                            prop:value=move || text_value.get() on:input=move |e| text_value.set(event_target_value(&e))/>
                    }.into_any()
                }}
            </div>
        }
    };
    let range_fields = move || {
        view! {
            <div class="grid-range">
                <RangeField unit label=t_string!(i18n, grid_filter_min).to_string() text=min disabled=presence_off/>
                <span class="grid-range-dash" aria-hidden="true">"–"</span>
                <RangeField unit label=t_string!(i18n, grid_filter_max).to_string() text=max disabled=presence_off/>
            </div>
        }
    };
    let within_preview = Memo::new(move |_| {
        let text = min.get();
        match read_side(Unit::Timestamp, &text) {
            Ok(None) => (false, String::new()),
            Err(_) => (true, invalid_message(Unit::Timestamp)),
            Ok(Some(token)) => (
                false,
                units::resolve_token(&token, super::query_grid::now_unix())
                    .map(|since| {
                        t_string!(i18n, grid_filter_since, time = local_minute(since)).to_string()
                    })
                    .unwrap_or_default(),
            ),
        }
    });
    let time_fields = move || {
        let tab_button = move |which: TimeTab, text: String| {
            view! {
                <button type="button" role="tab" aria-selected=move || (tab.get() == which).to_string()
                    on:click=move |_| tab.set(which)>{text}</button>
            }
        };
        view! {
            <div class="grid-filter-tabs" role="tablist">
                {tab_button(TimeTab::Within, t_string!(i18n, grid_filter_within_tab).to_string())}
                {tab_button(TimeTab::Range, t_string!(i18n, grid_filter_range_tab).to_string())}
            </div>
            {move || match tab.get() {
                TimeTab::Within => view! {
                    <div class="grid-filter-within">
                        <span class="grid-field-label">{t!(i18n, grid_filter_within_label)}</span>
                        <div class="grid-filter-quick">
                            {WITHIN_PRESETS.into_iter().map(|preset| view! {
                                <button type="button" class="grid-chip" disabled=move || presence_off.get()
                                    aria-pressed=move || (min.with(|m| m.trim() == preset)).to_string()
                                    on:click=move |_| min.set(preset.to_string())>{preset}</button>
                            }).collect_view()}
                        </div>
                        <span class="grid-field">
                            <input type="text" autocomplete="off" placeholder="7d" disabled=move || presence_off.get()
                                aria-label=t_string!(i18n, grid_filter_within_label).to_string()
                                aria-invalid=move || within_preview.with(|(bad, _)| bad.to_string())
                                prop:value=move || min.get() on:input=move |e| min.set(event_target_value(&e))/>
                        </span>
                        <span class="grid-field-preview" class:is-error=move || within_preview.with(|(bad, _)| *bad) aria-live="polite">
                            {move || within_preview.with(|(_, line)| line.clone())}
                        </span>
                    </div>
                }.into_any(),
                TimeTab::Range => view! {
                    <div class="grid-filter-dates">
                        <label><span class="grid-field-label">{t!(i18n, grid_filter_after)}</span>
                            <input type="datetime-local" disabled=move || presence_off.get()
                                prop:value=move || after.get() on:input=move |e| after.set(event_target_value(&e))/></label>
                        <label><span class="grid-field-label">{t!(i18n, grid_filter_before)}</span>
                            <input type="datetime-local" disabled=move || presence_off.get()
                                prop:value=move || before.get() on:input=move |e| before.set(event_target_value(&e))/></label>
                        <span class="grid-field-preview">{t_string!(i18n, grid_filter_local_time, zone = local_zone()).to_string()}</span>
                    </div>
                }.into_any(),
            }}
        }
    };
    view! {
        <form class="grid-filter-card" data-metric-filter=column data-unit=format!("{unit:?}").to_lowercase()
            on:submit=move |e| {
                e.prevent_default();
                match build() {
                    Ok(next) => write.run(next),
                    Err(message) => invalid.set(Some(message)),
                }
            }
            on:input=move |_| invalid.set(None)>
            {(kind == ValueKind::Mixed).then(|| view! {
                <div class="grid-filter-tabs" role="tablist">
                    <button type="button" role="tab" aria-selected=move || (!mixed_text.get()).to_string()
                        on:click=move |_| mixed_text.set(false)>{t!(i18n, grid_filter_range_tab)}</button>
                    <button type="button" role="tab" aria-selected=move || mixed_text.get().to_string()
                        on:click=move |_| mixed_text.set(true)>{t!(i18n, grid_query_value)}</button>
                </div>
            })}
            {move || if !numeric || mixed_text.get() {
                text_fields().into_any()
            } else if timestamp {
                time_fields().into_any()
            } else {
                range_fields().into_any()
            }}
            {presence_row()}
            {actions()}
        </form>
    }
}

fn op_token(op: FilterOp) -> String {
    serde_json::to_string(&op)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// A threshold filter as chip text: `≥ 1.2M`, `100 – 500`, `last 7d`.
/// `None` for comparisons that are not thresholds.
pub fn range_chip(filter: &MetricFilter, unit: Unit) -> Option<String> {
    use units::ChipBound;
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let (low, high) = filter.range_sides()?;
    let text = |token: &str| match units::format_chip(unit, token) {
        ChipBound::Text(text) | ChipBound::At(text) => text,
        ChipBound::Within(span) => t_string!(i18n, grid_chip_within, span = span).to_string(),
    };
    let at = |token: &str| matches!(units::format_chip(unit, token), ChipBound::At(_));
    Some(match (low, high) {
        (Some(low), None) if units::is_relative(low) => text(low),
        (Some(low), None) if at(low) => {
            t_string!(i18n, grid_chip_after, time = text(low)).to_string()
        }
        (None, Some(high)) if at(high) => {
            t_string!(i18n, grid_chip_before, time = text(high)).to_string()
        }
        (Some(low), None) => format!("≥ {}", text(low)),
        (None, Some(high)) if filter.op == FilterOp::Lt => format!("< {}", text(high)),
        (None, Some(high)) => format!("≤ {}", text(high)),
        (Some(low), Some(high)) => format!("{} – {}", text(low), text(high)),
        (None, None) => return None,
    })
}

/// One operator label used by menu editors and toolbar chips.
pub fn operator_label(op: FilterOp) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match op {
        FilterOp::Eq => t_string!(i18n, grid_query_eq).to_string(),
        FilterOp::Ne => t_string!(i18n, grid_query_ne).to_string(),
        FilterOp::Contains => t_string!(i18n, grid_query_contains).to_string(),
        FilterOp::Gte => t_string!(i18n, grid_query_gte).to_string(),
        FilterOp::Lte => t_string!(i18n, grid_query_lte).to_string(),
        FilterOp::Lt => t_string!(i18n, grid_query_lt).to_string(),
        FilterOp::Between => t_string!(i18n, grid_query_between).to_string(),
        FilterOp::Missing => t_string!(i18n, grid_query_missing).to_string(),
        FilterOp::Present => t_string!(i18n, grid_query_present).to_string(),
        FilterOp::Range => t_string!(i18n, grid_query_between).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::metrics::ValueKind;
    use super::*;
    use ultros_grid_core::units::Unit;

    #[test]
    fn metric_sort_replaces_native_sort_and_preserves_the_rest_of_the_url() {
        let query = params(&[
            ("sort", "profit"),
            ("dir", "asc"),
            ("window", "30"),
            ("cols", "market-sale-median-7,market-world"),
            ("gf", r#"{"market-quality":{"op":"eq","value":"HQ"}}"#),
            ("l", "saved-layout"),
            ("world", "Gilgamesh"),
        ]);
        for dir in ["desc", "asc"] {
            let href = metric_sort_href(
                "/venture-analyzer",
                query.clone(),
                "market-sale-median-7",
                dir,
            );
            let expected_sort = params(&[("sort", "grid:market-sale-median-7")]).to_query_string();
            assert!(href.contains(&expected_sort[1..]), "{href}");
            assert!(href.contains(&format!("dir={dir}")), "{href}");
            assert_eq!(href.matches("sort=").count(), 1);
            assert_eq!(href.matches("dir=").count(), 1);
            for key in ["window", "cols", "gf", "l", "world"] {
                let expected = params(&[(key, &query.get(key).unwrap())]).to_query_string();
                assert!(href.contains(&expected[1..]), "{href}");
            }
        }
    }

    #[test]
    fn metric_header_renders_a_sort_link_during_ssr() {
        Owner::new().with(|| {
            let html = view! {
                <MetricSortHeader column="market-sale-median-7" label=Signal::derive(|| "7d median".to_string()) />
            }.to_html();
            assert!(html.contains("href=\"?sort=grid"), "{html}");
            assert!(html.contains("market-sale-median-7"), "{html}");
            assert!(html.contains("dir=desc"), "{html}");
            assert!(html.contains("7d median"), "{html}");
        });
    }

    #[test]
    fn range_chips_read_like_the_editor_wrote_them() {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            let chip = |op, value: &str, unit| {
                range_chip(
                    &MetricFilter {
                        op,
                        value: value.into(),
                    },
                    unit,
                )
            };
            assert_eq!(
                chip(FilterOp::Range, "1200000,", Unit::Gil).as_deref(),
                Some("≥ 1.2M")
            );
            assert_eq!(
                chip(FilterOp::Gte, "1200000", Unit::Gil).as_deref(),
                Some("≥ 1.2M")
            );
            assert_eq!(
                chip(FilterOp::Range, ",40", Unit::Percent).as_deref(),
                Some("≤ 40%")
            );
            assert_eq!(
                chip(FilterOp::Between, "100,500", Unit::Plain).as_deref(),
                Some("100 – 500")
            );
            assert_eq!(
                chip(FilterOp::Lt, "86400", Unit::Seconds).as_deref(),
                Some("< 1d")
            );
            assert_eq!(
                chip(FilterOp::Range, "-7d,", Unit::Timestamp).as_deref(),
                Some("last 7d")
            );
            assert_eq!(
                chip(FilterOp::Range, "1756684800,", Unit::Timestamp).as_deref(),
                Some("after 2025-09-01 00:00")
            );
            assert_eq!(
                chip(FilterOp::Range, ",1756684800", Unit::Timestamp).as_deref(),
                Some("before 2025-09-01 00:00")
            );
            assert_eq!(chip(FilterOp::Eq, "HQ", Unit::Plain), None);
            assert_eq!(chip(FilterOp::Range, ",", Unit::Plain), None);
        });
    }

    fn editor_html(kind: ValueKind, unit: Unit) -> String {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            let mut filter = ColumnFilter::metric("profit", "Profit".into(), kind);
            filter.unit = unit;
            view! { <ColumnFilterEditor filter/> }.to_html()
        })
    }

    #[test]
    fn numeric_editors_offer_a_min_and_max_with_their_unit() {
        let html = editor_html(ValueKind::Number, Unit::Gil);
        assert!(html.contains(r#"data-unit="gil""#), "{html}");
        assert!(html.contains(r#"aria-label="Min""#), "{html}");
        assert!(html.contains(r#"aria-label="Max""#), "{html}");
        assert!(html.contains(r#"placeholder="1.2m""#), "{html}");
        assert!(html.contains(">gil<"), "{html}");
        assert!(
            html.contains("Has a value") && html.contains("No value"),
            "{html}"
        );
        assert!(!html.contains("role=\"tablist\""), "{html}");
        // Nothing to clear yet.
        assert!(!html.contains("data-clear-filter"), "{html}");
    }

    #[test]
    fn timestamp_editors_offer_within_presets_and_text_editors_offer_operators() {
        let html = editor_html(ValueKind::Number, Unit::Timestamp);
        assert!(html.contains(r#"role="tablist""#), "{html}");
        for preset in WITHIN_PRESETS {
            assert!(html.contains(&format!(">{preset}<")), "{preset}: {html}");
        }
        let html = editor_html(ValueKind::Text, Unit::Plain);
        assert!(html.contains("<select"), "{html}");
        assert!(!html.contains(r#"aria-label="Min""#), "{html}");
    }

    #[test]
    fn sort_controls_are_labelled_icon_links() {
        let _ = any_spawner::Executor::init_futures_executor();
        Owner::new().with(|| {
            let html = view! { <MetricSortControls column="profit"/> }.to_html();
            assert_eq!(html.matches("class=\"grid-icon-btn\"").count(), 2, "{html}");
            assert!(html.contains(r#"aria-label="Sort ascending""#), "{html}");
            assert!(html.contains(r#"aria-label="Sort descending""#), "{html}");
            assert!(
                html.contains("dir=asc") && html.contains("dir=desc"),
                "{html}"
            );
        });
    }

    fn params(pairs: &[(&'static str, &str)]) -> ParamsMap {
        let mut q = ParamsMap::new();
        for (k, v) in pairs {
            q.insert(*k, (*v).to_string());
        }
        q
    }

    fn plain(key: &'static str) -> ColumnFilter {
        ColumnFilter::new(key, key.to_string(), false)
    }

    fn metric(key: &'static str) -> ColumnFilter {
        ColumnFilter::metric(key, key.to_string(), ValueKind::Number)
    }

    #[test]
    fn a_plain_filter_drops_its_key_and_leaves_the_rest_alone() {
        let q = cleared_query(
            &params(&[("world", "Gilgamesh"), ("sort", "profit")]),
            &[plain("world")],
        );
        assert_eq!(q.get("world"), None);
        assert_eq!(q.get("sort").as_deref(), Some("profit"));
    }

    #[test]
    fn an_unlimited_default_clears_to_an_explicit_empty_value() {
        // Removing the key outright would let the landing default seed itself
        // back in, so "cleared" has to be spelled out.
        let q = cleared_query(&params(&[("min-sales", "3")]), &[plain("min-sales")]);
        assert_eq!(q.get("min-sales").as_deref(), Some(""));
    }

    #[test]
    fn a_metric_filter_drops_out_of_gf_without_disturbing_its_neighbours() {
        let gf =
            r#"{"grid:profit":{"op":"gte","value":"100"},"grid:roi":{"op":"gte","value":"5"}}"#;
        let q = cleared_query(&params(&[("gf", gf)]), &[metric("grid:profit")]);
        let left = parse_filters(q.get("gf").as_deref());
        assert!(!left.contains_key("grid:profit"), "{left:?}");
        assert_eq!(left.get("grid:roi").map(|f| f.value.as_str()), Some("5"));
    }

    #[test]
    fn gf_disappears_once_its_last_filter_is_cleared() {
        let gf = r#"{"grid:profit":{"op":"gte","value":"100"}}"#;
        let q = cleared_query(&params(&[("gf", gf)]), &[metric("grid:profit")]);
        assert_eq!(q.get("gf"), None);
    }

    #[test]
    fn one_call_clears_every_filter_a_column_carries() {
        // Cost / unit carries four; the popover only offers them one at a time.
        let gf = r#"{"grid:cost":{"op":"lte","value":"9"}}"#;
        let q = cleared_query(
            &params(&[
                ("gf", gf),
                ("cost-basis", "listing"),
                ("subcrafts", "1"),
                ("min-sales", "3"),
            ]),
            &[
                metric("grid:cost"),
                plain("cost-basis"),
                plain("subcrafts"),
                plain("min-sales"),
            ],
        );
        assert_eq!(q.get("gf"), None);
        assert_eq!(q.get("cost-basis"), None);
        assert_eq!(q.get("subcrafts"), None);
        assert_eq!(q.get("min-sales").as_deref(), Some(""));
    }

    #[test]
    fn a_column_with_no_live_filter_leaves_gf_untouched() {
        let gf = r#"{"grid:roi":{"op":"gte","value":"5"}}"#;
        let q = cleared_query(&params(&[("gf", gf)]), &[metric("grid:profit")]);
        assert_eq!(q.get("gf").as_deref(), Some(gf));
    }
}
