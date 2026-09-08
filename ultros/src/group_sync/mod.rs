//! Discord membership sync for groups.
//!
//! Three pieces, in order of how much you can trust them:
//!
//! - [`reconcile`] walks a guild's whole member list and makes the database
//!   match. It is the source of truth, and it runs on a timer, on demand from
//!   the group page, and immediately after a role is imported.
//! - [`events`] handles gateway events so a role change shows up in seconds
//!   instead of hours. Purely a latency optimization: everything it does,
//!   reconciliation would also do on its next pass.
//! - [`diff`] is the set logic both of them share, which is why they cannot
//!   disagree about what "in sync" means.
//!
//! The two write paths are idempotent and go through the same DB primitive
//! (`apply_role_sync`), so a reconcile racing an event converges to the same
//! state regardless of which lands first.

pub(crate) mod diff;
pub(crate) mod events;
pub(crate) mod reconcile;

pub(crate) use reconcile::{spawn_reconcile, spawn_reconcile_scheduler, sync_rate_limiter};
