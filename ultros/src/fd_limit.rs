//! Raise this process' open-file limit at startup.
//!
//! Docker does not pass a `nofile` ulimit through unless the run/compose config
//! asks for one, so the container inherits the daemon default: a **1024** soft
//! limit against a 524288 hard limit. That is far too low for this process —
//! every inbound connection, every loopback SSR fetch the renderer makes, the
//! sqlx pool, the ClickHouse pool, the Universalis websockets and the alert
//! websockets all consume descriptors from the same budget.
//!
//! When a crawler burst pushes concurrency past 1024, `accept()` starts
//! returning `EMFILE` — GlitchTip #7188, `accept error: Too many open files
//! (os error 24)`. It does not fail in isolation: the same exhaustion makes the
//! renderer's loopback fetches fail, so a burst of `Error doing leptos fetch`
//! (#7210–#7216, all inside one second of a #7188 event) and the
//! disposed-signal SSR panics that follow a failed resource land together. One
//! exhausted fd table takes out a whole window of page renders.
//!
//! Fixing this by editing the deploy's `docker-compose.yml` would work, but that
//! file lives on the host and is not in this repo — the limit would silently
//! revert on any fresh deploy. Raising the soft limit from inside the process
//! keeps the fix version-controlled and independent of how the container is
//! launched. Raising the *soft* limit up to the existing hard limit needs no
//! privileges; only raising the hard limit would.

// `info!` is only reachable from the unix implementation below, so importing it
// at module scope would be an unused import on Windows dev builds. Qualify it
// there instead and keep only `warn!`, which both paths use, imported here.
use tracing::warn;

/// What we ask for when `ULTROS_MAX_OPEN_FILES` is unset.
///
/// Chosen to be comfortably above any burst we have observed (steady state is
/// ~150 descriptors) while staying well under the 524288 hard limit. The cost of
/// a larger limit is a slightly bigger kernel fd table, not reserved memory.
pub(crate) const DEFAULT_MAX_OPEN_FILES: u64 = 65536;

/// Decide what to set the soft limit to, given the current limits and what we
/// want.
///
/// Split out from the syscalls so the policy is testable — the `setrlimit` path
/// itself is three lines of FFI with nothing to get wrong once this is right.
///
/// Returns `None` when no change is warranted, either because the soft limit is
/// already at least as high as we want or because it already equals the hard
/// limit (nothing left to claim without privileges).
// Genuinely dead on Windows: the only caller is the `#[cfg(unix)]` implementation
// below, and Windows has no `RLIMIT_NOFILE` to target. Keeping the function (and
// its tests) compiled on both platforms is worth more than silencing it by
// `#[cfg(unix)]`-gating the whole module — a dev on Windows still typechecks the
// policy they might be editing.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn target_soft_limit(soft: u64, hard: u64, requested: u64) -> Option<u64> {
    // Never ask for more than the hard limit: `setrlimit` would fail with
    // EPERM and we would lose the raise we *could* have had. `RLIM_INFINITY`
    // is u64::MAX, so this clamp is a no-op in that case, which is correct.
    let desired = requested.min(hard);
    (desired > soft).then_some(desired)
}

/// Raise `RLIMIT_NOFILE`'s soft limit toward the hard limit.
///
/// Best-effort by design: a failure here means we run with the limit we already
/// had, which is exactly today's behaviour, so it warns rather than aborting
/// startup. Call once, early in `main`, before anything opens sockets.
pub(crate) fn raise_open_file_limit() {
    let requested = match std::env::var("ULTROS_MAX_OPEN_FILES") {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(parsed) if parsed > 0 => parsed,
            _ => {
                warn!(
                    value = %raw,
                    default = DEFAULT_MAX_OPEN_FILES,
                    "ULTROS_MAX_OPEN_FILES is not a positive integer; using the default"
                );
                DEFAULT_MAX_OPEN_FILES
            }
        },
        Err(_) => DEFAULT_MAX_OPEN_FILES,
    };

    raise_open_file_limit_to(requested);
}

#[cfg(unix)]
fn raise_open_file_limit_to(requested: u64) {
    // Safety: `getrlimit`/`setrlimit` only read/write the `rlimit` we hand them,
    // and we pass a valid, fully-initialized pointer to a local in both calls.
    let mut limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) } != 0 {
        warn!(
            error = %std::io::Error::last_os_error(),
            "could not read RLIMIT_NOFILE; leaving the open-file limit alone"
        );
        return;
    }

    // Plain annotated bindings rather than `as u64` casts: `rlim_t` is already
    // `u64` on every platform we build for, so a cast would be a no-op that
    // trips `clippy::unnecessary_cast` under `-D warnings`. If some future
    // target defines `rlim_t` differently this becomes a compile error there,
    // which is the outcome we want — a silent truncation of a limit would be
    // much worse than a build break.
    let soft: u64 = limits.rlim_cur;
    let hard: u64 = limits.rlim_max;
    let Some(target) = target_soft_limit(soft, hard, requested) else {
        tracing::info!(
            soft,
            hard,
            requested,
            "RLIMIT_NOFILE soft limit is already high enough"
        );
        return;
    };

    // `target_soft_limit` clamped this to `hard`, i.e. to `rlim_max`, so it
    // always fits back into the field it came from.
    limits.rlim_cur = target;
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limits) } != 0 {
        warn!(
            error = %std::io::Error::last_os_error(),
            soft, hard, target,
            "failed to raise the RLIMIT_NOFILE soft limit"
        );
        return;
    }

    tracing::info!(
        previous_soft = soft,
        soft = target,
        hard,
        "raised the RLIMIT_NOFILE soft limit"
    );
}

/// Windows has no `RLIMIT_NOFILE`; the CRT's descriptor ceiling is not a
/// per-process rlimit and sockets do not draw from it. Nothing to do, but the
/// dev build must still compile.
#[cfg(not(unix))]
fn raise_open_file_limit_to(_requested: u64) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raises_toward_the_requested_limit() {
        assert_eq!(target_soft_limit(1024, 524288, 65536), Some(65536));
    }

    #[test]
    fn clamps_to_the_hard_limit() {
        // Asking past the hard limit must still take the raise we can have,
        // rather than issuing a request `setrlimit` would reject with EPERM.
        assert_eq!(target_soft_limit(1024, 4096, 65536), Some(4096));
    }

    #[test]
    fn no_change_when_already_high_enough() {
        assert_eq!(target_soft_limit(65536, 524288, 65536), None);
        assert_eq!(target_soft_limit(131072, 524288, 65536), None);
    }

    #[test]
    fn no_change_when_soft_already_equals_hard() {
        // Nothing left to claim without CAP_SYS_RESOURCE, so don't issue a
        // syscall that can only fail.
        assert_eq!(target_soft_limit(4096, 4096, 65536), None);
    }

    #[test]
    fn infinite_hard_limit_grants_the_full_request() {
        assert_eq!(
            target_soft_limit(1024, u64::MAX, DEFAULT_MAX_OPEN_FILES),
            Some(DEFAULT_MAX_OPEN_FILES)
        );
    }

    /// The exact production numbers this fix exists for: GlitchTip #7188 was
    /// raised by a container running the Docker default soft limit.
    #[test]
    fn fixes_the_observed_production_limits() {
        assert_eq!(
            target_soft_limit(1024, 524288, DEFAULT_MAX_OPEN_FILES),
            Some(65536)
        );
    }
}
