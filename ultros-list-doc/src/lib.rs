//! The list document: one Loro CRDT per list, shared by the server peer and
//! the browser. No Leptos, no database, no network (spec section 1).

// This code also runs inside the game process. Keep fallible operations on
// Result/Option paths; tests may still use assertions and fixture unwraps.
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::todo,
        clippy::unreachable,
        clippy::indexing_slicing,
        clippy::string_slice,
        clippy::arithmetic_side_effects,
        clippy::exit
    )
)]

pub mod client_sync;
pub mod document;
pub mod key;
pub mod recovery;
pub mod snapshot;
pub mod undo;
mod validation;

pub use document::{DocError, ImportReport, ListDocument, SCHEMA_VERSION, SyncPayload};
pub use key::{KeyError, Quality, RowKey};
pub use snapshot::{MetaSnapshot, RowChange, RowSnapshot, diff_rows, encode_scope, parse_scope};
pub use undo::ListUndo;

/// Re-exported so consumers can hold subscriptions without depending on
/// `loro` themselves.
pub use loro::Subscription;
