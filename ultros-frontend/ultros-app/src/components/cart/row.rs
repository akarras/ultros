//! One compact cart row: identity, quantity, quality, estimated line cost,
//! a details toggle and an icon-only delete button. Commit-on-change:
//! typing never writes or reorders the document; Enter blurs to commit,
//! Escape restores the committed value. Owned quantity, target price and
//! listing detail live in the row's details panel (`details.rs`).

use std::collections::HashSet;

use icondata as i;
use leptos::prelude::*;
use thousands::Separable;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use super::details::CartRowDetails;
use super::estimate::{Coverage, LineEstimate};
use crate::components::icon::Icon;
use crate::components::item_icon::*;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;

/// Shared by the header row and every item row so the columns line up.
pub const ROW_GRID: &str = "grid items-center gap-x-2 gap-y-1 px-2 py-1.5 grid-cols-[auto_minmax(0,1fr)_auto] sm:grid-cols-[2rem_minmax(0,1fr)_5.5rem_6.5rem_8rem_2.5rem_2.5rem]";

/// The commit-on-change numeric editor the row and its details share.
/// `field` selects quantity (0), owned (1) or target price (2).
pub fn numeric_editor(
    row: Signal<ListItem>,
    name: String,
    label: String,
    field: u8,
    can_write: Signal<bool>,
    on_edit: Callback<ListItem>,
    class: &'static str,
    id: Option<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let value = Memo::new(move |_| {
        let item = row.get();
        match field {
            0 => item.quantity.unwrap_or(1).to_string(),
            1 => item.acquired.unwrap_or(0).to_string(),
            _ => item.target_price.map(|v| v.to_string()).unwrap_or_default(),
        }
    });
    view! {
        <input id=id class=class type="number" inputmode="numeric" min=if field == 0 { "1" } else { "0" } aria-label=t_string!(i18n, lists_workspace_field_named, label = label, name = name) prop:value=move || value.get() readonly=move || !can_write.get()
            on:keydown=move |ev| {
                ev.stop_propagation();
                if ev.key() == "Enter" { let _ = event_target::<web_sys::HtmlInputElement>(&ev).blur(); }
                if ev.key() == "Escape" { event_target::<web_sys::HtmlInputElement>(&ev).set_value(&value.get_untracked()); }
            }
            on:change=move |ev| {
                let entered = event_target_value(&ev);
                let mut updated = row.get_untracked();
                let valid = if field == 2 && entered.is_empty() { updated.target_price = None; true }
                else if field == 2 { entered.parse::<i64>().ok().filter(|v| *v >= 0).map(|v| updated.target_price = Some(v)).is_some() }
                else { entered.parse::<i32>().ok().filter(|v| *v >= if field == 0 {1} else {0}).map(|v| if field == 0 { updated.quantity = Some(v); } else { updated.acquired = Some(v); }).is_some() };
                if valid && can_write.get_untracked() { on_edit.run(updated); } else { event_target::<web_sys::HtmlInputElement>(&ev).set_value(&value.get_untracked()); }
            } />
    }
}

/// Gil with thousands separators through the locale's gil template.
pub fn gil_text(i18n: leptos_i18n::I18nContext<Locale, I18nKeys>, amount: i64) -> String {
    t_string!(
        i18n,
        lists_workspace_gil,
        price = amount.separate_with_commas()
    )
    .to_string()
}

/// Move keyboard focus to the element with this id, if it is in the DOM.
pub fn focus_element(id: &str) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(element) = leptos::prelude::document()
            .get_element_by_id(id)
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = element.focus();
        }
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = id;
    }
}

pub fn details_toggle_id(row_id: i32) -> String {
    format!("cart-details-toggle-{row_id}")
}

pub fn remove_button_id(row_id: i32) -> String {
    format!("cart-remove-{row_id}")
}

pub fn quantity_input_id(row_id: i32) -> String {
    format!("cart-qty-{row_id}")
}

#[component]
pub fn CartRow(
    item: Signal<ListItem>,
    listings: Signal<Vec<ActiveListing>>,
    line: Signal<Option<LineEstimate>>,
    selected_items: RwSignal<HashSet<i32>>,
    /// Row ids whose details panel is open; keyed by id so a reactive
    /// update or re-sort never moves the open panel to another row.
    expanded: RwSignal<HashSet<i32>>,
    /// Rows whose removal is in flight; their delete button is disabled.
    #[prop(default = Signal::derive(HashSet::new))]
    removing: Signal<HashSet<i32>>,
    on_edit: Callback<ListItem>,
    on_delete: Callback<i32>,
    can_write: Signal<bool>,
    #[prop(default = Signal::derive(|| false))] highlighted: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let initial = item.get_untracked();
    let name = tracked_data()
        .items
        .get(&ItemId(initial.item_id))
        .map(|i| i.name.to_string())
        .unwrap_or_else(|| {
            t_string!(i18n, lists_workspace_item_fallback, id = initial.item_id).to_string()
        });
    let can_hq = tracked_data()
        .items
        .get(&ItemId(initial.item_id))
        .is_some_and(|i| i.can_be_hq);
    let row = item;
    let id = initial.id;
    let is_open = Memo::new(move |_| expanded.with(|open| open.contains(&id)));
    let details_id = format!("cart-details-{id}");
    let toggle_id = details_toggle_id(id);
    let close_details = Callback::new(move |()| {
        expanded.update(|open| {
            open.remove(&id);
        });
        focus_element(&details_toggle_id(id));
    });
    let quantity = numeric_editor(
        row,
        name.clone(),
        t_string!(i18n, lists_workspace_needed).to_string(),
        0,
        can_write,
        on_edit,
        "input w-full min-w-0 text-right tabular-nums",
        Some(quantity_input_id(id)),
    );
    let estimate_text = move || {
        let Some(line) = line.get() else {
            return view! { <span class="text-[color:var(--color-text-muted)]">"—"</span> }
                .into_any();
        };
        match (line.coverage, line.total) {
            (Coverage::None, _) => view! {
                <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, cart_line_no_listings)}</span>
            }
            .into_any(),
            (Coverage::Partial, Some(total)) => view! {
                <span class="flex flex-col items-end leading-tight">
                    <span>{"≥"}{gil_text(i18n, total)}</span>
                    <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, cart_line_partial, covered = line.covered, requested = line.requested)}</span>
                </span>
            }
            .into_any(),
            (_, Some(total)) => view! {
                <span class=if line.requested == 0 { "text-[color:var(--color-text-muted)]" } else { "" }>{gil_text(i18n, total)}</span>
            }
            .into_any(),
            (_, None) => view! { <span class="text-[color:var(--color-text-muted)]">"—"</span> }.into_any(),
        }
    };
    let select_name = name.clone();
    let quality_name = name.clone();
    let details_name = name.clone();
    let remove_name = name.clone();
    let panel_name = name.clone();
    let panel_controls = details_id.clone();
    view! {
        <li class=format!("{ROW_GRID} border-b border-[color:var(--color-outline)] last:border-b-0 hover:bg-[color:var(--color-background-panel)] transition-colors") class:ring-2=highlighted class:ring-brand-400=highlighted data-item-id=initial.item_id data-row-id=id>
            <input class="order-1 h-5 w-5 sm:justify-self-center" type="checkbox" aria-label=t_string!(i18n, lists_workspace_select_named, name = select_name) disabled=move || !can_write.get() prop:checked=move || selected_items.with(|s| s.contains(&id)) on:change=move |_| selected_items.update(|s| { if !s.remove(&id) {s.insert(id);} }) />
            <div class="order-2 flex min-w-0 items-center gap-2">
                <ItemIcon item_id=initial.item_id icon_size=IconSize::Small />
                <span class="truncate font-semibold" title=name.clone()>{name.clone()}</span>
            </div>
            <div class="order-3 text-right tabular-nums text-sm sm:order-5">
                <span class="sr-only">{t!(i18n, cart_est_cost)}</span>
                {estimate_text}
            </div>
            <div class="order-4 col-span-2 col-start-2 flex items-center gap-2 sm:contents">
                <div class="w-20 sm:order-3 sm:w-auto">{quantity}</div>
                <select class="input min-w-0 sm:order-4" aria-label=t_string!(i18n, lists_workspace_field_named, label = t_string!(i18n, lists_workspace_quality).to_string(), name = quality_name) disabled=move || !can_write.get() prop:value=move || match row.get().hq {Some(true) => "hq", Some(false) => "nq", None => "any"} on:change=move |ev| {let mut updated = row.get_untracked(); updated.hq = match event_target_value(&ev).as_str() {"hq" if can_hq => Some(true), "nq" => Some(false), _ => None}; on_edit.run(updated);}>
                    <option value="any">{t!(i18n, lists_workspace_any)}</option>
                    <option value="nq">{t!(i18n, lists_workspace_nq)}</option>
                    {can_hq.then(|| view! { <option value="hq">{t!(i18n, lists_workspace_hq)}</option> })}
                </select>
                <button type="button" id=toggle_id class="btn-ghost inline-flex h-10 w-10 items-center justify-center p-0 sm:order-6" aria-label=t_string!(i18n, cart_details_for, name = details_name) aria-expanded=move || is_open.get().to_string() aria-controls=details_id.clone() on:click=move |_| expanded.update(|open| { if !open.remove(&id) { open.insert(id); } })>
                    <span class="inline-flex transition-transform" class:rotate-180=move || is_open.get()><Icon icon=i::BiChevronDownRegular /></span>
                </button>
                <button type="button" class="btn-ghost inline-flex h-10 w-10 items-center justify-center p-0 text-[color:var(--color-text-muted)] hover:text-red-300 sm:order-7" aria-label=t_string!(i18n, cart_remove_named, name = remove_name) data-testid="cart-remove" id=remove_button_id(id) disabled=move || !can_write.get() || removing.with(|set| set.contains(&id)) on:click=move |_| on_delete.run(id)>
                    <Icon icon=i::BiTrashRegular />
                </button>
            </div>
            <Show when=move || is_open.get()>
                <CartRowDetails id=panel_controls.clone() item=row listings line name=panel_name.clone() can_write on_edit on_close=close_details />
            </Show>
        </li>
    }
}
