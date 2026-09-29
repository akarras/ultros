//! Crafting and travel-cost calculations shared by the server and browser.
//!
//! Callers supply recipes, prices, inventory, vendor prices, and world mappings.
//! This crate uses game-data types but does not load data or own reactive state.

pub mod cost;
pub mod hop;
