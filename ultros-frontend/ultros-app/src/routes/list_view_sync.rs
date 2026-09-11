//! Labs list workspace: inline construction and a stable shopping companion,
//! backed by the local document and account synchronization.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::global_state::xiv_data::tracked_data;

use crate::components::icon::Icon;
use crate::global_state::LocalWorldData;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::{
    ActiveListing,
    list::{ListCapabilities, ListItem, ListPermission, ListWithPermission},
    result::ApiError,
};

use crate::api::{get_list_activity, get_list_items_with_listings};
use crate::components::{
    item_icon::*,
    list::{
        auto_mark_purchases::AutoMarkPurchases,
        filter_row::{ListFilterRow, SortSpec, worlds_in_listings},
        list_settings_drawer::ListSettingsDrawer,
        list_summary::*,
    },
    list_subscribe_drawer::ListSubscribeDrawer,
    loading::*,
    make_place_importer::*,
    meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle},
    modal::Modal,
    realtime_status::RealtimeStatus,
    skeleton::TableSkeleton,
    tooltip::*,
};
use crate::error::AppError;
use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::i18n::*;
use crate::list_doc::adapter::Edit;
use crate::list_doc::handle::ListDocHandle;
use crate::query_defaults::filter_query_signal;
use crate::routes::list_view::{
    ActivityFeed, IdList, ListView, ListViewResult, MenuState, NameList, filter_excluded,
    list_item_table_skeleton_columns, remaining_quantity, sort_list_items,
};
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use ultros_api_types::websocket::{
    EventType as WEvent, FilterPredicate, ListEventData, ServerClient, SocketMessageType,
    is_list_market_update_relevant,
};
use xiv_gen::ItemId;

type CatalogSearchEntry<T> = (i32, String, i32, T);
type CatalogSearchIndex<T> = Memo<std::sync::Arc<Vec<CatalogSearchEntry<T>>>>;

/// Search the local catalog without blocking typing or requiring a network.
fn inline_catalog_search<T>(
    query: RwSignal<String>,
    index: CatalogSearchIndex<T>,
    limit: usize,
) -> Memo<Vec<T>>
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    let generation = RwSignal::new(0u64);
    let completed = RwSignal::new((String::new(), Vec::<T>::new()));
    Effect::new(move |_| {
        let query = query.get().trim().to_lowercase();
        let index = index.get();
        generation.update(|value| *value = value.wrapping_add(1));
        let current = generation.get_untracked();
        completed.set((String::new(), Vec::new()));
        if query.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            let mut matches = Vec::new();
            for chunk in index.chunks(512) {
                gloo_timers::future::TimeoutFuture::new(0).await;
                if generation.try_get_untracked() != Some(current) {
                    return;
                }
                matches.extend(chunk.iter().filter(|entry| entry.1.contains(&query)));
            }
            // Exact names lead; level, name and stable ID break ties. Only sort
            // the visible prefix, even when a short query matches many items.
            let compare = |a: &&CatalogSearchEntry<T>, b: &&CatalogSearchEntry<T>| {
                (a.1 != query, Reverse(a.2), &a.1, a.0).cmp(&(
                    b.1 != query,
                    Reverse(b.2),
                    &b.1,
                    b.0,
                ))
            };
            if matches.len() > limit {
                matches.select_nth_unstable_by(limit, compare);
                matches.truncate(limit);
            }
            matches.sort_unstable_by(compare);
            if generation.try_get_untracked() == Some(current) {
                completed.try_set((
                    query,
                    matches.into_iter().map(|entry| entry.3.clone()).collect(),
                ));
            }
        });
    });
    Memo::new(move |_| {
        let current = query.get().trim().to_lowercase();
        completed.with(|(searched, results)| {
            if *searched == current {
                results.clone()
            } else {
                Vec::new()
            }
        })
    })
}

/// Inline catalog composer shared by account and device lists.
#[component]
pub fn InlineListAdd(
    list_id: Signal<i32>,
    on_add: Callback<ListItem>,
    #[prop(default = Signal::derive(|| false))] pending: Signal<bool>,
    #[prop(default = Signal::derive(String::new))] feedback: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let search = RwSignal::new(String::new());
    let quantity = RwSignal::new("1".to_string());
    let quality = RwSignal::new("any".to_string());
    let input = NodeRef::<leptos::html::Input>::new();
    let index = Memo::new(move |_| {
        std::sync::Arc::new(
            tracked_data()
                .items
                .iter()
                .filter(|(_, item)| item.item_search_category > 0)
                .map(|(id, item)| {
                    (
                        id.0,
                        item.name.to_lowercase(),
                        item.level_item,
                        (id.0, item.name.to_string(), item.can_be_hq, item.level_item),
                    )
                })
                .collect::<Vec<_>>(),
        )
    });
    let results = inline_catalog_search(search, index, 12);
    let add = Callback::new(move |(id, can_hq): (i32, bool)| {
        if pending.get_untracked() {
            return;
        }
        let Ok(count) = quantity.get_untracked().parse::<i32>() else {
            return;
        };
        if count < 1 {
            return;
        }
        let hq = match quality.get_untracked().as_str() {
            "hq" if can_hq => Some(true),
            "nq" => Some(false),
            _ => None,
        };
        on_add.run(ListItem {
            item_id: id,
            list_id: list_id.get_untracked(),
            quantity: Some(count),
            hq,
            ..Default::default()
        });
        if let Some(input) = input.get() {
            let _ = input.focus();
            input.select();
        }
    });
    view! {
        <section class="panel rounded-xl p-4 sm:p-5" aria-label=t_string!(i18n, lists_workspace_add_items_label) data-testid="inline-list-add">
            <div class="mb-3"><h2 class="font-semibold">{t!(i18n, lists_workspace_build_title)}</h2><p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_build_hint)}</p></div>
            <div class="flex flex-wrap gap-2">
                <input node_ref=input class="input flex-1 min-w-48" placeholder=t_string!(i18n, lists_workspace_add_placeholder) aria-label=t_string!(i18n, lists_workspace_add_item) prop:value=search data-committed=""
                    on:input=move |ev| search.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Escape" { search.set(String::new()); ev.stop_propagation(); }
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            if let Some((id, _, can_hq, _)) = results.get_untracked().first() { add.run((*id, *can_hq)); }
                        }
                    } />
                <input type="number" min="1" max=i32::MAX class="input w-24" aria-label=t_string!(i18n, lists_workspace_add_quantity) prop:value=quantity attr:data-committed=move || quantity.get() on:input=move |ev| quantity.set(event_target_value(&ev)) />
                <select class="input" aria-label=t_string!(i18n, lists_workspace_add_quality) prop:value=quality on:change=move |ev| quality.set(event_target_value(&ev))><option value="any">{t!(i18n, lists_workspace_any_quality)}</option><option value="nq">{t!(i18n, lists_workspace_nq)}</option><option value="hq">{t!(i18n, lists_workspace_hq_available)}</option></select>
            </div>
            <p class="text-sm mt-2 text-[color:var(--color-text-muted)]" role="status">{feedback}</p>
            <Show when=move || !search.get().trim().is_empty()>
                <div class="mt-3 max-h-80 overflow-y-auto divide-y divide-[color:var(--color-outline)]" aria-label=t_string!(i18n, lists_workspace_catalog_results)>
                    <Show when=move || results.get().is_empty()><p class="p-3 text-sm">{t!(i18n, lists_workspace_no_items)}</p></Show>
                    <For each=move || results.get() key=|item| item.0 children=move |(id, name, can_hq, _)| view! {
                        <div class="flex items-center gap-3 py-2"><ItemIcon item_id=id icon_size=IconSize::Small /><span class="flex-1 min-w-0">{name.clone()}</span>
                            <button class="btn-primary" aria-label=t_string!(i18n, lists_workspace_add_named, name = name.clone()) disabled={move || pending.get() || quantity.get().parse::<i32>().map_or(true, |q| q < 1)} on:click=move |_| add.run((id, can_hq))>{t!(i18n, lists_workspace_add)}</button>
                        </div>
                    } />
                </div>
            </Show>
        </section>
    }
}

/// Recipe preview uses the same local add callback as ordinary catalog rows.
fn recipe_preview_ingredients(recipe: &xiv_gen::Recipe) -> impl Iterator<Item = (ItemId, i32)> {
    // Unused crystal slots can use item -1 with amount 0. The shared
    // iterator skips item 0, but deliberately preserves other sheet values.
    // Only positive quantities belong in a shopping preview; retain a
    // genuinely missing positive-quantity item so validation still fails.
    crate::components::crafting_cost::IngredientsIter::new(recipe).filter(|(_, amount)| *amount > 0)
}

#[component]
pub fn InlineRecipeAdd(list_id: Signal<i32>, on_add: Callback<Vec<ListItem>>) -> impl IntoView {
    let i18n = use_i18n();
    use crate::components::crafting_cost::CRYSTAL_SEARCH_CATEGORY;
    let query = RwSignal::new(String::new());
    let selected = RwSignal::new(None::<&'static xiv_gen::Recipe>);
    let crafts = RwSignal::new("1".to_string());
    let ingredients = RwSignal::new(true);
    let crystals = RwSignal::new(true);
    let quality = RwSignal::new("any".to_string());
    let index = Memo::new(move |_| {
        let data = tracked_data();
        std::sync::Arc::new(
            data.recipes
                .iter()
                .filter_map(|(id, recipe)| {
                    let item = data.items.get(&ItemId(recipe.item_result))?;
                    Some((
                        id.0,
                        item.name.to_lowercase(),
                        item.level_item,
                        (id.0, item.name.to_string(), recipe),
                    ))
                })
                .collect::<Vec<_>>(),
        )
    });
    let results = inline_catalog_search(query, index, 20);
    let preview = Memo::new(move |_| {
        let recipe = selected.get()?;
        let crafts = crafts
            .get()
            .parse::<i32>()
            .ok()
            .filter(|count| (1..=9999).contains(count))?;
        let data = tracked_data();
        let make_item = |item_id, count: i32| {
            let item = data.items.get(&ItemId(item_id))?;
            let count = count.checked_mul(crafts)?;
            let hq = match quality.get().as_str() {
                "hq" if item.can_be_hq => Some(true),
                "nq" => Some(false),
                _ => None,
            };
            Some(ListItem {
                list_id: list_id.get(),
                item_id,
                quantity: Some(count),
                hq,
                ..Default::default()
            })
        };
        if ingredients.get() {
            recipe_preview_ingredients(recipe)
                .filter(|(id, _)| {
                    crystals.get()
                        || data
                            .items
                            .get(id)
                            .is_none_or(|item| item.item_search_category != CRYSTAL_SEARCH_CATEGORY)
                })
                .map(|(id, count)| make_item(id.0, count))
                .collect::<Option<Vec<_>>>()
        } else {
            Some(vec![make_item(recipe.item_result, recipe.amount_result)?])
        }
    });
    view! {
        <section class="panel rounded-xl p-4 space-y-3" data-testid="inline-recipe-add" aria-label=t_string!(i18n, lists_workspace_add_recipe)>
            <h2 class="font-semibold">{t!(i18n, lists_workspace_add_recipe)}</h2>
            <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_recipe_hint)}</p>
            <input class="input w-full" aria-label=t_string!(i18n, lists_workspace_search_recipes) placeholder=t_string!(i18n, lists_workspace_recipe_placeholder) prop:value=query on:input=move |ev| query.set(event_target_value(&ev)) />
            <div class="max-h-48 overflow-y-auto flex flex-col gap-1">
                <For each=move || results.get() key=|(id, _, _)| *id children=move |(_, name, recipe)| view! {
                    <button class="btn-secondary justify-start" on:click=move |_| selected.set(Some(recipe))>{name}</button>
                } />
                <Show when=move || !query.get().trim().is_empty() && results.get().is_empty()><p>{t!(i18n, lists_workspace_no_recipes)}</p></Show>
            </div>
            <Show when=move || selected.get().is_some()>
                <h3 class="font-semibold">{move || selected.get().and_then(|recipe| tracked_data().items.get(&ItemId(recipe.item_result))).map(|item| item.name.to_string())}</h3>
                <div class="flex flex-wrap gap-3 items-center">
                    <label>{t!(i18n, lists_workspace_crafts)}<input type="number" min="1" max="9999" class="input w-24" aria-label=t_string!(i18n, lists_workspace_recipe_crafts) prop:value=crafts on:input=move |ev| crafts.set(event_target_value(&ev)) /></label>
                    <select class="input" aria-label=t_string!(i18n, lists_workspace_recipe_items) on:change=move |ev| ingredients.set(event_target_value(&ev) == "ingredients")><option value="ingredients">{t!(i18n, lists_workspace_ingredients)}</option><option value="finished">{t!(i18n, lists_workspace_finished)}</option></select>
                    <select class="input" aria-label=t_string!(i18n, lists_workspace_recipe_quality) on:change=move |ev| quality.set(event_target_value(&ev))><option value="any">{t!(i18n, lists_workspace_any_quality)}</option><option value="nq">{t!(i18n, lists_workspace_nq)}</option><option value="hq">{t!(i18n, lists_workspace_hq_available)}</option></select>
                    <Show when=move || ingredients.get()><label class="flex gap-2 items-center"><input type="checkbox" prop:checked=crystals on:change=move |ev| crystals.set(event_target_checked(&ev)) />{t!(i18n, lists_workspace_crystals)}</label></Show>
                </div>
                <ul class="space-y-2" aria-label=t_string!(i18n, lists_workspace_recipe_preview)>{move || preview.get().unwrap_or_default().into_iter().map(|item| {
                    let name = tracked_data().items.get(&ItemId(item.item_id)).map(|item| item.name.to_string()).unwrap_or_default();
                    view! { <li class="flex gap-3 items-center"><ItemIcon item_id=item.item_id icon_size=IconSize::Small /><span>{t_string!(i18n, lists_workspace_preview_row, quantity = item.quantity.unwrap_or(1), name = name, quality = if item.hq == Some(true) { format!(" {}", t_string!(i18n, lists_workspace_hq)) } else { String::new() })}</span></li> }
                }).collect_view()}</ul>
                <Show when=move || preview.get().is_none()>
                    <p role="status" class="text-sm text-red-200">{move || {
                        if !crafts.get().parse::<i32>().is_ok_and(|count| (1..=9999).contains(&count)) {
                            t_string!(i18n, lists_workspace_craft_count_error).to_string()
                        } else {
                            t_string!(i18n, lists_workspace_recipe_missing_error).to_string()
                        }
                    }}</p>
                </Show>
                <p class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_recipe_add_hint)}</p>
                <button class="btn-primary" disabled=move || preview.get().is_none_or(|items| items.is_empty()) on:click=move |_| { if let Some(items) = preview.get_untracked() { on_add.run(items); } }>{t!(i18n, lists_workspace_add_preview)}</button>
            </Show>
        </section>
    }
}

/// Commit-on-change grid: typing never writes or reorders the document.
#[component]
pub fn BuildListRow(
    item: Signal<ListItem>,
    #[prop(default = Signal::derive(|| None))] current_price: Signal<Option<i32>>,
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
    let display_name = tracked_data()
        .items
        .get(&ItemId(initial.item_id))
        .map(|i| i.name.to_string())
        .unwrap_or_else(|| {
            t_string!(i18n, lists_workspace_item_fallback, id = initial.item_id).to_string()
        });
    view! {
        <tr class="hover:bg-[color:var(--color-background-panel)] transition-colors" class:ring-2=highlighted class:ring-brand-400=highlighted data-item-id=initial.item_id>
            <td class="p-3"><input type="checkbox" aria-label=t_string!(i18n, lists_workspace_select_named, name = display_name.clone()) disabled=move || !can_write.get() prop:checked=move || selected_items.with(|s| s.contains(&id)) on:change=move |_| selected_items.update(|s| { if !s.remove(&id) {s.insert(id);} }) /></td>
            <td class="p-3"><div class="flex items-center gap-3"><ItemIcon item_id=initial.item_id icon_size=IconSize::Small /><span class="font-semibold">{display_name}</span></div></td>
            <td class="p-3"><select class="input min-w-24" aria-label=t_string!(i18n, lists_workspace_item_quality) disabled=move || !can_write.get() prop:value=move || match row.get().hq {Some(true) => "hq", Some(false) => "nq", None => "any"} on:change=move |ev| {let mut updated = row.get_untracked(); updated.hq = match event_target_value(&ev).as_str() {"hq" if can_hq => Some(true), "nq" => Some(false), _ => None}; on_edit.run(updated);}><option value="any">{t!(i18n, lists_workspace_any)}</option><option value="nq">{t!(i18n, lists_workspace_nq)}</option><option value="hq" disabled=!can_hq>{t!(i18n, lists_workspace_hq)}</option></select></td>
            <td class="p-3">{needed}</td><td class="p-3">{owned}</td><td class="p-3 tabular-nums">{move || current_price.get().map(|price| t_string!(i18n, lists_workspace_gil, price = price).to_string()).unwrap_or_else(|| "—".to_string())}</td><td class="p-3">{target}</td>
            <td class="p-3"><button class="btn-ghost" disabled=move || !can_write.get() on:click=move |_| on_delete.run(id)>{t!(i18n, lists_workspace_remove)}</button></td>
        </tr>
    }
}

#[component]
pub fn ListWorkspaceModes(shop: Signal<bool>, set_shop: Callback<bool>) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="inline-flex w-fit gap-1 rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background)] p-1" role="group" aria-label=t_string!(i18n, lists_workspace_mode)>
            <button class=move || if !shop.get() { "btn-primary min-w-20 justify-center font-semibold shadow-sm" } else { "btn-ghost min-w-20 justify-center text-[color:var(--color-text-muted)]" } aria-pressed=move || (!shop.get()).to_string() on:click=move |_| set_shop.run(false)>{t!(i18n, lists_workspace_build)}</button>
            <button class=move || if shop.get() { "btn-primary min-w-20 justify-center font-semibold shadow-sm" } else { "btn-ghost min-w-20 justify-center text-[color:var(--color-text-muted)]" } data-testid="guest-shop-mode" aria-pressed=move || shop.get().to_string() on:click=move |_| set_shop.run(true)>{t!(i18n, lists_workspace_shop)}</button>
        </div>
    }
}

/// Reactive presentation contract shared by account and device documents. Transport,
/// authorization and storage lifecycles stay with the route that owns the source.
#[derive(Clone, Copy)]
pub struct ListWorkspaceSource {
    pub list_id: Signal<i32>,
    pub add: Callback<ListItem>,
    pub add_many: Callback<Vec<ListItem>>,
    pub undo: Callback<()>,
    pub redo: Callback<()>,
    /// Reactive availability (#1430): the toolbar disables and explains an
    /// action with nothing to do, and the callbacks report the same through
    /// `feedback` when a shortcut hits an empty stack.
    pub can_undo: Signal<bool>,
    pub can_redo: Signal<bool>,
    pub pending: Signal<bool>,
    pub feedback: Signal<String>,
    pub recipe_open: Signal<bool>,
    pub toggle_recipe: Callback<()>,
    pub rows: Signal<Vec<(ListItem, Vec<ActiveListing>)>>,
    pub hide_acquired: Signal<bool>,
    pub can_write: Signal<bool>,
    pub edit: Callback<ListItem>,
    pub remove: Callback<i32>,
}

/// The grid is mounted once, independently of resource revisions. Row identity is
/// the document row key; each cell reads its current value from its own memo.
#[component]
pub fn ListBuildWorkspace(
    source: ListWorkspaceSource,
    selected_items: RwSignal<HashSet<i32>>,
    #[prop(default = Signal::derive(HashSet::new))] highlighted: Signal<HashSet<i32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let filter = RwSignal::new(String::new());
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
        <section class="space-y-3" data-testid="list-build-workspace">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=source.add pending=source.pending feedback=source.feedback />
                <div class="flex gap-2 flex-wrap">
                    <button class="btn-secondary" on:click=move |_| source.toggle_recipe.run(())>{t!(i18n, lists_workspace_add_recipe)}</button>
                    <button class="btn-secondary disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-undo" disabled=move || !source.can_undo.get() title=move || (!source.can_undo.get()).then(|| t_string!(i18n, lists_workspace_nothing_to_undo).to_string()) on:click=move |_| source.undo.run(())>{t!(i18n, lists_workspace_undo)}</button>
                    <button class="btn-secondary disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-redo" disabled=move || !source.can_redo.get() title=move || (!source.can_redo.get()).then(|| t_string!(i18n, lists_workspace_nothing_to_redo).to_string()) on:click=move |_| source.redo.run(())>{t!(i18n, lists_workspace_redo)}</button>
                </div>
                <Show when=move || source.recipe_open.get()><InlineRecipeAdd list_id=source.list_id on_add=source.add_many /></Show>
            </Show>
            <input class="input w-full" aria-label=t_string!(i18n, lists_workspace_filter_label) placeholder=t_string!(i18n, lists_workspace_filter_placeholder) prop:value=move || filter.get() data-committed="" on:input=move |ev| filter.set(event_target_value(&ev)) />
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
                        let price = Memo::new(move |_| source.rows.with(|rows| rows.iter().find(|(item, _)| item.id == id).and_then(|(item, listings)| listings.iter().filter(|listing| item.hq.is_none_or(|hq| listing.hq == hq)).map(|listing| listing.price_per_unit).min())));
                        view! { <BuildListRow item=item.into() current_price=price.into() selected_items on_edit=source.edit on_delete=source.remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
                    } />
                </tbody></table>
            </div>
        </section>
    }
}

/// Prices for the rows, fetched once per list and again only when the market
/// subscription or an import says so. The document — not this cache —
/// supplies the rows themselves, so a local edit never refetches prices.
#[derive(Clone)]
struct ListingsCache {
    list_id: i32,
    version: u32,
    /// The value of the page's `revalidate_version` this entry was fetched
    /// at. A revalidation (Global Constraint 2) must reach the server: if
    /// the cache could satisfy it, an unshared or deleted list would keep
    /// rendering from prices fetched while the client still had access, and
    /// the 403/404 that `is_denial` acts on would never arrive.
    revalidate: u32,
    list: ListWithPermission,
    listings: HashMap<i32, Vec<ActiveListing>>,
    /// The item ids the fetch that filled this cache covered — every row the
    /// *server* had, including rows whose price lookup came back empty. A
    /// row added locally is not in here until the socket has delivered the
    /// add and a later fetch sees it, so `load_view` treats "the document
    /// has an item this set doesn't" as a miss and refetches once. Caching
    /// the covered set (rather than the keys of `listings`) is what stops
    /// that from looping when the server genuinely has no price for the id.
    covered: HashSet<i32>,
}

/// The ids a fetch covered, built from the rows the server returned.
fn covered_ids(items: &[(ListItem, Vec<ActiveListing>)]) -> HashSet<i32> {
    items.iter().map(|(item, _)| item.item_id).collect()
}

/// How long a burst of relayed list broadcasts is allowed to coalesce into
/// a single revalidation. Every row edit on a shared list comes back to us
/// as a `ListItem` broadcast, and the Labs page already has those rows from
/// its document — the revalidation exists only so a *permission* change
/// (Global Constraint 2) is noticed by an idle page, so paying one REST
/// fetch per remote keystroke would be pure waste.
#[cfg(feature = "hydrate")]
const REVALIDATE_DEBOUNCE_MS: u32 = 1000;

/// The trailing-debounce timer. A real `Timeout` on the client; a unit
/// placeholder on the SSR half, where `Effect`s never run and so no
/// subscription is ever created to schedule one.
#[cfg(feature = "hydrate")]
type RevalidateTimer = gloo_timers::callback::Timeout;
#[cfg(not(feature = "hydrate"))]
type RevalidateTimer = ();

/// Ask for a revalidation `REVALIDATE_DEBOUNCE_MS` from now, replacing any
/// request already pending. Dropping the previous `Timeout` cancels it, so
/// N broadcasts inside the window cost exactly one probe, fired after the
/// last of them.
#[cfg(feature = "hydrate")]
fn schedule_revalidate(slot: &Rc<RefCell<Option<RevalidateTimer>>>, probe: impl Fn() + 'static) {
    let timer = gloo_timers::callback::Timeout::new(REVALIDATE_DEBOUNCE_MS, probe);
    *slot.borrow_mut() = Some(timer);
}

#[cfg(not(feature = "hydrate"))]
fn schedule_revalidate(_slot: &Rc<RefCell<Option<RevalidateTimer>>>, _probe: impl Fn() + 'static) {}

/// The revalidation itself: a silent permission probe. It re-fetches the
/// list over REST and touches the page only when the answer changes what
/// the page may do — a denial purges the local copy and re-runs the
/// resource so the error state renders; a changed permission re-runs it so
/// the write controls follow; anything else refreshes the listings cache
/// in place and leaves the rendered page alone, so a collaborator's edit
/// never re-renders an open drawer or modal under the user's cursor.
#[cfg(feature = "hydrate")]
fn revalidate(
    active_list: Memo<i32>,
    list_id: i32,
    handle: RwSignal<Option<ListDocHandle>>,
    cache: StoredValue<Option<ListingsCache>>,
    bump: WriteSignal<u32>,
) {
    let expected = handle.try_get_untracked().flatten().map(|h| h.revision);
    if !request_is_current(active_list, list_id, handle, expected) {
        return;
    }
    leptos::task::spawn_local(async move {
        let result = get_list_items_with_listings(list_id).await;
        if !request_is_current(active_list, list_id, handle, expected) {
            return;
        }
        match result {
            Ok((list, items)) => {
                let permission = list.permission;
                if let Some(doc_handle) = handle.get_untracked() {
                    doc_handle.remember_permission(permission as i16);
                }
                let changed = cache.with_value(|cached| {
                    cached
                        .as_ref()
                        .filter(|c| c.list_id == list_id)
                        .is_none_or(|c| c.list.permission != permission)
                });
                let covered = covered_ids(&items);
                cache.update_value(|cached| {
                    if let Some(c) = cached.as_mut().filter(|c| c.list_id == list_id) {
                        c.list = list;
                        c.listings = items
                            .into_iter()
                            .map(|(item, listings)| (item.item_id, listings))
                            .collect();
                        c.covered.extend(covered);
                    }
                });
                if changed {
                    bump.update(|v| *v += 1);
                }
            }
            Err(error) if is_denial(&error) => {
                if let Some(doc_handle) = handle.get_untracked() {
                    doc_handle.purge();
                }
                cache.set_value(None);
                handle.set(None);
                bump.update(|v| *v += 1);
            }
            // Transport or server trouble says nothing about permission;
            // the next broadcast tries again.
            Err(_) => {}
        }
    });
}

#[cfg(not(feature = "hydrate"))]
fn revalidate(
    _active_list: Memo<i32>,
    _list_id: i32,
    _handle: RwSignal<Option<ListDocHandle>>,
    _cache: StoredValue<Option<ListingsCache>>,
    _bump: WriteSignal<u32>,
) {
}

/// A failure that means the browser must stop keeping a local copy of this
/// list (Global Constraint 2): the server says the list is gone, or that
/// this session may not read it. Everything else — a dropped connection, a
/// 5xx, a body that would not parse — is transient, and the cached listings
/// or the offline document carry the page through it.
///
/// [`AppError::BadList`] is included because it is the client-side stand-in
/// for "there is no such list" (`api::get_list_items_with_listings` returns
/// it for a list id of `0`).
///
/// [`ApiError::NotAuthenticated`] is deliberately **not** a denial: a lapsed
/// session says nothing about whether this user still owns the list, and
/// destroying the snapshot would lose edits the user made offline and never
/// got to sync. It is handled exactly like signing out — close the handle,
/// keep the local copy — so signing back in resumes where they left off.
fn is_denial(error: &AppError) -> bool {
    matches!(
        error,
        AppError::BadList | AppError::ApiError(ApiError::Forbidden | ApiError::NotFound)
    )
}

/// Every write goes through the document; there is no REST fallback. A
/// signed-in visitor has a handle within the first client tick, and an
/// anonymous one cannot write to a list at all.
fn apply_edit(handle: RwSignal<Option<ListDocHandle>>, edit: Edit) -> Result<(), AppError> {
    match handle.get_untracked() {
        // A closed handle is detached from its document's subscriptions, so
        // applying an edit through it would change nothing anyone can see.
        // Say so rather than reporting a success that never happened; the
        // page normally clears `handle` alongside every `close`, so this is
        // the last line of defence, not the usual path.
        Some(handle) if handle.is_closed_or_disposed() => {
            Err(AppError::ListDoc("document is closed".to_string()))
        }
        Some(handle) => handle
            .apply(edit)
            .map_err(|error| AppError::ListDoc(error.to_string())),
        None => Err(AppError::ListDoc("document is not open yet".to_string())),
    }
}

/// Translate local lifecycle failures at the UI boundary, keeping the internal
/// error values stable for transport and retry decisions.
fn workspace_error(i18n: leptos_i18n::I18nContext<Locale, I18nKeys>, error: &AppError) -> String {
    match error {
        AppError::ListDoc(message) if message == "document is closed" => {
            t_string!(i18n, lists_workspace_document_closed).to_string()
        }
        AppError::ListDoc(message) if message == "document is not open yet" => {
            t_string!(i18n, lists_workspace_document_not_open).to_string()
        }
        AppError::ListDoc(message) if message == "document is no longer active" => {
            t_string!(i18n, lists_workspace_document_inactive).to_string()
        }
        _ => error.to_string(),
    }
}

/// The page's rows. With no document open — the SSR render, the first client
/// paint, an anonymous visitor — this is exactly the legacy REST read. With
/// one open, the listings come from the (cached) endpoint and the rows from
/// the document, so an offline edit renders immediately and a server that is
/// merely unreachable still leaves a usable page.
// A Resource does not cancel an in-flight fetch when its source changes.
// Guard side effects as well as the rendered result: the old handle may have
// been disposed, or a successor list may now own the shared signals.
fn request_is_current(
    list_id: Memo<i32>,
    id: i32,
    handle: RwSignal<Option<ListDocHandle>>,
    expected: Option<RwSignal<u64>>,
) -> bool {
    list_id.try_get_untracked() == Some(id)
        && handle.try_get_untracked().is_some_and(|current| {
            current.map(|h| h.revision) == expected
                && current.is_none_or(|h| !h.is_closed_or_disposed())
        })
}

async fn load_view(
    list_id: Memo<i32>,
    id: i32,
    handle: RwSignal<Option<ListDocHandle>>,
    cache: StoredValue<Option<ListingsCache>>,
    listings_version: u32,
    revalidate_version: u32,
) -> ListViewResult {
    let current = handle.try_get_untracked().flatten();
    let expected = current.map(|h| h.revision);
    let stale = || AppError::ListDoc("document is no longer active".to_string());
    if !request_is_current(list_id, id, handle, expected) {
        return Err(stale());
    }
    let Some(doc_handle) = current else {
        // No document yet (SSR, the first client paint, an anonymous
        // visitor). Cache what the REST read already paid for, so the first
        // handle-backed run below is a cache hit rather than a second fetch
        // of the same prices.
        let result = get_list_items_with_listings(id).await;
        if !request_is_current(list_id, id, handle, expected) {
            return Err(stale());
        }
        if let Ok((list, items)) = &result {
            cache.set_value(Some(ListingsCache {
                list_id: id,
                version: listings_version,
                revalidate: revalidate_version,
                list: list.clone(),
                listings: items
                    .iter()
                    .map(|(item, listings)| (item.item_id, listings.clone()))
                    .collect(),
                covered: covered_ids(items),
            }));
        }
        return result;
    };
    // A row added locally is in the document but in no cache entry, so it
    // would render priceless until an unrelated market event bumped
    // `listings_version`. Missing coverage is a miss.
    let wanted_ids: HashSet<i32> = doc_handle
        .rows()
        .into_iter()
        .map(|row| row.key.item_id)
        .collect();
    let cached = cache.get_value().filter(|c| {
        c.list_id == id
            && c.version == listings_version
            && c.revalidate == revalidate_version
            && wanted_ids.is_subset(&c.covered)
    });
    let base = match cached {
        Some(cached) => cached,
        None => {
            let result = get_list_items_with_listings(id).await;
            if !request_is_current(list_id, id, handle, expected) {
                return Err(stale());
            }
            match result {
                Ok((list, items)) => {
                    doc_handle.remember_permission(list.permission as i16);
                    let covered = covered_ids(&items);
                    let fresh = ListingsCache {
                        list_id: id,
                        version: listings_version,
                        revalidate: revalidate_version,
                        list,
                        listings: items
                            .into_iter()
                            .map(|(item, listings)| (item.item_id, listings))
                            .collect(),
                        // Whatever the server had this time. If it hasn't seen
                        // the new row yet, the id stays uncovered — but the
                        // cache is only consulted again when the id *set*
                        // changes, so this refetches once, not in a loop.
                        covered: covered.union(&wanted_ids).copied().collect(),
                    };
                    cache.set_value(Some(fresh.clone()));
                    fresh
                }
                // Forbidden or deleted: the local copy must not outlive the
                // server's answer. Clearing the handle also drops the sync
                // subscription (the Effect that owns it reads this signal).
                Err(error) if is_denial(&error) => {
                    doc_handle.purge();
                    cache.set_value(None);
                    handle.set(None);
                    return Err(error);
                }
                Err(error) => match cache.get_value().filter(|c| c.list_id == id) {
                    // Stale prices beat no page.
                    Some(stale) => stale,
                    None => match offline_list(id, doc_handle) {
                        Some(list) => ListingsCache {
                            list_id: id,
                            version: listings_version,
                            revalidate: revalidate_version,
                            list,
                            listings: HashMap::new(),
                            covered: wanted_ids.clone(),
                        },
                        None => return Err(error),
                    },
                },
            }
        }
    };
    // `ListDocument` clones share the underlying document, so this hands
    // `view_result` a reference without holding the stored value across it.
    let Some(doc) = doc_handle.with_doc(|doc| doc.clone()) else {
        // The page was torn down while this fetch was in flight; there is no
        // document left to render and nothing to render it into.
        return Err(AppError::ListDoc("document is closed".to_string()));
    };
    Ok(crate::list_doc::adapter::view_result(
        &base.list,
        &doc,
        &base.listings,
    ))
}

/// The list as far as the browser knows it with no server at all: the cached
/// permission and the document's own name and scope. `None` when the cached
/// permission says this user never had access, so an empty document can't
/// masquerade as a readable list.
fn offline_list(id: i32, handle: ListDocHandle) -> Option<ListWithPermission> {
    let meta = handle.meta();
    let permission = ListPermission::from(handle.permission.get_untracked());
    if permission == ListPermission::None {
        return None;
    }
    Some(ListWithPermission {
        list: ultros_api_types::list::List {
            id,
            owner: 0,
            name: meta.name,
            wdr_filter: meta.scope?,
        },
        permission,
        owner_name: None,
    })
}

#[component]
pub fn ListViewSync() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let list_id = Memo::new(move |_| {
        params
            .with(|p| p.get("id").as_ref().and_then(|id| id.parse::<i32>().ok()))
            .unwrap_or_default()
    });
    // ---- Local-first document (spec sections 3.1, 3.2) ----
    //
    // The handle keeps its document in `StoredValue::new_local`, which must
    // never be built on the SSR half (repo issue #1332), and its browser
    // storage is scoped to the signed-in user — so it can only open on the
    // client, once `get_login` has resolved. Until then, and forever for an
    // anonymous visitor, this stays `None` and the page is the same
    // read-only REST render the non-Labs page produces.
    let handle: RwSignal<Option<ListDocHandle>> = RwSignal::new(None);
    provide_context(handle);

    let add_item = Action::new(move |list_item: &ListItem| {
        let edit = Edit::Add(list_item.clone());
        async move { apply_edit(handle, edit) }
    });
    let mutation_feedback = RwSignal::new(String::new());
    // Shared by the toolbar buttons and the keyboard bindings (#1430): a
    // shortcut that finds nothing to do says so in the workspace feedback
    // line instead of appearing broken. The add action's own feedback is
    // cleared so the explanation is actually visible.
    let undo_action = Callback::new(move |()| {
        let done = handle.get_untracked().is_some_and(|handle| handle.undo());
        if !done {
            add_item.value().set(None);
            mutation_feedback.set(t_string!(i18n, lists_workspace_nothing_to_undo).to_string());
        }
    });
    let redo_action = Callback::new(move |()| {
        let done = handle.get_untracked().is_some_and(|handle| handle.redo());
        if !done {
            add_item.value().set(None);
            mutation_feedback.set(t_string!(i18n, lists_workspace_nothing_to_redo).to_string());
        }
    });
    let delete_item = Action::new(move |list_item: &i32| {
        let edit = Edit::Remove(*list_item);
        async move {
            let result = apply_edit(handle, edit);
            mutation_feedback.set(match &result {
                Ok(()) => t_string!(i18n, lists_workspace_removed).to_string(),
                Err(error) => workspace_error(i18n, error),
            });
            result
        }
    });

    let edit_item = Action::new(move |item: &ListItem| {
        // `Edit::Edit` finds the row by the id it was rendered with, so the
        // item arrives exactly as `ListItemRow` hands it back — original id
        // included, even when the edit changes its quality.
        let edit = Edit::Edit(item.clone());
        async move { apply_edit(handle, edit) }
    });
    let delete_items = Action::new(move |items: &Vec<i32>| {
        let edit = Edit::RemoveMany(items.clone());
        async move {
            let result = apply_edit(handle, edit);
            mutation_feedback.set(match &result {
                Ok(()) => t_string!(i18n, lists_workspace_removed_many).to_string(),
                Err(error) => workspace_error(i18n, error),
            });
            result
        }
    });
    let edit_items_hq = Action::new(move |(items, hq): &(Vec<i32>, Option<bool>)| {
        let items = items
            .iter()
            .copied()
            .filter(|id| {
                *hq != Some(true)
                    || crate::list_doc::adapter::key_of(*id)
                        .and_then(|key| tracked_data().items.get(&ItemId(key.item_id)))
                        .is_some_and(|item| item.can_be_hq)
            })
            .collect();
        let edit = Edit::SetQuality(items, *hq);
        async move { apply_edit(handle, edit) }
    });
    let edit_list_action = Action::new(move |list: &ultros_api_types::list::List| {
        let edit = Edit::Rename {
            name: list.name.clone(),
            scope: list.wdr_filter,
        };
        async move { apply_edit(handle, edit) }
    });

    let bulk_pending =
        Signal::derive(move || delete_items.pending().get() || edit_items_hq.pending().get());
    let bulk_error = Signal::derive(move || {
        delete_items
            .value()
            .get()
            .and_then(|result| result.err().map(|e| workspace_error(i18n, &e)))
            .or_else(|| {
                edit_items_hq
                    .value()
                    .get()
                    .and_then(|result| result.err().map(|e| workspace_error(i18n, &e)))
            })
    });

    // Listings come from the existing endpoint and are cached per list; the
    // document supplies rows, so a local edit never refetches prices.
    let (activity_update_version, set_activity_update_version) = signal(0);
    // Global Constraint 2: bumped (on a debounce) by ANY list broadcast for
    // this list, and part of the `list_view` resource's key, so an idle page
    // re-asks the server whether it may still read this list. An unshare or
    // a delete both reach a still-subscribed client as a list broadcast
    // (`ultros/src/web.rs`: `unshare_list_from_user` -> `record_list_activity`
    // + `broadcast_list_update`; `delete_list` -> `EventType::removed`), and
    // the refetch's 403/404 is what `is_denial` turns into a purge.
    let (revalidate_version, set_revalidate_version) = signal(0u32);
    let (listings_version, set_listings_version) = signal(0u32);
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
    let listings_cache: StoredValue<Option<ListingsCache>> = StoredValue::new(None);

    let list_view = Resource::new(
        move || {
            (
                list_id(),
                handle.get().map(|handle| handle.revision.get()),
                listings_version.get(),
                revalidate_version.get(),
            )
        },
        move |(id, _, listings_v, revalidate_v)| {
            load_view(
                list_id,
                id,
                handle,
                listings_cache,
                listings_v,
                revalidate_v,
            )
        },
    );
    let user_resource = Resource::new(|| {}, |_| async move { crate::api::get_login().await.ok() });
    let self_user_id = Signal::derive(move || user_resource.get().flatten().map(|u| u.id));

    let activity_view = Resource::new(
        move || (list_id(), activity_update_version.get()),
        move |(id, _)| get_list_activity(id),
    );

    let realtime = use_realtime();
    let socket_status = realtime.as_ref().map(|client| client.status);
    // The document reports its own sync state once it is open; before that
    // (and for a reader with no document) the socket's own state is the
    // honest answer, rather than a status stuck on "connecting" forever.
    let realtime_status = Signal::derive(move || match handle.get() {
        Some(handle) => handle.status.get(),
        None => socket_status
            .map(|status| status.get())
            .unwrap_or_else(|| "offline".to_string()),
    });
    let activity_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let list_market_subscription = StoredValue::new(None::<RealtimeSubscription>);

    // The legacy list subscription only drives the activity feed now; rows
    // arrive on the document's own subscription instead.
    let realtime_for_activity = realtime.clone();
    Effect::new(move |_| {
        activity_subscription.update_value(|sub| *sub = None);
        let id = list_id.get();
        let Some(realtime) = realtime_for_activity.clone() else {
            return;
        };
        if id != 0 {
            // Created inside the Effect body so the Effect's own closure
            // stays `Send + Sync` (it captures no `Rc`); the handler it is
            // moved into has no such bound.
            let revalidate_timer: Rc<RefCell<Option<RevalidateTimer>>> =
                Rc::new(RefCell::new(None));
            let sub = realtime.subscribe_list(id, move |message| {
                let ServerClient::ListUpdate(event) = message else {
                    return;
                };
                if matches!(event, WEvent::Added(ListEventData::Activity(_))) {
                    set_activity_update_version.update(|v| *v += 1);
                }
                // Any broadcast for this list — activity, the list row
                // itself, a row event — is a reason to re-ask the server
                // whether we may still read it. The page cannot tell a
                // revocation from a rename by the payload (an unshare
                // broadcasts an ordinary `List` update), so it revalidates
                // on all of them and lets the REST answer decide.
                schedule_revalidate(&revalidate_timer, move || {
                    revalidate(list_id, id, handle, listings_cache, set_revalidate_version)
                });
            });
            activity_subscription.set_value(Some(sub));
        }
    });
    let realtime_for_market = realtime.clone();
    Effect::new(move |_| {
        list_market_subscription.update_value(|sub| *sub = None);
        let Some(Ok((list, items))) = list_view.get() else {
            return;
        };
        let item_ids = items
            .iter()
            .map(|(item, _)| item.item_id)
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            return;
        }
        let Some(realtime) = realtime_for_market.clone() else {
            return;
        };
        let filter = FilterPredicate::World(list.list.wdr_filter)
            .and(FilterPredicate::Items(item_ids.clone()));
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            if is_list_market_update_relevant(&message, &item_ids) {
                set_last_update_at.set(Some(chrono::Utc::now()));
                set_listings_version.update(|v| *v += 1);
            }
        });
        list_market_subscription.set_value(Some(sub));
    });
    on_cleanup(move || {
        activity_subscription.update_value(|sub| *sub = None);
        list_market_subscription.update_value(|sub| *sub = None);
        if let Some(handle) = handle.get_untracked() {
            handle.close();
        }
    });

    let (menu, set_menu) = signal(MenuState::None);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);
    let (subscribe_open, set_subscribe_open) = signal(false);
    let (settings_open, set_settings_open) = signal(false);
    let (rename_open, set_rename_open) = signal(false);
    let (rename_value, set_rename_value) = signal(String::new());
    let (confirm_bulk_delete, set_confirm_bulk_delete) = signal(false);

    // ---- The document's lifecycle (spec sections 3.1, 3.3, 5) ----
    //
    // Both Effects below are hydrate-only: `ListDocHandle::open` builds
    // `StoredValue::new_local`s that must never exist on the SSR half
    // (#1332), and `SyncSubscription` owns `Rc`s, so it can only live in a
    // thread-local slot.
    let modal_open =
        Signal::derive(move || subscribe_open() || settings_open() || confirm_bulk_delete());
    let (resync, set_resync) = signal(0u32);
    #[cfg(feature = "hydrate")]
    {
        let sync_subscription: StoredValue<
            Option<crate::list_doc::sync::SyncSubscription>,
            LocalStorage,
        > = StoredValue::new_local(None);
        // The (user, list) pair the open handle belongs to, so a re-run that
        // changed neither doesn't throw the document away.
        let open_for: StoredValue<Option<(i64, i32)>> = StoredValue::new(None);
        // The page's owner. An Effect runs its body under a short-lived
        // child owner that is disposed on every re-run *and* on unmount —
        // before the page's own `on_cleanup`. `ListDocHandle::open` builds
        // `StoredValue`s and `RwSignal`s, so opening it inside the Effect
        // body would hand the handle nodes that are already gone by the time
        // anything closes it: reading them panics, and on wasm a panic is an
        // `unreachable` that kills the module. Opening under this owner
        // instead keeps the handle alive exactly as long as the page.
        let page_owner = Owner::current();
        // What the page *should* have open, diffed. The Effect below
        // registers a cleanup that closes the handle it opened, so it must
        // only re-run when the answer genuinely changed — a `Resource` that
        // notifies twice with the same login (hydration, a refetch) would
        // otherwise close a document the page is still using. `None` means
        // login hasn't resolved; `Some(None)` means signed out or no list.
        let doc_target = Memo::new(move |_| {
            let id = list_id.get();
            let login = user_resource.get()?;
            Some(match login {
                Some(user) if id != 0 => Some((user.id as i64, id)),
                _ => None,
            })
        });

        Effect::new(move |_| {
            let Some(target) = doc_target.get() else {
                // Login hasn't resolved yet: nothing to open or close.
                return;
            };
            match target {
                Some(wanted) => {
                    if open_for.get_value() == Some(wanted) {
                        return;
                    }
                    // An account switch (or a move to another list): flush and
                    // detach the old document before its successor claims
                    // the signal, then release its reactive nodes. The page
                    // owner outlives every list it shows, so without the
                    // `dispose` each superseded document, undo stack and
                    // signal set would sit in the arena until navigation
                    // away from the page. Every accessor on the handle is
                    // `try_*`-based, so the copies still held by a queued
                    // timer or socket callback keep behaving as closed.
                    if let Some(previous) = handle.get_untracked() {
                        previous.close();
                        handle.set(None);
                        previous.dispose();
                    }
                    let opened = match page_owner.clone() {
                        Some(owner) => owner.with(|| ListDocHandle::open(wanted.0, wanted.1)),
                        None => ListDocHandle::open(wanted.0, wanted.1),
                    };
                    open_for.set_value(Some(wanted));
                    handle.set(Some(opened));
                    // Flushed and detached here rather than only in the
                    // page's `on_cleanup`: an owner runs its own cleanups
                    // before it disposes anything, so this fires while the
                    // handle (owned by the page) is still readable, on every
                    // re-run and on unmount. The page-level cleanup calls
                    // `close` again; it is idempotent.
                    on_cleanup(move || opened.close());
                    // Re-installed alongside each handle so the keys always
                    // reach the live document. The bindings capture the
                    // handle by value, and one window listener per open is
                    // cheap: switching accounts without a page load isn't
                    // reachable (signing in navigates away and back), so in
                    // practice this runs exactly once per page. Left under
                    // the Effect's own owner on purpose: it creates no node
                    // the handle needs, and its listener is then removed
                    // when this run is superseded, instead of piling up.
                    crate::list_doc::undo::install(crate::list_doc::undo::UndoBindings {
                        undo: undo_action,
                        redo: redo_action,
                        modal_open,
                    });
                }
                _ => {
                    // Signed out, or no list id. Drop the document; the page
                    // falls back to the read-only REST render. `handle` is
                    // cleared unconditionally so no `close`d handle is ever
                    // left reachable through it. The nodes are NOT disposed:
                    // nothing replaces this document, and the page's own
                    // `on_cleanup` still closes whatever it finds here.
                    if let Some(previous) = handle.get_untracked() {
                        previous.close();
                    }
                    handle.set(None);
                    open_for.set_value(None);
                }
            }
        });

        let realtime_for_doc = realtime.clone();
        Effect::new(move |_| {
            resync.track();
            let open = handle.get();
            // Dropping the old subscription unsubscribes and disposes the
            // outbox drain. A denial that cleared `handle` lands here too,
            // which is what stops the socket traffic for a purged document.
            sync_subscription.set_value(None);
            let Some(open) = open else {
                return;
            };
            let Some(realtime) = realtime_for_doc.clone() else {
                open.set_status("offline");
                return;
            };
            let subscription = crate::list_doc::sync::start(
                open,
                realtime,
                move || set_resync.update(|n| *n += 1),
                move || set_last_update_at.set(Some(chrono::Utc::now())),
                move |kind| {
                    // Global Constraint 2: the server says this list is gone
                    // or not ours, so the local copy must not survive it.
                    // Clearing the handle re-runs this Effect, which drops
                    // the subscription. Safe from inside the callback: sync
                    // defers every callback past the socket's dispatch.
                    //
                    // `NotSignedIn` is the exception: a lapsed session says
                    // nothing about whether the user still has this list, so
                    // the snapshot is kept and only the sync stops — signing
                    // back in resumes where they left off. `classify_error`
                    // makes that distinction from the server's own wording,
                    // rather than the page guessing it from a login resource
                    // that never refetches (and so still reads "signed in"
                    // for exactly the cookie that just expired).
                    use crate::list_doc::sync::ErrorKind;
                    let purge = matches!(kind, ErrorKind::Denied | ErrorKind::NotFound);
                    let Some(denied) = handle.try_get_untracked().flatten() else {
                        return;
                    };
                    if denied.revision != open.revision || denied.is_closed_or_disposed() {
                        return;
                    }
                    if purge {
                        denied.purge();
                    } else {
                        denied.close();
                    }
                    handle.set(None);
                },
            );
            sync_subscription.set_value(Some(subscription));
        });

        on_cleanup(move || sync_subscription.set_value(None));
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (modal_open, resync, set_resync);
    }

    let selected_items = RwSignal::new(HashSet::new());

    // Shopping-view state lives in the URL so a shared link reproduces the
    // sender's exact view. Query params resolve on the server too, so SSR
    // renders the same view a hydrated client shows.
    let (buying_view_param, set_buying_view_param) = filter_query_signal::<bool>("buy");
    let buying_view = Memo::new(move |_| buying_view_param.get().unwrap_or(false));

    let (excluded_worlds_param, set_excluded_worlds_param) =
        filter_query_signal::<IdList>("excluded-worlds");
    let excluded_worlds = Memo::new(move |_| {
        excluded_worlds_param
            .get()
            .map(|list| list.0.into_iter().collect::<HashSet<i32>>())
            .unwrap_or_default()
    });
    let set_excluded_worlds = Callback::new(move |set: HashSet<i32>| {
        set_excluded_worlds_param.set((!set.is_empty()).then(|| IdList::from_set(set)));
    });

    let (excluded_datacenters_param, set_excluded_datacenters_param) =
        filter_query_signal::<NameList>("excluded-datacenters");
    let excluded_datacenters = Memo::new(move |_| {
        excluded_datacenters_param
            .get()
            .map(|list| list.0.into_iter().collect::<HashSet<String>>())
            .unwrap_or_default()
    });
    let set_excluded_datacenters = Callback::new(move |set: HashSet<String>| {
        set_excluded_datacenters_param.set((!set.is_empty()).then(|| NameList::from_set(set)));
    });

    let (hide_acquired_param, set_hide_acquired_param) =
        filter_query_signal::<bool>("hide-acquired");
    let hide_acquired = Memo::new(move |_| hide_acquired_param.get().unwrap_or(false));

    let (sort_spec, set_sort_spec) = filter_query_signal::<SortSpec>("sort");

    let game_items = &tracked_data().items;

    type RowSnapshot = std::collections::HashMap<i32, (Option<i32>, Option<i32>)>;
    let recently_changed: RwSignal<HashSet<i32>> = RwSignal::new(HashSet::new());
    let prev_snapshot: StoredValue<RowSnapshot> = StoredValue::new(RowSnapshot::new());

    Effect::new(move |_| {
        let Some(Ok((_list, items))) = list_view.get() else {
            return;
        };
        let new_snapshot: RowSnapshot = items
            .iter()
            .map(|(i, _)| (i.id, (i.quantity, i.acquired)))
            .collect();
        let mut newly_changed: HashSet<i32> = HashSet::new();
        let prev = prev_snapshot.get_value();
        for (id, current) in &new_snapshot {
            if let Some(prior) = prev.get(id)
                && prior != current
            {
                newly_changed.insert(*id);
            }
        }
        prev_snapshot.set_value(new_snapshot);

        if !newly_changed.is_empty() {
            recently_changed.update(|set| set.extend(newly_changed.iter().copied()));
            #[cfg(not(feature = "ssr"))]
            {
                use gloo_timers::callback::Timeout;
                let ids: Vec<i32> = newly_changed.into_iter().collect();
                Timeout::new(1500, move || {
                    recently_changed.update(|set| {
                        for id in &ids {
                            set.remove(id);
                        }
                    });
                })
                .forget();
            }
        }
    });

    // The settings drawer is rebuilt only when the list row it edits actually
    // changes, not on every document revision or price refresh the page
    // resource re-runs for; otherwise a collaborator's edit (or a market
    // update) would recreate an open drawer under the user's cursor.
    let drawer_key = Memo::new(move |_| {
        list_view.get().and_then(Result::ok).map(|(l, _)| {
            (
                l.list.id,
                l.list.name.clone(),
                l.list.wdr_filter,
                l.permission,
            )
        })
    });
    let view_caps = RwSignal::new(ListCapabilities::default());
    Effect::new(move |_| {
        let next = match list_view.get() {
            Some(Ok((list_with_perm, _))) => ListCapabilities::from(list_with_perm.permission),
            _ => ListCapabilities::default(),
        };
        view_caps.set(next);
    });

    let build_rows = Signal::derive(move || {
        let snapshot = list_view
            .get()
            .and_then(Result::ok)
            .map(|(_, rows)| rows)
            .unwrap_or_default();
        let snapshot = if let Some(doc) = handle.get() {
            doc.revision.track();
            let listings: HashMap<_, _> = snapshot
                .into_iter()
                .map(|(item, listings)| (item.item_id, listings))
                .collect();
            doc.rows()
                .iter()
                .filter_map(|row| crate::list_doc::adapter::to_list_item(list_id.get(), row))
                .map(|item| {
                    let prices = listings.get(&item.item_id).cloned().unwrap_or_default();
                    (item, prices)
                })
                .collect()
        } else {
            snapshot
        };
        let world_helper = use_context::<LocalWorldData>().and_then(|data| data.0.ok());
        let mut rows = filter_excluded(
            &snapshot,
            &excluded_worlds.get(),
            &excluded_datacenters.get(),
            world_helper.as_deref(),
        );
        if let Some(spec) = sort_spec.get() {
            sort_list_items(&mut rows, spec, |id| {
                game_items.get(&ItemId(id)).map(|item| item.name.as_str())
            });
        }
        rows
    });
    let build_source = ListWorkspaceSource {
        list_id: list_id.into(),
        add: Callback::new(move |item| {
            add_item.dispatch(item);
        }),
        add_many: Callback::new(move |items| {
            let result = apply_edit(handle, Edit::AddMany(items));
            mutation_feedback.set(match result {
                Ok(()) => t_string!(i18n, lists_workspace_recipe_added).to_string(),
                Err(error) => workspace_error(i18n, &error),
            });
        }),
        undo: undo_action,
        redo: redo_action,
        can_undo: Signal::derive(move || handle.get().is_some_and(|handle| handle.can_undo())),
        can_redo: Signal::derive(move || handle.get().is_some_and(|handle| handle.can_redo())),
        pending: add_item.pending().into(),
        feedback: Signal::derive(move || {
            add_item
                .value()
                .get()
                .map(|result| match result {
                    Ok(()) => t_string!(i18n, lists_workspace_added).to_string(),
                    Err(error) => workspace_error(i18n, &error),
                })
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| mutation_feedback.get())
        }),
        recipe_open: recipe_modal_open.into(),
        toggle_recipe: Callback::new(move |()| set_recipe_modal_open.update(|open| *open = !*open)),
        rows: build_rows,
        hide_acquired: hide_acquired.into(),
        can_write: Signal::derive(move || view_caps.with(|c| c.can_write)),
        edit: Callback::new(move |item| {
            edit_item.dispatch(item);
        }),
        remove: Callback::new(move |id| {
            delete_item.dispatch(id);
        }),
    };

    let drawer_refresh = Signal::derive(move || {
        last_update_at
            .get()
            .map(|t| t.timestamp_millis() as u32)
            .unwrap_or(0)
    });

    // Auto-mark logic moved to AutoMarkPurchases component

    let list_name_for_meta = Signal::derive(move || {
        list_view
            .get()
            .and_then(|r| r.ok().map(|(l, _)| l.list.name))
            .unwrap_or_default()
    });
    let (home_world, _) = crate::global_state::home_world::use_home_world();
    let shop_worlds = use_context::<LocalWorldData>().and_then(|data| data.0.ok());
    let shop_input = Signal::derive(move || {
        use crate::components::list_shop::{ShopInput, ShopRow};
        let Some(Ok((list, rows))) = list_view.get() else {
            return ShopInput::default();
        };
        let rows = filter_excluded(
            &rows,
            &excluded_worlds.get(),
            &excluded_datacenters.get(),
            shop_worlds.as_deref(),
        );
        let mut result = ShopInput {
            title: list.list.name,
            home_world: home_world.get().map(|world| world.id).unwrap_or_default(),
            observed_at: None,
            ..Default::default()
        };
        if let Some(scope) = shop_worlds
            .as_ref()
            .and_then(|helper| helper.lookup_selector(list.list.wdr_filter))
        {
            for world in scope.all_worlds() {
                result.world_names.insert(world.id, world.name.clone());
                result.datacenters.insert(world.id, world.datacenter_id);
            }
        }
        result.rows = rows
            .into_iter()
            .map(|(row, listings)| ShopRow {
                key: row.id.to_string(),
                name: tracked_data()
                    .items
                    .get(&ItemId(row.item_id))
                    .map(|item| item.name.to_string())
                    .unwrap_or_else(|| {
                        t_string!(i18n, lists_workspace_item_fallback, id = row.item_id).to_string()
                    }),
                item_id: row.item_id,
                hq: row.hq,
                needed: row.quantity.unwrap_or(1),
                acquired: row.acquired.unwrap_or(0),
                listings,
            })
            .collect();
        result
    });
    let meta_title = move || {
        let name = list_name_for_meta.get();
        if name.is_empty() {
            t_string!(i18n, list_view_default_meta_title).to_string()
        } else {
            t_string!(i18n, list_view_meta_title)
                .to_string()
                .replace("%name%", &name)
        }
    };

    view! {
        <MetaTitle title=meta_title />
        <MetaDescription text=move || t_string!(i18n, list_view_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        <div class="flex flex-col gap-4" data-testid="list-view-sync">
            <div class="sticky-bar rounded-lg px-3 py-3">
                // `list-toolbar` no longer carries any CSS (the compact
                // button-sizing rule it used to scope moved to the shared
                // `.sticky-bar-button` class on each button below and was
                // deleted from tailwind.css) — kept purely as a stable
                // `querySelector(".list-toolbar")` hook for
                // `integration/list-flow.cjs`, `screenshots.cjs` and
                // `shared-list.cjs`, which locate this row by that class.
                <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between list-toolbar">
                    <div class="flex flex-wrap items-center gap-2">
                        <Show when=move || view_caps.with(|c| c.can_write)>
                            <>
                                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_recipe).to_string()>
                                    <button
                                        class="sticky-bar-button sticky-bar-button-shrink"
                                        class:bg-brand-900=move || recipe_modal_open.get()
                                        class:border-brand-500=move || recipe_modal_open.get()
                                        on:click=move |_| {
                                            set_buying_view_param.set(None);
                                            set_recipe_modal_open(!recipe_modal_open.get_untracked());
                                        }
                                    >
                                        <Icon icon=i::BiBookAddRegular />
                                        <span class="sticky-bar-button-label">{t!(i18n, list_view_add_recipe)}</span>
                                    </button>
                                </Tooltip>
                                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_import_item).to_string()>
                                    <button
                                        class="sticky-bar-button sticky-bar-button-shrink"
                                        class:bg-brand-900=move || menu() == MenuState::MakePlace
                                        class:border-brand-500=move || menu() == MenuState::MakePlace
                                        on:click=move |_| set_menu(
                                            match menu() {
                                                MenuState::MakePlace => MenuState::None,
                                                _ => MenuState::MakePlace,
                                            },
                                        )
                                    >
                                        <Icon icon=i::BiImportRegular />
                                        <span class="sticky-bar-button-label">{t!(i18n, list_view_make_place)}</span>
                                    </button>
                                </Tooltip>
                            </>
                        </Show>
                    </div>

                    <div class="flex flex-wrap gap-2 self-start lg:self-auto">
                        <Show when=move || view_caps.with(|c| c.can_write)>
                            <Tooltip tooltip_text=t_string!(i18n, list_auto_mark_description).to_string()>
                                <AutoMarkPurchases
                                    list_view=list_view
                                    on_purchase=Callback::new(move |(item_id, hq): (i32, bool)| {
                                        if let Some(handle) = handle.get_untracked()
                                            && let Err(error) = handle
                                                .apply(Edit::AddAcquired {
                                                    item_id,
                                                    hq: Some(hq),
                                                    delta: 1,
                                                })
                                        {
                                            log::warn!(
                                                "auto-mark failed for item {item_id}: {error}"
                                            );
                                        }
                                    })
                                />
                            </Tooltip>
                        </Show>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_subscribe_tooltip).to_string()>
                            <button
                                class="sticky-bar-button sticky-bar-button-shrink"
                                aria-label=t_string!(i18n, list_view_subscribe_aria)
                                on:click=move |_| set_subscribe_open(true)
                            >
                                <Icon icon=i::BsBell />
                                <span class="sticky-bar-button-label">{t!(i18n, list_view_subscribe_button)}</span>
                            </button>
                        </Tooltip>
                        <ListWorkspaceModes shop=buying_view.into() set_shop=Callback::new(move |shop: bool| set_buying_view_param.set(shop.then_some(true))) />
                        <Tooltip tooltip_text=t_string!(i18n, list_view_settings_tooltip).to_string()>
                            <button
                                class="sticky-bar-button sticky-bar-button-shrink"
                                aria-label=t_string!(i18n, list_view_settings)
                                data-testid="list-settings-btn"
                                on:click=move |_| set_settings_open(true)
                            >
                                <Icon icon=i::BsGear />
                                <span class="sticky-bar-button-label">{t!(i18n, list_view_settings)}</span>
                            </button>
                        </Tooltip>
                    </div>
                </div>
            </div>

            <Show when=subscribe_open>
                {move || {
                    let name = list_view
                        .get()
                        .and_then(|r| r.ok().map(|(l, _)| l.list.name))
                        .unwrap_or_else(|| t_string!(i18n, lists_workspace_list_fallback, id = list_id()).to_string());
                    view! {
                        <ListSubscribeDrawer
                            list_id=list_id()
                            list_name=name
                            set_visible=set_subscribe_open.into()
                        />
                    }
                }}
            </Show>

            <Show when=move || buying_view.get()>
                <Suspense fallback=move || view! { <Loading /> }>
                    <crate::components::list_shop::ListShop input=shop_input
                        on_purchase=Callback::new(move |(key, delta): (String, i32)| {
                            if !view_caps.with_untracked(|c| c.can_write) { return; }
                            if let Ok(id) = key.parse::<i32>()
                                && let Some(key) = crate::list_doc::adapter::key_of(id)
                                && let Some(handle) = handle.get_untracked()
                                && let Err(error) = handle.apply(Edit::AddAcquired { item_id: key.item_id, hq: key.hq(), delta: i64::from(delta.max(0)) })
                            { mutation_feedback.set(workspace_error(i18n, &AppError::ListDoc(error.to_string()))); }
                        })
                        on_undo=Callback::new(move |()| { if let Some(handle) = handle.get_untracked() { handle.undo(); } })
                        can_edit=Signal::derive(move || view_caps.with(|c| c.can_write)) />
                </Suspense>
            </Show>

            {move || match menu() {
                MenuState::None => None,
                MenuState::MakePlace => {
                    Some(
                        view! {
                            <section class="panel rounded-lg p-4">
                                <MakePlaceImporter
                                    list_id=Signal::derive(move || {
                                        params
                                            .with(|p| {
                                                p.get("id").as_ref().map(|id| id.parse::<i32>().ok())
                                            })
                                            .flatten()
                                            .unwrap_or_default()
                                    })

                                    refresh=move || set_listings_version.update(|v| *v += 1)
                                />
                            </section>
                        },
                    )
                }
            }}

            <Transition fallback=move || {
                view! {
                    <section class="panel rounded-lg overflow-hidden">
                        <TableSkeleton
                            columns=list_item_table_skeleton_columns()
                            rows=6
                            row_class="px-1"
                        />
                    </section>
                }
            }>
                {move || {
                    list_view
                        .get()
                        .map(move |list| match list {
                            Ok((list, items)) => {
                                let items = StoredValue::new(items);
                                Either::Left(move || {
                                    let item_snapshot = items.get_value();
                                    let total_items = item_snapshot.len();
                                    let remaining_items = item_snapshot
                                        .iter()
                                        .filter(|(item, _)| {
                                            item.quantity.unwrap_or(1)
                                                > item.acquired.unwrap_or(0)
                                        })
                                        .count();
                                    let acquired_items = total_items.saturating_sub(remaining_items);
                                    let total_quantity: i32 = item_snapshot
                                        .iter()
                                        .map(|(i, _)| i.quantity.unwrap_or(1).max(1))
                                        .sum();
                                    let total_acquired: i32 = item_snapshot
                                        .iter()
                                        .map(|(i, _)| {
                                            let q = i.quantity.unwrap_or(1).max(1);
                                            i.acquired.unwrap_or(0).clamp(0, q)
                                        })
                                        .sum();
                                    let pct: i32 = if total_quantity > 0 {
                                        100 * total_acquired / total_quantity
                                    } else {
                                        0
                                    };
                                    let world_helper = use_context::<LocalWorldData>()
                                        .and_then(|world_data| world_data.0.ok());

                                    // Built here, inside the Transition, so its SSR render
                                    // comes from the resolved resource — a read of
                                    // `list_view` outside a suspense boundary doesn't
                                    // register, and the shell/first-client-render disagree
                                    // when the resource resolves after the shell flushes
                                    // (an unrecoverable hydration mismatch).
                                    let datacenters = world_helper
                                        .as_deref()
                                        .and_then(|helper| {
                                            helper.lookup_selector(list.list.wdr_filter).map(
                                                |result| {
                                                    helper
                                                        .get_datacenters(&result)
                                                        .into_iter()
                                                        .map(|dc| dc.name.clone())
                                                        .collect::<Vec<_>>()
                                                },
                                            )
                                        })
                                        .unwrap_or_default();
                                    let filter_row = view! {
                                        <ListFilterRow
                                            worlds=worlds_in_listings(
                                                &item_snapshot,
                                                world_helper.as_deref(),
                                            )
                                            datacenters=datacenters
                                            excluded_worlds=excluded_worlds
                                            set_excluded_worlds=set_excluded_worlds
                                            excluded_datacenters=excluded_datacenters
                                            set_excluded_datacenters=set_excluded_datacenters
                                            sort_spec=Signal::derive(move || sort_spec.get())
                                            set_sort_spec=Callback::new(move |spec| {
                                                set_sort_spec.set(spec)
                                            })
                                            hide_acquired=hide_acquired
                                            set_hide_acquired=Callback::new(move |hide: bool| {
                                                set_hide_acquired_param.set(hide.then_some(true));
                                            })
                                        />
                                    };

                                    if buying_view() {
                                        Either::Left(view! { {filter_row} })
                                    } else {
                                        Either::Right(
                                            view! {
                                                {filter_row}
                                                <section class="panel rounded-lg overflow-hidden">
                                                    <div class="border-b border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
                                                            <div>
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, list_view_list_label)}</p>
                                                                <div class="flex items-center gap-2">
                                                                    {
                                                                        let list_for_title = list.list.clone();
                                                                        let display_name = list_for_title.name.clone();
                                                                        move || {
                                                                            if rename_open() && view_caps.with(|c| c.can_admin) {
                                                                                let list_for_save = list_for_title.clone();
                                                                                Either::Left(view! {
                                                                                    <div class="flex flex-wrap items-center gap-2">
                                                                                        <input
                                                                                            class="input text-xl font-bold"
                                                                                            prop:value=rename_value
                                                                                            on:input=move |ev| set_rename_value(event_target_value(&ev))
                                                                                            data-testid="list-rename-input"
                                                                                        />
                                                                                        <button
                                                                                            class="btn-primary"
                                                                                            data-testid="list-rename-save"
                                                                                            on:click={
                                                                                                let list_for_save = list_for_save.clone();
                                                                                                move |_| {
                                                                                                    let mut new_list = list_for_save.clone();
                                                                                                    new_list.name = rename_value().trim().to_string();
                                                                                                    if !new_list.name.is_empty() {
                                                                                                        edit_list_action.dispatch(new_list);
                                                                                                        set_rename_open(false);
                                                                                                    }
                                                                                                }
                                                                                            }
                                                                                        >
                                                                                            <Icon icon=i::BiSaveSolid />
                                                                                            <span>{t!(i18n, list_view_settings_save)}</span>
                                                                                        </button>
                                                                                        <button
                                                                                            class="btn-secondary"
                                                                                            on:click=move |_| set_rename_open(false)
                                                                                        >
                                                                                            {t!(i18n, list_view_settings_cancel)}
                                                                                        </button>
                                                                                    </div>
                                                                                })
                                                                            } else {
                                                                                let display_name = display_name.clone();
                                                                                Either::Right(view! {
                                                                                    <>
                                                                                        <h1 class="text-xl sm:text-2xl font-bold text-[color:var(--brand-fg)]">{display_name.clone()}</h1>
                                                                                        <Show when=move || view_caps.with(|c| c.can_admin)>
                                                                                            <button
                                                                                                class="btn-ghost p-1"
                                                                                                aria-label=t_string!(i18n, edit_list).to_string()
                                                                                                data-testid="list-rename-btn"
                                                                                                on:click={
                                                                                                    let name = display_name.clone();
                                                                                                    move |_| {
                                                                                                        set_rename_value(name.clone());
                                                                                                        set_rename_open(true);
                                                                                                    }
                                                                                                }
                                                                                            >
                                                                                                <Icon icon=i::BsPencilFill />
                                                                                            </button>
                                                                                        </Show>
                                                                                    </>
                                                                                })
                                                                            }
                                                                        }
                                                                    }
                                                                </div>
                                                                <div class="mt-2">
                                                                    <RealtimeStatus
                                                                        status=realtime_status
                                                                        last_update=last_update_at
                                                                    />
                                                                </div>
                                                                <div class="mt-3 flex items-center gap-3 text-sm">
                                                                    {if total_quantity > 0 {
                                                                        Either::Left(view! {
                                                                            <div
                                                                                class="flex min-w-0 flex-1 flex-col gap-1"
                                                                                aria-label=t_string!(i18n, list_view_units_acquired_aria, acquired = total_acquired, quantity = total_quantity).to_string()
                                                                            >
                                                                                <span class="text-[color:var(--color-text-muted)]">
                                                                                    {t!(i18n, list_view_units_acquired_progress, acquired = total_acquired, quantity = total_quantity, pct = pct)}
                                                                                </span>
                                                                                <progress
                                                                                    class="progress progress-primary h-2 w-full rounded"
                                                                                    value=total_acquired
                                                                                    max=total_quantity
                                                                                ></progress>
                                                                            </div>
                                                                        })
                                                                    } else {
                                                                        Either::Right(view! {
                                                                            <span class="text-[color:var(--color-text-muted)]">
                                                                                {t!(i18n, list_view_no_items_yet)}
                                                                            </span>
                                                                        })
                                                                    }}
                                                                </div>
                                                            </div>
                                                            <div class="grid grid-cols-3 gap-2 text-center text-sm">
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{total_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, item_explorer_items)}</div>
                                                                </div>
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{remaining_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_remaining)}</div>
                                                                </div>
                                                                <Tooltip tooltip_text=Signal::derive(move || {
                                                                    t_string!(i18n, list_view_acquired_items_tooltip, count = acquired_items, total = total_items).to_string()
                                                                })>
                                                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                        <div class="text-lg font-bold">{acquired_items}</div>
                                                                        <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_acquired)}</div>
                                                                    </div>
                                                                </Tooltip>
                                                            </div>
                                                        </div>
                                                    </div>

                                                    <Show when=move || view_caps.with(|c| c.can_write)>
                                                        <div class="flex flex-col gap-3 border-b border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)]/60 p-3 lg:flex-row lg:items-center lg:justify-between">
                                                            <div class="flex flex-wrap items-center gap-2">
                                                                <span class="text-sm">{move || t_string!(i18n, lists_workspace_selected, count = selected_items.with(|s| s.len())).to_string()}</span>
                                                                <div
                                                                    class="flex flex-wrap items-center gap-2"
                                                                    class:hidden=move || selected_items.with(|s| s.is_empty())
                                                                >
                                                                    <button
                                                                        class="btn-danger"
                                                                        disabled=bulk_pending
                                                                        on:click=move |_| {
                                                                            if !selected_items.with_untracked(|s| s.is_empty()) {
                                                                                set_confirm_bulk_delete(true);
                                                                            }
                                                                        }
                                                                    >
                                                                        <Icon icon=i::BiTrashSolid />
                                                                        <span>{t!(i18n, list_view_delete)}</span>
                                                                    </button>
                                                                    <button
                                                                        class="btn-secondary"
                                                                        disabled=bulk_pending
                                                                        on:click=move |_| {
                                                                            let items = selected_items
                                                                                .with_untracked(|s| {
                                                                                    s.iter().copied().collect::<Vec<_>>()
                                                                                });
                                                                            edit_items_hq.dispatch((items, Some(true)));
                                                                        }
                                                                    >
                                                                        <span>{t!(i18n, list_view_bulk_set_hq)}</span>
                                                                    </button>
                                                                    <button
                                                                        class="btn-secondary"
                                                                        disabled=bulk_pending
                                                                        on:click=move |_| {
                                                                            let items = selected_items
                                                                                .with_untracked(|s| {
                                                                                    s.iter().copied().collect::<Vec<_>>()
                                                                                });
                                                                            edit_items_hq.dispatch((items, None));
                                                                        }
                                                                    >
                                                                        <span>{t!(i18n, list_view_bulk_any_quality)}</span>
                                                                    </button>
                                                                    <Show when=move || bulk_pending.get()>
                                                                        <span class="flex items-center gap-2 text-sm text-[color:var(--color-text-muted)]">
                                                                            <Loading />
                                                                            {t!(i18n, list_view_bulk_pending)}
                                                                        </span>
                                                                    </Show>
                                                                    {move || {
                                                                        (!bulk_pending.get())
                                                                            .then(|| bulk_error.get())
                                                                            .flatten()
                                                                            .map(|error| {
                                                                                view! {
                                                                                    <span class="text-sm text-red-200">
                                                                                        {format!(
                                                                                            "{} {error}",
                                                                                            t_string!(i18n, list_view_bulk_failed),
                                                                                        )}
                                                                                    </span>
                                                                                }
                                                                            })
                                                                    }}
                                                                </div>
                                                            </div>
                                                            <div
                                                                class="flex flex-wrap items-center gap-2"
                                                            >
                                                                <button
                                                                    class="btn-secondary"
                                                                    on:click=move |_| {
                                                                        selected_items
                                                                            .update(|i| {
                                                                                for (item, _) in items.get_value() {
                                                                                    i.insert(item.id);
                                                                                }
                                                                            })
                                                                    }
                                                                >
                                                                    {t!(i18n, list_view_select_all)}
                                                                </button>
                                                                <button
                                                                    class="btn-secondary"
                                                                    on:click=move |_| {
                                                                        selected_items.update(|i| i.clear());
                                                                    }
                                                                >
                                                                    {t!(i18n, list_view_deselect_all)}
                                                                </button>
                                                            </div>
                                                        </div>
                                                        <Show when=confirm_bulk_delete>
                                                            <Modal set_visible=set_confirm_bulk_delete>
                                                                <div class="flex flex-col gap-4">
                                                                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                                                                        {t!(i18n, list_view_bulk_delete_confirm_title)}
                                                                    </h2>
                                                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                                                        {move || t!(
                                                                            i18n,
                                                                            list_view_bulk_delete_confirm_body,
                                                                            count = selected_items.with(|s| s.len()),
                                                                        )}
                                                                    </p>
                                                                    <div class="flex justify-end gap-2">
                                                                        <button
                                                                            class="btn-secondary"
                                                                            on:click=move |_| set_confirm_bulk_delete(false)
                                                                        >
                                                                            {t!(i18n, cancel)}
                                                                        </button>
                                                                        <button
                                                                            class="btn-danger"
                                                                            on:click=move |_| {
                                                                                let items = selected_items
                                                                                    .with_untracked(|s| {
                                                                                        s.iter().copied().collect::<Vec<_>>()
                                                                                    });
                                                                                selected_items.update(|i| i.clear());
                                                                                delete_items.dispatch(items);
                                                                                set_confirm_bulk_delete(false);
                                                                            }
                                                                        >
                                                                            <Icon icon=i::BiTrashSolid />
                                                                            <span>{t!(i18n, list_view_delete)}</span>
                                                                        </button>
                                                                    </div>
                                                                </div>
                                                            </Modal>
                                                        </Show>
                                                    </Show>

                                                </section>
                                            },
                                        )
                                    }
                                })
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        <div class="panel rounded-lg p-4">{format!("{}\n{}", t_string!(i18n, list_view_failed_to_get_items), workspace_error(i18n, &e))}</div>
                                    },
                                )
                            }
                        })
                }}

            </Transition>

            <div class:hidden=move || buying_view.get()>
                <Transition fallback=move || view! { <Loading /> }>
                    <ListBuildWorkspace source=build_source selected_items highlighted=Signal::derive(move || recently_changed.get()) />
                    <div class="panel rounded-lg p-4 mt-3">
                        {move || list_view.get().and_then(Result::ok).map(|(_, items)| view! { <ListSummary items excluded_worlds=&[] excluded_datacenters /> })}
                        <ActivityFeed activity=activity_view />
                    </div>
                </Transition>
            </div>

            <Show when=settings_open>
                {move || {
                    if drawer_key.get().is_none() {
                        return view! { <div></div> }.into_any();
                    }
                    let Some(Ok((list_with_perm, _))) = list_view.get_untracked() else {
                        return view! { <div></div> }.into_any();
                    };
                    view! {
                        <ListSettingsDrawer
                            list=list_with_perm.list.clone()
                            permission=list_with_perm.permission
                            self_user_id=self_user_id
                            edit_list=edit_list_action
                            refresh_signal=drawer_refresh
                            set_visible=set_settings_open
                        />
                    }
                    .into_any()
                }}
            </Show>
        </div>
    }.into_any()
}

/// Picks the page for `/list/:id`. The `LABS` cookie is server-visible, so
/// the server and the hydrating client make the same choice. The id is
/// tracked so moving between lists builds a fresh page and document.
#[component]
pub fn ListRoute() -> impl IntoView {
    let sync = use_lab(LAB_LISTS_SYNC);
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|p| p.get("id").unwrap_or_default()));
    move || {
        id.track();
        if sync.get() {
            view! { <ListViewSync /> }.into_any()
        } else {
            view! { <ListView /> }.into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_preview_ignores_empty_crystal_sentinels_without_dropping_real_ingredients() {
        let recipe = xiv_gen::Recipe {
            key_id: xiv_gen::RecipeId(170),
            item_result: 5056,
            amount_result: 1,
            ingredient: [5106, 5107, 0, 0, 0, 0, 3, -1],
            amount_ingredient: [2, 1, 0, 0, 0, 0, 1, 0],
            craft_type: 0,
            recipe_level_table: 1,
        };
        assert_eq!(
            recipe_preview_ingredients(&recipe).collect::<Vec<_>>(),
            vec![(ItemId(5106), 2), (ItemId(5107), 1), (ItemId(3), 1),]
        );
        let mut broken = recipe;
        broken.amount_ingredient[7] = 1;
        assert!(
            recipe_preview_ingredients(&broken).any(|(id, amount)| id.0 == -1 && amount == 1),
            "a positive-quantity missing ingredient must remain visible to validation"
        );
    }

    /// Global Constraint 2: only "you may not have this list" purges the
    /// browser's copy.
    #[test]
    fn permission_and_missing_list_errors_are_denials() {
        for error in [
            AppError::BadList,
            AppError::ApiError(ApiError::Forbidden),
            AppError::ApiError(ApiError::NotFound),
        ] {
            assert!(is_denial(&error), "{error:?} must discard the local copy");
        }
    }

    /// A lapsed session is not a denial: the handle closes and the snapshot
    /// stays, so signing back in resumes the user's offline edits.
    #[test]
    fn a_lapsed_session_keeps_the_local_copy() {
        assert!(!is_denial(&AppError::ApiError(ApiError::NotAuthenticated)));
    }

    /// The complement: a transport failure, a 5xx flattened into a message,
    /// a body that would not parse, or an SSR timeout must leave the
    /// document alone so an offline edit survives the outage.
    #[test]
    fn transport_and_server_failures_are_not_denials() {
        for error in [
            AppError::ApiError(ApiError::Message("upstream exploded".to_string())),
            AppError::ApiError(ApiError::BadRequest("nope".to_string())),
            AppError::Json("expected value at line 1 column 1".to_string()),
            AppError::SystemError(crate::error::SystemError::Message(
                "connection reset".to_string(),
            )),
            AppError::InternalApiTimeout,
            AppError::ListDoc("document is not open yet".to_string()),
        ] {
            assert!(!is_denial(&error), "{error:?} is transient");
        }
    }

    /// With no document open, every write is refused rather than silently
    /// dropped — the page has no REST fallback to fall back to.
    #[test]
    fn edits_without_a_document_report_it() {
        let owner = Owner::new();
        owner.with(|| {
            let handle: RwSignal<Option<ListDocHandle>> = RwSignal::new(None);
            let error = apply_edit(handle, Edit::Remove(1)).unwrap_err();
            assert!(matches!(error, AppError::ListDoc(_)), "{error:?}");
        });
    }
}
