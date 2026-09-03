//! Shared building blocks for the profit analyzers: the formula ledger,
//! zero-copy price views, the fetch gate, the column tables (`columns`),
//! the cell renderer (`cells`) and the grid host (`grid`). See
//! docs/superpowers/specs/2026-09-01-analyzer-kit-design.md.
pub mod cells;
pub mod columns;
pub mod formula;
pub mod grid;
pub mod needed;
pub mod signals;
pub mod strip;
