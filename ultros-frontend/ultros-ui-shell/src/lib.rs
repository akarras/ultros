#![recursion_limit = "256"]
//! ultros-ui-shell — extracted frontend modules with explicit SSR/hydration features.

pub mod components;
pub use ultros_frontend_core::global_state::cookies::Cookies;
pub use ultros_frontend_core::{api, error, global_state, ws};
pub use ultros_i18n::{fallback as i18n_fallback, i18n};
