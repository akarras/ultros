//! Grid queries and layout calculations shared by the server and browser.
//!
//! Keep this crate independent of Leptos, DOM access, and market-data providers.
//! The app supplies column definitions and values, owns reactive state, and
//! renders the resulting rows and column positions.

pub mod layout;
pub mod metrics;
