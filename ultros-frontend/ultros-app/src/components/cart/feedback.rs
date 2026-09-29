//! Compact deletion feedback: a polite live region under the summary that
//! names what was removed and offers Undo, with no confirmation dialog.
//!
//! The toast only appears once the rows have actually left the document,
//! so a failed removal never reads as success. Every local write through
//! the cart bumps an action counter; the toast remembers the counter at its
//! removal and offers Undo only while nothing else has happened since, so
//! it can never undo the wrong action. Track A owns what an undo step is;
//! this file only decides when to offer one.

use leptos::prelude::*;

use crate::i18n::*;

/// How long a removal toast stays before it dismisses itself.
pub const TOAST_MS: u32 = 8_000;

/// One removal the player can still reverse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    /// Row ids that were removed, in display order.
    pub ids: Vec<i32>,
    /// Names for the announcement; one per id where known.
    pub names: Vec<String>,
    /// The cart's action counter right after this removal.
    pub seq: u64,
    /// The row to focus after the removal (the next row, else the previous).
    pub focus_next: Option<i32>,
}

/// Undo is offered only for the most recent local action, on a writable
/// document that reports something to undo.
pub fn undo_offered(toast_seq: u64, current_seq: u64, can_undo: bool, can_write: bool) -> bool {
    toast_seq == current_seq && can_undo && can_write
}

/// Which row should take focus once `removed` leaves `order`: the row after
/// the last removed one, else the row before the first, else nothing (the
/// caller falls back to the add composer).
pub fn focus_after_removal(order: &[i32], removed: &[i32]) -> Option<i32> {
    let last = order.iter().rposition(|id| removed.contains(id))?;
    let first = order.iter().position(|id| removed.contains(id))?;
    order[last + 1..]
        .iter()
        .copied()
        .find(|id| !removed.contains(id))
        .or_else(|| {
            order[..first]
                .iter()
                .copied()
                .rev()
                .find(|id| !removed.contains(id))
        })
}

#[component]
pub fn CartFeedback(
    toast: RwSignal<Option<Removal>>,
    action_seq: Signal<u64>,
    can_undo: Signal<bool>,
    can_write: Signal<bool>,
    on_undo: Callback<Removal>,
) -> impl IntoView {
    let i18n = use_i18n();
    let message = move || {
        toast.get().map(|removal| match removal.names.as_slice() {
            [name] => t_string!(i18n, cart_removed_named, name = name.clone()).to_string(),
            _ => t_string!(i18n, cart_removed_count, count = removal.ids.len()).to_string(),
        })
    };
    let offered = Memo::new(move |_| {
        toast.get().is_some_and(|removal| {
            undo_offered(
                removal.seq,
                action_seq.get(),
                can_undo.get(),
                can_write.get(),
            )
        })
    });
    view! {
        <div role="status" aria-live="polite" aria-atomic="true" class="min-h-0" data-testid="cart-feedback">
            <Show when=move || toast.get().is_some()>
                <div class="flex flex-wrap items-center gap-3 rounded-xl border border-[color:var(--color-outline)] bg-[color:var(--color-background-elevated)] px-3 py-2 text-sm shadow" data-testid="cart-removal-toast">
                    <span>{message}</span>
                    <Show when=move || offered.get() fallback=move || view! {
                        <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, cart_undo_unavailable)}</span>
                    }>
                        <button type="button" class="btn-secondary !m-0 !px-3 !py-1" data-testid="cart-undo-removal" on:click=move |_| {
                            if let Some(removal) = toast.get_untracked() {
                                on_undo.run(removal);
                            }
                        }>{t!(i18n, cart_undo_removal)}</button>
                    </Show>
                    <button type="button" class="btn-ghost ml-auto !px-2 !py-1 text-xs" aria-label=t_string!(i18n, cart_dismiss) on:click=move |_| toast.set(None)>{t!(i18n, cart_dismiss)}</button>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_is_offered_only_for_the_latest_action_on_a_writable_document() {
        assert!(undo_offered(3, 3, true, true));
        assert!(
            !undo_offered(3, 4, true, true),
            "a later edit hides the toast's undo"
        );
        assert!(!undo_offered(3, 3, false, true), "nothing to undo");
        assert!(!undo_offered(3, 3, true, false), "read-only");
    }

    #[test]
    fn focus_goes_to_the_next_row_then_the_previous_then_nowhere() {
        let order = [10, 20, 30, 40];
        assert_eq!(focus_after_removal(&order, &[20]), Some(30));
        assert_eq!(focus_after_removal(&order, &[40]), Some(30));
        assert_eq!(focus_after_removal(&order, &[20, 30]), Some(40));
        assert_eq!(focus_after_removal(&order, &[30, 40]), Some(20));
        assert_eq!(focus_after_removal(&order, &[10, 20, 30, 40]), None);
        assert_eq!(
            focus_after_removal(&order, &[99]),
            None,
            "unknown rows focus nothing"
        );
    }
}
