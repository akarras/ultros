//! The cart's always-visible summary: the estimated total for the units
//! still to buy, and an honest account of what that total covers.

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};

use super::estimate::{CartEstimate, estimate_cart};
use super::row::gil_text;
use crate::i18n::*;

#[component]
pub fn CartSummary(rows: Signal<Vec<(ListItem, Vec<ActiveListing>)>>) -> impl IntoView {
    let i18n = use_i18n();
    let cart = Memo::new(move |_| {
        rows.with(|rows| {
            estimate_cart(
                rows.iter()
                    .map(|(item, listings)| (item, listings.as_slice())),
            )
        })
    });
    let coverage = move || {
        let CartEstimate {
            short_lines,
            unknown_lines,
            requested_units,
            covered_units,
            ..
        } = cart.get();
        if requested_units == 0 {
            return t_string!(i18n, cart_nothing_to_buy).to_string();
        }
        let mut parts = Vec::new();
        if short_lines == 0 && unknown_lines == 0 {
            parts.push(t_string!(i18n, cart_covers_all, units = requested_units).to_string());
        } else {
            parts.push(
                t_string!(
                    i18n,
                    cart_covers_partial,
                    covered = covered_units,
                    requested = requested_units
                )
                .to_string(),
            );
        }
        if unknown_lines > 0 {
            parts.push(t_string!(i18n, cart_no_listings, count = unknown_lines).to_string());
        }
        parts.join(" · ")
    };
    view! {
        <div class="panel flex flex-wrap items-center justify-between gap-x-6 gap-y-2 rounded-xl px-4 py-3" data-testid="cart-summary">
            <div class="flex flex-col">
                <span class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, cart_estimated_total)}</span>
                <span class="text-2xl font-bold tabular-nums" data-testid="cart-total">
                    {move || {
                        let cart = cart.get();
                        let prefix = if cart.complete() { "" } else { "≥" };
                        format!("{prefix}{}", gil_text(i18n, cart.total))
                    }}
                </span>
            </div>
            <div class="flex flex-col gap-1 text-sm text-[color:var(--color-text-muted)]">
                <span data-testid="cart-coverage">{coverage}</span>
                <span>{move || t_string!(i18n, cart_items_count, count = cart.get().lines).to_string()}</span>
            </div>
            <p class="basis-full text-xs text-[color:var(--color-text-muted)]">{t!(i18n, cart_estimate_note)}</p>
        </div>
    }
}
