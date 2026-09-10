//! Lists: the local-first document's server side (spec section 4).
//!
//! `ListSync` is wired into `WebState` and `discord::Data`. The REST and bot
//! writers drive `edit_as_server` through it, and the websocket handler in
//! `crate::web::api::real_time_data` drives `subscribe_payload` and
//! `apply_update` (with `Origin::Socket`) for the document sync/relay path.

pub(crate) mod activity;
pub(crate) mod edits;
pub(crate) mod sync;

pub(crate) use edits::apply_list_item_edit;
pub(crate) use sync::{Actor, ListSync, Origin};
