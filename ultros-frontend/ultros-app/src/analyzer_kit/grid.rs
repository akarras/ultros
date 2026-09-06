//! Analyzer column metadata rendered by the shared two-dimensional grid.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

use leptos::prelude::*;
use leptos_i18n::I18nContext;

use crate::components::icon::Icon;
use crate::components::sort_header::{SortColumn, SortDir, SortableHeaderCell};
use crate::components::term_badge::TermRole;
use crate::components::virtual_grid::metrics::{GridMetric, GridValue};
use crate::components::virtual_grid::{ColumnFilter, GridColumn, query_grid::QueryGrid};
use crate::i18n::*;
use icondata as i;
use thousands::Separable;

use super::cells::{CellValue, render_cell};
use super::columns::{CellCtx, ColumnKind, Sortability, ToolColumnMeta};

/// A row a grid can render: whatever [`QueryGrid`] needs of it,
/// plus the identity its keyed `<For>` diffs on.
pub trait AnalyzerRow: Clone + Send + Sync + PartialEq + 'static {
    type Key: Clone + Send + Sync + Eq + Hash + 'static;
    fn key(&self) -> Self::Key;
}

/// Renders the cells whose extractor returned [`CellValue::Custom`].
/// Named, rather than written inline on the prop, because the closure
/// type trips `clippy::type_complexity` at every use site.
pub type CustomCell<T> = Arc<dyn Fn(&T, ColumnKind, &'static str) -> AnyView + Send + Sync>;
pub type CustomMeasure<T> = Arc<dyn Fn(&T, ColumnKind) -> (String, f64) + Send + Sync>;
pub type CustomValue<T> = Arc<dyn Fn(&T, ColumnKind) -> GridValue + Send + Sync>;

/// The sub-label a page hangs off each marked formula column's header
/// (`"listing · Aether"`, `"per unit · after 5% tax"`). A column with
/// no entry here is not marked, and renders exactly as it did before
/// marks existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkLabels {
    pub labels: HashMap<TermRole, String>,
}

/// The "use" pill on an alternative-signal header: pressed when that
/// signal is the selected input (the button is then disabled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderPill {
    pub aria: String,
    pub pressed: bool,
}

/// Line 2 of a header: `‹short signal› · ‹place›`, `"(= Cost / unit)"`, or
/// the window and source of a market column (`"7d · Gilgamesh"`). The pill
/// is the alternative-signal columns' "use" button; a column that has no
/// formula input to write leaves it `None` and renders text only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderLine2 {
    pub sub_label: String,
    pub pill: Option<HeaderPill>,
}

/// What a page hangs off a header: a hover title, optionally line 2, and
/// optionally the classes to use *while this extra is in effect* — a column
/// that becomes two-line only under a lab cannot carry the two-line width
/// in its static `header_class` without moving the flag-off DOM. Columns
/// with no entry render exactly as they did before this existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderExtra {
    pub title: String,
    pub line2: Option<HeaderLine2>,
    pub header_class: Option<&'static str>,
}

/// Header extras by column kind. Looked up by key only, never iterated.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HeaderExtras {
    pub by_kind: HashMap<ColumnKind, HeaderExtra>,
}

/// Line 2's own classes on a header the grid draws itself (an unsortable
/// one). The same class string `SortableHeaderCell` puts on its sub-label
/// (`sort_header.rs:273`), so the two kinds of header line up — but on a
/// `<span>`, where that one uses a `<div>`. `truncate` and `max-w-full`
/// only bite once the span is a flex item, and unlike `SortableHeaderCell`
/// this arm does not append `flex flex-col` of its own, so a column using
/// this arm has to supply the column direction in its own `header_class`.
const HEADER_SUB_LINE: &str = "text-[10px] leading-3 font-normal normal-case text-[color:var(--color-text-muted)] truncate max-w-full";

const PILL_OFF: &str = "inline-flex items-center gap-0.5 shrink-0 rounded-full border border-[color:var(--color-outline)] px-1.5 text-[10px] leading-3 font-medium text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] hover:border-[color:var(--brand-ring)]";
const PILL_ON: &str = "inline-flex items-center gap-0.5 shrink-0 rounded-full border border-[color:var(--brand-ring)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)] px-1.5 text-[10px] leading-3 font-medium text-[color:var(--brand-fg)]";

/// `<button type=button aria-pressed>`: pressing it writes one URL param
/// on the page (`on_pill`), which moves the badge, tint and sub-label to
/// the slot header; the pressed column stays on screen as a muted
/// duplicate with its pill filled and disabled.
fn pill_view(
    kind: ColumnKind,
    pill: HeaderPill,
    on_pill: Option<Callback<ColumnKind>>,
    i18n: I18nContext<Locale, I18nKeys>,
) -> AnyView {
    let pressed = pill.pressed;
    view! {
        <button
            type="button"
            class=if pressed { PILL_ON } else { PILL_OFF }
            aria-pressed=if pressed { "true" } else { "false" }
            aria-label=pill.aria
            disabled=pressed
            on:click=move |ev| {
                ev.prevent_default();
                ev.stop_propagation();
                if let Some(cb) = on_pill {
                    cb.run(kind);
                }
            }
        >
            <Icon icon=i::AiCalculatorOutlined width="0.9em" height="0.9em" />
            <span>{t_string!(i18n, analyzer_use_pill).to_string()}</span>
        </button>
    }
    .into_any()
}

/// Reads the marks in effect without cloning them: this runs once per
/// cell, and `Signal::get` would clone the whole label map each time.
/// `None` means either no `marks` prop or no marks right now.
fn with_marks<U>(
    marks: Option<Signal<Option<MarkLabels>>>,
    f: impl FnOnce(&MarkLabels) -> U,
) -> Option<U> {
    marks?.with(|m| m.as_ref().map(f))
}

/// The formula role this column plays right now: `Some` only when the
/// column declares a `side` and the marks in effect carry a label for
/// it. The label map is looked up by key and never iterated, so no
/// `HashMap` ordering can reach the DOM.
fn marked_role<T: 'static, M: 'static>(
    col: &'static ToolColumnMeta<T, M>,
    marks: Option<Signal<Option<MarkLabels>>>,
) -> Option<TermRole> {
    let role = col.side?;
    with_marks(marks, |m| m.labels.contains_key(&role))?.then_some(role)
}

/// A marked column's classes: the wider formula variant, falling back to
/// the plain one when the table left the variant empty — a header and its
/// cells must never end up on different widths.
fn marked_class(marked: bool, formula: &'static str, plain: &'static str) -> &'static str {
    if marked && !formula.is_empty() {
        formula
    } else {
        plain
    }
}

fn header_cell<T: 'static, M: SortColumn>(
    col: &'static ToolColumnMeta<T, M>,
    sort_mode: Signal<Option<M>>,
    sort_dir: Signal<Option<SortDir>>,
    i18n: I18nContext<Locale, I18nKeys>,
    marks: Option<Signal<Option<MarkLabels>>>,
    extras: Option<Signal<HeaderExtras>>,
    on_pill: Option<Callback<ColumnKind>>,
) -> AnyView {
    let label_fn = col.spec.label;
    let kind = col.spec.kind;
    let role = marked_role(col, marks);
    let class = marked_class(role.is_some(), col.formula_header_class, col.header_class);
    // One lookup for every path. The marked arm ignores it, so those columns
    // and the always-on unsortable ones gain a subscription on `extras` they
    // did not have: free while the toggle is off (the memo is a constant
    // empty map and `Memo` suppresses equal values) and one re-render per
    // sell-world change with it on. The map is keyed by kind, so no
    // iteration order can reach the DOM.
    let extra = extras.and_then(|e| e.with(|e| e.by_kind.get(&kind).cloned()));
    match (col.sort, role) {
        // Marked: the badge names the operator, the sub-label says which
        // price this is, and the tint plus hairline tie it to the strip.
        (Sortability::By(mode), Some(role)) => view! {
            <SortableHeaderCell embedded=true
                mode=mode
                label=label_fn(i18n)
                // The wide variant is still only `w-40`: clip a long
                // label (de/ja) rather than widen the column, and hand
                // the whole label back on hover.
                title=label_fn(i18n)
                class=format!("{} truncate",grid_class(class))
                sort_mode
                sort_dir
                badge=role
                sub_label=Signal::derive(move || {
                    with_marks(marks, |m| m.labels.get(&role).cloned())
                        .flatten()
                        .unwrap_or_default()
                })
                emphasized=Signal::derive(|| true)
            />
        }
        .into_any(),
        (Sortability::By(mode), None) => match extra {
            None => view! {
                <SortableHeaderCell embedded=true mode=mode label=label_fn(i18n) class=grid_class(col.header_class) sort_mode sort_dir />
            }
            .into_any(),
            Some(HeaderExtra { title, line2: None, header_class }) => view! {
                <SortableHeaderCell embedded=true mode=mode label=label_fn(i18n) title=title class=grid_class(header_class.unwrap_or(col.header_class)) sort_mode sort_dir />
            }
            .into_any(),
            Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, pill: None }), header_class }) => view! {
                <SortableHeaderCell embedded=true
                    mode=mode
                    label=label_fn(i18n)
                    title=title
                    class=grid_class(header_class.unwrap_or(col.header_class))
                    sort_mode
                    sort_dir
                    sub_label=Signal::derive(move || sub_label.clone())
                />
            }
            .into_any(),
            Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, pill: Some(pill) }), header_class }) => view! {
                <SortableHeaderCell embedded=true
                    mode=mode
                    label=label_fn(i18n)
                    title=title
                    class=format!("{} truncate", grid_class(header_class.unwrap_or(col.header_class)))
                    sort_mode
                    sort_dir
                    sub_label=Signal::derive(move || sub_label.clone())
                    trailing=ViewFn::from(move || pill_view(kind, pill.clone(), on_pill, i18n))
                />
            }
            .into_any(),
        },
        // Unsortable headers were `t!(..)` on the page (locale-reactive);
        // keep that by resolving the label inside a closure. A lazy column
        // is unsortable for a different reason and renders the same way. A
        // pill on one of these would have no formula input to write, so
        // line 2 renders its text only.
        (Sortability::No | Sortability::LazyNever, _) => match extra {
            None => view! {
                <div  class=grid_class(class)>{move || label_fn(i18n)}</div>
            }
            .into_any(),
            Some(HeaderExtra { title, line2: None, header_class }) => view! {
                <div  class=grid_class(header_class.unwrap_or(class)) title=title>
                    {move || label_fn(i18n)}
                </div>
            }
            .into_any(),
            Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, .. }), header_class }) => view! {
                <div  class=grid_class(header_class.unwrap_or(class)) title=title>
                    <span>{move || label_fn(i18n)}</span>
                    <span class=HEADER_SUB_LINE>{sub_label}</span>
                </div>
            }
            .into_any(),
        },
    }
}

/// One tool's table: a header row and virtualised body rows, both driven
/// by the same `columns` table so a column can never appear in one and
/// not the other.
///
/// Rows retain their stable source index while the grid handles striping.
#[component]
pub fn AnalyzerGrid<T: AnalyzerRow, M: SortColumn>(
    /// The page's whole column table, in DOM order.
    columns: &'static [ToolColumnMeta<T, M>],
    /// Rows paired with their position in the unsorted list, so the page
    /// can stripe them and key them independently of the row value.
    #[prop(into)]
    rows: Signal<Vec<(usize, T)>>,
    /// The `?cols=` set. Columns whose `id` is empty are always on.
    #[prop(into)]
    visible_cols: Signal<HashSet<&'static str>>,
    #[prop(into)] sort_mode: Signal<Option<M>>,
    #[prop(into)] sort_dir: Signal<Option<SortDir>>,
    #[prop(into)] ctx: Signal<CellCtx>,
    /// Draws the [`CellValue::Custom`] cells, keyed by the column's kind
    /// (always-on columns have no `id` to key on).
    custom: CustomCell<T>,
    #[prop(optional)] custom_measure: Option<CustomMeasure<T>>,
    #[prop(optional)] custom_value: Option<CustomValue<T>>,
    #[prop(optional)] on_rows: Option<Callback<Vec<(usize, T)>>>,
    #[prop(default = "recipe-analyzer-grid".to_string(), into)] id: String,
    #[prop(optional, into)] label: String,
    #[prop(optional)] column_filters: Option<Callback<ColumnKind, Vec<ColumnFilter>>>,
    #[prop(default = 60.0)] row_height: f64,
    /// Per-role header sub-labels. `None` leaves every column unmarked.
    #[prop(optional, into)]
    marks: Option<Signal<Option<MarkLabels>>>,
    /// Per-kind header titles and line-2 (sub-label + "use" pill) for the
    /// unmarked sortable columns. `None` leaves every header as it was.
    #[prop(optional, into)]
    extras: Option<Signal<HeaderExtras>>,
    /// Runs when a header pill is pressed, with the column's kind.
    #[prop(optional)]
    on_pill: Option<Callback<ColumnKind>>,
    /// Whether lab-gated columns (`lab.is_some()`) are part of this mount.
    /// Off, they are dropped from the header at build time: a hidden
    /// optional column still writes a `<!>` marker (an `Option` child), so
    /// a `?cols=` contract alone cannot keep the flag-off header
    /// byte-identical. The page remounts the grid on a lab flip.
    #[prop(optional)]
    lab_columns: bool,
    /// Rendered row range for lazy market-data enrichment.
    #[prop(optional)]
    visible_range: Option<RwSignal<(usize, usize)>>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let label = if label.is_empty() {
        t_string!(i18n, recipe_analyzer_title).to_string()
    } else {
        label
    };
    let metrics = columns
        .iter()
        .filter(|col| {
            (col.lab.is_none() || lab_columns) && !matches!(col.spec.kind, ColumnKind::Actions)
        })
        .map(|col| {
            let custom_value = custom_value.clone();
            let value = move |(_, row): &(usize, T)| match (col.cell)(row, &ctx.get()) {
                CellValue::Custom => custom_value
                    .as_ref()
                    .map(|f| f(row, col.spec.kind))
                    .unwrap_or(GridValue::Missing),
                value => query_cell(value, i18n),
            };
            let metric = if matches!(
                col.spec.kind,
                ColumnKind::Item
                    | ColumnKind::ListingWorld
                    | ColumnKind::ListingDc
                    | ColumnKind::HopWorlds
                    | ColumnKind::Confidence
            ) {
                GridMetric::text(grid_id(col), value)
            } else if col.spec.kind == ColumnKind::HopGain {
                GridMetric::mixed(grid_id(col), value)
            } else {
                GridMetric::number(grid_id(col), value)
            };
            if matches!(col.sort, Sortability::LazyNever) {
                metric.partial()
            } else {
                metric
            }
        })
        .collect::<Vec<_>>();
    let defs = Memo::new(move |_| {
        columns
            .iter()
            .filter(|col| col.lab.is_none() || lab_columns)
            .map(|col| {
                let id = grid_id(col);
                let optional = !col.id.is_empty();
                let mut def = GridColumn::new(
                    id,
                    (col.spec.label)(i18n),
                    if col.spec.kind == ColumnKind::Item {
                        330.0
                    } else {
                        140.0
                    },
                    optional,
                    !optional || visible_cols.with(|v| v.contains(col.id)),
                );
                if let Sortability::By(mode) = col.sort
                    && sort_mode.get().unwrap_or_else(M::fallback) == mode
                {
                    def.aria_sort = if sort_dir.get().unwrap_or(col.default_dir) == SortDir::Asc {
                        "ascending"
                    } else {
                        "descending"
                    };
                }
                def.filters = column_filters
                    .map(|f| f.run(col.spec.kind))
                    .unwrap_or_default();
                def
            })
            .collect::<Vec<_>>()
    });
    view! {
        <QueryGrid id label metrics
            on_rows=on_rows.unwrap_or_else(||Callback::new(|_|{}))
            each=rows columns=defs row_height=row_height visible_range=visible_range.unwrap_or_else(|| RwSignal::new((0,0)))
            key=move |(_, row): &(usize, T)| row.key()
            header=move |id| {
                let col = columns.iter().find(|c| grid_id(c) == id).expect("registered column");
                (move || header_cell(col, sort_mode, sort_dir, i18n, marks, extras, on_pill)).into_any()
            }
            view=move |(_, row): (usize, T), id| {
                let col = columns.iter().find(|c| grid_id(c) == id).expect("registered column");
                let custom = custom.clone();
                (move || {
                    let c = ctx.get();
                    let class = if col.spec.kind == ColumnKind::Item { "w-full min-w-0 flex items-center gap-2" } else { "w-full min-w-0 text-right" };
                    let decoration = grid_class(marked_class(marked_role(col, marks).is_some(), col.formula_cell_class, col.cell_class));
                    let cell = match (col.cell)(&row, &c) {
                        CellValue::Custom => custom(&row, col.spec.kind, class),
                        value => render_cell(class, value, i18n, &c).expect("only Custom renders None"),
                    };
                    view! { <div class=decoration>{cell}</div> }
                }).into_any()
            }
            measure=move |(_, row): &(usize, T), id| {
                let col = columns.iter().find(|c| grid_id(c) == id).expect("registered column");
                match (col.cell)(row, &ctx.get_untracked()) {
                    CellValue::Custom => custom_measure.as_ref().map(|f| f(row,col.spec.kind)).unwrap_or_else(|| (String::new(), if col.spec.kind == ColumnKind::Item { 330.0 } else { 140.0 })),
                    value => measure_cell(value, i18n, &ctx.get_untracked()),
                }
            }
        />
    }
}

fn query_cell(value: CellValue, i18n: I18nContext<Locale, I18nKeys>) -> GridValue {
    use super::{cells::Enrich, hop::HopGain};
    match value {
        CellValue::Gil(n) | CellValue::RoiBadge(n) | CellValue::GilWithNote { amount: n, .. } => {
            GridValue::Number(n as f64)
        }
        CellValue::Count(n) => GridValue::Number(n as f64),
        CellValue::LastSoldUnix(n) => {
            if n > 0 {
                GridValue::Number(n as f64)
            } else {
                GridValue::Missing
            }
        }
        CellValue::Confidence(band) => {
            crate::components::confidence_badge::get_confidence_verdict_display(band)
                .map(|(label, _)| GridValue::Text(label.get_text(i18n)))
                .unwrap_or(GridValue::Missing)
        }
        CellValue::GilWithPct { amount, .. } => {
            if amount > 0 {
                GridValue::Number(amount as f64)
            } else {
                GridValue::Missing
            }
        }
        CellValue::MutedGil { amount, capped, .. } => {
            if capped {
                GridValue::Missing
            } else {
                amount
                    .map(|n| GridValue::Number(n as f64))
                    .unwrap_or(GridValue::Missing)
            }
        }
        CellValue::SignedGil { delta, .. } => delta
            .map(|n| GridValue::Number(n as f64))
            .unwrap_or(GridValue::Missing),
        CellValue::LateCount(Enrich::Ready(n)) => GridValue::Number(n as f64),
        CellValue::LateGilWithPct(Enrich::Ready((n, _))) => {
            if n > 0 {
                GridValue::Number(n as f64)
            } else {
                GridValue::Missing
            }
        }
        CellValue::LazyPct(Enrich::Ready(Some(n))) => GridValue::Number(n as f64),
        CellValue::Sparkline(Enrich::Ready(s)) => s
            .delta_pct
            .map(|n| GridValue::Number(n as f64))
            .unwrap_or(GridValue::Missing),
        CellValue::Sparkline(Enrich::Loading) => GridValue::Pending,
        CellValue::LateCount(Enrich::Loading)
        | CellValue::LateGilWithPct(Enrich::Loading)
        | CellValue::LazyPct(Enrich::Loading) => GridValue::Pending,
        CellValue::Sparkline(Enrich::Unavailable)
        | CellValue::LateCount(Enrich::Unavailable)
        | CellValue::LateGilWithPct(Enrich::Unavailable)
        | CellValue::LazyPct(Enrich::Unavailable) => GridValue::Unavailable,
        CellValue::Hop {
            gain: HopGain::Gain(n),
            ..
        } => GridValue::Number(n as f64),
        CellValue::Hop {
            gain: HopGain::Needed,
            ..
        } => GridValue::Text(t_string!(i18n, analyzer_hop_needed).to_string()),
        _ => GridValue::Missing,
    }
}

fn grid_id<T, M>(col: &ToolColumnMeta<T, M>) -> &'static str {
    if !col.id.is_empty() {
        return col.id;
    }
    match col.spec.kind {
        ColumnKind::Item => "item",
        ColumnKind::Profit => "profit",
        ColumnKind::Roi => "roi",
        ColumnKind::CostSlot => "cost",
        ColumnKind::RevenueSlot => "price",
        ColumnKind::SalesPerDay7 => "daily-sales",
        ColumnKind::Actions => "actions",
        _ => col.sort_id,
    }
}

fn grid_class(class: &str) -> String {
    let mut classes = class
        .split_whitespace()
        .filter(|c| {
            !c.starts_with("w-")
                && !c.starts_with("min-w-")
                && !c.starts_with("max-w-")
                && !c.starts_with("p-")
                && !c.starts_with("px-")
                && !c.starts_with("py-")
                && *c != "hidden"
                && *c != "md:block"
                && !c.starts_with("md:w-")
                && *c != "md:flex"
        })
        .collect::<Vec<_>>();
    classes.extend(["w-full", "min-w-0"]);
    if class.split_whitespace().any(|c| c == "md:flex") && !classes.contains(&"flex") {
        classes.push("flex");
    }
    classes.join(" ")
}

fn measure_cell(
    value: CellValue,
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &CellCtx,
) -> (String, f64) {
    use super::cells::Enrich;
    match value {
        CellValue::Gil(n)
        | CellValue::GilWithPct { amount: n, .. }
        | CellValue::GilWithNote { amount: n, .. } => (n.separate_with_commas(), 42.0),
        CellValue::RoiBadge(n) => (format!("{n}%"), 30.0),
        CellValue::Count(n) | CellValue::LateCount(Enrich::Ready(n)) => {
            (n.separate_with_commas(), 24.0)
        }
        CellValue::MutedGil { amount, .. } => (
            amount.map(|n| n.separate_with_commas()).unwrap_or_default(),
            42.0,
        ),
        CellValue::SignedGil { delta, .. } => {
            (delta.map(|n| format!("{n:+}")).unwrap_or_default(), 42.0)
        }
        CellValue::LateGilWithPct(Enrich::Ready((n, _))) => (n.separate_with_commas(), 42.0),
        CellValue::LazyPct(Enrich::Ready(Some(n))) => (format!("{n:+.0}%"), 24.0),
        CellValue::LastSoldUnix(unix) => (
            super::cells::last_sold_label(i18n, unix, ctx.now_unix),
            24.0,
        ),
        CellValue::Confidence(_) => (
            [
                t_string!(i18n, analyzer_confidence_low),
                t_string!(i18n, analyzer_confidence_medium),
                t_string!(i18n, analyzer_confidence_high),
            ]
            .into_iter()
            .max_by_key(|s| s.len())
            .map(|s| s.to_string())
            .unwrap_or_default(),
            32.0,
        ),
        _ => (String::new(), 120.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::columns::{
        ColumnKind, ColumnSpec, Layer, LazyFeed, PickerGroup, sortability_for,
    };
    use leptos_i18n::context::init_i18n_context;
    use std::fmt;

    #[test]
    fn failed_enrichment_retains_rows_and_reports_filter_coverage() {
        use crate::analyzer_kit::cells::Enrich;
        use crate::components::virtual_grid::metrics::{
            FilterOp, GridMetric, MetricFilter, query_rows,
        };
        let owner = Owner::new();
        owner.with(|| {
            let i18n = init_i18n_context::<crate::i18n::Locale>();
            let cells = vec![
                CellValue::Sparkline(Enrich::Unavailable),
                CellValue::LazyPct(Enrich::Unavailable),
                CellValue::LateCount(Enrich::Unavailable),
                CellValue::LateGilWithPct(Enrich::Unavailable),
            ];
            let metric = GridMetric::number("history", move |cell: &CellValue| {
                query_cell(cell.clone(), i18n)
            });
            for op in [FilterOp::Gte, FilterOp::Missing, FilterOp::Present] {
                let filters = [(
                    "history".to_string(),
                    MetricFilter {
                        op,
                        value: "10".into(),
                    },
                )]
                .into_iter()
                .collect();
                let result =
                    query_rows(&cells, std::slice::from_ref(&metric), &filters, None, true);
                assert_eq!(result.rows, cells);
                assert_eq!(result.lacking_data, cells.len());
            }
            assert_eq!(
                query_cell(CellValue::LateCount(Enrich::Missing), i18n),
                GridValue::Missing
            );
        });
    }

    #[derive(Clone, PartialEq)]
    struct Row(i32);
    impl AnalyzerRow for Row {
        type Key = i32;
        fn key(&self) -> i32 {
            self.0
        }
    }
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum Col {
        Profit,
    }
    impl fmt::Display for Col {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("profit")
        }
    }
    impl SortColumn for Col {
        fn fallback() -> Self {
            Col::Profit
        }
    }
    fn label_a(_: I18nContext<Locale, I18nKeys>) -> String {
        "Item".into()
    }
    fn label_b(_: I18nContext<Locale, I18nKeys>) -> String {
        "Profit".into()
    }
    fn label_c(_: I18nContext<Locale, I18nKeys>) -> String {
        "Extra".into()
    }
    static A: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Item,
        label: label_a,
        group: PickerGroup::Other,
    };
    static B: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Profit,
        label: label_b,
        group: PickerGroup::Other,
    };
    static C: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Tax,
        label: label_c,
        group: PickerGroup::Other,
    };
    fn custom_cell(_: &Row, _: &CellCtx) -> CellValue {
        CellValue::Custom
    }
    fn gil(r: &Row, _: &CellCtx) -> CellValue {
        CellValue::Gil(r.0)
    }

    /// Every field at its table-wide default, so each column below
    /// spells out only what it actually differs in.
    const BASE: ToolColumnMeta<Row, Col> = ToolColumnMeta {
        spec: &A,
        id: "",
        sort_id: "",
        sort: Sortability::No,
        default_dir: SortDir::Desc,
        header_class: "",
        cell_class: "",
        default_on: true,
        cell: gil,
        side: None,
        formula_header_class: "",
        formula_cell_class: "",
        lab: None,
    };

    static COLS: [ToolColumnMeta<Row, Col>; 3] = [
        ToolColumnMeta {
            spec: &A,
            header_class: "w-64",
            cell_class: "w-64",
            cell: custom_cell,
            ..BASE
        },
        ToolColumnMeta {
            spec: &B,
            sort_id: "profit",
            sort: sortability_for(Layer::Computed, Some(Col::Profit)),
            header_class: "w-32",
            cell_class: "w-32",
            side: Some(TermRole::Revenue),
            formula_header_class: "w-40 px-3 py-2 leading-tight",
            formula_cell_class: "w-40",
            ..BASE
        },
        ToolColumnMeta {
            spec: &C,
            id: "extra",
            header_class: "w-28",
            cell_class: "w-28",
            default_on: false,
            ..BASE
        },
    ];
    /// A lazy column names a sort id and a mode, and is still unsortable.
    static LAZY_COLS: [ToolColumnMeta<Row, Col>; 1] = [ToolColumnMeta {
        spec: &C,
        id: "extra",
        sort_id: "trend",
        sort: sortability_for(
            Layer::Lazy(LazyFeed::Sparklines { hours: 168 }),
            Some(Col::Profit),
        ),
        header_class: "w-28",
        cell_class: "w-28",
        // A custom cell, so the only markup this test can see besides the
        // header is an inert `<div >` — `BASE`'s Gil cell would
        // put a `<button>` in the body and blunt the assertion below.
        cell: custom_cell,
        ..BASE
    }];

    #[test]
    fn grid_renders_visible_columns_only() {
        // The Profit cell renders `<Gil>`, which reads the i18n context.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let visible = RwSignal::new(HashSet::<&'static str>::new());
            let html = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=visible
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                    custom=Arc::new(|r: &Row, kind: ColumnKind, _class: &'static str| {
                        view! { <div  class="w-64">{format!("custom {kind:?} {}", r.0)}</div> }
                            .into_any()
                    })
                    row_height=60.0

                />
            }
            .to_html();
            assert!(html.contains("custom Item 7"), "{html}");
            assert!(html.contains("Profit"), "{html}");
            assert!(!html.contains("Extra"), "{html}");
            assert_eq!(html.matches("role=\"gridcell\"").count(), 2, "{html}");
            // The sortable Profit header goes through `SortableHeaderCell`,
            // which emits a live `aria-sort`; the plain unsortable Item
            // header does not.
            assert_eq!(html.matches("aria-sort=\"descending\"").count(), 1, "{html}");
        });
    }

    /// The page's range signal reaches the scroller and changes no markup:
    /// the scroller writes it from a client `Effect`, which never runs on
    /// the server.
    #[test]
    fn visible_range_is_optional_and_changes_no_markup() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let range = RwSignal::new((0usize, 0usize));
            let render = |range: Option<RwSignal<(usize, usize)>>| {
                match range {
                    Some(range) => view! {
                        <AnalyzerGrid
                            columns=&COLS
                            rows=Signal::derive(|| vec![(0usize, Row(7))])
                            visible_cols=Signal::derive(HashSet::new)
                            sort_mode=Signal::derive(|| None::<Col>)
                            sort_dir=Signal::derive(|| None::<SortDir>)
                            ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                            custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div  class=class>"x"</div> }.into_any())
                            row_height=60.0

                            visible_range=range
                        />
                    }
                    .to_html(),
                    None => view! {
                        <AnalyzerGrid
                            columns=&COLS
                            rows=Signal::derive(|| vec![(0usize, Row(7))])
                            visible_cols=Signal::derive(HashSet::new)
                            sort_mode=Signal::derive(|| None::<Col>)
                            sort_dir=Signal::derive(|| None::<SortDir>)
                            ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                            custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div  class=class>"x"</div> }.into_any())
                            row_height=60.0

                        />
                    }
                    .to_html(),
                }
            };
            assert_eq!(render(Some(range)), render(None));
            // Untouched on the server: the scroller's writer is an Effect.
            assert_eq!(range.get_untracked(), (0, 0));
        });
    }

    #[test]
    fn grid_renders_optional_columns_when_visible() {
        // The Profit cell renders `<Gil>`, which reads the i18n context.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let visible = RwSignal::new(HashSet::from(["extra"]));
            let html = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=visible
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                    custom=Arc::new(|r: &Row, kind: ColumnKind, _class: &'static str| {
                        view! { <div  class="w-64">{format!("custom {kind:?} {}", r.0)}</div> }
                            .into_any()
                    })
                    row_height=60.0

                />
            }
            .to_html();
            assert!(html.contains("Extra"), "{html}");
            assert_eq!(html.matches("role=\"gridcell\"").count(), 3, "{html}");
        });
    }

    #[test]
    fn marks_switch_the_formula_columns_to_the_wide_two_line_variant() {
        // `TermBadge` builds an I18nContext (spawns an Effect) and `<Gil>`
        // reads it: stand up the executor and the context, as
        // components/list/filter_row.rs's tests do.
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let labels = MarkLabels {
                labels: [(TermRole::Revenue, "listing · Gilgamesh".to_string())]
                    .into_iter()
                    .collect(),
            };
            let html = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=Signal::derive(HashSet::new)
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                    custom=Arc::new(|_: &Row, _: ColumnKind, _: &'static str| {
                        view! { <div ></div> }.into_any()
                    })
                    row_height=60.0

                    marks=Signal::derive(move || Some(labels.clone()))
                />
            }
            .to_html();
            assert!(html.contains("listing · Gilgamesh"), "{html}");
            assert!(html.contains("leading-tight"), "{html}");
            // The marked *cell* class has to reach the row too, or the
            // header and its cells sit on different widths.
            assert!(!html.contains("class=\"w-40\""), "grid geometry must own cell widths: {html}");
            assert!(
                html.contains("shadow-[inset_0_-2px_0_var(--brand-ring)]"),
                "{html}"
            );
        });
    }

    fn label_d(_: I18nContext<Locale, I18nKeys>) -> String {
        "Sale median (7d)".into()
    }
    static D: ColumnSpec = ColumnSpec {
        kind: ColumnKind::CostSignal(crate::analyzer_kit::formula::PriceSignal::SaleMedian),
        label: label_d,
        group: crate::analyzer_kit::columns::PickerGroup::Cost,
    };
    static SIGNAL_COL: ToolColumnMeta<Row, Col> = ToolColumnMeta {
        spec: &D,
        id: "cost-sale-median",
        sort_id: "cost-sale-median",
        sort: sortability_for(Layer::Computed, Some(Col::Profit)),
        header_class: "w-40 px-3 py-2 leading-tight",
        cell_class: "w-40",
        lab: Some("analyzer-recipe"),
        ..BASE
    };

    static COLS_PLUS: [ToolColumnMeta<Row, Col>; 4] = [
        ToolColumnMeta {
            spec: &A,
            header_class: "w-64",
            cell_class: "w-64",
            cell: custom_cell,
            ..BASE
        },
        ToolColumnMeta {
            spec: &B,
            sort_id: "profit",
            sort: sortability_for(Layer::Computed, Some(Col::Profit)),
            header_class: "w-32",
            cell_class: "w-32",
            side: Some(TermRole::Revenue),
            formula_header_class: "w-40 px-3 py-2 leading-tight",
            formula_cell_class: "w-40",
            ..BASE
        },
        ToolColumnMeta {
            spec: &C,
            id: "extra",
            header_class: "w-28",
            cell_class: "w-28",
            default_on: false,
            ..BASE
        },
        ToolColumnMeta {
            spec: &D,
            id: "cost-sale-median",
            sort_id: "cost-sale-median",
            sort: sortability_for(Layer::Computed, Some(Col::Profit)),
            header_class: "w-40 px-3 py-2 leading-tight",
            cell_class: "w-40",
            lab: Some("analyzer-recipe"),
            ..BASE
        },
    ];

    #[test]
    fn header_extras_render_title_sub_label_and_pill() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let kind = SIGNAL_COL.spec.kind;
            let extras = |pressed: bool| {
                let mut by_kind = HashMap::new();
                by_kind.insert(
                    kind,
                    HeaderExtra {
                        title: "The middle price".into(),
                        line2: Some(HeaderLine2 {
                            sub_label: "7d median · Aether".into(),
                            pill: Some(HeaderPill {
                                aria: "Use Sale median (7d) as the cost in Profit".into(),
                                pressed,
                            }),
                        }),
                        header_class: None,
                    },
                );
                Signal::derive(move || HeaderExtras {
                    by_kind: by_kind.clone(),
                })
            };
            let clicked = RwSignal::new(None::<ColumnKind>);
            let on_pill = Callback::new(move |k| clicked.set(Some(k)));
            let none = Signal::derive(|| None::<Col>);
            let none_dir = Signal::derive(|| None::<SortDir>);
            let off = header_cell(
                &SIGNAL_COL,
                none,
                none_dir,
                i18n,
                None,
                Some(extras(false)),
                Some(on_pill),
            )
            .to_html();
            assert!(off.contains("title=\"The middle price\""), "{off}");
            assert!(off.contains("7d median · Aether"), "{off}");
            assert!(off.contains("aria-pressed=\"false\""), "{off}");
            assert!(
                off.contains("aria-label=\"Use Sale median (7d) as the cost in Profit\""),
                "{off}"
            );
            assert!(off.contains(">use<"), "{off}");
            assert!(!off.contains("disabled"), "{off}");
            let on = header_cell(
                &SIGNAL_COL,
                none,
                none_dir,
                i18n,
                None,
                Some(extras(true)),
                Some(on_pill),
            )
            .to_html();
            assert!(
                on.contains("aria-pressed=\"true\"") && on.contains("disabled"),
                "{on}"
            );
            // No extras: the plain sortable header, exactly as before.
            let plain = header_cell(&SIGNAL_COL, none, none_dir, i18n, None, None, None).to_html();
            assert!(
                !plain.contains("<button") && !plain.contains("title="),
                "{plain}"
            );
            // The flag-off page passes `Some(empty map)`: identical by construction.
            let empty = header_cell(
                &SIGNAL_COL,
                none,
                none_dir,
                i18n,
                None,
                Some(Signal::derive(HeaderExtras::default)),
                Some(on_pill),
            )
            .to_html();
            assert_eq!(
                empty, plain,
                "an empty extras map is the flag-off page path"
            );
        });
    }

    /// Line 2 without a pill: the sub-label renders, no button appears, and
    /// the extra's `header_class` replaces the column's while it is in
    /// effect (Daily sales and Confidence become two-line only under the
    /// lab; their flag-off classes must not move).
    #[test]
    fn a_second_line_without_a_pill_renders_no_button() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let mut by_kind = HashMap::new();
            by_kind.insert(
                SIGNAL_COL.spec.kind,
                HeaderExtra {
                    title: "Sales per day over 7 days".into(),
                    line2: Some(HeaderLine2 {
                        sub_label: "7d · Gilgamesh".into(),
                        pill: None,
                    }),
                    header_class: Some("w-28 px-4 py-2 leading-tight hidden md:flex"),
                },
            );
            let extras = Signal::derive(move || HeaderExtras {
                by_kind: by_kind.clone(),
            });
            let html = header_cell(
                &SIGNAL_COL,
                Signal::derive(|| None::<Col>),
                Signal::derive(|| None::<SortDir>),
                i18n,
                None,
                Some(extras),
                None,
            )
            .to_html();
            assert!(
                html.contains("title=\"Sales per day over 7 days\""),
                "{html}"
            );
            assert!(html.contains("7d · Gilgamesh"), "{html}");
            assert!(!html.contains("<button"), "{html}");
            assert!(
                html.contains("leading-tight") && !html.contains("hidden md:flex"),
                "{html}"
            );
        });
    }

    /// An unsortable header renders exactly today's markup with no extra,
    /// and gains a title (and a second line) when the page gives it one.
    #[test]
    fn unsortable_headers_take_a_title_and_a_second_line() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let none = Signal::derive(|| None::<Col>);
            let none_dir = Signal::derive(|| None::<SortDir>);
            // COLS[0] is the unsortable Item column.
            let plain = header_cell(&COLS[0], none, none_dir, i18n, None, None, None).to_html();
            assert!(
                !plain.contains("role=\"columnheader\""),
                "outer grid cell owns the role: {plain}"
            );
            assert!(
                !plain.contains("title=") && !plain.contains("<span"),
                "{plain}"
            );
            let empty = header_cell(
                &COLS[0],
                none,
                none_dir,
                i18n,
                None,
                Some(Signal::derive(HeaderExtras::default)),
                None,
            )
            .to_html();
            assert_eq!(empty, plain, "an empty extras map is the flag-off path");

            let with_line2 = |line2| {
                let mut by_kind = HashMap::new();
                by_kind.insert(
                    COLS[0].spec.kind,
                    HeaderExtra {
                        title: "Hourly price, last 7 days".into(),
                        line2,
                        header_class: None,
                    },
                );
                let extras = Signal::derive(move || HeaderExtras {
                    by_kind: by_kind.clone(),
                });
                header_cell(&COLS[0], none, none_dir, i18n, None, Some(extras), None).to_html()
            };
            let titled = with_line2(None);
            assert!(
                titled.contains("title=\"Hourly price, last 7 days\""),
                "{titled}"
            );
            assert!(!titled.contains("<span"), "{titled}");
            let two_line = with_line2(Some(HeaderLine2 {
                sub_label: "7d · Gilgamesh".into(),
                pill: None,
            }));
            assert!(
                two_line.contains("title=\"Hourly price, last 7 days\""),
                "{two_line}"
            );
            assert!(two_line.contains("7d · Gilgamesh"), "{two_line}");
            assert_eq!(
                two_line.matches("role=\"columnheader\"").count(),
                0,
                "{two_line}"
            );
            // The arm renders `header_class.unwrap_or(class)`, and this arm
            // appends no direction of its own — so whatever the column
            // carries is the whole of what stacks the two lines. The recipe
            // analyzer's lazy columns depend on that reaching the DOM.
            assert!(
                two_line.contains(&grid_class(COLS[0].header_class)),
                "the column's own header_class must reach the rendered header: {two_line}"
            );
        });
    }

    /// A hidden optional column still writes a `<!>` marker into the header
    /// (an `Option` child), so the flag-off header would grow by one marker
    /// per lab column; `lab_columns=false` drops them at build time.
    #[test]
    fn lab_columns_are_absent_from_the_header_unless_enabled() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let render = |cols: &'static [ToolColumnMeta<Row, Col>], lab: bool, visible: &'static [&'static str]| {
                view! {
                    <AnalyzerGrid
                        columns=cols
                        rows=Signal::derive(|| vec![(0usize, Row(1))])
                        visible_cols=Signal::derive(move || visible.iter().copied().collect::<HashSet<_>>())
                        sort_mode=Signal::derive(|| None::<Col>)
                        sort_dir=Signal::derive(|| None::<SortDir>)
                        ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                        custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div  class=class>"x"</div> }.into_any())
                        row_height=10.0

                        lab_columns=lab
                    />
                }
                .to_html()
            };
            let base = render(&COLS, false, &[]);
            let with_lab_col_off = render(&COLS_PLUS, false, &[]);
            assert_eq!(with_lab_col_off, base, "a hidden lab column must add nothing to the flag-off header");
            let with_lab_col_on = render(&COLS_PLUS, true, &["cost-sale-median"]);
            assert!(with_lab_col_on.contains("Sale median (7d)"), "{with_lab_col_on}");
        });
    }

    /// `LazyNever` shares `header_cell`'s unsortable arm with `No`, so it
    /// must render the plain header even though the column names both a
    /// sort id and a sort mode.
    #[test]
    fn a_lazy_column_renders_an_unsortable_header() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let html = view! {
                <AnalyzerGrid
                    columns=&LAZY_COLS
                    rows=Signal::derive(|| vec![(0usize, Row(1))])
                    visible_cols=Signal::derive(|| ["extra"].into_iter().collect::<HashSet<_>>())
                    sort_mode=Signal::derive(|| Some(Col::Profit))
                    sort_dir=Signal::derive(|| Some(SortDir::Desc))
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, preview: false, capped_cost: [false; 4], sparklines: None, stats_30: None, stats_30_unavailable: None })
                    custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div  class=class>"x"</div> }.into_any())
                    row_height=10.0

                    lab_columns=true
                />
            }
            .to_html();
            assert!(html.contains("role=\"columnheader\""), "{html}");
            assert!(
                !html.contains("?sort=") && !html.contains("&sort="),
                "a lazy column must never render a sort control: {html}"
            );
            assert!(!html.contains("aria-sort=\"ascending\"") && !html.contains("aria-sort=\"descending\""), "{html}");
        });
    }
}
