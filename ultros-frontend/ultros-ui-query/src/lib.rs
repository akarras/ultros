#![recursion_limit = "256"]
//! ultros-ui-query — extracted frontend modules with explicit SSR/hydration features.

pub mod components;
pub mod last_view;
pub mod query_defaults;
pub use ultros_i18n::{fallback as i18n_fallback, i18n};
