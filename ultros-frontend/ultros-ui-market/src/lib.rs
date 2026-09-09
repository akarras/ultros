#![recursion_limit = "256"]
//! ultros-ui-market — extracted frontend modules with explicit SSR/hydration features.

pub mod components;
pub use ultros_frontend_core::{api, error, global_state, ws};
pub mod analysis;
pub mod freshness;
pub mod sales_cadence;
pub use ultros_i18n::{fallback as i18n_fallback, i18n};
