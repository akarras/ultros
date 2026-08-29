//! One description of a table's columns, shared by the header and the body.
//!
//! # The debt this retires
//!
//! The Item Explorer's "spreadsheet" was an inline `<div role="table">` whose
//! column template lived in **four** hand-copied Tailwind class strings — a
//! header and a body copy, each in a single-world and a multi-world variant.
//! They had to stay character-identical or the columns stopped lining up, and
//! nothing in the compiler or the test suite would notice if they drifted.
//! Adding a column meant editing four long strings by hand. See issue #1080.
//!
//! Here the columns are described once, in order, as [`Column`] values, and
//! the grid template is *derived* from that one list for every breakpoint.
//!
//! # Substrate decision: `role="table"` div grid
//!
//! Two shapes existed in the app: the Item Explorer's `role="table"` div grid
//! and the Currency Exchange's real `<table>`. The div grid is the primary
//! substrate here ([`DataTableGrid`]) because it is the one that supports a
//! *responsive column set* — the explorer shows four columns on a phone,
//! seven at `lg` and eight or nine at `xl`, out of the same DOM — which a
//! real `<table>` cannot express without either duplicating the markup or
//! collapsing to `display: block` and losing the column alignment that is the
//! whole point.
//!
//! A real `<table>` still earns its keep where columns are content-sized and
//! toggled by the user rather than by the viewport (the Currency Exchange's
//! `?cols=`): `<table>` auto-layout sizes those columns to their content, a
//! grid template cannot without inventing widths, and the empty state's
//! `colspan` has no grid equivalent. So [`Column`] is substrate-neutral, and
//! [`header_cells`] / [`body_cells`] / [`visible_column_count`] let such a
//! page keep its `<table>` while still describing its columns once. That is
//! the *only* sanctioned second substrate; new tables should use
//! [`DataTableGrid`].
//!
//! # Why the grid template is a CSS variable, not a Tailwind class
//!
//! Tailwind scans the *source* for class candidates. A `grid-cols-[…]` class
//! assembled at runtime never appears in any source file, so no rule would be
//! generated for it and the table would lay out as a single column. The
//! derived template is therefore written to `--dt-cols` / `--dt-cols-lg` /
//! `--dt-cols-xl` in the element's `style`, and `.dt-grid-row` in
//! `style/tailwind.css` reads them at the matching breakpoints. Same
//! precedent as the Flip Finder's `--tool-optional-cols`. The computed
//! `grid-template-columns` is identical to what the four class strings
//! produced — see the tests at the bottom of this file, which assert the
//! derivation against those strings verbatim.
//!
//! # Hydration rules
//!
//! * Columns are an ordered [`Vec`]. Never key column order off a map — a
//!   `HashMap`/`HashSet` iteration reaching the DOM is an SSR/CSR mismatch and
//!   a hard panic in tachys' hydration walker.
//! * This module reads **no page context**, and never `.expect()`s one. A
//!   panicking context read copied into shared code turns one page's bug into
//!   a site-wide one; page-specific reads stay in the page's cell closures.
//! * On the grid substrate an invisible column keeps its header cell in the
//!   DOM (rendered `class="hidden"`) and only drops out of the grid template,
//!   so element shape and count do not depend on the flag. On the `<table>`
//!   substrate an invisible column is omitted entirely — there the flag is
//!   `?cols=`, which is read from the URL and therefore identical on the
//!   server and the client.

use std::sync::Arc;

use leptos::prelude::*;

/// The grid track a column contributes at each breakpoint the grid substrate
/// understands. `None` means the column is not part of the template there —
/// it is `display: none` at that width and must not occupy a track.
///
/// Values are raw CSS track sizes (`"2.5rem"`, `"minmax(0,1fr)"`, `"auto"`),
/// joined with spaces. The Tailwind arbitrary-value syntax these replace used
/// `_` where CSS wants a space; nothing else about them changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackWidths {
    pub base: Option<&'static str>,
    pub lg: Option<&'static str>,
    pub xl: Option<&'static str>,
}

impl TrackWidths {
    /// Same track at every breakpoint.
    pub const fn everywhere(width: &'static str) -> Self {
        Self {
            base: Some(width),
            lg: Some(width),
            xl: Some(width),
        }
    }

    /// A column that only exists from `lg` up.
    pub const fn from_lg(lg: &'static str) -> Self {
        Self {
            base: None,
            lg: Some(lg),
            xl: Some(lg),
        }
    }

    /// A column that only exists from `xl` up.
    pub const fn from_xl(xl: &'static str) -> Self {
        Self {
            base: None,
            lg: None,
            xl: Some(xl),
        }
    }

    /// A column whose track differs per breakpoint.
    pub const fn responsive(
        base: Option<&'static str>,
        lg: Option<&'static str>,
        xl: Option<&'static str>,
    ) -> Self {
        Self { base, lg, xl }
    }
}

/// What goes in a column's header cell.
pub enum ColumnHeader {
    /// A spacer column: a header cell with no content (icon and action
    /// columns). Still emitted, so the header has one cell per track.
    Empty,
    /// Contents of the header cell; the substrate supplies the surrounding
    /// element. A closure rather than a `String` so a label can stay a
    /// reactive `t!(…)` fragment and follow a language switch, which is what
    /// the non-sortable headers did inline.
    Content(Arc<dyn Fn() -> AnyView + Send + Sync>),
    /// The column renders its *whole* header cell, given the class the table
    /// derived for it. Used for `SortableHeaderCell`, which owns its
    /// `role="columnheader"` element so it can keep `aria-sort` live, and for
    /// `<table>` headers, which need `<th scope="col">`.
    Cell(Arc<dyn Fn(Option<String>) -> AnyView + Send + Sync>),
}

impl ColumnHeader {
    pub fn content(render: impl Fn() -> AnyView + Send + Sync + 'static) -> Self {
        ColumnHeader::Content(Arc::new(render))
    }

    pub fn cell(render: impl Fn(Option<String>) -> AnyView + Send + Sync + 'static) -> Self {
        ColumnHeader::Cell(Arc::new(render))
    }
}

/// One column, described once for the header and the body.
///
/// The cell renderer returns the **complete** grid child / `<td>`, not just
/// its contents. That is deliberate: the explorer's first two cells are not
/// `role="cell"` divs at all (an item tooltip wrapper and a stacked
/// name-plus-metadata block), and wrapping every cell in a uniform element
/// would have changed the DOM of the very table this extraction is supposed
/// to leave pixel-identical. The table owns column *order*, the grid
/// template, and the header; the page owns what a cell looks like.
pub struct Column<T> {
    /// Grid tracks this column contributes. Ignored by the `<table>`
    /// substrate, which lets the browser size columns to their content.
    pub widths: TrackWidths,
    /// Whether the column participates. See the hydration rules in the module
    /// docs for what "no" means on each substrate.
    pub visible: Signal<bool>,
    pub header: ColumnHeader,
    /// Classes for the header cell, when [`ColumnHeader::Empty`] or
    /// [`ColumnHeader::Content`] build it. An invisible column's header class
    /// is replaced with `"hidden"` on the grid substrate.
    pub header_class: &'static str,
    /// Renders the complete body cell for one row.
    pub cell: Arc<dyn Fn(&T) -> AnyView + Send + Sync>,
}

impl<T> Clone for Column<T> {
    fn clone(&self) -> Self {
        Self {
            widths: self.widths,
            visible: self.visible,
            header: match &self.header {
                ColumnHeader::Empty => ColumnHeader::Empty,
                ColumnHeader::Content(f) => ColumnHeader::Content(f.clone()),
                ColumnHeader::Cell(f) => ColumnHeader::Cell(f.clone()),
            },
            header_class: self.header_class,
            cell: self.cell.clone(),
        }
    }
}

impl<T> Column<T> {
    pub fn new(
        widths: TrackWidths,
        header: ColumnHeader,
        cell: impl Fn(&T) -> AnyView + Send + Sync + 'static,
    ) -> Self {
        Self {
            widths,
            visible: Signal::derive(|| true),
            header,
            header_class: "",
            cell: Arc::new(cell),
        }
    }

    /// Classes for a header cell this module builds itself.
    pub fn header_class(mut self, class: &'static str) -> Self {
        self.header_class = class;
        self
    }

    pub fn visible(mut self, visible: impl Into<Signal<bool>>) -> Self {
        self.visible = visible.into();
        self
    }
}

/// A column's participation and tracks, snapshotted out of its signal. The
/// template derivation works on this rather than on `Column` so it stays a
/// pure function of plain data — testable without a reactive owner, and
/// obviously free of any map iteration.
pub type ResolvedTracks = (bool, TrackWidths);

/// Picks one breakpoint's track out of a column's [`TrackWidths`].
pub type TrackPick = fn(&TrackWidths) -> Option<&'static str>;

fn resolve<T>(columns: &[Column<T>]) -> Vec<ResolvedTracks> {
    columns
        .iter()
        .map(|c| (c.visible.get(), c.widths))
        .collect()
}

/// The `grid-template-columns` value for one breakpoint: every visible
/// column's track for that breakpoint, in column order, space separated.
/// Empty when no visible column has a track there.
pub fn grid_template(columns: &[ResolvedTracks], pick: TrackPick) -> String {
    columns
        .iter()
        .filter(|(visible, _)| *visible)
        .filter_map(|(_, widths)| pick(widths))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The inline `style` carrying the derived template to `.dt-grid-row`.
///
/// A breakpoint with no tracks writes no variable, and `.dt-grid-row` falls
/// back to the next breakpoint down.
pub fn grid_style(columns: &[ResolvedTracks]) -> String {
    let picks: [(&str, TrackPick); 3] = [
        ("--dt-cols", |w| w.base),
        ("--dt-cols-lg", |w| w.lg),
        ("--dt-cols-xl", |w| w.xl),
    ];
    let mut style = String::new();
    for (var, pick) in picks {
        let template = grid_template(columns, pick);
        if !template.is_empty() {
            style.push_str(var);
            style.push(':');
            style.push_str(&template);
            style.push(';');
        }
    }
    style
}

/// Classes shared by the header row and every body row of the grid
/// substrate: the derived-template hook plus the cell rhythm.
const GRID_ROW_BASE: &str = "dt-grid-row items-center gap-x-3 px-3 py-2";

fn header_cell_class<T>(column: &Column<T>, visible: bool) -> Option<String> {
    let class = if visible {
        column.header_class
    } else {
        "hidden"
    };
    (!class.is_empty()).then(|| class.to_string())
}

/// Header cells for `columns`, each wrapped in the substrate's own element
/// unless the column renders its whole cell.
///
/// An invisible column keeps its cell here and is hidden with CSS, so the
/// element count does not depend on the flag — see the hydration rules in the
/// module docs.
fn cells_for_header<T>(columns: &[Column<T>]) -> AnyView {
    columns
        .iter()
        .map(|column| {
            let class = header_cell_class(column, column.visible.get());
            match &column.header {
                ColumnHeader::Cell(render) => render(class),
                ColumnHeader::Empty => {
                    view! { <div role="columnheader" class=class></div> }.into_any()
                }
                ColumnHeader::Content(render) => {
                    let content = render();
                    view! { <div role="columnheader" class=class>{content}</div> }.into_any()
                }
            }
        })
        .collect::<Vec<_>>()
        .into_any()
}

/// Header cells for a page that keeps its own `<table>`: the caller supplies
/// the `<tr>` and the `<th>` classes, this supplies the cells in column order
/// with the invisible columns dropped.
///
/// Dropping rather than hiding is right here and wrong on the grid: on a
/// `<table>` an invisible column is one the visitor switched off with
/// `?cols=`, a URL-borne flag that reads the same on the server and the
/// client, and a `<td>` left behind would still occupy a column and throw the
/// empty state's `colspan` off.
pub fn header_cells<T>(columns: &[Column<T>]) -> AnyView {
    columns
        .iter()
        .filter(|column| column.visible.get())
        .map(|column| match &column.header {
            ColumnHeader::Cell(render) => render(header_cell_class(column, true)),
            ColumnHeader::Empty => {
                view! { <th scope="col" class=header_cell_class(column, true)></th> }.into_any()
            }
            ColumnHeader::Content(render) => {
                let content = render();
                view! { <th scope="col" class=header_cell_class(column, true)>{content}</th> }
                    .into_any()
            }
        })
        .collect::<Vec<_>>()
        .into_any()
}

/// Body cells for one row of a page that keeps its own `<table>`, in the same
/// order and with the same columns dropped as [`header_cells`] — which is the
/// point: the header and the body can no longer disagree about which columns
/// are on, or about what order they come in.
pub fn body_cells<T>(columns: &[Column<T>], row: &T) -> AnyView {
    columns
        .iter()
        .filter(|column| column.visible.get())
        .map(|column| (column.cell)(row))
        .collect::<Vec<_>>()
        .into_any()
}

/// How many columns a `<table>` row spans right now — the `colspan` of an
/// empty-state row, which has to track the header or the message sits under
/// the wrong columns.
pub fn visible_column_count<T>(columns: &[Column<T>]) -> usize {
    columns.iter().filter(|column| column.visible.get()).count()
}

/// Does `class` re-show an otherwise-`hidden` element at `prefix`?
///
/// Only display utilities count. `xl:text-sm` does not un-hide anything, and
/// treating it as though it did is how a header cell ends up silently visible
/// in a breakpoint where it owns no grid track.
#[cfg(test)]
fn unhides_at(class: &str, prefix: &str) -> bool {
    class.split_whitespace().any(|c| {
        matches!(
            c.strip_prefix(prefix),
            Some(
                "block"
                    | "flex"
                    | "grid"
                    | "inline"
                    | "inline-block"
                    | "inline-flex"
                    | "table-cell"
            )
        )
    })
}

/// Check one column's header class against its tracks, for the grid
/// substrate's header row.
///
/// The invariant: a header cell must be visible at exactly the breakpoints
/// where its column owns a grid track. Get it wrong in the permissive
/// direction and the header row has more unhidden cells than it has tracks —
/// every header from that column rightwards slides one track over and the last
/// one wraps to an implicit second row, while the body rows (whose cells carry
/// their own classes) stay correct. There is no compile error and no visual
/// symptom until you look at the one breakpoint band where it happens.
///
/// The header row itself is `hidden lg:grid`, so only `lg` and `xl` are
/// checked; `base` is never rendered.
#[cfg(test)]
pub fn check_header_class(widths: &TrackWidths, header_class: &str) -> Result<(), String> {
    let hidden = header_class.split_whitespace().any(|c| c == "hidden");
    // `lg:` utilities keep applying at `xl` unless `xl:` overrides them.
    let shown_at_lg = !hidden || unhides_at(header_class, "lg:");
    let shown_at_xl = shown_at_lg || unhides_at(header_class, "xl:");
    for (breakpoint, tracked, shown) in [
        ("lg", widths.lg.is_some(), shown_at_lg),
        ("xl", widths.xl.is_some(), shown_at_xl),
    ] {
        if tracked != shown {
            return Err(format!(
                "header class {header_class:?} is {} at `{breakpoint}` but the column {} a track there",
                if shown { "shown" } else { "hidden" },
                if tracked { "owns" } else { "owns no" },
            ));
        }
    }
    Ok(())
}

/// A sortable data table on the `role="table"` div-grid substrate.
///
/// The header row is `hidden` below `lg`: a responsive grid's narrow tier
/// shows a compact subset of the columns, where a header row of labels is
/// noise. Pages that need sorting below `lg` put a sort control in their
/// toolbar (the explorer does).
#[component]
pub fn DataTableGrid<T, K, KF>(
    /// Columns in DOM order. A [`Vec`], never a map — see the module docs.
    columns: Vec<Column<T>>,
    #[prop(into)] rows: Signal<Vec<T>>,
    /// Row identity for the keyed `<For>`.
    key: KF,
    /// Classes for the `role="table"` container.
    #[prop(into, optional)]
    class: &'static str,
    /// Extra classes for the header row, on top of the derived template.
    #[prop(into, optional)]
    header_class: &'static str,
    /// Extra classes for every body row, on top of the derived template.
    #[prop(into, optional)]
    row_class: &'static str,
) -> impl IntoView
where
    T: Clone + Send + Sync + 'static,
    K: Eq + std::hash::Hash + 'static,
    KF: Fn(&T) -> K + Clone + Send + Sync + 'static,
{
    let columns = Arc::new(columns);
    let style_columns = columns.clone();
    let header_columns = columns.clone();
    let row_columns = columns;

    // One derivation, read by the header row and by every body row — the
    // thing the four hand-copied class strings could not guarantee.
    let style = Signal::derive(move || grid_style(&resolve(style_columns.as_slice())));
    let header_row_class = format!("hidden lg:grid {GRID_ROW_BASE} {header_class}");
    let body_row_class = format!("grid {GRID_ROW_BASE} {row_class}");

    view! {
        <div role="table" class=class>
            <div role="row" class=header_row_class style=style>
                {move || cells_for_header(header_columns.as_slice())}
            </div>
            <For
                each=move || rows.get()
                key=key
                children=move |row| {
                    let cells = row_columns
                        .iter()
                        .map(|column| (column.cell)(&row))
                        .collect::<Vec<_>>();
                    view! {
                        <div role="row" class=body_row_class.clone() style=style>
                            {cells}
                        </div>
                    }
                    .into_any()
                }
            />
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod test {
    use super::*;

    /// The Item Explorer's nine columns, exactly as the four hand-copied
    /// class strings expressed them. `world` is the one that toggles.
    fn explorer_tracks(single_world: bool) -> Vec<ResolvedTracks> {
        vec![
            // icon
            (true, TrackWidths::everywhere("2.5rem")),
            // name
            (
                true,
                TrackWidths::responsive(
                    Some("minmax(0,1fr)"),
                    Some("minmax(6rem,1fr)"),
                    Some("minmax(6rem,1fr)"),
                ),
            ),
            // item level
            (true, TrackWidths::from_lg("3.5rem")),
            // equip level
            (true, TrackWidths::from_lg("3rem")),
            // NQ price
            (
                true,
                TrackWidths::responsive(Some("auto"), Some("6.5rem"), Some("6.5rem")),
            ),
            // HQ price
            (true, TrackWidths::from_lg("6.5rem")),
            // vendor
            (true, TrackWidths::from_xl("6rem")),
            // world
            (!single_world, TrackWidths::from_xl("6.5rem")),
            // actions
            (
                true,
                TrackWidths::responsive(Some("auto"), Some("5rem"), Some("5rem")),
            ),
        ]
    }

    /// Turn a Tailwind arbitrary grid template back into the CSS it stands
    /// for, so the assertions below can quote the original class strings
    /// verbatim instead of a hand-transcribed copy of them.
    fn from_tailwind(class_fragment: &str) -> String {
        let inner = class_fragment
            .rsplit_once('[')
            .expect("a grid-cols-[…] fragment")
            .1
            .strip_suffix(']')
            .expect("a grid-cols-[…] fragment");
        inner.replace('_', " ")
    }

    const BASE: &str = "grid-cols-[2.5rem_minmax(0,1fr)_auto_auto]";
    const LG: &str = "lg:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_5rem]";
    const XL_SINGLE: &str =
        "xl:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_6rem_5rem]";
    const XL_MULTI: &str =
        "xl:grid-cols-[2.5rem_minmax(6rem,1fr)_3.5rem_3rem_6.5rem_6.5rem_6rem_6.5rem_5rem]";

    #[test]
    fn the_derived_template_matches_the_single_world_class_strings() {
        // The whole point of #1080: these four strings were maintained by
        // hand and had to stay character-identical to each other. Now one
        // column list produces all of them.
        let cols = explorer_tracks(true);
        assert_eq!(grid_template(&cols, |w| w.base), from_tailwind(BASE));
        assert_eq!(grid_template(&cols, |w| w.lg), from_tailwind(LG));
        assert_eq!(grid_template(&cols, |w| w.xl), from_tailwind(XL_SINGLE));
    }

    #[test]
    fn the_derived_template_matches_the_multi_world_class_strings() {
        let cols = explorer_tracks(false);
        assert_eq!(grid_template(&cols, |w| w.base), from_tailwind(BASE));
        assert_eq!(grid_template(&cols, |w| w.lg), from_tailwind(LG));
        assert_eq!(grid_template(&cols, |w| w.xl), from_tailwind(XL_MULTI));
    }

    #[test]
    fn the_world_column_is_the_only_difference_between_the_two_sets() {
        // Single- and multi-world differ at `xl` and nowhere else — the
        // header row's `lg` template was identical in both hand-copies too.
        let single = explorer_tracks(true);
        let multi = explorer_tracks(false);
        assert_eq!(
            grid_template(&single, |w| w.base),
            grid_template(&multi, |w| w.base)
        );
        assert_eq!(
            grid_template(&single, |w| w.lg),
            grid_template(&multi, |w| w.lg)
        );
        assert_ne!(
            grid_template(&single, |w| w.xl),
            grid_template(&multi, |w| w.xl)
        );
    }

    #[test]
    fn every_breakpoint_has_one_track_per_column_shown_there() {
        // A track count that disagrees with the number of cells CSS leaves
        // visible is exactly the misalignment the four strings could drift
        // into, and it has no visual symptom until a column is added.
        let cols = explorer_tracks(false);
        assert_eq!(grid_template(&cols, |w| w.base).split(' ').count(), 4);
        assert_eq!(grid_template(&cols, |w| w.lg).split(' ').count(), 7);
        assert_eq!(grid_template(&cols, |w| w.xl).split(' ').count(), 9);
        let cols = explorer_tracks(true);
        assert_eq!(grid_template(&cols, |w| w.xl).split(' ').count(), 8);
    }

    #[test]
    fn the_style_carries_all_three_breakpoints() {
        let style = grid_style(&explorer_tracks(false));
        assert_eq!(
            style,
            format!(
                "--dt-cols:{};--dt-cols-lg:{};--dt-cols-xl:{};",
                from_tailwind(BASE),
                from_tailwind(LG),
                from_tailwind(XL_MULTI)
            )
        );
    }

    #[test]
    fn a_breakpoint_with_no_tracks_writes_no_variable() {
        // The header row of a table whose columns all start at `lg`: there
        // is no base template to write, and `.dt-grid-row` must fall back
        // rather than compute `grid-template-columns: ;`.
        let cols = vec![
            (true, TrackWidths::from_lg("3rem")),
            (true, TrackWidths::from_lg("1fr")),
        ];
        assert_eq!(
            grid_style(&cols),
            "--dt-cols-lg:3rem 1fr;--dt-cols-xl:3rem 1fr;"
        );
    }

    #[test]
    fn an_invisible_column_drops_out_of_every_breakpoint() {
        let cols = vec![
            (true, TrackWidths::everywhere("1fr")),
            (false, TrackWidths::everywhere("2rem")),
            (true, TrackWidths::everywhere("3rem")),
        ];
        assert_eq!(grid_template(&cols, |w| w.base), "1fr 3rem");
        assert_eq!(grid_template(&cols, |w| w.lg), "1fr 3rem");
        assert_eq!(grid_template(&cols, |w| w.xl), "1fr 3rem");
    }

    /// The end-to-end shape: what actually reaches the DOM. Asserted here
    /// rather than only on the pure derivation, because the thing that broke
    /// tables before was the *rendered* class string, not the arithmetic.
    #[test]
    fn the_rendered_rows_carry_the_derived_template() {
        let owner = Owner::new();
        owner.with(|| {
            let columns = vec![
                Column::new(
                    TrackWidths::everywhere("2.5rem"),
                    ColumnHeader::Empty,
                    |_: &u32| ().into_any(),
                ),
                Column::new(
                    TrackWidths::responsive(
                        Some("minmax(0,1fr)"),
                        Some("minmax(6rem,1fr)"),
                        Some("minmax(6rem,1fr)"),
                    ),
                    ColumnHeader::content(|| view! { "Name" }.into_any()),
                    |n: &u32| view! { <div role="cell">{*n}</div> }.into_any(),
                ),
                // Off, the way the explorer's world column is on a
                // single-world scope.
                Column::new(
                    TrackWidths::from_xl("6.5rem"),
                    ColumnHeader::Empty,
                    |_: &u32| ().into_any(),
                )
                .header_class("hidden xl:block")
                .visible(Signal::derive(|| false)),
            ];
            let html = view! {
                <DataTableGrid
                    columns=columns
                    rows=Signal::derive(|| vec![7u32])
                    key=|n: &u32| *n
                    class="panel"
                    header_class="text-xs"
                    row_class="hover:bg-white/5"
                />
            }
            .to_html();
            assert!(
                html.contains(
                    r#"class="grid dt-grid-row items-center gap-x-3 px-3 py-2 hover:bg-white/5""#
                ),
                "{html}"
            );
            assert!(
                html.contains(
                    r#"class="hidden lg:grid dt-grid-row items-center gap-x-3 px-3 py-2 text-xs""#
                ),
                "{html}"
            );
            // The off column contributes no track at any breakpoint...
            assert!(html.contains("--dt-cols:2.5rem minmax(0,1fr);"), "{html}");
            assert!(
                html.contains("--dt-cols-xl:2.5rem minmax(6rem,1fr);"),
                "{html}"
            );
            // ...but keeps its header cell, so element count does not depend
            // on the flag. That is the hydration-safety property.
            assert!(
                html.contains(r#"role="columnheader" class="hidden""#),
                "{html}"
            );
        });
    }
}
