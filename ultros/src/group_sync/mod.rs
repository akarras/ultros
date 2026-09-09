//! Discord membership sync for groups.
//!
//! Three pieces, in order of how much you can trust them:
//!
//! - [`reconcile`] walks a guild's whole member list and makes the database
//!   match. It is the source of truth, and it runs on a timer, on demand from
//!   the group page, and immediately after a role is imported.
//! - [`events`] handles gateway events so a role change shows up in seconds
//!   instead of hours. Later accepted reconciliation passes repair missed events;
//!   refusal guards can leave membership stale until an operator intervenes.
//! - [`diff`] is the set logic both of them share, which is why they cannot
//!   disagree about what "in sync" means.
//!
//! Both write paths are idempotent and go through the same DB primitive, but
//! idempotence alone does not make them order-independent: reconciliation acts
//! on a snapshot it took minutes ago, and an event that landed since is newer.
//! A persisted group revision rejects the entire snapshot when a newer event
//! or lifecycle transition has committed, protecting both grants and revocations.
//! Reconciliation retries with fresh snapshots at most three times; exhausted
//! retries leave membership unchanged and report that the sync was skipped.

use poise::serenity_prelude as serenity;

pub(crate) mod diff;
pub(crate) mod events;
pub(crate) mod reconcile;

pub(crate) use reconcile::{spawn_reconcile, spawn_reconcile_scheduler, sync_rate_limiter};

/// The name to store for a Discord user, per the spec: their global display
/// name, falling back to their username.
///
/// Deliberately *not* the per-guild nickname, which is what serenity's
/// `Member::display_name` prefers. `discord_user` is one global row shared by
/// every group and every list share, so writing a nickname into it renames
/// that person everywhere on Ultros on the strength of what one server calls
/// them.
pub(crate) fn global_display_name(user: &serenity::User) -> String {
    user.global_name
        .as_deref()
        .unwrap_or(user.name.as_str())
        .to_string()
}
