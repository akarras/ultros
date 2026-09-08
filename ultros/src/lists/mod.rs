//! Lists: the local-first document's server side (spec section 4).
//!
//! `ListSync` is wired into `WebState` and `discord::Data` by this task, but
//! nothing calls its methods yet — the REST, websocket and bot handlers that
//! construct an `Actor` and drive `apply_update`/`edit_as_server` land in
//! Task 5/6 of this plan. Until then the whole module tree is unreachable
//! from any live call graph, which `dead_code` cannot tell apart from code
//! that is simply unused.
#![allow(dead_code, unused_imports)]

pub(crate) mod activity;
pub(crate) mod sync;

pub(crate) use sync::{Actor, Applied, ListSync, Origin};
