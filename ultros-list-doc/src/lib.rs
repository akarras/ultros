//! The list document: one Loro CRDT per list, shared by the server peer and
//! the browser. No Leptos, no database, no network (spec section 1).

pub mod key;
pub mod snapshot;

pub use key::{KeyError, Quality, RowKey};
pub use snapshot::{MetaSnapshot, RowChange, RowSnapshot, diff_rows, encode_scope, parse_scope};
