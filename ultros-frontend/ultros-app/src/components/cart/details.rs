//! Per-row details: the secondary fields that used to sit beside every item
//! (owned quantity, target price). Opened from the row's details toggle;
//! listings and pricing detail join here in #1435.

use leptos::prelude::*;
use ultros_api_types::list::ListItem;

use super::row::numeric_editor;
use crate::i18n::*;

#[component]
pub fn CartRowDetails(
    /// The element id the row's toggle names in `aria-controls`.
    id: String,
    item: Signal<ListItem>,
    name: String,
    can_write: Signal<bool>,
    on_edit: Callback<ListItem>,
) -> impl IntoView {
    let i18n = use_i18n();
    let owned = numeric_editor(
        item,
        name.clone(),
        t_string!(i18n, lists_workspace_owned).to_string(),
        1,
        can_write,
        on_edit,
        "input w-24 text-right tabular-nums",
    );
    let target = numeric_editor(
        item,
        name,
        t_string!(i18n, lists_workspace_target_price).to_string(),
        2,
        can_write,
        on_edit,
        "input w-32 text-right tabular-nums",
    );
    view! {
        <div id=id class="order-last col-span-full flex flex-wrap items-end gap-4 rounded-lg bg-[color:var(--color-background)] px-3 py-2 text-sm" data-testid="cart-row-details">
            <label class="flex flex-col gap-1">
                <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_owned)}</span>
                {owned}
            </label>
            <label class="flex flex-col gap-1">
                <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_target_price)}</span>
                {target}
            </label>
        </div>
    }
}
