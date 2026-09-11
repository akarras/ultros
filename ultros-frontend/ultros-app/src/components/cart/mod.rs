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

pub mod estimate;
pub mod row;
pub mod summary;

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::list_view::remaining_quantity;
use crate::routes::list_view_sync::{InlineListAdd, InlineRecipeAdd, ListWorkspaceSource};

use row::CartRow;
use summary::CartSummary;

/// `?cart=legacy` mounts the previous Labs grid (`ListBuildWorkspace`) so a
/// tester can compare the two presentations on the same list. The
/// redesigned cart is the default under the experiment; no cookie or Labs
/// token changes here.
pub fn use_legacy_cart() -> Signal<bool> {
    let query = ultros_ui::components::app_link::use_query_map_or_default();
    Memo::new(move |_| query.with(|q| q.get("cart").is_some_and(|v| v == "legacy"))).into()
}

/// The grid is mounted once, independently of resource revisions. Row
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
    // While an editor inside the grid has focus, rows keep their order and
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
    view! {
        <section class="space-y-3" data-testid="list-cart">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=source.add pending=source.pending feedback=source.feedback />
                <div class="flex gap-2 flex-wrap">
                    <button class="btn-secondary" on:click=move |_| source.toggle_recipe.run(())>{t!(i18n, lists_workspace_add_recipe)}</button>
                    <button class="btn-secondary" disabled=move || !source.can_undo.get() on:click=move |_| source.undo.run(())>{t!(i18n, lists_workspace_undo)}</button>
                    <button class="btn-secondary" disabled=move || !source.can_redo.get() on:click=move |_| source.redo.run(())>{t!(i18n, lists_workspace_redo)}</button>
                </div>
                <Show when=move || source.recipe_open.get()><InlineRecipeAdd list_id=source.list_id on_add=source.add_many /></Show>
            </Show>
            <CartSummary rows=source.rows />
            <input class="input w-full" aria-label=t_string!(i18n, lists_workspace_filter_label) placeholder=t_string!(i18n, lists_workspace_filter_placeholder) prop:value=move || filter.get() on:input=move |ev| filter.set(event_target_value(&ev)) />
            <div class="overflow-x-auto panel rounded-xl" node_ref=grid on:focusin=move |_| editing.set(true) on:focusout=move |ev| {
                #[cfg(feature = "hydrate")]
                {
                use wasm_bindgen::JsCast;
                let inside = ev.related_target().and_then(|target| target.dyn_into::<web_sys::Node>().ok()).is_some_and(|target| grid.get().is_some_and(|grid| grid.contains(Some(&target))));
                if !inside { editing.set(false); }
                }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            }>
                <table class="w-full min-w-[880px] text-sm"><thead><tr class="text-left border-b border-[color:var(--color-outline)]">
                    <th class="p-3">{t!(i18n, lists_workspace_select)}</th><th class="p-3">{t!(i18n, lists_workspace_item)}</th><th class="p-3">{t!(i18n, lists_workspace_quality)}</th><th class="p-3">{t!(i18n, lists_workspace_needed)}</th><th class="p-3">{t!(i18n, lists_workspace_owned)}</th><th class="p-3">{t!(i18n, lists_workspace_current_price)}</th><th class="p-3">{t!(i18n, lists_workspace_target_price)}</th><th class="p-3">{t!(i18n, lists_workspace_actions)}</th>
                </tr></thead><tbody>
                    <For each=move || { visible.get().into_iter().map(|(item, _)| item).collect::<Vec<_>>() } key=|item| item.id children=move |initial| {
                        let id = initial.id;
                        let fallback = StoredValue::new(initial);
                        let item = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(item, _)| item.clone())).unwrap_or_else(|| fallback.get_value()));
                        let line = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).map(|(item, listings)| estimate::estimate_line(item, listings))));
                        view! { <CartRow item=item.into() line=line.into() selected_items on_edit=source.edit on_delete=source.remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
                    } />
                </tbody></table>
            </div>
        </section>
    }
}
