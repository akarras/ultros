//! The list document: one Loro CRDT per list, shared by the server peer and
//! the browser. No Leptos, no database, no network (spec section 1).

pub mod document;
pub mod key;
pub mod snapshot;
pub mod undo;

pub use document::{
    DocError, ImportReport, ListDocument, SCHEMA_VERSION, SyncPayload, rebase_rows,
};
pub use key::{KeyError, Quality, RowKey};
pub use snapshot::{MetaSnapshot, RowChange, RowSnapshot, diff_rows, encode_scope, parse_scope};
pub use undo::ListUndo;

/// Re-exported so consumers can hold subscriptions without depending on
/// `loro` themselves.
pub use loro::Subscription;
