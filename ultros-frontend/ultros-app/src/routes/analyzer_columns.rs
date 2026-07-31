//! Column registry for the Flip Finder table.
//!
//! One entry per rendered column, in DOM order. This is the single source
//! of truth for column ids, default/minimum widths, resizability, and
//! `?cols=` visibility — replacing widths that used to be encoded three
//! times (Tailwind class on the header cell, again on the row cell, and a
//! px table in `extra_column_width_px`).
//!
//! Widths render as CSS custom properties (`--colw-<id>`) on the table
//! pane; header and row cells both size themselves with
//! `width: var(--colw-<id>)`, and the row min-width is a `calc()` sum of
//! the visible columns' variables so a live drag updates everything in one
//! style write.

// TEMPORARY: the width/registry surface (ColumnSpec, COLUMNS, colw_style, …)
// lands ahead of the table markup that consumes it; until that wiring exists
// only the tests reference it, which `-D warnings` treats as dead code.
// Remove this allow when the analyzer markup consumes the registry.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

// Required columns — always render, not present in `?cols=`.
pub const COL_HQ: &str = "hq";
pub const COL_ITEM: &str = "item";
pub const COL_PROFIT: &str = "profit";
pub const COL_BUY_PRICE: &str = "buy_price";

/// Stable URL IDs for optional columns. Order here is the columns-picker +
/// `?cols=` serialization order: default-on columns first, opt-ins after.
/// It is deliberately *not* the DOM order — [`COLUMNS`] is DOM order.
pub const COL_PROFIT_PER_DAY: &str = "profit_per_day";
pub const COL_VELOCITY: &str = "velocity";
pub const COL_DRIFT: &str = "drift";
pub const COL_CONFIDENCE: &str = "confidence";
pub const COL_WORLD: &str = "world";
pub const COL_LAST_SOLD: &str = "last_sold";
pub const COL_ROI: &str = "roi";
pub const COL_DATACENTER: &str = "datacenter";
pub const COL_TREND: &str = "trend";
pub const COL_SALES_PER_DAY: &str = "sales_per_day";
pub const COL_VOLUME_30D: &str = "volume_30d";

pub const ALL_OPTIONAL_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
    COL_ROI,
    COL_DATACENTER,
    COL_TREND,
    COL_SALES_PER_DAY,
    COL_VOLUME_30D,
];

/// Default visible set when `?cols=` is absent from the URL. Once the
/// user explicitly sets the param (even to ""), we respect that exact
/// set instead of falling back to defaults.
///
/// ClickHouse-only columns (trend, sales/day, 30d volume) are off because
/// the rollup covers ~7% of traded items, so they would be blank on most
/// rows. ROI is off because it ranks by ratio, which is the wrong
/// objective when retainer slots are the scarce resource.
pub const DEFAULT_VISIBLE_COLS: &[&str] = &[
    COL_PROFIT_PER_DAY,
    COL_VELOCITY,
    COL_DRIFT,
    COL_CONFIDENCE,
    COL_WORLD,
    COL_LAST_SOLD,
];

/// localStorage key for user width overrides (`HashMap<String, f64>`, px).
pub const COL_WIDTHS_KEY: &str = "ultros.flipfinder.colwidths";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnSpec {
    pub id: &'static str,
    /// Rendered width in px when the user has not resized the column.
    /// Values carried over from the Tailwind classes they replace
    /// (`w-28` = 112px, `w-[88px]` = 88px, …).
    pub default_width: f64,
    /// Drag-resize floor. Keeps every column wide enough to remain
    /// clickable/readable; also the clamp for stored overrides.
    pub min_width: f64,
    pub resizable: bool,
    /// Participates in `?cols=` visibility. Required columns are `false`.
    pub optional: bool,
}

/// Every column in DOM order. The markup in analyzer.rs renders exactly
/// this sequence.
pub const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        id: COL_HQ,
        default_width: 44.0,
        min_width: 44.0,
        resizable: false,
        optional: false,
    },
    ColumnSpec {
        id: COL_ITEM,
        default_width: 288.0,
        min_width: 140.0,
        resizable: true,
        optional: false,
    },
    ColumnSpec {
        id: COL_PROFIT,
        default_width: 112.0,
        min_width: 90.0,
        resizable: true,
        optional: false,
    },
    ColumnSpec {
        id: COL_PROFIT_PER_DAY,
        default_width: 112.0,
        min_width: 90.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_VELOCITY,
        default_width: 88.0,
        min_width: 70.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_DRIFT,
        default_width: 88.0,
        min_width: 70.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_CONFIDENCE,
        default_width: 72.0,
        min_width: 60.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_ROI,
        default_width: 112.0,
        min_width: 80.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_BUY_PRICE,
        default_width: 112.0,
        min_width: 90.0,
        resizable: true,
        optional: false,
    },
    ColumnSpec {
        id: COL_WORLD,
        default_width: 112.0,
        min_width: 80.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_DATACENTER,
        default_width: 112.0,
        min_width: 80.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_TREND,
        default_width: 100.0,
        min_width: 80.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_SALES_PER_DAY,
        default_width: 140.0,
        min_width: 90.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_VOLUME_30D,
        default_width: 88.0,
        min_width: 70.0,
        resizable: true,
        optional: true,
    },
    ColumnSpec {
        id: COL_LAST_SOLD,
        default_width: 112.0,
        min_width: 80.0,
        resizable: true,
        optional: true,
    },
];

pub fn column_spec(id: &str) -> Option<&'static ColumnSpec> {
    COLUMNS.iter().find(|c| c.id == id)
}

/// Effective width of a column: the stored override clamped to the spec's
/// minimum, or the default. Unknown override ids are simply ignored by the
/// callers (they iterate [`COLUMNS`], never the map).
pub fn effective_width(spec: &ColumnSpec, overrides: &HashMap<String, f64>) -> f64 {
    overrides
        .get(spec.id)
        .copied()
        .map(|w| w.max(spec.min_width))
        .unwrap_or(spec.default_width)
}

/// Whether a column currently renders: required columns always, optional
/// columns per the `?cols=` set.
pub fn is_column_visible(spec: &ColumnSpec, visible: &HashSet<&'static str>) -> bool {
    !spec.optional || visible.contains(spec.id)
}

/// Inline `style` for the table pane: one `--colw-<id>` per column plus
/// `--analyzer-row-min-width` as a `calc()` sum of the *visible* columns.
/// Using a calc-of-vars (rather than a precomputed px sum) means a live
/// drag that rewrites a single `--colw-*` property updates the row width
/// for free, with no Rust in the loop.
pub fn colw_style(visible: &HashSet<&'static str>, overrides: &HashMap<String, f64>) -> String {
    let mut style = String::new();
    let mut sum_terms: Vec<String> = Vec::new();
    for spec in COLUMNS {
        let width = effective_width(spec, overrides);
        style.push_str(&format!("--colw-{}:{}px;", spec.id, width));
        if is_column_visible(spec, visible) {
            sum_terms.push(format!("var(--colw-{})", spec.id));
        }
    }
    style.push_str(&format!(
        "--analyzer-row-min-width:calc({});",
        sum_terms.join(" + ")
    ));
    style
}

pub fn parse_visible_cols(raw: Option<&str>) -> HashSet<&'static str> {
    match raw {
        None => DEFAULT_VISIBLE_COLS.iter().copied().collect(),
        Some(s) => s
            .split(',')
            .filter_map(|tok| ALL_OPTIONAL_COLS.iter().find(|c| **c == tok).copied())
            .collect(),
    }
}

pub fn serialize_visible_cols(visible: &HashSet<&'static str>) -> String {
    ALL_OPTIONAL_COLS
        .iter()
        .filter(|c| visible.contains(*c))
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_ids_are_unique() {
        // A duplicated id would silently break `column_spec` lookups and
        // emit duplicate `--colw-*` declarations.
        let ids: HashSet<&str> = COLUMNS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), COLUMNS.len());
    }

    #[test]
    fn every_optional_col_has_a_spec_and_vice_versa() {
        for id in ALL_OPTIONAL_COLS {
            let spec = column_spec(id).expect("optional col must be in COLUMNS");
            assert!(spec.optional, "{id} spec must be marked optional");
        }
        let optional_in_columns: Vec<_> = COLUMNS
            .iter()
            .filter(|c| c.optional)
            .map(|c| c.id)
            .collect();
        assert_eq!(optional_in_columns.len(), ALL_OPTIONAL_COLS.len());
    }

    #[test]
    fn effective_width_clamps_to_min_and_falls_back_to_default() {
        let spec = column_spec(COL_ITEM).unwrap();
        let mut overrides = HashMap::new();
        assert_eq!(effective_width(spec, &overrides), spec.default_width);
        overrides.insert(COL_ITEM.to_string(), 10.0);
        assert_eq!(effective_width(spec, &overrides), spec.min_width);
        overrides.insert(COL_ITEM.to_string(), 400.0);
        assert_eq!(effective_width(spec, &overrides), 400.0);
    }

    #[test]
    fn colw_style_sums_only_visible_columns() {
        let visible: HashSet<&'static str> = [COL_PROFIT_PER_DAY].into_iter().collect();
        let style = colw_style(&visible, &HashMap::new());
        // Required columns always in the sum:
        assert!(style.contains("var(--colw-hq)"));
        assert!(style.contains("var(--colw-item)"));
        assert!(style.contains("var(--colw-profit)"));
        assert!(style.contains("var(--colw-buy_price)"));
        assert!(style.contains("var(--colw-profit_per_day)"));
        // Hidden optional column: variable declared, but not in the sum.
        assert!(style.contains("--colw-roi:112px;"));
        let sum = style.split("--analyzer-row-min-width").nth(1).unwrap();
        assert!(!sum.contains("--colw-roi"));
    }

    #[test]
    fn default_widths_are_at_least_min_widths() {
        for spec in COLUMNS {
            assert!(
                spec.default_width >= spec.min_width,
                "{}: default {} < min {}",
                spec.id,
                spec.default_width,
                spec.min_width
            );
        }
    }
}
