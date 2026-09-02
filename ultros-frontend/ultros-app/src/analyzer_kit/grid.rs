//! The table host: header and rows rendered from a page's static column
//! table over the existing `VirtualScroller`, which needs no changes.
//! Visibility derives only from `?cols=` (URL-borne, identical on server
//! and client) and is read once per row, replacing one gate closure per
//! optional cell per row.

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;

use leptos::prelude::*;
use leptos_i18n::I18nContext;

use crate::components::sort_header::{SortColumn, SortDir, SortableHeaderCell};
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
pub type CustomCell<T> = Arc<dyn Fn(&T, ColumnKind) -> AnyView + Send + Sync>;

fn header_cell<T: 'static, M: SortColumn>(
    col: &'static ToolColumnMeta<T, M>,
    sort_mode: Signal<Option<M>>,
    sort_dir: Signal<Option<SortDir>>,
    i18n: I18nContext<Locale, I18nKeys>,
) -> AnyView {
    let label_fn = col.spec.label;
    match col.sort {
        Sortability::By(mode) => view! {
            <SortableHeaderCell mode=mode label=label_fn(i18n) class=col.header_class sort_mode sort_dir />
        }
        .into_any(),
        // Unsortable headers were `t!(..)` on the page (locale-reactive);
        // keep that by resolving the label inside a closure.
        Sortability::No => view! {
            <div role="columnheader" class=col.header_class>{move || label_fn(i18n)}</div>
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
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();

    let header = view! {
        <div class=header_class role="rowgroup">
            {columns
                .iter()
                .map(|col| {
                    if col.id.is_empty() {
                        header_cell(col, sort_mode, sort_dir, i18n)
                    } else {
                        (move || {
                            visible_cols
                                .get()
                                .contains(col.id)
                                .then(|| header_cell(col, sort_mode, sort_dir, i18n))
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
                                .map(|col| match (col.cell)(&row, &c) {
                                    CellValue::Custom => custom(&row, col.spec.kind),
                                    value => {
                                        render_cell(col.cell_class, value, i18n, &c)
                                            .expect("only Custom renders None")
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
    static COLS: [ToolColumnMeta<Row, Col>; 3] = [
        ToolColumnMeta {
            spec: &A,
            id: "",
            sort_id: "",
            sort: Sortability::No,
            default_dir: SortDir::Desc,
            header_class: "w-64",
            cell_class: "w-64",
            default_on: true,
            cell: custom_cell,
        },
        ToolColumnMeta {
            spec: &B,
            id: "",
            sort_id: "profit",
            sort: sortability_for(Layer::Computed, Some(Col::Profit)),
            default_dir: SortDir::Desc,
            header_class: "w-32",
            cell_class: "w-32",
            default_on: true,
            cell: gil,
        },
        ToolColumnMeta {
            spec: &C,
            id: "extra",
            sort_id: "",
            sort: Sortability::No,
            default_dir: SortDir::Desc,
            header_class: "w-28",
            cell_class: "w-28",
            default_on: false,
            cell: gil,
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
                    custom=Arc::new(|r: &Row, kind: ColumnKind| {
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
                    custom=Arc::new(|r: &Row, kind: ColumnKind| {
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
}
