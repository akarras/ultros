//! Shared building blocks for the profit analyzers: the formula ledger,
//! zero-copy price views, the fetch gate, the column tables (`columns`),
//! the cell renderer (`cells`), the grid host (`grid`), the hop maths (`hop`),
//! the visible-window enrichment store and hook (`enrichment`), and more.
//! See docs/superpowers/specs/2026-09-01-analyzer-kit-design.md.
pub mod cells;
pub mod columns;
pub mod enrichment;
pub mod formula;
pub mod grid;
pub mod hop;
pub mod needed;
pub mod signals;
pub mod strip;
