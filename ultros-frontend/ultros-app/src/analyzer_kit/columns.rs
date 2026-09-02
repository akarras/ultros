//! A page's column table as data. `ColumnSpec` is page-independent;
//! `ToolColumnMeta` binds a spec to one page's URL token, sort token,
//! classes and cell extractor. The whole table is a `static`, so the
//! context-free `FromStr`/`Display` impls on a page's `SortMode` and the
//! `&'static` id slices `parse_visible_cols` needs can read it.

use leptos_i18n::I18nContext;

use crate::components::control_bar::ColumnOption;
use crate::components::sort_header::{SortColumn, SortDir};
use crate::components::term_badge::TermRole;
use crate::i18n::*;

use super::cells::CellValue;

/// A label resolver. A plain `fn` so a column table can be a `static`;
/// the page resolves it inside a reactive closure so headers follow the
/// locale.
pub type LabelFn = fn(I18nContext<Locale, I18nKeys>) -> String;

/// What a column IS, independent of the page showing it. Kinds name a
/// definition, not a label: a 7-day unit volume and a 30-day one are
/// different kinds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Item,
    Profit,
    Roi,
    CostSlot,
    RevenueSlot,
    SalesPerDay7,
    AvgPrice,
    Confidence,
    LastSold,
    VolumeUnits7,
    Vwap7,
    Tax,
    ListingWorld,
    ListingDc,
    Actions,
}

/// Page-independent, closure-free description of a column.
pub struct ColumnSpec {
    /// Read by the grid to route `CellValue::Custom` cells to the page.
    pub kind: ColumnKind,
    pub label: LabelFn,
}

/// Where a column's value comes from. Sortability is derived from it:
/// anything complete for every row before the sorted memo runs sorts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Present on the row as built.
    RowLocal,
    /// Derived from other row fields.
    Computed,
    /// From one whole-scope body fetched before the table renders.
    Bulk,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sortability<M> {
    No,
    By(M),
}

pub const fn sortability_for<M: Copy>(layer: Layer, wanted: Option<M>) -> Sortability<M> {
    match (layer, wanted) {
        (Layer::RowLocal | Layer::Computed | Layer::Bulk, Some(m)) => Sortability::By(m),
        (_, None) => Sortability::No,
    }
}

/// Per-render context a cell extractor may read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellCtx {
    pub now_unix: i64,
}

/// One page's binding of a [`ColumnSpec`]: its `?cols=` token (`""` for
/// an always-on column), `?sort=` token (`""` when unsortable), classes
/// copied verbatim from the page's markup, and a `fn` pointer extracting
/// the cell value from a row. Everything URL-facing lives here so the
/// page's `SortMode` impls and `parse_visible_cols` can read a `static`.
pub struct ToolColumnMeta<T: 'static, M: 'static> {
    pub spec: &'static ColumnSpec,
    pub id: &'static str,
    pub sort_id: &'static str,
    pub sort: Sortability<M>,
    pub default_dir: SortDir,
    pub header_class: &'static str,
    pub cell_class: &'static str,
    /// Whether an optional column starts visible. Ignored when `id` is
    /// empty (always-on columns): both the picker and the default set
    /// read it only for optional columns.
    pub default_on: bool,
    pub cell: fn(&T, &CellCtx) -> CellValue,
    /// The formula role this column plays, for the pages that mark their
    /// formula columns. `None` for a column that is never marked.
    pub side: Option<TermRole>,
    /// The wider, two-line header classes used in place of
    /// `header_class` while this column is marked. `""` for a column
    /// that is never marked.
    pub formula_header_class: &'static str,
    /// The cell classes matching `formula_header_class`, used in place
    /// of `cell_class` while this column is marked. `""` for a column
    /// that is never marked.
    pub formula_cell_class: &'static str,
}

pub fn picker_options<T, M>(
    cols: &'static [ToolColumnMeta<T, M>],
    i18n: I18nContext<Locale, I18nKeys>,
) -> Vec<ColumnOption> {
    cols.iter()
        .filter(|c| !c.id.is_empty())
        .map(|c| ColumnOption::new(c.id, (c.spec.label)(i18n)))
        .collect()
}

pub fn sort_token<T, M: SortColumn>(
    cols: &'static [ToolColumnMeta<T, M>],
    m: M,
) -> Option<&'static str> {
    cols.iter()
        .find(|c| matches!(c.sort, Sortability::By(x) if x == m))
        .map(|c| c.sort_id)
}

pub fn sort_from_token<T, M: SortColumn>(
    cols: &'static [ToolColumnMeta<T, M>],
    s: &str,
) -> Option<M> {
    cols.iter()
        .find(|c| !c.sort_id.is_empty() && c.sort_id == s)
        .and_then(|c| match c.sort {
            Sortability::By(m) => Some(m),
            Sortability::No => None,
        })
}

pub fn default_dir_for<T, M: SortColumn>(cols: &'static [ToolColumnMeta<T, M>], m: M) -> SortDir {
    cols.iter()
        .find(|c| matches!(c.sort, Sortability::By(x) if x == m))
        .map(|c| c.default_dir)
        .unwrap_or(SortDir::Desc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::sort_header::SortColumn;
    use std::fmt;

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    enum Col {
        Profit,
        Cost,
    }
    impl fmt::Display for Col {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(sort_token(&COLS, *self).unwrap_or("profit"))
        }
    }
    impl SortColumn for Col {
        fn fallback() -> Self {
            Col::Profit
        }
        fn default_dir(self) -> SortDir {
            default_dir_for(&COLS, self)
        }
    }

    fn label_item(_: I18nContext<Locale, I18nKeys>) -> String {
        "Item".into()
    }
    fn label_profit(_: I18nContext<Locale, I18nKeys>) -> String {
        "Profit".into()
    }
    fn label_cost(_: I18nContext<Locale, I18nKeys>) -> String {
        "Cost".into()
    }
    static SPEC_ITEM: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Item,
        label: label_item,
    };
    static SPEC_PROFIT: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Profit,
        label: label_profit,
    };
    static SPEC_COST: ColumnSpec = ColumnSpec {
        kind: ColumnKind::CostSlot,
        label: label_cost,
    };

    fn no_cell(_: &i32, _: &CellCtx) -> CellValue {
        CellValue::Custom
    }
    fn gil_cell(v: &i32, _: &CellCtx) -> CellValue {
        CellValue::Gil(*v)
    }

    /// Every field at its table-wide default, so each column below
    /// spells out only what it actually differs in.
    const BASE: ToolColumnMeta<i32, Col> = ToolColumnMeta {
        spec: &SPEC_ITEM,
        id: "",
        sort_id: "",
        sort: Sortability::No,
        default_dir: SortDir::Desc,
        header_class: "",
        cell_class: "",
        default_on: true,
        cell: gil_cell,
        side: None,
        formula_header_class: "",
        formula_cell_class: "",
    };

    static COLS: [ToolColumnMeta<i32, Col>; 3] = [
        ToolColumnMeta {
            spec: &SPEC_ITEM,
            header_class: "w-64",
            cell_class: "w-64",
            cell: no_cell,
            ..BASE
        },
        ToolColumnMeta {
            spec: &SPEC_PROFIT,
            sort_id: "profit",
            sort: sortability_for(Layer::Computed, Some(Col::Profit)),
            header_class: "w-32",
            cell_class: "w-32",
            ..BASE
        },
        ToolColumnMeta {
            spec: &SPEC_COST,
            id: "cost",
            sort_id: "cost",
            sort: sortability_for(Layer::Computed, Some(Col::Cost)),
            default_dir: SortDir::Asc,
            header_class: "w-32",
            cell_class: "w-32",
            default_on: false,
            ..BASE
        },
    ];

    #[test]
    fn ids_and_defaults_come_from_the_table_in_order() {
        // Derived inline: a kit fn with only test callers is dead code.
        let ids: Vec<&str> = COLS
            .iter()
            .filter(|c| !c.id.is_empty())
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["cost"]);
        let defaults: Vec<&str> = COLS
            .iter()
            .filter(|c| !c.id.is_empty() && c.default_on)
            .map(|c| c.id)
            .collect();
        assert_eq!(defaults, Vec::<&str>::new());
    }

    #[test]
    fn sort_tokens_round_trip_and_fall_back() {
        assert_eq!(sort_token(&COLS, Col::Profit), Some("profit"));
        assert_eq!(sort_from_token(&COLS, "cost"), Some(Col::Cost));
        assert_eq!(sort_from_token(&COLS, "bogus"), None);
        assert_eq!(default_dir_for(&COLS, Col::Cost), SortDir::Asc);
        assert_eq!(default_dir_for(&COLS, Col::Profit), SortDir::Desc);
        assert_eq!(Col::Cost.to_string(), "cost");
    }

    #[test]
    fn sortability_follows_the_layer() {
        assert_eq!(
            sortability_for(Layer::RowLocal, Some(Col::Profit)),
            Sortability::By(Col::Profit)
        );
        assert_eq!(
            sortability_for(Layer::Bulk, Some(Col::Profit)),
            Sortability::By(Col::Profit)
        );
        assert_eq!(
            sortability_for(Layer::Computed, None::<Col>),
            Sortability::No
        );
    }

    #[test]
    fn cell_extractors_are_plain_fn_pointers() {
        let ctx = CellCtx { now_unix: 0 };
        assert_eq!((COLS[1].cell)(&42, &ctx), CellValue::Gil(42));
        assert_eq!((COLS[0].cell)(&42, &ctx), CellValue::Custom);
    }
}
