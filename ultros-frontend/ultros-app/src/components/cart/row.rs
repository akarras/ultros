//! One cart row. Commit-on-change: typing never writes or reorders the
//! document; Enter blurs to commit, Escape restores the committed value.

use std::collections::HashSet;

use leptos::prelude::*;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use super::estimate::LineEstimate;
use crate::components::item_icon::*;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;

#[component]
pub fn CartRow(
    item: Signal<ListItem>,
    line: Signal<Option<LineEstimate>>,
    selected_items: RwSignal<HashSet<i32>>,
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
    let display_name = name.clone();
    let row = item;
    let id = initial.id;
    let numeric = move |label: String, field: u8| {
        let value = Memo::new(move |_| {
            let item = row.get();
            match field {
                0 => item.quantity.unwrap_or(1).to_string(),
                1 => item.acquired.unwrap_or(0).to_string(),
                _ => item.target_price.map(|v| v.to_string()).unwrap_or_default(),
            }
        });
        view! {
            <input class="input w-24" type="number" min=if field == 0 { "1" } else { "0" } aria-label=t_string!(i18n, lists_workspace_field_named, label = label.clone(), name = name.clone()) prop:value=move || value.get() attr:data-committed=move || value.get() readonly=move || !can_write.get()
                on:keydown=move |ev| {
                    // Only the keys this cell handles stop here; Ctrl+Z must
                    // reach the window listener, which decides between native
                    // text undo (a draft) and document undo (a clean cell) from
                    // `data-committed` (#1430).
                    if ev.key() == "Enter" { ev.stop_propagation(); let _ = event_target::<web_sys::HtmlInputElement>(&ev).blur(); }
                    if ev.key() == "Escape" { ev.stop_propagation(); event_target::<web_sys::HtmlInputElement>(&ev).set_value(&value.get_untracked()); }
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
    };
    let needed = numeric(t_string!(i18n, lists_workspace_needed).to_string(), 0);
    let owned = numeric(t_string!(i18n, lists_workspace_owned).to_string(), 1);
    let target = numeric(t_string!(i18n, lists_workspace_target_price).to_string(), 2);
    view! {
        <tr class="hover:bg-[color:var(--color-background-panel)] transition-colors" class:ring-2=highlighted class:ring-brand-400=highlighted data-item-id=initial.item_id>
            <td class="p-3"><input type="checkbox" aria-label=t_string!(i18n, lists_workspace_select_named, name = display_name.clone()) disabled=move || !can_write.get() prop:checked=move || selected_items.with(|s| s.contains(&id)) on:change=move |_| selected_items.update(|s| { if !s.remove(&id) {s.insert(id);} }) /></td>
            <td class="p-3"><div class="flex items-center gap-3"><ItemIcon item_id=initial.item_id icon_size=IconSize::Small /><span class="font-semibold">{display_name}</span></div></td>
            <td class="p-3"><select class="input min-w-24" aria-label=t_string!(i18n, lists_workspace_item_quality) disabled=move || !can_write.get() prop:value=move || match row.get().hq {Some(true) => "hq", Some(false) => "nq", None => "any"} on:change=move |ev| {let mut updated = row.get_untracked(); updated.hq = match event_target_value(&ev).as_str() {"hq" if can_hq => Some(true), "nq" => Some(false), _ => None}; on_edit.run(updated);}><option value="any">{t!(i18n, lists_workspace_any)}</option><option value="nq">{t!(i18n, lists_workspace_nq)}</option><option value="hq" disabled=!can_hq>{t!(i18n, lists_workspace_hq)}</option></select></td>
            <td class="p-3">{needed}</td><td class="p-3">{owned}</td><td class="p-3 tabular-nums">{move || line.get().and_then(|line| line.unit_price).map(|price| t_string!(i18n, lists_workspace_gil, price = price).to_string()).unwrap_or_else(|| "—".to_string())}</td><td class="p-3">{target}</td>
            <td class="p-3"><button class="btn-ghost" disabled=move || !can_write.get() on:click=move |_| on_delete.run(id)>{t!(i18n, lists_workspace_remove)}</button></td>
        </tr>
    }
}
