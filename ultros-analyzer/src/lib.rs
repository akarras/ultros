//! The in-memory market analyzer (resale/flip opportunities, trends, sale
//! history) that the web API and the Discord bot query.

// Root alias so the modules' `crate::event` paths read the same as they did
// inside the server crate.
use ultros_server_core::event;

pub mod analyzer_service;
pub mod resale_eligibility;
pub mod trend_candidates;
