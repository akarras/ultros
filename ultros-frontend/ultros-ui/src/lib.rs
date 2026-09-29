//! Shared Leptos components without application routes, market data, or game packs.
//!
//! Applications provide router, locale and feedback contexts. Rendered grids
//! live in `ultros-ui-grid`. Enable `ssr` or `hydrate` to match the consuming app.

pub mod components;
pub mod global_state;

pub use ultros_i18n::{fallback as i18n_fallback, i18n};
