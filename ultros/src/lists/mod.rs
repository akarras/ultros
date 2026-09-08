//! Lists: the local-first document's server side (spec section 4).
//!
//! `ListSync` is wired into `WebState` and `discord::Data`, and the REST and
//! bot writers (Task 5 of this plan) now drive `edit_as_server` through it.
//! `Origin::Socket`, `subscribe_payload` and `apply_update` still have no
//! caller — those land with the websocket handler in Task 6 — so they keep
//! their own narrow `#[allow(dead_code)]` below instead of a file-level one.

pub(crate) mod activity;
pub(crate) mod sync;

pub(crate) use sync::{Actor, ListSync, Origin};
