//! Operator pause switch for the promoted Lists 2.0 browser surfaces.
//!
//! Setting `ULTROS_LISTS_READONLY` (any [`crate::env_flag`] truthy spelling)
//! and restarting the process pauses the promoted browser list routes and
//! list-document socket edits without a rebuild or a data change: the SSR
//! bootstrap carries the flag to the browser, the list routes render a
//! read-only view over the REST projection instead of opening a list
//! document, list-document websocket subscriptions and updates are refused,
//! and guest offline-cache preparation is skipped. Unsetting it (or giving
//! it a falsy value) and restarting restores normal browser editing. See
//! the rollback runbook in `docs/lists-sync.md`.
//!
//! Scope limits, by design: this is *not* an all-writer freeze. REST
//! writers — the Discord bot, imports, sharing and other API callers —
//! keep merging and projecting through `ListSync`, the site-wide service
//! worker stays registered, and a previously prepared offline device
//! editor is not stopped. If a regression implicates the server merge or
//! projection code those writers share, this switch does not contain it;
//! operators must stop every affected writer process per the runbook.
//!
//! This is deliberately *not* a user preference: there is no cookie, no
//! Settings toggle, and no per-user cohort. It exists so a regression in
//! the promoted browser surfaces can be paused with an env flip + restart
//! of the same image, which is faster and less risky than building and
//! deploying a revert during an incident.

use std::sync::OnceLock;

use crate::env_flag::env_flag_enabled;
use tracing::warn;

/// Name of the env var that pauses the promoted browser list routes and
/// list-document socket edits in this process. It does not pause REST/bot
/// list writers.
pub const LISTS_READONLY_ENV: &str = "ULTROS_LISTS_READONLY";

/// Whether this process pauses the promoted browser list surfaces (see the
/// module docs for what stays writable). Read once per process: the
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
                "promoted browser list routes and document socket sync are paused; \
                 REST/bot list writers are not paused by this flag"
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
