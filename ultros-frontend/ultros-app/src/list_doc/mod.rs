//! The browser side of the local-first list document (spec section 3).
//!
//! Dead-code allow retained: this task lands the persistence layer
//! (`store.rs`) on its own; nothing in the app calls it yet, so every item
//! reads as unused to clippy in both the SSR and hydrate targets. The
//! follow-up task that wires the sync engine to `BrowserStorage` should
//! narrow or remove this allow once `store` gains real call sites.
#![allow(dead_code)]

pub mod adapter;
pub mod handle;
pub mod store;
pub mod sync;
pub mod undo;
