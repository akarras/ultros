#![recursion_limit = "256"]
//! ultros-ui-grid — extracted frontend modules with explicit SSR/hydration features.

pub mod components;
pub use ultros_i18n::{fallback as i18n_fallback, i18n};
