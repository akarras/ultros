//! Price lookups and the app-owned reactive sale-statistics state.

use std::sync::Arc;

use leptos::prelude::RwSignal;

pub use ultros_calc::pricing::*;

/// A client-only sale-statistics body, filled by a page `Effect` after the
/// table has rendered: `None` on the server and on the first client paint,
/// `Some(index)` once it lands — an *empty* index if the fetch failed, so
/// cells settle to "—" instead of shimmering forever.
pub type LateStats = RwSignal<Option<Arc<StatsIndex>>>;
