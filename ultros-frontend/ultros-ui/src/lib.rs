//! Shared Leptos components without application routes, market data, or game packs.
//!
//! Applications provide router and locale contexts and supply grid rows, columns,
//! and cell renderers. Enable `ssr` or `hydrate` to match the consuming app.

pub mod components;

pub use ultros_i18n::{fallback as i18n_fallback, i18n};
