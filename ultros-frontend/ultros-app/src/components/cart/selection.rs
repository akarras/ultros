//! Bulk controls, revealed only while something is selected. The checkboxes
//! that feed the selection stay visible on every row (`row.rs`); this bar
//! is what appears once the first one is ticked.

use std::collections::HashSet;

use icondata as i;
use leptos::prelude::*;

use crate::components::icon::Icon;
use crate::components::loading::Loading;
use crate::i18n::*;

/// Only rows whose item can be HQ take part in a bulk "Set HQ"; the rest
/// keep their quality instead of failing the whole edit.
pub fn hq_capable(ids: Vec<i32>) -> Vec<i32> {
    ids.into_iter()
        .filter(|id| {
            crate::list_doc::adapter::key_of(*id)
                .and_then(|key| {
                    crate::global_state::xiv_data::tracked_data()
                        .items
                        .get(&xiv_gen::ItemId(key.item_id))
                })
                .is_some_and(|item| item.can_be_hq)
        })
        .collect()
}

/// Drop selections (or open panels) whose row is no longer in the cart, so
/// a deletion — local or remote — never leaves a phantom count behind or
/// applies a later bulk action to a row the player cannot see.
pub fn retain_present(set: &mut HashSet<i32>, present: impl Fn(i32) -> bool) -> bool {
    let before = set.len();
    set.retain(|id| present(*id));
    set.len() != before
}

#[component]
pub fn CartSelectionBar(
    selected_items: RwSignal<HashSet<i32>>,
    /// Ids of the rows currently shown, in display order.
    visible_ids: Signal<Vec<i32>>,
    can_write: Signal<bool>,
    pending: Signal<bool>,
    on_remove_many: Callback<Vec<i32>>,
    on_set_quality: Callback<(Vec<i32>, Option<bool>)>,
) -> impl IntoView {
    let i18n = use_i18n();
    let count = Memo::new(move |_| selected_items.with(|s| s.len()));
    let selected_vec =
        move || selected_items.with_untracked(|s| s.iter().copied().collect::<Vec<_>>());
    let all_visible_selected = Memo::new(move |_| {
        let ids = visible_ids.get();
        !ids.is_empty() && selected_items.with(|s| ids.iter().all(|id| s.contains(id)))
    });
    view! {
        <Show when=move || !selected_items.with(|s| s.is_empty()) && can_write.get()>
            <div class="flex flex-wrap items-center gap-2 rounded-xl border border-brand-500/40 bg-brand-900/20 px-3 py-2 text-sm" role="region" aria-label=t_string!(i18n, cart_selection_region) data-testid="cart-selection-bar">
                <span class="font-semibold" role="status">{move || t_string!(i18n, cart_selected_count, count = count.get()).to_string()}</span>
                <button type="button" class="btn-ghost" disabled=move || all_visible_selected.get() on:click=move |_| {
                    let ids = visible_ids.get_untracked();
                    selected_items.update(|s| s.extend(ids));
                }>{t!(i18n, cart_select_all_visible)}</button>
                <span class="mx-1 h-4 border-l border-[color:var(--color-outline)]" aria-hidden="true"></span>
                <button type="button" class="btn-ghost" disabled=pending data-testid="cart-bulk-hq" on:click=move |_| on_set_quality.run((hq_capable(selected_vec()), Some(true)))>{t!(i18n, list_view_bulk_set_hq)}</button>
                <button type="button" class="btn-ghost" disabled=pending data-testid="cart-bulk-any" on:click=move |_| on_set_quality.run((selected_vec(), None))>{t!(i18n, list_view_bulk_any_quality)}</button>
                <button type="button" class="btn-ghost text-red-300 hover:text-red-200" disabled=pending data-testid="cart-bulk-delete" on:click=move |_| {
                    let ids = selected_vec();
                    selected_items.update(|s| s.clear());
                    on_remove_many.run(ids);
                }>
                    <Icon icon=i::BiTrashRegular />
                    <span>{t!(i18n, cart_delete_selected)}</span>
                </button>
                <button type="button" class="btn-ghost ml-auto" on:click=move |_| selected_items.update(|s| s.clear())>{t!(i18n, cart_clear_selection)}</button>
                <Show when=move || pending.get()>
                    <span class="flex items-center gap-2 text-[color:var(--color-text-muted)]"><Loading />{t!(i18n, list_view_bulk_pending)}</span>
                </Show>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retain_present_drops_only_missing_ids_and_reports_a_change() {
        let mut set: HashSet<i32> = [1, 2, 3].into_iter().collect();
        assert!(retain_present(&mut set, |id| id != 2));
        let expected: HashSet<i32> = [1, 3].into_iter().collect();
        assert_eq!(set, expected);
        assert!(!retain_present(&mut set, |_| true));
        assert_eq!(set.len(), 2);
    }
}
