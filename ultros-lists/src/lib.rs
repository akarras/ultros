//! Lists: the local-first document's server side (spec section 4).
//!
//! `ListSync` is wired into `WebState` and `discord::Data`. The REST and bot
//! writers drive `edit_as_server` through it, and the websocket handler in
//! `ultros::web::api::real_time_data` drives `subscribe_payload` and
//! `apply_update` (with `Origin::Socket`) for the document sync/relay path.

// Root aliases so the modules' `crate::lists::…` / `crate::alerts` /
// `crate::event` paths read the same as they did inside the server crate.
use crate as lists;
use ultros_alerts as alerts;
use ultros_server_core::event;

pub mod activity;
pub mod edits;
pub mod sync;

pub use edits::apply_list_item_edit;
pub use sync::{Actor, ListSync, Origin};
