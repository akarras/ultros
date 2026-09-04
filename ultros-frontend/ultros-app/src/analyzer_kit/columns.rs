//! A page's column table as data. `ColumnSpec` is page-independent;
//! `ToolColumnMeta` binds a spec to one page's URL token, sort token,
//! classes and cell extractor. The whole table is a `static`, so the
//! context-free `FromStr`/`Display` impls on a page's `SortMode` and the
//! `&'static` id slices `parse_visible_cols` needs can read it.

use std::collections::BTreeSet;

use leptos::prelude::RwSignal;
use leptos_i18n::I18nContext;

use crate::components::control_bar::{ColumnOption, PickerHeading};
use crate::components::sort_header::{SortColumn, SortDir};
use crate::components::term_badge::TermRole;
use crate::i18n::*;

use super::cells::CellValue;
use super::enrichment::SparkStore;
use super::formula::PriceSignal;
use super::signals::LateStats;

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
    /// Profit times a sales-per-day rate. Computed, never fetched.
    ProfitPerDay,
    CostSlot,
    RevenueSlot,
    SalesPerDay7,
    AvgPrice,
    Confidence,
    LastSold,
    VolumeUnits7,
    Vwap7,
    /// Units sold in a 30-day window (a different kind from the 7-day one:
    /// kinds name definitions, not labels).
    VolumeUnits30,
    /// Volume-weighted average price over a 30-day window.
    Vwap30,
    /// The hourly price series over a lazily fetched window.
    Trend,
    /// The first-to-last percent of that same series. Named for its
    /// definition: the spec's `DriftBuffer` is the recent-sales-buffer
    /// drift the flip finder shows, a different number from a different
    /// body.
    DriftSpark,
    Tax,
    ListingWorld,
    ListingDc,
    /// An alternative revenue signal on the sell world, as a column.
    RevSignal(PriceSignal),
    /// An alternative cost signal over the buy scope, as a column.
    CostSignal(PriceSignal),
    HopGain,
    HopWorlds,
    Actions,
}

/// Where a column sits in the grouped Columns picker. Declaration order is
/// picker order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PickerGroup {
    /// "Revenue · ‹sell world›".
    Revenue,
    /// "Cost · ‹buy scope›".
    Cost,
    Travel,
    /// Sale-history columns: confidence, last sold, volume, VWAP, tax,
    /// profit/day, trend, drift and the 30-day pair.
    Market,
    /// Where the cheapest listing is: world, datacenter.
    Location,
    /// The fallback group for everything the others don't name. It is not
    /// picker-invisible: a column here with a `?cols=` token renders under
    /// an "Other" heading. The recipe analyzer's always-on columns sit here
    /// and none of them has a token, so its grouped picker ends at
    /// [`PickerGroup::Location`] — but the heading is live for any page that
    /// leaves an optional column ungrouped.
    Other,
}

/// Page-independent, closure-free description of a column.
pub struct ColumnSpec {
    /// Read by the grid to route `CellValue::Custom` cells to the page.
    pub kind: ColumnKind,
    pub label: LabelFn,
    pub group: PickerGroup,
}

/// A lazily fetched, visible-window feed. The window is part of the feed:
/// kinds name definitions, so a 168-hour sparkline and a 24-hour one are
/// the same feed with different windows, and a column declares which.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LazyFeed {
    /// `POST /api/v1/sparklines/{world}`: `hours` hourly VWAP points,
    /// oldest first, zeros for hours with no trade. The server clamps
    /// `hours` to [6, 168] and rejects more than 200 keys per request.
    Sparklines { hours: u16 },
}

impl LazyFeed {
    /// The feed's window, for the request the page builds — the reader
    /// that keeps `hours` from being a write-only field.
    pub fn hours(self) -> u16 {
        match self {
            LazyFeed::Sparklines { hours } => hours,
        }
    }
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
    /// Fetched per visible window after the table renders, so most rows
    /// have no value when the sorted memo runs.
    Lazy(LazyFeed),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sortability<M> {
    No,
    By(M),
    /// A lazy column: never sortable, whatever the page asks for.
    LazyNever,
}

pub const fn sortability_for<M: Copy>(layer: Layer, wanted: Option<M>) -> Sortability<M> {
    match (layer, wanted) {
        (Layer::Lazy(_), _) => Sortability::LazyNever,
        (Layer::RowLocal | Layer::Computed | Layer::Bulk, Some(m)) => Sortability::By(m),
        (_, None) => Sortability::No,
    }
}

/// Per-render context a cell extractor may read. The two signal handles let
/// a `fn`-pointer extractor reach page-level lazy data without the table
/// giving up its `static` column list; they are read inside the row's
/// reactive closure, so a merge re-renders the mounted rows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellCtx {
    pub now_unix: i64,
    /// The page's Labs toggle (`analyzer-recipe` on the recipe analyzer):
    /// the Price slot renders its note sub-line only under it.
    pub preview: bool,
    /// Cost signals the sub-craft cap left unpriced, by
    /// `PriceSignal::index`; their cells render "—" with the cap title.
    pub capped_cost: [bool; 4],
    /// The page's visible-window sparkline store. `None` on a page without
    /// one (and in tests): the cell then renders its loading shape, which
    /// is what the server renders too.
    pub sparklines: Option<RwSignal<SparkStore>>,
    /// The page's client-only 30-day statistics body. `None` on a page
    /// without one; `Some(signal holding None)` while it is in flight.
    pub stats_30: Option<LateStats>,
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
    /// The Labs token that gates this column. A gated column is absent
    /// from the flat picker and from the `?cols=` contract the page uses
    /// while the lab is off, so an old URL renders exactly as before.
    pub lab: Option<&'static str>,
}

pub fn picker_options<T, M>(
    cols: &'static [ToolColumnMeta<T, M>],
    i18n: I18nContext<Locale, I18nKeys>,
) -> Vec<ColumnOption> {
    cols.iter()
        .filter(|c| !c.id.is_empty() && c.lab.is_none())
        .map(|c| ColumnOption::new(c.id, (c.spec.label)(i18n)))
        .collect()
}

/// What the grouped picker needs beyond the table: the places named in the
/// two signal-group headings, the effective formula (for the "(= Price)" /
/// "(= Cost / unit)" suffix) and the cost signals the sub-craft cap left
/// unpriced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerContext {
    pub sell_place: String,
    pub buy_place: String,
    pub revenue: PriceSignal,
    pub cost: PriceSignal,
    pub capped: BTreeSet<PriceSignal>,
}

fn heading(
    group: PickerGroup,
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &PickerContext,
) -> PickerHeading {
    match group {
        PickerGroup::Revenue => PickerHeading {
            label: t_string!(
                i18n,
                analyzer_picker_group_place,
                name = t_string!(i18n, revenue).to_string(),
                place = ctx.sell_place.clone()
            )
            .to_string(),
            title: None,
        },
        PickerGroup::Cost => PickerHeading {
            label: t_string!(
                i18n,
                analyzer_picker_group_place,
                name = t_string!(i18n, cost).to_string(),
                place = ctx.buy_place.clone()
            )
            .to_string(),
            title: Some(
                t_string!(
                    i18n,
                    analyzer_picker_cost_group_title,
                    place = ctx.buy_place.clone()
                )
                .to_string(),
            ),
        },
        PickerGroup::Travel => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_travel).to_string(),
            title: None,
        },
        PickerGroup::Market => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_market).to_string(),
            title: None,
        },
        PickerGroup::Location => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_location).to_string(),
            title: None,
        },
        PickerGroup::Other => PickerHeading {
            label: t_string!(i18n, analyzer_picker_group_other).to_string(),
            title: None,
        },
    }
}

/// The picker with group headings: every optional column (lab-gated ones
/// included), sorted by group then table position, the selected signals
/// suffixed, the capped cost columns hinted (and, in the list, disabled
/// only while unchecked — a ticked column must stay untickable).
pub fn grouped_picker_options<T, M>(
    cols: &'static [ToolColumnMeta<T, M>],
    i18n: I18nContext<Locale, I18nKeys>,
    ctx: &PickerContext,
) -> Vec<ColumnOption> {
    let mut entries: Vec<(PickerGroup, usize, ColumnOption)> = cols
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.id.is_empty())
        .map(|(i, c)| {
            let mut label = (c.spec.label)(i18n);
            let mut disabled = false;
            let mut hint = None;
            match c.spec.kind {
                // Plain-key `t_string!` yields a `&'static str`: pass it
                // straight through (`&t_string!(..)` is `needless_borrow`).
                ColumnKind::RevSignal(s) if s == ctx.revenue => {
                    label.push(' ');
                    label.push_str(t_string!(i18n, analyzer_equals_price_slot));
                }
                ColumnKind::CostSignal(s) => {
                    if s == ctx.cost {
                        label.push(' ');
                        label.push_str(t_string!(i18n, analyzer_equals_cost_slot));
                    }
                    if ctx.capped.contains(&s) {
                        disabled = true;
                        hint = Some(t_string!(i18n, analyzer_picker_subcraft_cap_hint).to_string());
                    }
                }
                _ => {}
            }
            let option = ColumnOption {
                id: c.id,
                label,
                group: Some(heading(c.spec.group, i18n, ctx)),
                disabled,
                hint,
            };
            (c.spec.group, i, option)
        })
        .collect();
    entries.sort_by_key(|(g, i, _)| (*g, *i));
    entries.into_iter().map(|(_, _, o)| o).collect()
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
            Sortability::No | Sortability::LazyNever => None,
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
        group: PickerGroup::Other,
    };
    static SPEC_PROFIT: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Profit,
        label: label_profit,
        group: PickerGroup::Other,
    };
    static SPEC_COST: ColumnSpec = ColumnSpec {
        kind: ColumnKind::CostSlot,
        label: label_cost,
        group: PickerGroup::Other,
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
        lab: None,
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
        // A lazy column never sorts, even when the page names a mode: the
        // visible window holds a fraction of the rows.
        let feed = Layer::Lazy(LazyFeed::Sparklines { hours: 168 });
        assert_eq!(sortability_for(feed, None::<Col>), Sortability::LazyNever);
        assert_eq!(
            sortability_for(feed, Some(Col::Profit)),
            Sortability::LazyNever
        );
        assert_eq!(LazyFeed::Sparklines { hours: 168 }.hours(), 168);
    }

    /// A `?sort=` token pointing at a lazy column resolves to nothing, so a
    /// bookmarked URL cannot sort by data most rows do not have.
    #[test]
    fn a_lazy_column_is_unreachable_from_a_sort_token() {
        // `P_TREND` deliberately carries `sort_id: "trend"`, so this reaches
        // the new `Sortability::No | Sortability::LazyNever => None` arm
        // rather than the "no column has that token" path.
        assert_eq!(sort_from_token(&PICKER, "trend"), None);
        assert!(
            PICKER
                .iter()
                .all(|c| !matches!(c.sort, Sortability::By(_)) || c.sort_id != "trend")
        );
    }

    #[test]
    fn cell_extractors_are_plain_fn_pointers() {
        let ctx = CellCtx {
            now_unix: 0,
            preview: false,
            capped_cost: [false; 4],
            sparklines: None,
            stats_30: None,
        };
        assert_eq!((COLS[1].cell)(&42, &ctx), CellValue::Gil(42));
        assert_eq!((COLS[0].cell)(&42, &ctx), CellValue::Custom);
    }

    use crate::analyzer_kit::formula::PriceSignal;
    use leptos::prelude::{Owner, provide_context};
    use std::collections::BTreeSet;

    fn lbl_conf(_: I18nContext<Locale, I18nKeys>) -> String {
        "Confidence".into()
    }
    fn lbl_rev(_: I18nContext<Locale, I18nKeys>) -> String {
        "Sale median (7d)".into()
    }
    fn lbl_cost(_: I18nContext<Locale, I18nKeys>) -> String {
        "Cheapest listing".into()
    }
    fn lbl_cost2(_: I18nContext<Locale, I18nKeys>) -> String {
        "Sale average (7d)".into()
    }
    fn lbl_hop(_: I18nContext<Locale, I18nKeys>) -> String {
        "Hop gain / unit".into()
    }
    fn lbl_trend(_: I18nContext<Locale, I18nKeys>) -> String {
        "Trend".into()
    }
    fn lbl_world(_: I18nContext<Locale, I18nKeys>) -> String {
        "Listing world".into()
    }
    fn lbl_other(_: I18nContext<Locale, I18nKeys>) -> String {
        "Tax".into()
    }
    static P_CONF: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Confidence,
        label: lbl_conf,
        group: PickerGroup::Market,
    };
    static P_REV: ColumnSpec = ColumnSpec {
        kind: ColumnKind::RevSignal(PriceSignal::SaleMedian),
        label: lbl_rev,
        group: PickerGroup::Revenue,
    };
    static P_COST: ColumnSpec = ColumnSpec {
        kind: ColumnKind::CostSignal(PriceSignal::ListingMin),
        label: lbl_cost,
        group: PickerGroup::Cost,
    };
    static P_COST2: ColumnSpec = ColumnSpec {
        kind: ColumnKind::CostSignal(PriceSignal::SaleAvg),
        label: lbl_cost2,
        group: PickerGroup::Cost,
    };
    static P_HOP: ColumnSpec = ColumnSpec {
        kind: ColumnKind::HopGain,
        label: lbl_hop,
        group: PickerGroup::Travel,
    };
    static P_TREND: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Trend,
        label: lbl_trend,
        group: PickerGroup::Market,
    };
    static P_WORLD: ColumnSpec = ColumnSpec {
        kind: ColumnKind::ListingWorld,
        label: lbl_world,
        group: PickerGroup::Location,
    };
    static P_OTHER: ColumnSpec = ColumnSpec {
        kind: ColumnKind::Tax,
        label: lbl_other,
        group: PickerGroup::Other,
    };
    fn any_cell(_: &(), _: &CellCtx) -> CellValue {
        CellValue::Custom
    }
    const PBASE: ToolColumnMeta<(), Col> = ToolColumnMeta {
        spec: &P_CONF,
        id: "",
        sort_id: "",
        sort: Sortability::No,
        default_dir: SortDir::Desc,
        header_class: "",
        cell_class: "",
        default_on: false,
        cell: any_cell,
        side: None,
        formula_header_class: "",
        formula_cell_class: "",
        lab: None,
    };
    static PICKER: [ToolColumnMeta<(), Col>; 8] = [
        ToolColumnMeta {
            spec: &P_CONF,
            id: "confidence",
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_REV,
            id: "rev-sale-median",
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_COST,
            id: "cost-listing-min",
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_COST2,
            id: "cost-sale-avg",
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_HOP,
            id: "hop-gain",
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_TREND,
            id: "trend",
            // A token on an unsortable column: `sort_from_token` must still
            // refuse it (`a_lazy_column_is_unreachable_from_a_sort_token`).
            sort_id: "trend",
            sort: sortability_for(Layer::Lazy(LazyFeed::Sparklines { hours: 168 }), None),
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
        ToolColumnMeta {
            spec: &P_WORLD,
            id: "listing-world",
            ..PBASE
        },
        // `Other` is still a live picker heading until the market columns
        // move, so one fixture column has to keep covering it. Lab-gated so
        // the flat-picker assertion below stays unchanged.
        ToolColumnMeta {
            spec: &P_OTHER,
            id: "tax",
            lab: Some("analyzer-recipe"),
            ..PBASE
        },
    ];

    /// Groups come out in `PickerGroup` order, entries in table order within
    /// a group; the selected signals carry their "(= …)" suffix; capped cost
    /// columns are disabled with the hint; the Cost heading carries the
    /// loads-once title.
    #[test]
    fn grouped_picker_keeps_option_order() {
        let _ = any_spawner::Executor::init_futures_executor();
        let owner = Owner::new();
        owner.with(|| {
            provide_context(leptos_i18n::context::init_i18n_context::<crate::i18n::Locale>());
            let i18n = crate::i18n::use_i18n();
            let ctx = PickerContext {
                sell_place: "Gilgamesh".into(),
                buy_place: "Aether".into(),
                revenue: PriceSignal::SaleMedian,
                cost: PriceSignal::ListingMin,
                capped: BTreeSet::from([PriceSignal::SaleAvg]),
            };
            let got = grouped_picker_options(&PICKER, i18n, &ctx);
            let ids: Vec<&str> = got.iter().map(|o| o.id).collect();
            assert_eq!(
                ids,
                [
                    "rev-sale-median",
                    "cost-listing-min",
                    "cost-sale-avg",
                    "hop-gain",
                    "confidence",
                    "trend",
                    "listing-world",
                    "tax"
                ]
            );
            assert_eq!(got[0].label, "Sale median (7d) (= Price)");
            assert_eq!(got[0].group.as_ref().unwrap().label, "Revenue · Gilgamesh");
            assert_eq!(got[1].label, "Cheapest listing (= Cost / unit)");
            let cost_heading = got[1].group.as_ref().unwrap();
            assert_eq!(cost_heading.label, "Cost · Aether");
            assert_eq!(
                cost_heading.title.as_deref(),
                Some("Shows sale history for Aether (loads once)")
            );
            assert!(got[2].disabled && got[2].hint.is_some(), "{:?}", got[2]);
            assert!(!got[1].disabled && got[1].hint.is_none());
            assert_eq!(got[3].group.as_ref().unwrap().label, "Travel");
            assert_eq!(got[4].group.as_ref().unwrap().label, "Market");
            assert_eq!(got[5].group.as_ref().unwrap().label, "Market");
            assert_eq!(got[6].group.as_ref().unwrap().label, "Location");
            assert_eq!(got[7].group.as_ref().unwrap().label, "Other");
            // The flat picker never lists a lab-gated column.
            let flat = picker_options(&PICKER, i18n);
            assert_eq!(
                flat.iter().map(|o| o.id).collect::<Vec<_>>(),
                ["confidence", "listing-world"]
            );
            assert_eq!(
                flat[0],
                ColumnOption::new("confidence", "Confidence".into())
            );
        });
    }
}
