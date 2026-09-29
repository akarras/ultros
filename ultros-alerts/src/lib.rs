//! Alert evaluation and delivery: price, below-median, back-in-stock,
//! undercut, sold and list-update alerts, the notification inbox, and Discord/web-push dispatch.

// Root aliases so the modules' `crate::alerts::…` / `crate::event` paths read
// the same as they did inside the server crate.
use crate as alerts;
use ultros_server_core::event;

pub mod alert_manager;
pub mod delivery;
pub mod inbox;
pub mod list_update_alert_tracker;
pub mod market_trigger_tracker;
pub mod median_cache;
pub mod price_alert_tracker;
pub mod sold_alert;
pub mod sold_matcher;
#[allow(unused)]
pub mod undercut_alert;
