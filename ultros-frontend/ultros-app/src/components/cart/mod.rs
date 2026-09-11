//! The redesigned Build cart (Track B of #1427), mounted behind the
//! `lists-sync` Labs experiment for account and device lists alike.
//!
//! Presentation only: the [`ListWorkspaceSource`] it consumes carries the
//! rows, capabilities and edit callbacks, and the route that built the
//! source keeps the document, sync, authorization and guest storage
//! lifecycles. Each part of the cart lives in its own file so the follow-on
//! tickets can change rows, summary, details, selection and feedback
//! independently. The legacy Labs grid, `ListBuildWorkspace`, stays in
//! `routes::list_view_sync` untouched.

pub mod details;
pub mod estimate;
pub mod row;

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::list_view::remaining_quantity;
use crate::routes::list_view_sync::{InlineListAdd, InlineRecipeAdd, ListWorkspaceSource};

use row::{CartRow, ROW_GRID};

/// `?cart=legacy` mounts the previous Labs grid (`ListBuildWorkspace`) so a
/// tester can compare the two presentations on the same list. The
/// redesigned cart is the default under the experiment; no cookie or Labs
/// token changes here.
pub fn use_legacy_cart() -> Signal<bool> {
    let query = ultros_ui::components::app_link::use_query_map_or_default();
    Memo::new(move |_| query.with(|q| q.get("cart").is_some_and(|v| v == "legacy"))).into()
}

/// The list is mounted once, independently of resource revisions. Row
/// identity is the document row key; each cell reads its current value
/// from its own memo, so a remote update never rebuilds an editor.
#[component]
pub fn ListCart(
    source: ListWorkspaceSource,
    selected_items: RwSignal<HashSet<i32>>,
    #[prop(default = Signal::derive(HashSet::new))] highlighted: Signal<HashSet<i32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let filter = RwSignal::new(String::new());
    // Row ids with an open details panel. Keyed by id, not position.
    let expanded: RwSignal<HashSet<i32>> = RwSignal::new(HashSet::new());
    // While an editor inside the list has focus, rows keep their order and
    // an acquired row stays visible, so committing an edit never moves the
    // control the player is typing in.
    let editing = RwSignal::new(false);
    let grid = NodeRef::<leptos::html::Div>::new();
    let visible = Memo::new(
        move |previous: Option<&Vec<(ListItem, Vec<ActiveListing>)>>| {
            let query = filter.get().to_lowercase();
            let data = tracked_data();
            let pinned: HashSet<i32> = if editing.get() {
                previous
                    .into_iter()
                    .flatten()
                    .map(|(item, _)| item.id)
                    .collect()
            } else {
                HashSet::new()
            };
            let mut rows = source
                .rows
                .get()
                .into_iter()
                .filter(|(item, _)| {
                    !source.hide_acquired.get()
                        || remaining_quantity(item) > 0
                        || pinned.contains(&item.id)
                })
                .filter(|(item, _)| {
                    query.is_empty()
                        || data
                            .items
                            .get(&ItemId(item.item_id))
                            .is_some_and(|i| i.name.to_lowercase().contains(&query))
                        || item.item_id.to_string().contains(&query)
                })
                .collect::<Vec<_>>();
            if editing.get()
                && let Some(previous) = previous
            {
                let positions: HashMap<_, _> = previous
                    .iter()
                    .enumerate()
                    .map(|(index, (item, _))| (item.id, index))
                    .collect();
                rows.sort_by_key(|(item, _)| {
                    positions.get(&item.id).copied().unwrap_or(usize::MAX)
                });
            }
            rows
        },
    );
    let is_empty = Memo::new(move |_| source.rows.with(|rows| rows.is_empty()));
    // The whole cart, not the filtered view, priced by the shared estimator
    // (#1431/#1432): a filter narrows what the list shows, never what it
    // costs, and both Labs presentations must agree on the total.
    let estimate = Memo::new(move |_| {
        source
            .rows
            .with(|rows| ultros_calc::list_estimate::estimate_list_items(rows))
    });
    view! {
        <section class="space-y-3" data-testid="list-cart">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=source.add pending=source.pending feedback=source.feedback />
                <div class="flex flex-wrap items-center gap-2 text-sm">
                    <button type="button" class="btn-ghost" aria-expanded=move || source.recipe_open.get().to_string() on:click=move |_| source.toggle_recipe.run(())>{t!(i18n, lists_workspace_add_recipe)}</button>
                    <span class="mx-1 h-4 border-l border-[color:var(--color-outline)]" aria-hidden="true"></span>
                    <button type="button" class="btn-ghost disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-undo" disabled=move || !source.can_undo.get() title=move || (!source.can_undo.get()).then(|| t_string!(i18n, lists_workspace_nothing_to_undo).to_string()) on:click=move |_| source.undo.run(())>{t!(i18n, lists_workspace_undo)}</button>
                    <button type="button" class="btn-ghost disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-redo" disabled=move || !source.can_redo.get() title=move || (!source.can_redo.get()).then(|| t_string!(i18n, lists_workspace_nothing_to_redo).to_string()) on:click=move |_| source.redo.run(())>{t!(i18n, lists_workspace_redo)}</button>
                </div>
                <Show when=move || source.recipe_open.get()><InlineRecipeAdd list_id=source.list_id on_add=source.add_many /></Show>
            </Show>
            <crate::components::list_estimate_summary::ListEstimateSummary estimate=estimate.into() feed=source.market scope=source.scope_name />
            <input class="input w-full" type="search" aria-label=t_string!(i18n, lists_workspace_filter_label) placeholder=t_string!(i18n, lists_workspace_filter_placeholder) prop:value=move || filter.get() data-committed="" on:input=move |ev| filter.set(event_target_value(&ev)) />
            <div class="panel rounded-xl" node_ref=grid on:focusin=move |_| editing.set(true) on:focusout=move |ev| {
                #[cfg(feature = "hydrate")]
                {
                use wasm_bindgen::JsCast;
                let inside = ev.related_target().and_then(|target| target.dyn_into::<web_sys::Node>().ok()).is_some_and(|target| grid.get().is_some_and(|grid| grid.contains(Some(&target))));
                if !inside { editing.set(false); }
                }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            }>
                <div class=format!("{ROW_GRID} hidden sm:grid border-b border-[color:var(--color-outline)] text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]") aria-hidden="true">
                    <span></span>
                    <span>{t!(i18n, lists_workspace_item)}</span>
                    <span class="text-right">{t!(i18n, cart_qty)}</span>
                    <span>{t!(i18n, lists_workspace_quality)}</span>
                    <span class="text-right">{t!(i18n, cart_est_cost)}</span>
                    <span></span>
                    <span></span>
                </div>
                <ul class="text-sm" data-testid="cart-rows">
                    <For each=move || { visible.get().into_iter().map(|(item, _)| item).collect::<Vec<_>>() } key=|item| item.id children=move |initial| {
                        let id = initial.id;
                        let fallback = StoredValue::new(initial);
                        let item = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(item, _)| item.clone())).unwrap_or_else(|| fallback.get_value()));
                        let line = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(item, listings)| estimate::estimate_line(item, listings))));
                        view! { <CartRow item=item.into() line=line.into() selected_items expanded on_edit=source.edit on_delete=source.remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
                    } />
                </ul>
                <Show when=move || is_empty.get()>
                    <p class="px-4 py-6 text-center text-sm text-[color:var(--color-text-muted)]">{t!(i18n, cart_empty)}</p>
                </Show>
            </div>
        </section>
    }
}
