//! The table host: header and rows rendered from a page's static column
//! table over the existing `VirtualScroller`, which needs no changes.
//! Visibility derives only from `?cols=` (URL-borne, identical on server
//! and client) and is read once per row, replacing one gate closure per
//! optional cell per row.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

use leptos::prelude::*;
use leptos_i18n::I18nContext;

use crate::components::sort_header::{SortColumn, SortDir, SortableHeaderCell};
use crate::components::term_badge::TermRole;
use crate::components::virtual_scroller::VirtualScroller;
use crate::i18n::*;

use super::cells::{CellValue, render_cell};
use super::columns::{CellCtx, ColumnKind, Sortability, ToolColumnMeta};

/// A row a grid can render: whatever [`VirtualScroller`] needs of it,
/// plus the identity its keyed `<For>` diffs on.
pub trait AnalyzerRow: Clone + Send + Sync + PartialEq + 'static {
    type Key: Eq + Hash + 'static;
    fn key(&self) -> Self::Key;
}

/// Fixed geometry the host hands the scroller.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridLayout {
    pub viewport_height: f64,
    pub row_height: f64,
    pub header_height: f64,
    pub overscan: u32,
}

/// Renders the cells whose extractor returned [`CellValue::Custom`].
/// Named, rather than written inline on the prop, because the closure
/// type trips `clippy::type_complexity` at every use site.
pub type CustomCell<T> = Arc<dyn Fn(&T, ColumnKind, &'static str) -> AnyView + Send + Sync>;

/// The sub-label a page hangs off each marked formula column's header
/// (`"listing · Aether"`, `"per unit · after 5% tax"`). A column with
/// no entry here is not marked, and renders exactly as it did before
/// marks existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkLabels {
    pub labels: HashMap<TermRole, String>,
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
) -> AnyView {
    let label_fn = col.spec.label;
    let role = marked_role(col, marks);
    let class = marked_class(role.is_some(), col.formula_header_class, col.header_class);
    match (col.sort, role) {
        // Marked: the badge names the operator, the sub-label says which
        // price this is, and the tint plus hairline tie it to the strip.
        (Sortability::By(mode), Some(role)) => view! {
            <SortableHeaderCell
                mode=mode
                label=label_fn(i18n)
                // The wide variant is still only `w-40`: clip a long
                // label (de/ja) rather than widen the column, and hand
                // the whole label back on hover.
                title=label_fn(i18n)
                class=format!("{class} truncate")
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
        (Sortability::By(mode), None) => view! {
            <SortableHeaderCell mode=mode label=label_fn(i18n) class=col.header_class sort_mode sort_dir />
        }
        .into_any(),
        // Unsortable headers were `t!(..)` on the page (locale-reactive);
        // keep that by resolving the label inside a closure.
        (Sortability::No, _) => view! {
            <div role="columnheader" class=class>{move || label_fn(i18n)}</div>
        }
        .into_any(),
    }
}

/// One tool's table: a header row and virtualised body rows, both driven
/// by the same `columns` table so a column can never appear in one and
/// not the other.
///
/// Rows carry their index because the pages stripe by it; `row_class` is
/// a `fn` rather than a closure so it stays copyable into the scroller's
/// row view.
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
    layout: GridLayout,
    header_class: &'static str,
    row_class: fn(usize) -> &'static str,
    /// Per-role header sub-labels. `None` leaves every column unmarked.
    #[prop(optional, into)]
    marks: Option<Signal<Option<MarkLabels>>>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();

    let header = view! {
        <div class=header_class role="rowgroup">
            {columns
                .iter()
                .map(|col| {
                    if col.id.is_empty() {
                        // Reactive even though visibility is fixed: a
                        // marked column has to re-render when the marks
                        // (or their labels) change.
                        (move || header_cell(col, sort_mode, sort_dir, i18n, marks)).into_any()
                    } else {
                        (move || {
                            visible_cols
                                .get()
                                .contains(col.id)
                                .then(|| header_cell(col, sort_mode, sort_dir, i18n, marks))
                        })
                            .into_any()
                    }
                })
                .collect_view()}
        </div>
    }
    .into_any();

    view! {
        <VirtualScroller
            viewport_height=layout.viewport_height
            row_height=layout.row_height
            overscan=layout.overscan
            header_height=layout.header_height
            variable_height=false
            header=header
            each=rows
            key=move |(index, row): &(usize, T)| (*index, row.key())
            view=move |(index, row): (usize, T)| {
                let custom = custom.clone();
                view! {
                    // `role="row-group"` verbatim from the analyzer tables
                    // this replaces; changing it is a separate change.
                    <div class=row_class(index) role="row-group">
                        {move || {
                            let vis = visible_cols.get();
                            let c = ctx.get();
                            columns
                                .iter()
                                .filter(|col| col.id.is_empty() || vis.contains(col.id))
                                .map(|col| {
                                    let class = marked_class(
                                        marked_role(col, marks).is_some(),
                                        col.formula_cell_class,
                                        col.cell_class,
                                    );
                                    match (col.cell)(&row, &c) {
                                        CellValue::Custom => custom(&row, col.spec.kind, class),
                                        value => {
                                            render_cell(class, value, i18n, &c)
                                                .expect("only Custom renders None")
                                        }
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                }
            }
        />
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_kit::columns::{ColumnKind, ColumnSpec, Layer, sortability_for};
    use leptos_i18n::context::init_i18n_context;
    use std::fmt;

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
    };
    static B: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Profit,
        label: label_b,
    };
    static C: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Tax,
        label: label_c,
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
    fn stripe(_: usize) -> &'static str {
        "row"
    }

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
                    ctx=Signal::derive(|| CellCtx { now_unix: 0 })
                    custom=Arc::new(|r: &Row, kind: ColumnKind, _class: &'static str| {
                        view! { <div role="cell" class="w-64">{format!("custom {kind:?} {}", r.0)}</div> }
                            .into_any()
                    })
                    layout=GridLayout {
                        viewport_height: 720.0,
                        row_height: 60.0,
                        header_height: 64.0,
                        overscan: 8,
                    }
                    header_class="thead"
                    row_class=stripe
                />
            }
            .to_html();
            assert!(html.contains("custom Item 7"), "{html}");
            assert!(html.contains("Profit"), "{html}");
            assert!(!html.contains("Extra"), "{html}");
            assert_eq!(html.matches("role=\"cell\"").count(), 2, "{html}");
            // The sortable Profit header goes through `SortableHeaderCell`,
            // which emits a live `aria-sort`; the plain unsortable Item
            // header does not.
            assert_eq!(html.matches("aria-sort=").count(), 1, "{html}");
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
                    ctx=Signal::derive(|| CellCtx { now_unix: 0 })
                    custom=Arc::new(|r: &Row, kind: ColumnKind, _class: &'static str| {
                        view! { <div role="cell" class="w-64">{format!("custom {kind:?} {}", r.0)}</div> }
                            .into_any()
                    })
                    layout=GridLayout {
                        viewport_height: 720.0,
                        row_height: 60.0,
                        header_height: 64.0,
                        overscan: 8,
                    }
                    header_class="thead"
                    row_class=stripe
                />
            }
            .to_html();
            assert!(html.contains("Extra"), "{html}");
            assert_eq!(html.matches("role=\"cell\"").count(), 3, "{html}");
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
                    ctx=Signal::derive(|| CellCtx { now_unix: 0 })
                    custom=Arc::new(|_: &Row, _: ColumnKind, _: &'static str| {
                        view! { <div role="cell"></div> }.into_any()
                    })
                    layout=GridLayout {
                        viewport_height: 720.0,
                        row_height: 60.0,
                        header_height: 64.0,
                        overscan: 8,
                    }
                    header_class="thead"
                    row_class=stripe
                    marks=Signal::derive(move || Some(labels.clone()))
                />
            }
            .to_html();
            assert!(html.contains("listing · Gilgamesh"), "{html}");
            assert!(html.contains("w-40 px-3 py-2 leading-tight"), "{html}");
            // The marked *cell* class has to reach the row too, or the
            // header and its cells sit on different widths.
            assert!(html.contains("class=\"w-40\""), "{html}");
            assert!(
                html.contains("shadow-[inset_0_-2px_0_var(--brand-ring)]"),
                "{html}"
            );
        });
    }
}
