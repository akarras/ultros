//! Ultros: account for decompressed block bytes before extending a destination.
//! This bounds LZ4 expansion, not arbitrary allocations or native faults.
use loro_common::{LoroError, LoroResult};
use std::cell::Cell;

thread_local! { static REMAINING: Cell<Option<usize>> = const { Cell::new(None) }; }
thread_local! { static REDECODING: Cell<bool> = const { Cell::new(false) }; }

pub(crate) fn active() -> bool {
    REMAINING.get().is_some()
}

/// A table was already fully validated/accounted at import. Its bounded cache
/// may evict a block; revisiting it must not exhaust a budget inside an
/// infallible lazy getter and poison Loro's internal mutexes.
pub(crate) fn validated_redecode<T>(operation: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            REDECODING.set(self.0);
        }
    }
    let guard = Restore(REDECODING.replace(true));
    let value = operation();
    drop(guard);
    value
}

pub fn with_decode_budget<T>(limit: usize, operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<usize>);
    impl Drop for Reset {
        fn drop(&mut self) {
            REMAINING.set(self.0);
        }
    }
    // A nested decode can reduce, never expand, the caller's remaining budget.
    let previous = REMAINING.get();
    let active = previous.map_or(limit, |remaining| remaining.min(limit));
    REMAINING.set(Some(active));
    let reset = Reset(previous);
    let result = operation();
    let consumed = active.saturating_sub(REMAINING.get().unwrap_or(0));
    drop(reset);
    if let Some(previous) = previous {
        REMAINING.set(Some(previous.saturating_sub(consumed)));
    }
    result
}

pub(crate) fn consume(bytes: usize) -> LoroResult<()> {
    if REDECODING.get() {
        return Ok(());
    }
    if let Some(remaining) = REMAINING.get() {
        let next = remaining.checked_sub(bytes).ok_or_else(|| {
            LoroError::DecodeError("decoded list history exceeds the memory budget".into())
        })?;
        REMAINING.set(Some(next));
    }
    Ok(())
}
