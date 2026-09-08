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
//! Both write paths are idempotent and go through the same DB primitive, but
//! idempotence alone does not make them order-independent: reconciliation acts
//! on a snapshot it took minutes ago, and an event that landed since is newer.
//! What is guaranteed, and what is not, is spelled out in [`reconcile`] —
//! briefly, a stale reconcile can no longer *remove* someone an event just
//! added, and a stale reconcile that re-adds someone an event just removed is
//! corrected on the next pass.

pub(crate) mod diff;
pub(crate) mod events;
pub(crate) mod reconcile;

pub(crate) use reconcile::{spawn_reconcile, spawn_reconcile_scheduler, sync_rate_limiter};
