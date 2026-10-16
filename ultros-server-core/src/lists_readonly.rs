//! Operator kill-switch for the Lists 2.0 promotion.
//!
//! Setting `ULTROS_LISTS_READONLY` (any [`crate::env_flag`] truthy spelling)
//! and restarting the process puts every list surface into read-only mode
//! without a rebuild or a data change: the SSR bootstrap carries the flag to
//! the browser, the list routes render a read-only view over the REST
//! projection instead of opening a list document, list-document websocket
//! subscriptions and updates are refused, and guest offline-cache preparation
//! is skipped. Unsetting it (or giving it a falsy value) and restarting
//! restores normal editing. See the rollback runbook in `docs/lists-sync.md`.
//!
//! This is deliberately *not* a user preference: there is no cookie, no
//! Settings toggle, and no per-user cohort. It exists so a bad sync/CRDT
//! regression in production can be stopped with an env flip + restart of
//! the same image, which is faster and less risky than building and
//! deploying a revert during an incident.

use std::sync::OnceLock;

use crate::env_flag::env_flag_enabled;
use tracing::warn;

/// Name of the env var that turns list editing off.
pub const LISTS_READONLY_ENV: &str = "ULTROS_LISTS_READONLY";

/// Whether this process serves lists read-only. Read once per process: the
/// value is process configuration, and caching it keeps the per-request SSR
/// path from re-reading (and re-warning about) the environment.
pub fn lists_readonly() -> bool {
    static READONLY: OnceLock<bool> = OnceLock::new();
    *READONLY.get_or_init(|| {
        let on = env_flag_enabled(
            LISTS_READONLY_ENV,
            std::env::var(LISTS_READONLY_ENV).ok().as_deref(),
        );
        if on {
            warn!(
                variable = LISTS_READONLY_ENV,
                "lists are in read-only mode; editing and document sync are paused"
            );
        }
        on
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kill_switch_env_name_is_stable() {
        // The runbook in docs/lists-sync.md spells this name; renaming it
        // would break the incident procedure, so pin it.
        assert_eq!(LISTS_READONLY_ENV, "ULTROS_LISTS_READONLY");
    }

    #[test]
    fn truthy_and_falsy_spellings_follow_the_shared_env_contract() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            assert!(
                env_flag_enabled(LISTS_READONLY_ENV, Some(value)),
                "{value:?}"
            );
        }
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("no"),
            Some("off"),
        ] {
            assert!(!env_flag_enabled(LISTS_READONLY_ENV, value), "{value:?}");
        }
    }
}
