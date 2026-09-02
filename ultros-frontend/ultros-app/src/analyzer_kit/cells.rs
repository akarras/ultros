//! The cell value union a [`super::columns::ToolColumnMeta::cell`] extractor
//! produces, and (from Task 2) the renderer that turns one into markup.

use ultros_api_types::trends::ConfidenceBand;

/// What one cell holds, independent of how it renders. A
/// [`super::columns::ColumnKind`] names the column; this names the value —
/// the two vary independently only for `Custom`, where the page supplies its
/// own view keyed off the kind.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Gil(i32),
    Confidence(ConfidenceBand),
    /// The kit has no generic rendering for this cell; the page looks up its
    /// own view by the column's [`super::columns::ColumnKind`].
    Custom,
}
