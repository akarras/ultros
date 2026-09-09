//! Calculation models shared by Ultros's server and browser.
//!
//! Keep this crate independent of Leptos, UI state, and game-data loading so
//! calculations can be compiled and tested without building application views.

pub mod analysis;
pub mod formula;
pub mod math;
pub mod recipe_planner;

pub mod needed;
pub mod pricing;

#[cfg(test)]
mod price_basis;
