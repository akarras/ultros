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

use crate::components::icon::Icon;
use crate::components::sort_header::{SortColumn, SortDir, SortableHeaderCell};
use crate::components::term_badge::TermRole;
use crate::components::virtual_scroller::VirtualScroller;
use crate::i18n::*;
use icondata as i;

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

/// The "use" pill on an alternative-signal header: pressed when that
/// signal is the selected input (the button is then disabled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderPill {
    pub aria: String,
    pub pressed: bool,
}

/// Line 2 of an alternative-signal header: `‹short signal› · ‹place›` (or
/// "(= Cost / unit)") plus the pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderLine2 {
    pub sub_label: String,
    pub pill: HeaderPill,
}

/// What a page hangs off an unmarked sortable header: a hover title and,
/// for the signal columns, line 2. Columns with no entry render exactly as
/// they did before this existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderExtra {
    pub title: String,
    pub line2: Option<HeaderLine2>,
}

/// Header extras by column kind. Looked up by key only, never iterated.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HeaderExtras {
    pub by_kind: HashMap<ColumnKind, HeaderExtra>,
}

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
        (Sortability::By(mode), None) => {
            let kind = col.spec.kind;
            let extra = extras.and_then(|e| e.with(|e| e.by_kind.get(&kind).cloned()));
            match extra {
                None => view! {
                    <SortableHeaderCell mode=mode label=label_fn(i18n) class=col.header_class sort_mode sort_dir />
                }
                .into_any(),
                Some(HeaderExtra { title, line2: None }) => view! {
                    <SortableHeaderCell mode=mode label=label_fn(i18n) title=title class=col.header_class sort_mode sort_dir />
                }
                .into_any(),
                Some(HeaderExtra { title, line2: Some(HeaderLine2 { sub_label, pill }) }) => view! {
                    <SortableHeaderCell
                        mode=mode
                        label=label_fn(i18n)
                        title=title
                        class=format!("{} truncate", col.header_class)
                        sort_mode
                        sort_dir
                        sub_label=Signal::derive(move || sub_label.clone())
                        trailing=ViewFn::from(move || pill_view(kind, pill.clone(), on_pill, i18n))
                    />
                }
                .into_any(),
            }
        }
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
    /// Per-kind header titles and line-2 (sub-label + "use" pill) for the
    /// unmarked sortable columns. `None` leaves every header as it was.
    #[prop(optional, into)]
    extras: Option<Signal<HeaderExtras>>,
    /// Runs when a header pill is pressed, with the column's kind.
    #[prop(optional)]
    on_pill: Option<Callback<ColumnKind>>,
    /// CSS `min-width` for the scroller's row spacer, for a table whose
    /// columns are wider than the viewport.
    ///
    /// The scroller's row box carries `contain: layout`, so widening the rows
    /// alone never reaches the scroller's scrollable overflow region — sizing
    /// the spacer that holds them is what actually lets the rows paint past
    /// the port width instead of being clipped at it while the header (a
    /// sibling outside that box) keeps painting the full grid.
    ///
    /// `"max-content"` is the right value for a table of fixed-width `w-*`
    /// columns: it tracks the real total on its own, including columns that
    /// only exist above a breakpoint (`hidden md:block`), so no arithmetic
    /// here has to be kept in step with the column table. Omitting it leaves
    /// the spacer unsized, exactly as before this prop existed.
    #[prop(optional, into)]
    row_min_width: String,
    /// Whether lab-gated columns (`lab.is_some()`) are part of this mount.
    /// Off, they are dropped from the header at build time: a hidden
    /// optional column still writes a `<!>` marker (an `Option` child), so
    /// a `?cols=` contract alone cannot keep the flag-off header
    /// byte-identical. The page remounts the grid on a lab flip.
    #[prop(optional)]
    lab_columns: bool,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();

    let header = view! {
        <div class=header_class role="rowgroup">
            {columns
                .iter()
                .filter(|col| col.lab.is_none() || lab_columns)
                .map(|col| {
                    if col.id.is_empty() {
                        // Reactive even though visibility is fixed: a
                        // marked column has to re-render when the marks
                        // (or their labels) change.
                        (move || {
                            header_cell(col, sort_mode, sort_dir, i18n, marks, extras, on_pill)
                        })
                            .into_any()
                    } else {
                        (move || {
                            visible_cols.get().contains(col.id).then(|| {
                                header_cell(col, sort_mode, sort_dir, i18n, marks, extras, on_pill)
                            })
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
            row_min_width=row_min_width
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
    use crate::analyzer_kit::columns::{
        ColumnKind, ColumnSpec, Layer, PickerGroup, sortability_for,
    };
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
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
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
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
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
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
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
        lab: Some("analyzer-signal-columns"),
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
            lab: Some("analyzer-signal-columns"),
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
                            pill: HeaderPill {
                                aria: "Use Sale median (7d) as the cost in Profit".into(),
                                pressed,
                            },
                        }),
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

    /// A grid whose columns are wider than the viewport has to size the
    /// scroller's row spacer, or the rows are clipped at the port width while
    /// the header (outside that box) keeps painting the full grid.
    #[test]
    fn row_min_width_reaches_the_scrollers_spacer() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(init_i18n_context::<crate::i18n::Locale>());
            let with_min = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=Signal::derive(HashSet::new)
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
                    custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div role="cell" class=class>"x"</div> }.into_any())
                    layout=GridLayout { viewport_height: 100.0, row_height: 10.0, header_height: 10.0, overscan: 1 }
                    header_class="h"
                    row_class=stripe
                    row_min_width="max-content"
                />
            }
            .to_html();
            assert!(with_min.contains("min-width: max-content;"), "{with_min}");

            // Omitting it forwards `String::default()`, which must not reach
            // the spacer as an empty `min-width: ;` declaration.
            let without = view! {
                <AnalyzerGrid
                    columns=&COLS
                    rows=Signal::derive(|| vec![(0usize, Row(7))])
                    visible_cols=Signal::derive(HashSet::new)
                    sort_mode=Signal::derive(|| None::<Col>)
                    sort_dir=Signal::derive(|| None::<SortDir>)
                    ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
                    custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div role="cell" class=class>"x"</div> }.into_any())
                    layout=GridLayout { viewport_height: 100.0, row_height: 10.0, header_height: 10.0, overscan: 1 }
                    header_class="h"
                    row_class=stripe
                />
            }
            .to_html();
            assert!(!without.contains("min-width"), "{without}");
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
                        ctx=Signal::derive(|| CellCtx { now_unix: 0, signal_columns: false, capped_cost: [false; 4] })
                        custom=Arc::new(|_: &Row, _: ColumnKind, class: &'static str| view! { <div role="cell" class=class>"x"</div> }.into_any())
                        layout=GridLayout { viewport_height: 100.0, row_height: 10.0, header_height: 10.0, overscan: 1 }
                        header_class="h"
                        row_class=|_| "r"
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
}
