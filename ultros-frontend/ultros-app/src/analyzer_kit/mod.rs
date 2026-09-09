//! Shared building blocks for the profit analyzers: the formula ledger,
//! zero-copy price views, the fetch gate, the column tables (`columns`),
//! the cell renderer (`cells`), the grid host (`grid`), the hop maths (`hop`),
//! the visible-window enrichment store and hook (`enrichment`), and more.
//! See docs/superpowers/specs/2026-09-01-analyzer-kit-design.md.
pub mod cells;
pub mod columns;
pub mod enrichment;
pub mod filters;
pub use ultros_calc::formula;
pub mod grid;
pub use ultros_crafting::hop;
pub mod market;
pub use ultros_calc::needed;
pub mod signals;
pub mod stat_columns;
pub mod strip;
pub mod window;
