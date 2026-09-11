//! The cart's always-visible summary. Counts in this slice; the estimated
//! total joins it once the compact rows land (#1434).

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};

use crate::i18n::*;
use crate::routes::list_view::remaining_quantity;

#[component]
pub fn CartSummary(rows: Signal<Vec<(ListItem, Vec<ActiveListing>)>>) -> impl IntoView {
    let i18n = use_i18n();
    let items = Memo::new(move |_| rows.with(|rows| rows.len()));
    let remaining = Memo::new(move |_| {
        rows.with(|rows| {
            rows.iter()
                .filter(|(item, _)| remaining_quantity(item) > 0)
                .count()
        })
    });
    view! {
        <dl class="flex flex-wrap gap-4 text-sm" data-testid="cart-summary">
            <div class="flex items-baseline gap-1">
                <dt class="text-[color:var(--color-text-muted)]">{t!(i18n, item_explorer_items)}</dt>
                <dd class="font-semibold tabular-nums">{move || items.get()}</dd>
            </div>
            <div class="flex items-baseline gap-1">
                <dt class="text-[color:var(--color-text-muted)]">{t!(i18n, list_view_remaining)}</dt>
                <dd class="font-semibold tabular-nums">{move || remaining.get()}</dd>
            </div>
        </dl>
    }
}
