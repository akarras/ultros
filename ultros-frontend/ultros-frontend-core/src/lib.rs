#![recursion_limit = "256"]
//! ultros-frontend-core — extracted frontend modules with explicit SSR/hydration features.

pub mod api;
pub mod global_state;
pub mod ws;
pub use ultros_api_client::error;
#[cfg(feature = "ssr")]
pub use ultros_api_client::ssr_api;
pub use ultros_calc::recipe_planner;
pub use ultros_i18n::{fallback as i18n_fallback, i18n};
