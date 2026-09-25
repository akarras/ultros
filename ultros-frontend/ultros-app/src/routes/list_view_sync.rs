//! Labs list workspace: inline construction and a stable shopping companion,
//! backed by the local document and account synchronization.

#[cfg(any(feature = "hydrate", test))]
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
#[cfg(any(feature = "hydrate", test))]
use std::rc::Rc;

use crate::global_state::xiv_data::tracked_data;

use crate::components::icon::Icon;
use crate::global_state::LocalWorldData;
use icondata as i;
use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use ultros_api_types::{
    ActiveListing,
    list::{ListCapabilities, ListItem, ListPermission, ListWithPermission},
    result::ApiError,
};

use crate::api::{get_list_activity, get_list_items_with_listings};
use crate::components::list_workspace_shell::ListWorkspaceShell;
use crate::components::{
    cart::{ListCart, use_legacy_cart},
    item_icon::*,
    list::{auto_mark_purchases::AutoMarkPurchases, filter_row::SortSpec, list_summary::*},
    list_subscribe_drawer::ListSubscribeDrawer,
    loading::*,
    make_place_importer::*,
    meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle},
    modal::Modal,
    skeleton::TableSkeleton,
};
use crate::error::AppError;
use crate::global_state::labs::{LAB_LISTS_SYNC, use_lab};
use crate::i18n::*;
use crate::list_doc::adapter::Edit;
use crate::list_doc::handle::{ListDocHandle, RecoveryState, SaveState};
use crate::query_defaults::filter_query_signal;
use crate::routes::list_view::{
    ActivityFeed, IdList, ListView, ListViewResult, NameList, filter_excluded,
    list_item_table_skeleton_columns, remaining_quantity, sort_list_items,
};
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
#[cfg(feature = "hydrate")]
use ultros_api_types::websocket::{EventType as WEvent, ListEventData, ServerClient};
use ultros_api_types::websocket::{
    FilterPredicate, SocketMessageType, is_list_market_update_relevant,
};
use ultros_api_types::world_helper::AnySelector;
use ultros_calc::list_estimate::PriceFeed;
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

/// Inline composer shared by account and device lists. One panel, two modes:
/// the Items mode searches the catalog, the Recipes mode previews a recipe's
/// finished items or ingredients. The mode is the source's `recipe_open`
/// signal, so the legacy toolbar button and the guest follow guard keep
/// their meaning.
#[component]
pub fn InlineListAdd(
    list_id: Signal<i32>,
    on_add: Callback<ListItem>,
    on_add_many: Callback<Vec<ListItem>>,
    recipe_mode: Signal<bool>,
    toggle_recipe: Callback<()>,
    #[prop(default = Signal::derive(|| false))] pending: Signal<bool>,
    #[prop(default = Signal::derive(String::new))] feedback: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let set_recipe_mode = move |want: bool| {
        if recipe_mode.get_untracked() != want {
            toggle_recipe.run(());
        }
    };
    let search = RwSignal::new(String::new());
    let committed_search = RwSignal::new(String::new());
    let quantity = RwSignal::new("1".to_string());
    let invalid_quantity =
        Memo::new(move |_| !crate::components::cart::row::valid_numeric_value(0, &quantity.get()));
    let quality = RwSignal::new("any".to_string());
    let committed_quantity = RwSignal::new("1".to_string());
    let committed_quality = RwSignal::new("any".to_string());
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
        committed_quantity.set(quantity.get_untracked());
        committed_quality.set(quality.get_untracked());
        committed_search.set(search.get_untracked());
        if let Some(input) = input.get() {
            let _ = input.focus();
            input.select();
        }
    });
    view! {
        <section class="panel rounded-xl p-4 sm:p-5" aria-label=t_string!(i18n, lists_workspace_add_items_label) data-testid="inline-list-add">
            <div class="mb-3 flex flex-wrap items-start justify-between gap-2">
                <div class="min-w-0">
                    <h2 class="font-semibold">{t!(i18n, lists_workspace_build_title)}</h2>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{move || if recipe_mode.get() { t_string!(i18n, lists_workspace_recipe_hint).to_string() } else { t_string!(i18n, lists_workspace_build_hint).to_string() }}</p>
                </div>
                <div class="segmented-control w-fit" role="group" aria-label=t_string!(i18n, lists_workspace_composer_mode) data-testid="list-composer-mode">
                    <button type="button" class="segmented-option min-w-20" data-testid="list-composer-items" aria-pressed=move || (!recipe_mode.get()).to_string() on:click=move |_| set_recipe_mode(false)>{t!(i18n, lists_workspace_composer_items)}</button>
                    <button type="button" class="segmented-option min-w-20" data-testid="list-composer-recipes" aria-pressed=move || recipe_mode.get().to_string() on:click=move |_| set_recipe_mode(true)>{t!(i18n, lists_workspace_composer_recipes)}</button>
                </div>
            </div>
            <Show when=move || recipe_mode.get()><InlineRecipeAdd list_id=list_id on_add=on_add_many /></Show>
            <Show when=move || !recipe_mode.get()>
            <div class="flex flex-wrap gap-2">
                <input node_ref=input id="list-cart-add-input" class="input flex-1 min-w-48" placeholder=t_string!(i18n, lists_workspace_add_placeholder) aria-label=t_string!(i18n, lists_workspace_add_item) prop:value=search data-committed="" data-handoff-committed=move || committed_search.get()
                    on:input=move |ev| search.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Escape" { search.set(String::new()); committed_search.set(String::new()); ev.stop_propagation(); }
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            if let Some((id, _, can_hq, _)) = results.get_untracked().first() { add.run((*id, *can_hq)); }
                        }
                    } />
                <input type="number" min="1" max=i32::MAX class="input w-24" aria-label=t_string!(i18n, lists_workspace_add_quantity) aria-invalid=move || invalid_quantity.get().to_string() aria-describedby=move || invalid_quantity.get().then_some("list-add-quantity-error") prop:value=quantity data-committed=move || quantity.get() data-handoff-committed=move || committed_quantity.get() on:input=move |ev| quantity.set(event_target_value(&ev)) on:keydown=move |ev| {
                    if ev.key() == "Escape" { quantity.set(committed_quantity.get_untracked()); ev.stop_propagation(); }
                } />
                <select class="input" aria-label=t_string!(i18n, lists_workspace_add_quality) prop:value=quality data-handoff-committed=move || committed_quality.get() on:change=move |ev| quality.set(event_target_value(&ev)) on:keydown=move |ev| {
                    if ev.key() == "Escape" { quality.set(committed_quality.get_untracked()); ev.stop_propagation(); }
                }><option value="any">{t!(i18n, lists_workspace_any_quality)}</option><option value="nq">{t!(i18n, lists_workspace_nq)}</option><option value="hq">{t!(i18n, lists_workspace_hq_available)}</option></select>
            </div>
            <Show when=move || invalid_quantity.get()><p id="list-add-quantity-error" role="alert" class="mt-2 text-sm text-negative">{move || if crate::components::cart::row::numeric_value_too_large(0, &quantity.get()) {
                t_string!(i18n, cart_number_too_large).to_string()
            } else {
                t_string!(i18n, cart_quantity_invalid).to_string()
            }}</p></Show>
            <p class="text-sm mt-2 text-[color:var(--color-text-muted)]" role="status">{feedback}</p>
            <Show when=move || !search.get().trim().is_empty()>
                <div class="mt-3 max-h-80 overflow-y-auto divide-y divide-[color:var(--color-outline)]" aria-label=t_string!(i18n, lists_workspace_catalog_results)>
                    <Show when=move || results.get().is_empty()><p class="p-3 text-sm">{t!(i18n, lists_workspace_no_items)}</p></Show>
                    <For each=move || results.get() key=|item| item.0 children=move |(id, name, can_hq, _)| view! {
                        <div class="flex items-center gap-3 py-2"><ItemIcon item_id=id icon_size=IconSize::Small /><span class="flex-1 min-w-0">{name.clone()}</span>
                            <button class="btn-primary" aria-label=t_string!(i18n, lists_workspace_add_named, name = name.clone()) disabled={move || pending.get() || invalid_quantity.get()} on:click=move |_| add.run((id, can_hq))>{t!(i18n, lists_workspace_add)}</button>
                        </div>
                    } />
                </div>
            </Show>
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
        <div class="space-y-3" data-testid="inline-recipe-add" role="group" aria-label=t_string!(i18n, lists_workspace_add_recipe)>
            <input class="input w-full" aria-label=t_string!(i18n, lists_workspace_search_recipes) placeholder=t_string!(i18n, lists_workspace_recipe_placeholder) prop:value=query on:input=move |ev| query.set(event_target_value(&ev)) />
            <Show when=move || !query.get().trim().is_empty()>
                <div class="max-h-80 overflow-y-auto divide-y divide-[color:var(--color-outline)]" aria-label=t_string!(i18n, lists_workspace_catalog_results)>
                    <Show when=move || results.get().is_empty()><p class="p-3 text-sm">{t!(i18n, lists_workspace_no_recipes)}</p></Show>
                    <For each=move || results.get() key=|(id, _, _)| *id children=move |(id, name, recipe)| {
                        let active = move || selected.get().is_some_and(|current| std::ptr::eq(current, recipe));
                        view! {
                            // The row shows the recipe's finished item; the
                            // small button is the only control, so the icon
                            // and name read as a catalog row, not a button.
                            <div class="flex items-center gap-3 py-2 px-1 rounded" data-recipe-id=id>
                                <ItemIcon item_id=recipe.item_result icon_size=IconSize::Small />
                                <div class="flex-1 min-w-0"><span class="block truncate">{name.clone()}</span><span class="text-xs text-[color:var(--color-text-muted)]">{["CRP", "BSM", "ARM", "GSM", "LTW", "WVR", "ALC", "CUL"].get(recipe.craft_type as usize).copied().unwrap_or_default()}</span></div>
                                <button type="button" class="segmented-option shrink-0" aria-label=t_string!(i18n, lists_recipe_preview_named, name = name.clone()) aria-pressed=move || active().to_string() on:click=move |_| selected.set(Some(recipe))>{t!(i18n, lists_recipe_preview_action)}</button>
                            </div>
                        }
                    } />
                </div>
            </Show>
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
        </div>
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
            <input class="input w-24" type="number" min=if field == 0 { "1" } else { "0" } aria-label=t_string!(i18n, lists_workspace_field_named, label = label.clone(), name = name.clone()) prop:value=move || value.get() data-committed=move || value.get() readonly=move || !can_write.get()
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
pub fn ListWorkspaceModes(
    shop: Signal<bool>,
    set_shop: Callback<bool>,
    /// Sized like a `.sticky-bar-button`, for the account page's control
    /// bar, where the full-height segmented control towered over its
    /// neighbours.
    #[prop(optional)]
    compact: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let group_class = if compact {
        "inline-flex w-fit gap-0.5 rounded-md border border-[color:var(--color-outline)] bg-[color:var(--color-background)] p-0.5"
    } else {
        "segmented-control w-fit"
    };
    let button_class = move |active: bool| match (compact, active) {
        (true, true) => {
            "rounded px-2.5 py-0.5 text-[0.85rem] font-semibold bg-brand-900 text-[color:var(--brand-fg)]"
        }
        (true, false) => {
            "rounded px-2.5 py-0.5 text-[0.85rem] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
        }
        (false, true) => "segmented-option min-w-20",
        (false, false) => "segmented-option min-w-20",
    };
    view! {
        <div class=group_class role="group" aria-label=t_string!(i18n, lists_workspace_mode)>
            <button type="button" class=move || button_class(!shop.get()) data-testid="guest-build-mode" aria-pressed=move || (!shop.get()).to_string() on:click=move |_| set_shop.run(false)>{t!(i18n, lists_workspace_build)}</button>
            <button type="button" class=move || button_class(shop.get()) data-testid="guest-shop-mode" aria-pressed=move || shop.get().to_string() on:click=move |_| set_shop.run(true)>{t!(i18n, lists_workspace_shop)}</button>
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
    /// Changes whenever document undo/redo is requested, including shortcuts.
    pub history_revision: Signal<u64>,
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
    /// Shared Build result, including lookup coverage, also frozen by Shop.
    pub estimate: Signal<ultros_calc::list_estimate::CartEstimate>,
    /// False while no readable document exists; an unavailable document is
    /// not an empty or fully acquired cart and must not display zero cost.
    pub estimate_available: Signal<bool>,
    /// Where the listings in `rows` stand: loading, missing (and why), or
    /// observed at a client-clock instant, possibly marked by a failed
    /// refresh. Drives the estimate's status text; never inferred from a
    /// listing's own timestamp.
    pub market: Signal<PriceFeed>,
    /// The world, datacenter or region the listings in `rows` were served
    /// for — the scope the prices are *for*, not the one currently picked.
    pub scope_name: Signal<Option<String>>,
    pub hide_acquired: Signal<bool>,
    /// The cart's own Hide-acquired toggle. Account lists keep it in the
    /// URL (`hide-acquired`); device lists keep it in memory.
    pub set_hide_acquired: Callback<bool>,
    pub reset_filters: Callback<()>,
    pub can_write: Signal<bool>,
    pub edit: Callback<ListItem>,
    pub remove: Callback<i32>,
    /// Bulk edits from the cart's selection bar; `bulk_pending` disables the
    /// bar while the host's actions are in flight.
    pub remove_many: Callback<Vec<i32>>,
    pub set_quality_many: Callback<(Vec<i32>, Option<bool>)>,
    pub bulk_pending: Signal<bool>,
    /// Row order. Account lists keep it in the URL `sort` param (shared
    /// with the filter row); device lists keep it in memory.
    pub sort: Signal<Option<SortSpec>>,
    pub set_sort: Callback<Option<SortSpec>>,
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
    // The whole cart, not the filtered view: a filter narrows what the grid
    // shows, never what the list will cost. Rows already track the document
    // revision and the listings cache, so quantity, quality, list and market
    // changes all reprice through this one memo.
    let estimate = source.estimate;
    view! {
        <section class="space-y-3" data-testid="list-build-workspace">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=source.add on_add_many=source.add_many recipe_mode=source.recipe_open toggle_recipe=source.toggle_recipe pending=source.pending feedback=source.feedback />
            </Show>
            <input class="input w-full" aria-label=t_string!(i18n, lists_workspace_filter_label) placeholder=t_string!(i18n, lists_workspace_filter_placeholder) prop:value=move || filter.get() data-committed="" on:input=move |ev| filter.set(event_target_value(&ev)) />
            <Show when=move || source.estimate_available.get()>
                <crate::components::list_estimate_summary::ListEstimateSummary estimate feed=source.market scope=source.scope_name />
            </Show>
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
                        // ⚡ Bolt Optimization: Batch O(N) searches into a single Memo and use Signal::derive for projections
                        let row_data = Memo::new(move |_| {
                            source.rows.with(|rows| {
                                rows.iter()
                                    .find(|(item, _)| item.id == id)
                                    .map(|(item, listings)| {
                                        let min_price = listings
                                            .iter()
                                            .filter(|listing| item.hq.is_none_or(|hq| listing.hq == hq))
                                            .map(|listing| listing.price_per_unit)
                                            .min();
                                        (item.clone(), min_price)
                                    })
                            })
                        });
                        let item = Signal::derive(move || row_data.with(|data| data.as_ref().map(|(item, _)| item.clone()).unwrap_or_else(|| fallback.get_value())));
                        let price = Signal::derive(move || row_data.with(|data| data.as_ref().and_then(|(_, price)| *price)));
                        view! { <BuildListRow item=item current_price=price selected_items on_edit=source.edit on_delete=source.remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
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
    /// The scope the document had when this entry was fetched. A scope edit
    /// (`Edit::Rename { scope }`) bumps the document revision but not
    /// `listings_version`, so without this the re-run would be a cache hit
    /// and the page would keep estimating from the old scope's prices.
    requested_scope: Option<AnySelector>,
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

/// Both price loads and silent permission probes may be in flight together.
/// Order their authoritative replies by request start, not arrival time. Only
/// applied replies advance the watermark: starting another slow request must
/// not starve replies that can already refresh the page.
#[derive(Default)]
struct ListResponseOrder {
    next: u64,
    applied: u64,
    /// A stale resource fetch returns this result instead of publishing its
    /// own old permission. Keep the last authoritative reply through outages.
    latest: Option<ListViewResult>,
}

#[derive(Clone, Copy)]
struct ListReads {
    cache: StoredValue<Option<ListingsCache>>,
    responses: StoredValue<ListResponseOrder>,
}

impl ListReads {
    fn new() -> Self {
        Self {
            cache: StoredValue::new(None),
            responses: StoredValue::new(ListResponseOrder::default()),
        }
    }

    fn begin(self) -> u64 {
        let mut request = 0;
        self.responses.update_value(|state| {
            state.next += 1;
            request = state.next;
        });
        request
    }

    /// Call only after checking the route and exact document identity. A 401
    /// or transport failure neither replaces nor invalidates known permission.
    fn accept(self, request: u64, result: &ListViewResult) -> bool {
        let mut accepted = false;
        self.responses.update_value(|state| {
            if request < state.applied {
                return;
            }
            accepted = true;
            if result.is_ok() || result.as_ref().is_err_and(is_denial) {
                state.applied = request;
                state.latest = Some(result.clone());
            }
        });
        accepted
    }

    fn current_result(self, id: i32, handle: Option<ListDocHandle>) -> ListViewResult {
        let (list, items) = self
            .responses
            .with_value(|state| state.latest.clone())
            .unwrap_or_else(|| Err(AppError::ListDoc("no current list response".into())))?;
        if list.list.id != id {
            return Err(AppError::ListDoc("document is no longer active".into()));
        }
        let Some(handle) = handle else {
            return Ok((list, items));
        };
        let listings = items
            .into_iter()
            .map(|(item, listings)| (item.item_id, listings))
            .collect();
        handle
            .with_doc(|doc| crate::list_doc::adapter::view_result(&list, doc, &listings))
            .ok_or_else(|| AppError::ListDoc("document is closed".into()))
    }
}

/// The ids a fetch covered, built from the rows the server returned.
fn covered_ids(items: &[(ListItem, Vec<ActiveListing>)]) -> HashSet<i32> {
    items.iter().map(|(item, _)| item.item_id).collect()
}

/// One successful response: empty item entries are coverage too. Keep the
/// offers and their identities together, fenced to the list that was served.
#[derive(Clone, Debug)]
struct ServedListings {
    list_id: i32,
    listings: HashMap<i32, Vec<ActiveListing>>,
}

/// The estimate's account of its prices, written by every listings fetch.
#[derive(Clone, Copy)]
struct PriceStatus {
    active_list: Memo<i32>,
    feed_list: RwSignal<Option<i32>>,
    feed: RwSignal<PriceFeed>,
    /// The scope the server priced the last successful fetch for.
    served_scope: RwSignal<Option<AnySelector>>,
    served_listings: RwSignal<Option<ServedListings>>,
}

type FetchedPrices<'a> = (&'a ListWithPermission, &'a [(ListItem, Vec<ActiveListing>)]);

/// Where a listings fetch left the price feed and the served scope. Every
/// account-list fetch reports through here so the estimate's freshness is
/// exactly "when the last listings response arrived".
///
/// Client only. The SSR half renders the feed as loading: its `Utc::now()`
/// would be baked into the HTML while the client's signal starts at
/// `Loading` (nothing of the feed is serialized), and the client's first
/// handle-backed run always fetches — its cache starts empty — so the real
/// instant is recorded within the first client tick. A signed-in visitor
/// always gets a handle; an anonymous one cannot read an account list at all.
fn note_fetch(prices: PriceStatus, outcome: Option<FetchedPrices<'_>>) {
    #[cfg(feature = "hydrate")]
    {
        let id = prices.active_list.get_untracked();
        if prices.feed_list.get_untracked() != Some(id) {
            // A failure may retain prices only from this same list. SPA route
            // reuse must never borrow another list's timestamp or scope.
            let _ = prices.feed.try_set(PriceFeed::Loading);
            let _ = prices.served_scope.try_set(None);
            let _ = prices.served_listings.try_set(None);
            let _ = prices.feed_list.try_set(Some(id));
        }
        let fetched_at = outcome.map(|_| chrono::Utc::now());
        let _ = prices
            .feed
            .try_update(|feed| *feed = feed.after_fetch(fetched_at));
        if let Some((list, items)) = outcome {
            let _ = prices.served_scope.try_set(Some(list.list.wdr_filter));
            let _ = prices.served_listings.try_set(Some(ServedListings {
                list_id: list.list.id,
                listings: items
                    .iter()
                    .map(|(item, rows)| (item.item_id, rows.clone()))
                    .collect(),
            }));
        }
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = (
        prices.active_list,
        prices.feed_list,
        prices.feed,
        prices.served_scope,
        prices.served_listings,
        outcome,
    );
}

/// How long a burst of relayed list broadcasts is allowed to coalesce into
/// a single revalidation. Every row edit on a shared list comes back to us
/// as a `ListItem` broadcast, and the Labs page already has those rows from
/// its document — the revalidation exists only so a *permission* change
/// (Global Constraint 2) is noticed by an idle page, so paying one REST
/// fetch per remote keystroke would be pure waste.
#[cfg(any(feature = "hydrate", test))]
const REVALIDATE_MAX_WAIT_MS: u32 = 1000;

/// Owned client timer; dropping its document lifetime cancels it.
#[cfg(feature = "hydrate")]
type RevalidateTimer = gloo_timers::callback::Timeout;

/// Keep the first broadcast's deadline: subsequent broadcasts coalesce but
/// cannot postpone it. In a normally scheduled foreground tab the probe starts
/// within one second of that first broadcast; network latency is additional.
/// Browser suspension/background timer throttling can delay execution.
#[cfg(any(feature = "hydrate", test))]
fn schedule_revalidate_with<T: 'static>(
    slot: &Rc<RefCell<Option<T>>>,
    delay_ms: u32,
    create_timer: impl FnOnce(u32, Box<dyn FnOnce()>) -> T,
    probe: impl FnOnce() + 'static,
) {
    if slot.borrow().is_some() {
        return;
    }
    // Do not make slot -> timer -> slot a cycle: dropping the subscription
    // must still cancel its pending timer on navigation or sign-out.
    let pending = Rc::downgrade(slot);
    let timer = create_timer(
        delay_ms,
        Box::new(move || {
            let Some(pending) = pending.upgrade() else {
                return;
            };
            // Release ownership before invoking the probe, so later updates
            // can start a new window instead of seeing a fired timer forever.
            pending.borrow_mut().take();
            probe();
        }),
    );
    *slot.borrow_mut() = Some(timer);
}

// One recovery owner per exact document lifetime. Socket handlers and timers
// only hold Weak references; dropping the owner cancels retries and makes late
// REST completions unable to restore a successor's subscriptions.
#[cfg(any(feature = "hydrate", test))]
#[derive(Default)]
struct RelayRecoveryState {
    notices: [u64; 2],
    restored: [u64; 2],
    attempts: u32,
    activity: bool,
}

#[cfg(any(feature = "hydrate", test))]
impl RelayRecoveryState {
    fn notice(&mut self, relay: Option<usize>) {
        if let Some(relay) = relay {
            self.notices[relay] += 1;
        }
        self.activity = true;
    }

    fn pending(&self) -> bool {
        self.notices != self.restored
    }

    fn delay_ms(&self) -> u32 {
        if self.pending() {
            (1u32 << self.attempts.min(5)).min(30) * REVALIDATE_MAX_WAIT_MS
        } else {
            REVALIDATE_MAX_WAIT_MS
        }
    }

    fn begin(&mut self) -> [u64; 2] {
        self.activity = false;
        if self.pending() {
            self.attempts = self.attempts.saturating_add(1);
        }
        self.notices
    }

    fn finish(&mut self, captured: [u64; 2], restored: [bool; 2]) {
        for relay in 0..2 {
            // Probes may overlap; an older completion must not roll back a
            // newer one's restoration and re-arm recovery for nothing.
            if restored[relay] && captured[relay] > self.restored[relay] {
                self.restored[relay] = captured[relay];
            }
        }
        if !self.pending() {
            self.attempts = 0;
        }
    }
}

#[cfg(feature = "hydrate")]
type RelayProbe = dyn Fn(Box<dyn FnOnce(bool)>);

#[cfg(feature = "hydrate")]
struct ListRelayRecovery {
    state: RefCell<RelayRecoveryState>,
    timer: Rc<RefCell<Option<RevalidateTimer>>>,
    current: Box<dyn Fn() -> bool>,
    probe: Box<RelayProbe>,
    restore: Box<dyn Fn(usize) -> bool>,
}

#[cfg(feature = "hydrate")]
impl ListRelayRecovery {
    fn notice(self: &Rc<Self>, relay: Option<usize>) {
        if !(self.current)() {
            return;
        }
        self.state.borrow_mut().notice(relay);
        self.schedule();
    }

    fn schedule(self: &Rc<Self>) {
        let state = self.state.borrow();
        // Never gate on an outstanding probe: a held, hung or lost REST
        // response must not stop the one-second access window that #1473
        // guarantees under sustained broadcasts. `schedule_revalidate_with`
        // coalesces bursts onto the first pending deadline and `ListReads`
        // fences stale responses, so overlapping probes are safe.
        if !(state.activity || state.pending()) || !(self.current)() {
            return;
        }
        let delay = state.delay_ms();
        drop(state);
        let weak = Rc::downgrade(self);
        schedule_revalidate_with(
            &self.timer,
            delay,
            gloo_timers::callback::Timeout::new,
            move || {
                let Some(owner) = weak.upgrade().filter(|owner| (owner.current)()) else {
                    return;
                };
                let captured = owner.state.borrow_mut().begin();
                let weak = Rc::downgrade(&owner);
                (owner.probe)(Box::new(move |success| {
                    let Some(owner) = weak.upgrade().filter(|owner| (owner.current)()) else {
                        return;
                    };
                    let mut restored = [false; 2];
                    if success {
                        for relay in 0..2 {
                            let needed = captured[relay] != owner.state.borrow().restored[relay];
                            if needed {
                                // We are past dispatch and the REST identity/watermark fence.
                                // Resend surviving factories, preserving native rebase guards.
                                restored[relay] = (owner.restore)(relay);
                            }
                        }
                    }
                    owner.state.borrow_mut().finish(captured, restored);
                    owner.schedule();
                }));
            },
        );
    }
}

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
    reads: ListReads,
    bump: WriteSignal<u32>,
    prices: PriceStatus,
    finished: impl FnOnce(bool) + 'static,
) {
    let cache = reads.cache;
    let expected = handle.try_get_untracked().flatten().map(|h| h.revision);
    if !request_is_current(active_list, list_id, handle, expected) {
        return;
    }
    leptos::task::spawn_local(async move {
        if !request_is_current(active_list, list_id, handle, expected) {
            return;
        }
        let request = reads.begin();
        let result = get_list_items_with_listings(list_id).await;
        if !request_is_current(active_list, list_id, handle, expected) {
            return;
        }
        if !reads.accept(request, &result) {
            finished(false);
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
                note_fetch(prices, Some((&list, &items)));
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
                // A revoke racing the server's separate data/permission reads
                // can produce 200 with None. It disables UI access but does not
                // prove a surviving relay is authorized; retry without purging.
                finished(permission >= ListPermission::Read);
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
            // a terminated relay retries without needing another broadcast. It is
            // though, so the prices on the page are marked as such.
            Err(_) => {
                note_fetch(prices, None);
                finished(false);
            }
        }
    });
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
/// The connection clause of the shell's status line, from the handle's (or
/// the socket's) status keyword.
fn live_status_label(i18n: leptos_i18n::I18nContext<Locale, I18nKeys>, status: &str) -> String {
    match status {
        "live" => t_string!(i18n, list_view_live_status_live).to_string(),
        "reconnecting" => t_string!(i18n, list_view_live_status_reconnecting).to_string(),
        "offline" => t_string!(i18n, list_view_live_status_offline).to_string(),
        _ => t_string!(i18n, list_view_live_status_connecting).to_string(),
    }
}

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
    reads: ListReads,
    listings_version: u32,
    revalidate_version: u32,
    prices: PriceStatus,
) -> ListViewResult {
    let cache = reads.cache;
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
        let request = reads.begin();
        let result = get_list_items_with_listings(id).await;
        if !request_is_current(list_id, id, handle, expected) {
            return Err(stale());
        }
        if !reads.accept(request, &result) {
            return reads.current_result(id, None);
        }
        note_fetch(
            prices,
            result
                .as_ref()
                .ok()
                .map(|(list, items)| (list, items.as_slice())),
        );
        if let Ok((list, items)) = &result {
            cache.set_value(Some(ListingsCache {
                list_id: id,
                version: listings_version,
                revalidate: revalidate_version,
                // The document, once open, carries the server's scope unless
                // an offline edit changed it — in which case the mismatch
                // below is exactly the refetch that edit deserves.
                requested_scope: Some(list.list.wdr_filter),
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
    // A document that has never recorded a scope prices against whatever
    // the server scoped the cached listings to; one that has must match.
    let wanted_scope = doc_handle.meta().scope;
    let cached = cache.get_value().filter(|c| {
        c.list_id == id
            && c.version == listings_version
            && c.revalidate == revalidate_version
            && wanted_ids.is_subset(&c.covered)
            && wanted_scope.is_none_or(|scope| c.requested_scope == Some(scope))
    });
    let base = match cached {
        Some(cached) => cached,
        None => {
            let request = reads.begin();
            let result = get_list_items_with_listings(id).await;
            if !request_is_current(list_id, id, handle, expected) {
                return Err(stale());
            }
            if !reads.accept(request, &result) {
                return reads.current_result(id, Some(doc_handle));
            }
            match result {
                Ok((list, items)) => {
                    doc_handle.remember_permission(list.permission as i16);
                    let covered = covered_ids(&items);
                    note_fetch(prices, Some((&list, &items)));
                    let fresh = ListingsCache {
                        list_id: id,
                        version: listings_version,
                        revalidate: revalidate_version,
                        requested_scope: wanted_scope.or(Some(list.list.wdr_filter)),
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
                    note_fetch(prices, None);
                    return Err(error);
                }
                Err(error) => {
                    // The refresh failed. Whatever was observed before stays
                    // on the page, marked; with nothing observed, the
                    // estimate reports that prices are unavailable. Either
                    // way the document keeps rendering and editing.
                    note_fetch(prices, None);
                    match cache.get_value().filter(|c| c.list_id == id) {
                        // Stale prices beat no page.
                        Some(stale) => stale,
                        None => match offline_list(id, doc_handle) {
                            Some(list) => ListingsCache {
                                list_id: id,
                                version: listings_version,
                                revalidate: revalidate_version,
                                requested_scope: wanted_scope,
                                list,
                                listings: HashMap::new(),
                                covered: wanted_ids.clone(),
                            },
                            None => return Err(error),
                        },
                    }
                }
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
    let history_revision = RwSignal::new(0_u64);
    let undo_action = Callback::new(move |()| {
        history_revision.update(|revision| *revision += 1);
        let done = handle.get_untracked().is_some_and(|handle| handle.undo());
        if !done {
            add_item.value().set(None);
            mutation_feedback.set(t_string!(i18n, lists_workspace_nothing_to_undo).to_string());
        }
    });
    let redo_action = Callback::new(move |()| {
        history_revision.update(|revision| *revision += 1);
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
        async move {
            let result = apply_edit(handle, edit);
            if let Err(error) = &result {
                mutation_feedback.set(workspace_error(i18n, error));
            }
            result
        }
    });
    let bulk_pending =
        Signal::derive(move || delete_items.pending().get() || edit_items_hq.pending().get());

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
    #[cfg(not(feature = "hydrate"))]
    let _ = (set_activity_update_version, set_revalidate_version);
    let (listings_version, set_listings_version) = signal(0u32);
    // Only written: the market and sync callbacks note the instant so a
    // later reader (none on the page today) can show "updated N s ago".
    let (_last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
    let listings_cache = ListReads::new();
    // The estimate's account of its prices (see `note_fetch`): every fetch
    // above reports here, so "prices fetched 2 minutes ago" is the arrival
    // of the last listings response and nothing else.
    let price_feed = RwSignal::new(PriceFeed::Loading);
    let served_scope = RwSignal::new(None::<AnySelector>);
    let served_listings = RwSignal::new(None::<ServedListings>);
    let feed_list = RwSignal::new(None::<i32>);
    let prices = PriceStatus {
        active_list: list_id,
        feed_list,
        feed: price_feed,
        served_scope,
        served_listings,
    };

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
                prices,
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

    #[cfg(feature = "hydrate")]
    let relay_recovery: StoredValue<Option<Rc<ListRelayRecovery>>, LocalStorage> =
        StoredValue::new_local(None);
    #[cfg(feature = "hydrate")]
    let sync_subscription: StoredValue<
        Option<crate::list_doc::sync::SyncSubscription>,
        LocalStorage,
    > = StoredValue::new_local(None);

    // Activity and document relay failures share the same authoritative REST
    // probe. A relay error is ambiguous: database trouble uses the same text
    // as revoked access. Only the REST 403/404 may destroy cached work.
    #[cfg(feature = "hydrate")]
    {
        let realtime_for_activity = realtime.clone();
        Effect::new(move |_| {
            relay_recovery.set_value(None);
            activity_subscription.set_value(None);
            let id = list_id.get();
            // Account readers, including read-only collaborators, have a live
            // document handle. Wait for it initially and stay unsubscribed after
            // denial/sign-out; a None-handle relay would repeatedly re-open itself.
            let Some(document) = handle.get().filter(|doc| !doc.is_closed_or_disposed()) else {
                return;
            };
            let expected = Some(document.revision);
            let Some(realtime) = realtime_for_activity.clone().filter(|_| id != 0) else {
                return;
            };
            let recovery = Rc::new(ListRelayRecovery {
                state: RefCell::new(RelayRecoveryState::default()),
                timer: Rc::new(RefCell::new(None)),
                current: Box::new(move || request_is_current(list_id, id, handle, expected)),
                probe: Box::new(move |finished| {
                    revalidate(
                        list_id,
                        id,
                        handle,
                        listings_cache,
                        set_revalidate_version,
                        prices,
                        finished,
                    );
                }),
                restore: Box::new(move |relay| {
                    if !request_is_current(list_id, id, handle, expected) {
                        return false;
                    }
                    if relay == 0 {
                        activity_subscription.with_value(|sub| {
                            sub.as_ref().is_some_and(RealtimeSubscription::resubscribe)
                        })
                    } else {
                        sync_subscription.with_value(|sub| {
                            sub.as_ref()
                                .is_some_and(crate::list_doc::sync::SyncSubscription::resubscribe)
                        })
                    }
                }),
            });
            let weak = Rc::downgrade(&recovery);
            let sub = realtime.subscribe_list(id, move |message| {
                let Some(recovery) = weak.upgrade() else {
                    return;
                };
                match message {
                    ServerClient::ListUpdate(event) => {
                        if matches!(event, WEvent::Added(ListEventData::Activity(_))) {
                            set_activity_update_version.update(|v| *v += 1);
                        }
                        recovery.notice(None);
                    }
                    ServerClient::Error { message }
                        if crate::list_doc::sync::is_relay_authorization_error(&message, id) =>
                    {
                        recovery.notice(Some(0));
                    }
                    _ => {}
                }
            });
            activity_subscription.set_value(Some(sub));
            relay_recovery.set_value(Some(recovery));
        });
        on_cleanup(move || relay_recovery.set_value(None));
    }
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

    let (make_place_open, set_make_place_open) = signal(false);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);
    let (subscribe_open, set_subscribe_open) = signal(false);
    let (access_open, set_access_open) = signal(false);
    // The shell's ⋮ menu (Subscribe, import, auto-mark, recovery copy,
    // leave, delete). A dialog for the undo keybindings like the others.
    let menu_open = RwSignal::new(false);
    let confirm_delete = RwSignal::new(false);

    // ---- The document's lifecycle (spec sections 3.1, 3.3, 5) ----
    //
    // Both Effects below are hydrate-only: `ListDocHandle::open` builds
    // `StoredValue::new_local`s that must never exist on the SSR half
    // (#1332), and `SyncSubscription` owns `Rc`s, so it can only live in a
    // thread-local slot.
    let modal_open = Signal::derive(move || {
        subscribe_open() || menu_open.get() || access_open() || make_place_open()
    });
    let (resync, set_resync) = signal(0u32);
    #[cfg(feature = "hydrate")]
    {
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
                move || {
                    if !request_is_current(list_id, open.list_id, handle, Some(open.revision)) {
                        return;
                    }
                    relay_recovery.with_value(|recovery| {
                        if let Some(recovery) = recovery {
                            recovery.notice(Some(1));
                        }
                    });
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
    // Latches once Shop has been opened; see the Shop mount below.
    let shop_mounted = Memo::new(move |previous: Option<&bool>| {
        buying_view.get() || previous.copied().unwrap_or(false)
    });

    // Still honoured from shared links; narrowing worlds is Shop's job now,
    // so nothing on this page writes these two params.
    let (excluded_worlds_param, _) = filter_query_signal::<IdList>("excluded-worlds");
    let excluded_worlds = Memo::new(move |_| {
        excluded_worlds_param
            .get()
            .map(|list| list.0.into_iter().collect::<HashSet<i32>>())
            .unwrap_or_default()
    });
    let (excluded_datacenters_param, _) = filter_query_signal::<NameList>("excluded-datacenters");
    let excluded_datacenters = Memo::new(move |_| {
        excluded_datacenters_param
            .get()
            .map(|list| list.0.into_iter().collect::<HashSet<String>>())
            .unwrap_or_default()
    });
    // Editor-owned travel limit (#1480): narrows the served rows both Build
    // and Shop price, and travels with the URL like the exclusions above.
    let travel = crate::components::list_travel_state::use_list_travel();

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

    let view_caps = RwSignal::new(ListCapabilities::default());
    Effect::new(move |_| {
        let next = match list_view.get() {
            Some(Ok((list_with_perm, _)))
                if list_with_perm.list.id == list_id.get()
                    && handle.get().is_none_or(|doc| {
                        doc.list_id == list_id.get() && doc.has_readable_content()
                    }) =>
            {
                ListCapabilities::from(list_with_perm.permission)
            }
            _ => ListCapabilities::default(),
        };
        view_caps.set(next);
    });

    let legacy_cart = use_legacy_cart();
    // The cart sorts its own rows (so an active editor can pin its row);
    // only the legacy grid still expects them pre-sorted.
    let build_rows = Signal::derive(move || {
        let snapshot = list_view
            .get()
            .and_then(Result::ok)
            .filter(|(list, _)| list.list.id == list_id.get())
            .map(|(_, rows)| rows)
            .unwrap_or_default();
        let snapshot = if let Some(doc) = handle.get().filter(|doc| doc.list_id == list_id.get()) {
            doc.revision.track();
            // Revalidation can deliver fresh offers without rerunning the
            // page resource. Read the exact response that owns coverage.
            let listings = served_listings
                .get()
                .filter(|prices| prices.list_id == list_id.get())
                .map(|prices| prices.listings)
                .unwrap_or_else(|| {
                    snapshot
                        .into_iter()
                        .map(|(item, rows)| (item.item_id, rows))
                        .collect()
                });
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
        let travel_policy = travel.policy.get();
        if travel_policy.narrows() {
            rows = travel_policy.filter_rows(&rows);
        }
        if legacy_cart.get()
            && let Some(spec) = sort_spec.get()
        {
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
        history_revision: history_revision.into(),
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
        estimate: Memo::new(move |_| {
            let fetched = served_listings.with(|served| {
                served
                    .as_ref()
                    .filter(|prices| prices.list_id == list_id.get())
                    .map(|prices| prices.listings.keys().copied().collect())
                    .unwrap_or_default()
            });
            build_rows.with(|rows| {
                ultros_calc::list_estimate::estimate_list_items_with_coverage(rows, &fetched)
            })
        })
        .into(),
        estimate_available: Signal::derive(move || {
            if let Some(doc) = handle.get() {
                if doc.list_id != list_id.get() || !doc.has_readable_content() {
                    return false;
                }
                doc.recovery_state.track();
                !doc.incompatible()
                    && ListPermission::from(doc.permission.get()) != ListPermission::None
            } else {
                matches!(list_view.get(), Some(Ok((list, _))) if list.list.id == list_id.get())
            }
        }),
        market: Signal::derive(move || {
            if feed_list.get() == Some(list_id.get()) {
                price_feed.get()
            } else {
                PriceFeed::Loading
            }
        }),
        scope_name: Signal::derive(move || {
            if feed_list.get() != Some(list_id.get()) {
                return None;
            }
            let scope = served_scope.get()?;
            let helper = use_context::<LocalWorldData>()?.0.ok()?;
            helper
                .lookup_selector(scope)
                .map(|result| result.get_name().to_string())
        }),
        hide_acquired: hide_acquired.into(),
        set_hide_acquired: Callback::new(move |hide: bool| {
            set_hide_acquired_param.set(hide.then_some(true))
        }),
        reset_filters: Callback::new(move |()| set_hide_acquired_param.set(None)),
        can_write: Signal::derive(move || view_caps.with(|c| c.can_write)),
        edit: Callback::new(move |item| {
            edit_item.dispatch(item);
        }),
        remove: Callback::new(move |id| {
            delete_item.dispatch(id);
        }),
        remove_many: Callback::new(move |ids| {
            delete_items.dispatch(ids);
        }),
        set_quality_many: Callback::new(move |(ids, hq)| {
            edit_items_hq.dispatch((ids, hq));
        }),
        bulk_pending,
        sort: Signal::derive(move || sort_spec.get()),
        set_sort: Callback::new(move |spec| set_sort_spec.set(spec)),
    };

    // Auto-mark logic moved to AutoMarkPurchases component

    // The document's own name once it is open (so a rename shows at once),
    // otherwise the server's.
    let list_name = Signal::derive(move || {
        handle
            .get()
            .filter(|doc| doc.list_id == list_id.get() && !doc.is_closed_or_disposed())
            .map(|doc| {
                doc.revision.track();
                doc.meta().name
            })
            .or_else(|| {
                list_view
                    .get()
                    .and_then(Result::ok)
                    .filter(|(list, _)| list.list.id == list_id.get())
                    .map(|(list, _)| list.list.name)
            })
            .unwrap_or_default()
    });
    let list_name_for_meta = list_name;
    let (home_world, _) = crate::global_state::home_world::use_home_world();
    // The scope the document is priced for: the document's own once it has
    // recorded one, otherwise the server's. Read through the handle so the
    // picker only ever exists on the client (see the markup below).
    let list_scope = Signal::derive(move || {
        let doc = handle
            .get()
            .filter(|doc| doc.list_id == list_id.get() && !doc.is_closed_or_disposed())?;
        doc.revision.track();
        doc.meta().scope.or_else(|| {
            list_view
                .get()
                .and_then(Result::ok)
                .filter(|(list, _)| list.list.id == list_id.get())
                .map(|(list, _)| list.list.wdr_filter)
        })
    });
    let set_list_scope = move |scope: Option<AnySelector>| {
        let Some(scope) = scope else {
            return;
        };
        if let Err(error) = apply_edit(handle, Edit::SetScope(scope)) {
            mutation_feedback.set(workspace_error(i18n, &error));
        }
    };
    let rename = Callback::new(move |value: String| {
        let name = value.trim().to_string();
        if name.is_empty() {
            return;
        }
        let Some(scope) = list_scope.get_untracked() else {
            return;
        };
        if let Err(error) = apply_edit(handle, Edit::Rename { name, scope }) {
            mutation_feedback.set(workspace_error(i18n, &error));
        }
    });
    let can_write = Signal::derive(move || view_caps.with(|c| c.can_write));
    let can_admin = Signal::derive(move || view_caps.with(|c| c.can_admin));
    let can_leave = Signal::derive(move || view_caps.with(|c| c.can_leave));
    let open_doc = move || {
        handle
            .get()
            .filter(|doc| doc.list_id == list_id.get() && !doc.is_closed_or_disposed())
    };
    // One status line: where the local copy stands. An incompatible
    // snapshot says nothing here; the recovery panel explains it instead.
    let doc_status = Signal::derive(move || {
        let Some(doc) = open_doc() else {
            return String::new();
        };
        if doc.recovery_state.try_get() == Some(RecoveryState::Incompatible) {
            return String::new();
        }
        match doc.save_state.try_get().unwrap_or(SaveState::Pending) {
            SaveState::Saved => t_string!(i18n, lists_online_local_saved).to_string(),
            SaveState::Pending => t_string!(i18n, lists_online_local_saving).to_string(),
            SaveState::Failed => t_string!(i18n, account_list_save_failed).to_string(),
        }
    });
    let save_failed = Signal::derive(move || {
        open_doc().is_some_and(|doc| {
            doc.save_state.try_get() == Some(SaveState::Failed)
                && doc.recovery_state.try_get() != Some(RecoveryState::Incompatible)
        })
    });
    let live_status = Signal::derive(move || live_status_label(i18n, &realtime_status.get()));
    let navigate = use_navigate();
    let delete_action = Action::new(move |_: &()| {
        let id = list_id.get_untracked();
        async move { crate::api::delete_list(id).await }
    });
    let leave_action = Action::new(move |user_id: &u64| {
        let id = list_id.get_untracked();
        let user_id = *user_id;
        async move { crate::api::leave_list(id, user_id).await }
    });
    Effect::new(move |_| {
        if matches!(delete_action.value().get(), Some(Ok(_)))
            || matches!(leave_action.value().get(), Some(Ok(_)))
        {
            navigate("/list?labs=lists-sync", Default::default());
        }
    });
    let danger_error = move || {
        delete_action
            .value()
            .get()
            .and_then(|result| result.err())
            .or_else(|| leave_action.value().get().and_then(|result| result.err()))
            .map(|error| workspace_error(i18n, &error))
    };
    let shop_worlds = use_context::<LocalWorldData>().and_then(|data| data.0.ok());
    let shop_input = Signal::derive(move || {
        use crate::components::list_shop::{ShopInput, ShopRow};
        let server_list = list_view
            .get()
            .and_then(Result::ok)
            .filter(|(list, _)| list.list.id == list_id.get())
            .map(|(list, _)| list.list);
        let local_meta = handle
            .get()
            .filter(|doc| doc.list_id == list_id.get() && !doc.is_closed_or_disposed())
            .map(|handle| {
                handle.revision.track();
                handle.meta()
            });
        let title = local_meta
            .as_ref()
            .map(|meta| meta.name.clone())
            .or_else(|| server_list.as_ref().map(|list| list.name.clone()))
            .unwrap_or_default();
        let rows = build_source.rows.get();
        let mut result = ShopInput {
            title,
            price_feed: build_source.market.get(),
            build_estimate: build_source.estimate.get(),
            estimate_available: build_source.estimate_available.get(),
            home_world: home_world.get().map(|world| world.id).unwrap_or_default(),
            ..Default::default()
        };
        if let Some(helper) = shop_worlds.as_ref() {
            // Labels describe the actual served offers, including stale ones
            // retained after a desired-scope change. Never narrow metadata by
            // the new, not-yet-served document scope.
            for world in helper.iter().filter_map(|world| world.as_world()) {
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
            // The same frame as a device list: name, one status line, one
            // primary action (Access) and a ⋮ menu, Build / Shop, then the
            // price row. The scope picker exists on the client alone (the
            // document that carries it does), so the first server render
            // matches the hydrating client: neither shows it.
            <ListWorkspaceShell
                name=list_name
                can_rename=Signal::derive(move || can_admin.get() && open_doc().is_some())
                on_rename=rename
                status=doc_status
                status_testid="account-list-save-state"
                status_detail=live_status
                status_detail_key=realtime_status
                status_actions=move || view! {
                    <Show when=move || save_failed.get()>
                        <button type="button" class="btn-secondary" on:click=move |_| { if let Some(doc) = handle.get_untracked() { doc.save_now(); } }>{t!(i18n, account_list_save_retry)}</button>
                        <button type="button" class="btn-secondary" on:click=move |_| { if let Some(doc) = handle.get_untracked() { doc.download_recovery(); } }>{t!(i18n, account_list_save_export)}</button>
                    </Show>
                }
                primary=move || view! {
                    <Show when=move || can_admin.get()>
                        <button type="button" class="btn-secondary inline-flex items-center gap-2" data-testid="list-access-btn" aria-haspopup="dialog" on:click=move |_| set_access_open(true)>
                            <Icon icon=i::BiShareAltRegular aria_hidden=true />
                            {t!(i18n, online_access)}
                        </button>
                    </Show>
                }
                menu_testid="list-settings-btn"
                menu_open
                menu=move || view! {
                    <div class="list-action-menu gap-2">
                        <button type="button" class="btn-secondary inline-flex items-center gap-2" data-testid="list-subscribe-btn" on:click=move |_| { menu_open.set(false); set_subscribe_open(true); }>
                            <Icon icon=i::BsBell aria_hidden=true />
                            {t!(i18n, list_view_subscribe_button)}
                        </button>
                        <Show when=move || can_write.get()>
                            <button type="button" class="btn-secondary inline-flex items-center gap-2" data-testid="list-make-place-btn" on:click=move |_| { menu_open.set(false); set_make_place_open(true); }>
                                <Icon icon=i::BiImportRegular aria_hidden=true />
                                {t!(i18n, list_view_make_place)}
                            </button>
                            <AutoMarkPurchases
                                list_view=list_view
                                button_class="btn-secondary inline-flex items-center gap-2"
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
                        </Show>
                        <Show when=move || open_doc().is_some()>
                            <button type="button" class="btn-secondary" data-testid="list-download-recovery" on:click=move |_| { if let Some(doc) = handle.get_untracked() { doc.download_recovery(); } }>{t!(i18n, account_list_save_export)}</button>
                        </Show>
                    </div>
                    <Show when=move || can_admin.get() || can_leave.get()>
                        <div class="space-y-3 border-t border-[color:var(--color-outline)] pt-3" data-testid="list-danger-zone">
                            <p class="text-sm font-semibold text-negative">{t!(i18n, list_view_settings_danger_zone)}</p>
                            <div class="flex flex-wrap gap-2">
                                <Show when=move || can_leave.get()>
                                    <button type="button" class="btn-secondary inline-flex items-center gap-2" data-testid="list-leave-btn" disabled=move || leave_action.pending().get() on:click=move |_| { if let Some(user) = self_user_id.get_untracked() { leave_action.dispatch(user); } }>
                                        <Icon icon=i::BiExitRegular aria_hidden=true />
                                        {t!(i18n, leave_list)}
                                    </button>
                                </Show>
                                <Show when=move || can_admin.get()>
                                    <button type="button" class="btn-danger inline-flex items-center gap-2" data-testid="list-delete-btn" on:click=move |_| confirm_delete.set(true)>
                                        <Icon icon=i::BiTrashSolid aria_hidden=true />
                                        {t!(i18n, delete)}
                                    </button>
                                </Show>
                            </div>
                            <Show when=move || confirm_delete.get() && can_admin.get()>
                                <div class="rounded-lg border border-red-400/50 p-4 space-y-3" role="group" aria-label=move || t_string!(i18n, guest_workspace_delete_label).to_string()>
                                    <p>{t!(i18n, guest_workspace_delete_confirm)}</p>
                                    <div class="flex flex-wrap gap-2">
                                        <button type="button" class="btn-secondary" disabled=move || delete_action.pending().get() on:click=move |_| confirm_delete.set(false)>{t!(i18n, guest_workspace_keep)}</button>
                                        <button type="button" class="btn-danger" data-testid="list-confirm-delete" disabled=move || delete_action.pending().get() on:click=move |_| { delete_action.dispatch(()); }>{t!(i18n, guest_workspace_delete_permanent)}</button>
                                    </div>
                                </div>
                            </Show>
                            {move || danger_error().map(|error| view! { <p role="alert" class="text-sm text-negative">{error}</p> })}
                        </div>
                    </Show>
                }
                // Recovery belongs to the active document in either mode. An
                // incompatible snapshot intentionally makes estimates unavailable,
                // so readiness must not hide its explanation and original export.
                notices=move || view! {
                    {move || open_doc().map(|doc| view! { <crate::list_doc::recovery_ui::ListRecovery doc=doc /> })}
                }
                show_controls=Signal::derive(|| true)
                shop=buying_view
                set_shop=Callback::new(move |shop: bool| set_buying_view_param.set(shop.then_some(true)))
                scope=list_scope
                set_scope=Callback::new(set_list_scope)
                can_set_scope=Signal::derive(move || can_admin.get() && open_doc().is_some())
                history=move || view! { <crate::components::list_workspace_shell::ListHistoryControls source=build_source /> }
                price_row_testid="list-price-controls"
                travel
            >
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

                // Shop mounts the first time it is opened and then stays mounted
                // but hidden, so returning to Build keeps the chosen trip, its
                // recorded stacks and an open companion. The first server render
                // is unchanged: without `?buy=true` nothing is mounted yet.
                <div class:hidden=move || !buying_view.get()>
                    <Show when=move || shop_mounted.get()>
                        <Suspense fallback=move || view! { <Loading /> }>
                            <crate::components::list_shop::ListShop input=shop_input
                                on_purchase=Callback::new(move |(key, delta): (String, i32)| {
                                    if !view_caps.with_untracked(|c| c.can_write) { return; }
                                    if let Ok(id) = key.parse::<i32>()
                                        && let Some(key) = crate::list_doc::adapter::key_of(id)
                                        && let Some(handle) = handle.get_untracked()
                                        && let Err(error) = handle.apply(Edit::RecordPurchase { key, quantity: i64::from(delta.max(0)) })
                                    { mutation_feedback.set(workspace_error(i18n, &AppError::ListDoc(error.to_string()))); }
                                })
                                on_undo=Callback::new(move |()| {
                                    if let Some(handle) = handle.get_untracked()
                                        && let Err(error) = handle.apply(Edit::UndoPurchase) {
                                        mutation_feedback.set(workspace_error(i18n, &AppError::ListDoc(error.to_string())));
                                    }
                                })
                                can_undo_purchase=Signal::derive(move || handle.get().is_some_and(|h| h.can_undo_purchase()))
                                can_edit=Signal::derive(move || view_caps.with(|c| c.can_write))
                                travel_policy=travel.policy />
                        </Suspense>
                    </Show>
                </div>

                <Show when=make_place_open>
                    <Modal set_visible=set_make_place_open aria_label=Signal::derive(move || t_string!(i18n, list_view_make_place).to_string())>
                        <div class="space-y-3">
                            <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, list_view_make_place)}</h2>
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
                        </div>
                    </Modal>
                </Show>

                <div class:hidden=move || buying_view.get()>
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
                        {move || list_view.get().and_then(|result| result.err()).map(|e| view! {
                            <div class="panel rounded-lg p-4">{format!("{}\n{}", t_string!(i18n, list_view_failed_to_get_items), workspace_error(i18n, &e))}</div>
                        })}
                        {move || if legacy_cart.get() {
                            view! { <ListBuildWorkspace source=build_source selected_items highlighted=Signal::derive(move || recently_changed.get()) /> }.into_any()
                        } else {
                            view! { <ListCart source=build_source selected_items highlighted=Signal::derive(move || recently_changed.get()) /> }.into_any()
                        }}
                        // The compact cart owns its remaining-unit estimate. Only
                        // the legacy grid uses the whole-stack per-world summary.
                        <Show when=move || legacy_cart.get()>
                            <div class="panel rounded-lg p-4 mt-3">
                                {move || list_view.get().and_then(Result::ok).map(|(_, items)| view! { <ListSummary items excluded_worlds=&[] excluded_datacenters /> })}
                            </div>
                        </Show>
                        // Collaboration history, closed by default: present
                        // without competing with the cart.
                        <details class="panel rounded-lg p-4 mt-3" data-testid="list-activity">
                            <summary class="cursor-pointer text-lg font-bold text-[color:var(--brand-fg)]">{t!(i18n, list_view_activity_heading)}</summary>
                            <div class="pt-3">
                                <ActivityFeed activity=activity_view show_heading=false />
                            </div>
                        </details>
                    </Transition>
                </div>

                <Show when=move || access_open() && can_admin.get()>
                    // Capture the list only when this guarded dialog opens. A
                    // document or price refresh must not remount its live form.
                    // The outer capability guard still dismisses it on denial.
                    {move || list_view.get_untracked().and_then(Result::ok).map(|(list,_)|view! {
                        <crate::components::list::share_list_modal::ShareListModal list=list.list set_visible=set_access_open />
                    })}
                </Show>
            </ListWorkspaceShell>
        </div>
    }.into_any()
}

/// Picks the page for `/list/:id`. The `LABS` cookie is server-visible, so
/// the server and the hydrating client make the same choice. The id is
/// tracked so moving between lists builds a fresh page and document.
#[component]
pub fn ListRoute() -> impl IntoView {
    // Decided once per mount, not tracked: the Labs flag only changes
    // together with a route change (every `?labs=` link targets another
    // route, and the cookie is set on the Labs page). Tracking it made the
    // route view rebuild a whole list page while the router was already
    // leaving `/list/:id` for a URL that carried the flag; that page's owner
    // was disposed at once, but its `<Title>` stayed on leptos_meta's stack
    // and panicked the client on the next navigation (GlitchTip #7389).
    let sync = use_lab(LAB_LISTS_SYNC).get_untracked();
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|p| p.get("id").unwrap_or_default()));
    move || match list_route_view(id.try_get(), sync) {
        ListRouteView::Sync => view! { <ListViewSync /> }.into_any(),
        ListRouteView::Legacy => view! { <ListView /> }.into_any(),
        ListRouteView::Leaving => ().into_any(),
    }
}

/// Which page the `/list/:id` route builds for the URL it currently sees.
#[derive(Debug, PartialEq, Eq)]
enum ListRouteView {
    Sync,
    Legacy,
    /// The router is on its way out of this route: its params already
    /// describe the next match (no `id`), or the route's own memo is gone.
    /// Rebuilding a list page here is wasted work at best; at worst it
    /// registers a `<Title>` whose closure outlives the page.
    Leaving,
}

fn list_route_view(id: Option<String>, sync: bool) -> ListRouteView {
    match id {
        Some(id) if !id.is_empty() && sync => ListRouteView::Sync,
        Some(id) if !id.is_empty() => ListRouteView::Legacy,
        _ => ListRouteView::Leaving,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leaving `/list/:id` re-runs the route view with the next match's
    /// params before the old page is dropped; that run must not build a page.
    #[test]
    fn list_route_builds_nothing_once_the_router_is_leaving() {
        assert_eq!(
            list_route_view(Some("24".into()), true),
            ListRouteView::Sync
        );
        assert_eq!(
            list_route_view(Some("24".into()), false),
            ListRouteView::Legacy
        );
        assert_eq!(
            list_route_view(Some(String::new()), true),
            ListRouteView::Leaving
        );
        assert_eq!(list_route_view(None, true), ListRouteView::Leaving);
    }

    fn permission_reply(permission: ListPermission) -> ListViewResult {
        Ok((
            ListWithPermission {
                list: ultros_api_types::list::List {
                    id: 101,
                    owner: 1,
                    name: "Permission ordering".into(),
                    wdr_filter: AnySelector::World(1),
                },
                permission,
                owner_name: None,
            },
            vec![],
        ))
    }

    #[test]
    fn stale_write_and_denial_replies_cannot_overwrite_newer_access() {
        Owner::new().with(|| {
            for (old_reply, permission) in [
                (
                    permission_reply(ListPermission::Write),
                    ListPermission::Read,
                ),
                (
                    Err(AppError::ApiError(ApiError::Forbidden)),
                    ListPermission::Write,
                ),
                (
                    Err(AppError::ApiError(ApiError::NotFound)),
                    ListPermission::Read,
                ),
            ] {
                let reads = ListReads::new();
                // Either the price loader or the silent probe uses this same
                // sequence; a separate fence per caller would miss the race.
                let old = reads.begin();
                let newer = reads.begin();
                assert!(reads.accept(newer, &permission_reply(permission)));
                assert!(!reads.accept(old, &old_reply));
                assert_eq!(
                    reads.current_result(101, None).unwrap().0.permission,
                    permission
                );
            }
        });
    }

    #[test]
    fn pending_newer_requests_do_not_starve_successful_replies() {
        Owner::new().with(|| {
            let reads = ListReads::new();
            let first = reads.begin();
            let second = reads.begin();
            for _ in 0..20 {
                reads.begin();
            }
            assert!(reads.accept(first, &permission_reply(ListPermission::Read)));
            assert_eq!(
                reads.current_result(101, None).unwrap().0.permission,
                ListPermission::Read
            );
            assert!(reads.accept(second, &permission_reply(ListPermission::Write)));
            assert_eq!(
                reads.current_result(101, None).unwrap().0.permission,
                ListPermission::Write
            );
        });
    }

    #[test]
    fn transient_replies_preserve_the_last_authoritative_permission() {
        Owner::new().with(|| {
            for error in [
                AppError::ApiError(ApiError::NotAuthenticated),
                AppError::ApiError(ApiError::Message("temporary outage".into())),
                AppError::InternalApiTimeout,
            ] {
                let reads = ListReads::new();
                let old = reads.begin();
                let confirmed = reads.begin();
                assert!(reads.accept(confirmed, &permission_reply(ListPermission::Read)));
                let failed = reads.begin();
                assert!(reads.accept(failed, &Err(error)));
                assert!(!reads.accept(old, &permission_reply(ListPermission::Write)));
                assert_eq!(
                    reads.current_result(101, None).unwrap().0.permission,
                    ListPermission::Read
                );
            }
        });
    }

    #[test]
    fn newer_failures_do_not_suppress_a_pending_authoritative_reply() {
        Owner::new().with(|| {
            for error in [
                AppError::ApiError(ApiError::NotAuthenticated),
                AppError::ApiError(ApiError::Message("temporary outage".into())),
            ] {
                let reads = ListReads::new();
                let old_success = reads.begin();
                let newer_failure = reads.begin();
                assert!(reads.accept(newer_failure, &Err(error)));
                assert!(reads.accept(old_success, &permission_reply(ListPermission::Read)));
                assert_eq!(
                    reads.current_result(101, None).unwrap().0.permission,
                    ListPermission::Read
                );
            }
        });
    }

    #[test]
    fn stale_price_load_uses_current_permission_and_live_document_rows() {
        Owner::new().with(|| {
            let reads = ListReads::new();
            let old_price_load = reads.begin();
            let permission_probe = reads.begin();
            assert!(reads.accept(permission_probe, &permission_reply(ListPermission::Read)));
            assert!(!reads.accept(old_price_load, &permission_reply(ListPermission::Write)));

            let document = ultros_list_doc::ListDocument::new();
            document
                .add_row(ultros_list_doc::RowKey::new(5056, None), 5, None)
                .unwrap();
            let handle = ListDocHandle::open(1, 101);
            handle.import(&document.export_snapshot().unwrap()).unwrap();
            let (list, items) = reads.current_result(101, Some(handle)).unwrap();
            assert_eq!(list.permission, ListPermission::Read);
            assert_eq!(items.len(), 1, "local rows survive a stale network reply");
            assert_eq!(items[0].0.item_id, 5056);
            assert_eq!(items[0].0.quantity, Some(5));
            handle.dispose();
        });
    }

    #[test]
    fn relay_recovery_retries_without_broadcasts_and_caps_backoff() {
        let mut state = RelayRecoveryState::default();
        state.notice(Some(0));
        for delay in [1000, 2000, 4000, 8000, 16000, 30000, 30000] {
            assert_eq!(state.delay_ms(), delay);
            let captured = state.begin();
            assert!(!state.activity);
            state.finish(captured, [false, false]);
            assert!(
                state.pending(),
                "a failed permission read cannot retire recovery"
            );
        }
        let captured = state.begin();
        state.finish(captured, [true, false]);
        assert!(!state.pending());
        assert!(!state.activity, "restored idle relay no longer polls");
        state.notice(Some(1));
        assert_eq!(
            state.delay_ms(),
            1000,
            "a later independent revocation cannot inherit an old outage's backoff"
        );
    }

    #[test]
    fn relay_recovery_keeps_new_notices_and_separate_native_generation() {
        let mut state = RelayRecoveryState::default();
        state.notice(Some(0));
        let captured = state.begin();
        state.notice(Some(0));
        state.notice(Some(1));
        state.finish(captured, [true, false]);
        assert_eq!(state.restored, [1, 0]);
        assert_eq!(state.notices, [2, 1]);
        assert!(
            state.pending(),
            "an earlier REST success cannot consume a newer error"
        );
        let captured = state.begin();
        state.finish(captured, [false, true]);
        assert!(state.pending(), "failed legacy resend still needs recovery");
        assert_eq!(state.restored, [1, 1]);
        let captured = state.begin();
        state.finish(captured, [true, false]);
        assert!(!state.pending());
        // Probes overlap: an older completion arriving late must not roll a
        // newer restoration back and re-arm recovery.
        state.notice(Some(0));
        let newer = state.begin();
        state.finish(newer, [true, false]);
        assert_eq!(state.restored, [3, 1]);
        state.finish([2, 1], [true, false]);
        assert_eq!(
            state.restored,
            [3, 1],
            "a late older completion cannot regress"
        );
        assert!(!state.pending());
    }

    #[test]
    fn ordinary_activity_during_a_probe_schedules_one_follow_up_without_idle_polling() {
        let mut state = RelayRecoveryState::default();
        state.notice(None);
        let captured = state.begin();
        for _ in 0..100 {
            state.notice(None);
        }
        assert_eq!(state.attempts, 0);
        state.finish(captured, [false, false]);
        assert!(state.activity);
        assert!(!state.pending());
        let captured = state.begin();
        state.finish(captured, [false, false]);
        assert!(!state.activity);
        assert!(!state.pending());
    }

    #[test]
    fn continuous_broadcasts_keep_the_first_deadline_and_start_new_windows() {
        use std::cell::Cell;

        let slot = Rc::new(RefCell::new(None::<()>));
        let probes = Rc::new(Cell::new(0));
        let mut pending: Option<(u32, Box<dyn FnOnce()>)> = None;
        for now in (0..=3000).step_by(100) {
            if pending.as_ref().is_some_and(|(due, _)| *due <= now) {
                pending.take().unwrap().1();
                assert!(slot.borrow().is_none(), "fired timer releases its slot");
            }
            let probes = probes.clone();
            schedule_revalidate_with(
                &slot,
                REVALIDATE_MAX_WAIT_MS,
                |delay, callback| {
                    assert_eq!(delay, 1000, "maximum normal scheduling delay");
                    assert!(pending.is_none(), "bursts do not replace pending probes");
                    pending = Some((now + delay, callback));
                },
                move || probes.set(probes.get() + 1),
            );
        }
        assert_eq!(
            probes.get(),
            3,
            "continuous 100ms updates cannot starve probes"
        );
        assert_eq!(pending.as_ref().unwrap().0, 4000);
    }

    #[test]
    fn dropping_subscription_cancels_pending_permission_probe() {
        let slot = Rc::new(RefCell::new(None::<()>));
        let weak = Rc::downgrade(&slot);
        let mut callback = None;
        schedule_revalidate_with(
            &slot,
            REVALIDATE_MAX_WAIT_MS,
            |_, fire| callback = Some(fire),
            || panic!("a retired subscription must not probe a successor document"),
        );
        drop(slot);
        assert!(
            weak.upgrade().is_none(),
            "timer does not retain its subscription"
        );
        // Even a callback queued by the browser before cancellation is harmless.
        callback.unwrap()();
    }

    #[test]
    fn delayed_permission_results_require_the_same_route_and_open_document() {
        Owner::new().with(|| {
            let route = RwSignal::new(101);
            let active = Memo::new(move |_| route.get());
            let original = ListDocHandle::open(1, 101);
            let handle = RwSignal::new(Some(original));
            let expected = Some(original.revision);
            assert!(request_is_current(active, 101, handle, expected));

            route.set(102);
            assert!(!request_is_current(active, 101, handle, expected));
            route.set(101);
            let successor = ListDocHandle::open(1, 101);
            handle.set(Some(successor));
            assert!(
                !request_is_current(active, 101, handle, expected),
                "same route does not identify the same document lifetime"
            );
            assert!(request_is_current(
                active,
                101,
                handle,
                Some(successor.revision)
            ));
            successor.close();
            assert!(!request_is_current(
                active,
                101,
                handle,
                Some(successor.revision)
            ));
            handle.set(None);
            assert!(
                !request_is_current(active, 101, handle, expected),
                "sign-out invalidates responses captured before it"
            );
            original.dispose();
            successor.dispose();
        });
    }

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
