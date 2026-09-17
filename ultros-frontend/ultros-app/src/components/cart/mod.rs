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
pub mod feedback;
pub mod row;
pub mod selection;

use std::collections::{HashMap, HashSet};

use icondata as i;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use crate::components::icon::Icon;
use crate::components::list::filter_row::{SortKey, SortSpec};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::list_view::remaining_quantity;
use crate::routes::list_view_sync::{InlineListAdd, ListWorkspaceSource};

use feedback::{CartFeedback, Removal, focus_after_removal};
use row::{CartRow, ROW_GRID, focus_element, quantity_input_id, remove_button_id};
use selection::{CartSelectionBar, retain_present};

/// Identify rows changed after the editor first opens, including a duplicate
/// add that increases an existing row. Price refreshes never highlight rows.
///
/// Only the device editor (a hydrate-only module) calls this; the SSR-only
/// build of this crate has no caller.
#[cfg(feature = "hydrate")]
pub fn use_changed_row_highlight(
    rows: Signal<Vec<(ListItem, Vec<ActiveListing>)>>,
) -> Signal<HashSet<i32>> {
    use leptos::leptos_dom::helpers::{TimeoutHandle, set_timeout_with_handle};
    type Snapshot = HashMap<i32, (Option<i32>, Option<i32>)>;
    let previous = StoredValue::new(None::<Snapshot>);
    let highlighted = RwSignal::new(HashSet::<i32>::new());
    let timer = StoredValue::new(None::<TimeoutHandle>);
    on_cleanup(move || {
        if let Some(timer) = timer.get_value() {
            timer.clear();
        }
    });
    Effect::new(move |_| {
        let current: Snapshot = rows.with(|rows| {
            rows.iter()
                .map(|(row, _)| (row.id, (row.quantity, row.acquired)))
                .collect()
        });
        if let Some(previous) = previous.get_value() {
            let changed: HashSet<i32> = current
                .iter()
                .filter(|(id, value)| previous.get(*id) != Some(*value))
                .map(|(id, _)| *id)
                .collect();
            if !changed.is_empty() {
                highlighted.set(changed);
                if let Some(timer) = timer.get_value() {
                    timer.clear();
                }
                timer.set_value(
                    set_timeout_with_handle(
                        move || {
                            highlighted.try_update(HashSet::clear);
                        },
                        std::time::Duration::from_millis(1500),
                    )
                    .ok(),
                );
            }
        }
        previous.set_value(Some(current));
    });
    highlighted.into()
}

/// Whether an event target is one of the cart's editors (a control whose
/// row must stay visible while it has focus), as opposed to a button.
#[cfg(feature = "hydrate")]
fn is_editor(target: Option<web_sys::EventTarget>) -> bool {
    use wasm_bindgen::JsCast;
    target
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|element| {
            matches!(
                element.tag_name().to_ascii_uppercase().as_str(),
                "INPUT" | "SELECT" | "TEXTAREA"
            )
        })
}

/// Whether the event target is the element that currently has focus.
#[cfg(feature = "hydrate")]
fn target_is_active(target: Option<web_sys::EventTarget>) -> bool {
    use wasm_bindgen::JsCast;
    let active = leptos::prelude::document().active_element();
    target
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|element| active.is_some_and(|active| active == element))
}

/// Whether an event target is a cart editor holding a draft: a text input
/// whose live value differs from the `data-committed` value the document
/// holds. That is the only state the order pin protects — a clean cell, a
/// select, a sort header or a toggle taking focus must not freeze the
/// order, and a commit (which makes the cell clean) releases it so the
/// re-sort the commit asked for can apply.
#[cfg(feature = "hydrate")]
fn editor_drafting(target: Option<web_sys::EventTarget>) -> bool {
    use wasm_bindgen::JsCast;
    target
        .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
        .is_some_and(|input| {
            input
                .get_attribute("data-committed")
                .is_some_and(|committed| input.value().trim() != committed.trim())
        })
}

/// `?cart=legacy` mounts the previous Labs grid (`ListBuildWorkspace`) so a
/// tester can compare the two presentations on the same list. The
/// redesigned cart is the default under the experiment; no cookie or Labs
/// token changes here.
pub fn use_legacy_cart() -> Signal<bool> {
    let query = ultros_ui::components::app_link::use_query_map_or_default();
    Memo::new(move |_| query.with(|q| q.get("cart").is_some_and(|v| v == "legacy"))).into()
}

/// Deterministic, total ordering for the cart. `Price` sorts by the line
/// estimate (what the column shows), unknown lines last; equal keys fall
/// back to the row id so the order can never surface map iteration order.
pub fn sort_cart_rows<'a>(
    rows: &mut [(ListItem, Vec<ActiveListing>)],
    spec: SortSpec,
    name_of: impl Fn(i32) -> Option<&'a str>,
    lines: &HashMap<i32, estimate::LineEstimate>,
) {
    rows.sort_by(|(a, _), (b, _)| {
        let ordering = match spec.key {
            SortKey::Name => name_of(a.item_id)
                .unwrap_or_default()
                .cmp(name_of(b.item_id).unwrap_or_default()),
            SortKey::Price => {
                let cost = |item: &ListItem| {
                    lines
                        .get(&item.id)
                        .filter(|line| {
                            !matches!(
                                line.status,
                                estimate::LineStatus::NoSupply | estimate::LineStatus::NotRequested
                            )
                        })
                        .map(|line| line.total)
                };
                match (cost(a), cost(b)) {
                    // Unknown placement is independent of direction.
                    (None, Some(_)) => return std::cmp::Ordering::Greater,
                    (Some(_), None) => return std::cmp::Ordering::Less,
                    (Some(a), Some(b)) => a.cmp(&b),
                    (None, None) => std::cmp::Ordering::Equal,
                }
            }
            // Preserve the query key, but compare the Qty editor's value.
            SortKey::Acquired => a.quantity.unwrap_or(1).cmp(&b.quantity.unwrap_or(1)),
        };
        let ordering = if spec.descending {
            ordering.reverse()
        } else {
            ordering
        };
        ordering.then_with(|| a.id.cmp(&b.id))
    });
}

/// The `sort` query value for a header click: first click sorts ascending,
/// a second click on the same column flips it, a third clears it.
pub fn next_sort(current: Option<SortSpec>, key: SortKey) -> Option<SortSpec> {
    match current {
        Some(spec) if spec.key == key && !spec.descending => Some(SortSpec {
            key,
            descending: true,
        }),
        Some(spec) if spec.key == key => None,
        _ => Some(SortSpec {
            key,
            descending: false,
        }),
    }
}

fn sort_option(spec: Option<SortSpec>) -> &'static str {
    match spec {
        None => "",
        Some(SortSpec {
            key: SortKey::Name,
            descending: false,
        }) => "name",
        Some(SortSpec {
            key: SortKey::Name,
            descending: true,
        }) => "name-desc",
        Some(SortSpec {
            key: SortKey::Acquired,
            descending: false,
        }) => "acquired",
        Some(SortSpec {
            key: SortKey::Acquired,
            descending: true,
        }) => "acquired-desc",
        Some(SortSpec {
            key: SortKey::Price,
            descending: false,
        }) => "price",
        Some(SortSpec {
            key: SortKey::Price,
            descending: true,
        }) => "price-desc",
    }
}

fn parse_sort_option(value: &str) -> Option<SortSpec> {
    let (key, descending) = match value.strip_suffix("-desc") {
        Some(key) => (key, true),
        None => (value, false),
    };
    let key = match key {
        "name" => SortKey::Name,
        "acquired" => SortKey::Acquired,
        "price" => SortKey::Price,
        _ => return None,
    };
    Some(SortSpec { key, descending })
}

/// The list is mounted once, independently of resource revisions. Row
/// identity is the document row key; each cell reads its current value
/// from its own memo, so a remote update never rebuilds an editor.
/// The toast text for one add: the quantity and name for a single item, a
/// count for a recipe's ingredients.
fn added_message(i18n: I18nContext<Locale, I18nKeys>, items: &[ListItem]) -> String {
    match items {
        [item] => {
            let name = tracked_data()
                .items
                .get(&ItemId(item.item_id))
                .map(|i| i.name.to_string())
                .unwrap_or_default();
            t_string!(
                i18n,
                cart_added_named,
                quantity = item.quantity.unwrap_or(1),
                name = name
            )
            .to_string()
        }
        _ => t_string!(i18n, cart_added_count, count = items.len()).to_string(),
    }
}

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
    // While an editor inside the list has focus, an acquired row stays
    // visible, so completing a row never yanks focus out from under the
    // player. While that editor also holds a *draft*, rows keep their order
    // — including under an active sort — so a change elsewhere cannot move
    // the control being typed in; the commit itself releases the order pin
    // so the re-sort it asked for can apply.
    let editing = RwSignal::new(false);
    let drafting = RwSignal::new(false);
    let grid = NodeRef::<leptos::html::Div>::new();
    // Allocate before filtering or sorting. Hidden rows still need their
    // units; every visible cost, detail and total uses this same result.
    let estimate = source.estimate;
    let allocated_lines = Memo::new(move |_| estimate.with(estimate::lines_by_id));
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
            if let Some(spec) = source.sort.get() {
                allocated_lines.with(|lines| {
                    sort_cart_rows(
                        &mut rows,
                        spec,
                        |id| data.items.get(&ItemId(id)).map(|item| item.name.as_str()),
                        lines,
                    )
                });
            }
            if drafting.get()
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
    let visible_ids = Memo::new(move |_| {
        visible.with(|rows| rows.iter().map(|(item, _)| item.id).collect::<Vec<_>>())
    });

    // ---- Removal feedback (#1436) ----
    //
    // Every local write bumps `action_seq`; a removal toast remembers the
    // value it was created at and offers Undo only while it is still the
    // latest action. A removal is only *announced* once its rows have left
    // the document, so a failed edit never reads as success; the pending
    // record is dropped after a short grace period if the rows never leave.
    let action_seq = RwSignal::new(0u64);
    let bump = move || action_seq.update(|n| *n += 1);
    let pending_removal: RwSignal<Option<Removal>> = RwSignal::new(None);
    let toast: RwSignal<Option<Removal>> = RwSignal::new(None);
    // Rows whose removal is in flight; their delete button is disabled so a
    // second click cannot queue a duplicate mutation.
    let removing: RwSignal<HashSet<i32>> = RwSignal::new(HashSet::new());
    // The row whose quantity should take focus once it is back (after Undo).
    let refocus: RwSignal<Option<i32>> = RwSignal::new(None);
    let schedule = move |ms: u32, f: Box<dyn FnOnce()>| {
        #[cfg(feature = "hydrate")]
        {
            gloo_timers::callback::Timeout::new(ms, f).forget();
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = (ms, f);
        }
    };
    let request_removal = move |ids: Vec<i32>| {
        if ids.is_empty() || !source.can_write.get_untracked() {
            return;
        }
        let data = tracked_data();
        let names = source.rows.with_untracked(|rows| {
            ids.iter()
                .filter_map(|id| rows.iter().find(|(item, _)| item.id == *id))
                .map(|(item, _)| {
                    data.items
                        .get(&ItemId(item.item_id))
                        .map(|item| item.name.to_string())
                        .unwrap_or_else(|| {
                            t_string!(i18n, lists_workspace_item_fallback, id = item.item_id)
                                .to_string()
                        })
                })
                .collect::<Vec<_>>()
        });
        let focus_next = focus_after_removal(&visible_ids.get_untracked(), &ids);
        removing.update(|set| set.extend(ids.iter().copied()));
        bump();
        let seq = action_seq.get_untracked();
        pending_removal.set(Some(Removal {
            ids: ids.clone(),
            names,
            seq,
            focus_next,
        }));
        if let [id] = ids.as_slice() {
            source.remove.run(*id);
        } else {
            source.remove_many.run(ids.clone());
        }
        schedule(
            1_500,
            Box::new(move || {
                if pending_removal
                    .try_get_untracked()
                    .flatten()
                    .is_some_and(|p| p.seq == seq)
                {
                    // The rows never left: the edit failed (the host's
                    // feedback line says why). Re-enable the buttons.
                    pending_removal.set(None);
                    removing.update(|set| {
                        for id in &ids {
                            set.remove(id);
                        }
                    });
                }
            }),
        );
    };
    Effect::new(move |_| {
        let present: HashSet<i32> = source
            .rows
            .with(|rows| rows.iter().map(|(item, _)| item.id).collect());
        if let Some(pending) = pending_removal.get_untracked()
            && pending.ids.iter().all(|id| !present.contains(id))
        {
            pending_removal.set(None);
            removing.update(|set| {
                for id in &pending.ids {
                    set.remove(id);
                }
            });
            let seq = pending.seq;
            match pending.focus_next {
                Some(next) => focus_element(&remove_button_id(next)),
                None => focus_element("list-cart-add-input"),
            }
            toast.set(Some(pending));
            schedule(
                feedback::TOAST_MS,
                Box::new(move || {
                    if toast
                        .try_get_untracked()
                        .flatten()
                        .is_some_and(|t| t.seq == seq)
                    {
                        toast.set(None);
                    }
                }),
            );
        }
        if let Some(id) = refocus.get_untracked()
            && present.contains(&id)
        {
            refocus.set(None);
            focus_element(&quantity_input_id(id));
        }
    });
    let undo_removal = Callback::new(move |removal: Removal| {
        if !source.can_undo.get_untracked() || !source.can_write.get_untracked() {
            return;
        }
        toast.set(None);
        if let Some(first) = removal.ids.first() {
            refocus.set(Some(*first));
        }
        bump();
        source.undo.run(());
    });
    let on_edit = Callback::new(move |item| {
        bump();
        source.edit.run(item);
    });
    // Adding confirms through the global toast: the new row usually lands
    // below the fold, so the composer alone gives no visible feedback.
    let toasts = crate::global_state::toasts::use_toast();
    let on_add = Callback::new(move |item: ListItem| {
        bump();
        if let Some(toasts) = toasts {
            toasts.success(added_message(i18n, std::slice::from_ref(&item)));
        }
        source.add.run(item);
    });
    let on_add_many = Callback::new(move |items: Vec<ListItem>| {
        bump();
        if let Some(toasts) = toasts {
            toasts.success(added_message(i18n, &items));
        }
        source.add_many.run(items);
    });
    let on_set_quality_many = Callback::new(move |(ids, hq): (Vec<i32>, Option<bool>)| {
        bump();
        // A row's id encodes its quality (`adapter::row_id`), so this edit
        // re-keys every row it touches. Carry the selection and any open
        // panel over to the new ids; the pruning effect below drops whichever
        // of the old or new ids the document ends up not holding (a row the
        // host declined to change keeps its old id).
        let renamed: Vec<i32> = ids
            .iter()
            .filter_map(|id| crate::list_doc::adapter::key_of(*id))
            .filter_map(|key| {
                crate::list_doc::adapter::row_id(&ultros_list_doc::RowKey::new(key.item_id, hq))
            })
            .collect();
        selected_items.update(|s| s.extend(renamed.iter().copied()));
        expanded.update(|s| s.extend(renamed.iter().copied()));
        source.set_quality_many.run((ids, hq));
    });
    let on_undo = Callback::new(move |()| {
        bump();
        toast.set(None);
        source.undo.run(());
    });
    let on_redo = Callback::new(move |()| {
        bump();
        toast.set(None);
        source.redo.run(());
    });
    let on_remove = Callback::new(move |id| request_removal(vec![id]));
    let on_remove_many = Callback::new(request_removal);
    // A row that left the document (deleted here, by a collaborator or by
    // undo) leaves the selection and the open-panel set with it.
    Effect::new(move |_| {
        let present: HashSet<i32> = source
            .rows
            .with(|rows| rows.iter().map(|(item, _)| item.id).collect());
        if selected_items.with_untracked(|s| s.iter().any(|id| !present.contains(id))) {
            selected_items.update(|s| {
                retain_present(s, |id| present.contains(&id));
            });
        }
        if expanded.with_untracked(|s| s.iter().any(|id| !present.contains(id))) {
            expanded.update(|s| {
                retain_present(s, |id| present.contains(&id));
            });
        }
    });
    let is_empty = Memo::new(move |_| source.rows.with(|rows| rows.is_empty()));
    let all_visible_selected = Memo::new(move |_| {
        let ids = visible_ids.get();
        !ids.is_empty() && selected_items.with(|s| ids.iter().all(|id| s.contains(id)))
    });
    let sort_header = move |key: SortKey, label: Signal<String>, align_right: bool| {
        let active = Memo::new(move |_| source.sort.get().filter(|spec| spec.key == key));
        let state_id = match key {
            SortKey::Name => "cart-sort-name-state",
            SortKey::Acquired => "cart-sort-quantity-state",
            SortKey::Price => "cart-sort-cost-state",
        };
        let description = move || match active.get() {
            None => String::new(),
            Some(spec) => match (spec.key, spec.descending) {
                (SortKey::Name, false) => t_string!(i18n, cart_sort_name_asc).to_string(),
                (SortKey::Name, true) => t_string!(i18n, cart_sort_name_desc).to_string(),
                (SortKey::Acquired, false) => t_string!(i18n, cart_sort_qty_asc).to_string(),
                (SortKey::Acquired, true) => t_string!(i18n, cart_sort_qty_desc).to_string(),
                (SortKey::Price, false) => t_string!(i18n, cart_sort_cost_asc).to_string(),
                (SortKey::Price, true) => t_string!(i18n, cart_sort_cost_desc).to_string(),
            },
        };
        view! {
            <div class=if align_right { "text-right" } else { "" }>
                <button type="button" class="inline-flex items-center gap-1 rounded px-1 uppercase tracking-wide hover:text-[color:var(--color-text)]" class:text-brand-300=move || active.get().is_some() aria-label=move || t_string!(i18n, cart_sort_column, column = label.get()).to_string() aria-pressed=move || active.get().is_some().to_string() aria-describedby=move || active.get().is_some().then_some(state_id) aria-controls="cart-row-list" on:click=move |_| source.set_sort.run(next_sort(source.sort.get_untracked(), key))>
                    <span>{move || label.get()}</span>
                    <span aria-hidden="true">{move || match active.get() { Some(spec) if spec.descending => "▼", Some(_) => "▲", None => "" }}</span>
                </button>
                <span id=state_id class="sr-only" role="status">{description}</span>
            </div>
        }
    };
    let item_label = Signal::derive(move || t_string!(i18n, lists_workspace_item).to_string());
    let qty_label = Signal::derive(move || t_string!(i18n, cart_qty).to_string());
    let cost_label = Signal::derive(move || t_string!(i18n, cart_est_cost).to_string());
    view! {
        // Wide screens put the composer beside the rows instead of above
        // them, so search results and the recipe panel never push the list
        // below the fold; the composer column sticks while the rows scroll.
        <section class="space-y-3 xl:grid xl:grid-cols-[minmax(20rem,26rem)_minmax(0,1fr)] xl:items-start xl:gap-4 xl:space-y-0" data-testid="list-cart">
            <div class="space-y-3 xl:sticky xl:top-4 xl:max-h-[calc(100vh-2rem)] xl:overflow-y-auto" class:hidden=move || !source.can_write.get() data-testid="list-cart-composer">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=on_add on_add_many=on_add_many recipe_mode=source.recipe_open toggle_recipe=source.toggle_recipe pending=source.pending feedback=source.feedback />
                <div class="flex flex-wrap items-center gap-2 text-sm">
                    <button type="button" class="btn-ghost h-10 w-10 shrink-0 p-0 disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-undo" disabled=move || !source.can_undo.get() aria-label=t_string!(i18n, lists_workspace_undo) title=move || if source.can_undo.get() { t_string!(i18n, lists_workspace_undo).to_string() } else { t_string!(i18n, lists_workspace_nothing_to_undo).to_string() } on:click=move |_| on_undo.run(())><Icon icon=i::BiUndoRegular width="1.25rem" height="1.25rem" aria_hidden=true /></button>
                    <button type="button" class="btn-ghost h-10 w-10 shrink-0 p-0 disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-redo" disabled=move || !source.can_redo.get() aria-label=t_string!(i18n, lists_workspace_redo) title=move || if source.can_redo.get() { t_string!(i18n, lists_workspace_redo).to_string() } else { t_string!(i18n, lists_workspace_nothing_to_redo).to_string() } on:click=move |_| on_redo.run(())><Icon icon=i::BiRedoRegular width="1.25rem" height="1.25rem" aria_hidden=true /></button>
                </div>
            </Show>
            </div>
            <div class="space-y-3 min-w-0" class=("xl:col-span-2", move || !source.can_write.get()) data-testid="list-cart-rows-column">
            <Show when=move || source.estimate_available.get()>
                <crate::components::list_estimate_summary::ListEstimateSummary estimate feed=source.market scope=source.scope_name />
            </Show>
            <CartFeedback toast action_seq=action_seq.into() can_undo=source.can_undo can_write=source.can_write on_undo=undo_removal />
            <div class="flex flex-wrap items-center gap-2">
                <input class="input min-w-0 flex-1" type="search" aria-label=t_string!(i18n, lists_workspace_filter_label) placeholder=t_string!(i18n, lists_workspace_filter_placeholder) prop:value=move || filter.get() on:input=move |ev| filter.set(event_target_value(&ev)) />
                <label class="flex items-center gap-2 text-sm sm:hidden">
                    <span class="text-[color:var(--color-text-muted)]">{t!(i18n, cart_sort_by)}</span>
                    <select class="input" aria-label=t_string!(i18n, cart_sort_by) prop:value=move || sort_option(source.sort.get()) on:change=move |ev| source.set_sort.run(parse_sort_option(&event_target_value(&ev)))>
                        <option value="">{t!(i18n, cart_sort_default)}</option>
                        <option value="name">{t!(i18n, cart_sort_name_asc)}</option>
                        <option value="name-desc">{t!(i18n, cart_sort_name_desc)}</option>
                        <option value="acquired">{t!(i18n, cart_sort_qty_asc)}</option>
                        <option value="acquired-desc">{t!(i18n, cart_sort_qty_desc)}</option>
                        <option value="price">{t!(i18n, cart_sort_cost_asc)}</option>
                        <option value="price-desc">{t!(i18n, cart_sort_cost_desc)}</option>
                    </select>
                </label>
            </div>
            <CartSelectionBar selected_items visible_ids=visible_ids.into() can_write=source.can_write pending=source.bulk_pending on_remove_many=on_remove_many on_set_quality=on_set_quality_many />
            // The order pin follows a *drafting* editor only (see
            // `editor_drafting`): it engages as the player types and releases
            // on commit, focus loss or Escape.
            <div class="panel rounded-xl" node_ref=grid on:focusin=move |ev| {
                #[cfg(feature = "hydrate")]
                { editing.set(is_editor(ev.target())); drafting.set(editor_drafting(ev.target())); }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            } on:input=move |ev| {
                #[cfg(feature = "hydrate")]
                { drafting.set(editor_drafting(ev.target())); }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            } on:change=move |ev| {
                // Only the focused editor's own commit releases the pin; a
                // change dispatched on another row while this one drafts (a
                // remote or scripted edit) must not.
                #[cfg(feature = "hydrate")]
                { if target_is_active(ev.target()) { drafting.set(false); } }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            } on:focusout=move |ev| {
                #[cfg(feature = "hydrate")]
                {
                use wasm_bindgen::JsCast;
                let next = ev.related_target();
                let inside = next.clone().and_then(|target| target.dyn_into::<web_sys::Node>().ok()).is_some_and(|target| grid.get().is_some_and(|grid| grid.contains(Some(&target))));
                editing.set(inside && is_editor(next.clone()));
                drafting.set(inside && editor_drafting(next));
                }
                #[cfg(not(feature = "hydrate"))]
                { let _ = ev; }
            }>
                <div role="group" aria-label=t_string!(i18n, cart_sort_by) class=format!("{ROW_GRID} hidden sm:grid border-b border-[color:var(--color-outline)] text-xs text-[color:var(--color-text-muted)]")>
                    <div class="justify-self-center">
                        <Show when=move || source.can_write.get()>
                            <input type="checkbox" class="h-5 w-5" aria-label=t_string!(i18n, cart_select_all_visible) prop:checked=move || all_visible_selected.get() disabled=move || visible_ids.with(|ids| ids.is_empty()) on:change=move |_| {
                                let ids = visible_ids.get_untracked();
                                if all_visible_selected.get_untracked() {
                                    selected_items.update(|s| { for id in &ids { s.remove(id); } });
                                } else {
                                    selected_items.update(|s| s.extend(ids));
                                }
                            } />
                        </Show>
                    </div>
                    {sort_header(SortKey::Name, item_label, false)}
                    {sort_header(SortKey::Acquired, qty_label, true)}
                    <div class="uppercase tracking-wide">{t!(i18n, lists_workspace_quality)}</div>
                    {sort_header(SortKey::Price, cost_label, true)}
                    <div></div>
                    <div></div>
                </div>
                <ul id="cart-row-list" class="text-sm" data-testid="cart-rows">
                    <For each=move || { visible.get().into_iter().map(|(item, _)| item).collect::<Vec<_>>() } key=|item| item.id children=move |initial| {
                        let id = initial.id;
                        let fallback = StoredValue::new(initial);
                        // ⚡ Bolt Optimization: Batch O(N) searches into a single Memo and use Signal::derive for projections
                        let row_data = Memo::new(move |_| {
                            source.rows.with(|rows| {
                                rows.iter()
                                    .find(|(item, _)| item.id == id)
                                    .map(|(item, _)| item.clone())
                            })
                        });
                        let item = Signal::derive(move || row_data.with(|data| data.clone().unwrap_or_else(|| fallback.get_value())));
                        let line = Signal::derive(move || allocated_lines.with(|lines| lines.get(&id).cloned()));
                        view! { <CartRow item=item line=line selected_items expanded removing=removing.into() on_edit=on_edit on_delete=on_remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
                    } />
                </ul>
                <Show when=move || is_empty.get()>
                    <p class="px-4 py-6 text-center text-sm text-[color:var(--color-text-muted)]">{t!(i18n, cart_empty)}</p>
                </Show>
                <Show when=move || !is_empty.get() && visible_ids.with(|ids| ids.is_empty())>
                    <div class="space-y-2 px-4 py-6 text-center text-sm" data-testid="cart-no-matches">
                        <p role="status">{t!(i18n, cart_no_matches)}</p>
                        <button type="button" class="btn-ghost" on:click=move |_| {
                            filter.set(String::new());
                            source.reset_filters.run(());
                        }>{t!(i18n, cart_clear_filters)}</button>
                    </div>
                </Show>
            </div>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::cart::estimate::fixture_listing;

    fn item(id: i32, item_id: i32, quantity: i32, acquired: i32) -> ListItem {
        ListItem {
            id,
            item_id,
            list_id: 0,
            hq: None,
            quantity: Some(quantity),
            acquired: Some(acquired),
            target_price: None,
        }
    }

    fn ids(rows: &[(ListItem, Vec<ActiveListing>)]) -> Vec<i32> {
        rows.iter().map(|(item, _)| item.id).collect()
    }

    #[test]
    fn header_clicks_cycle_ascending_descending_off() {
        let first = next_sort(None, SortKey::Name);
        assert_eq!(
            first,
            Some(SortSpec {
                key: SortKey::Name,
                descending: false
            })
        );
        let second = next_sort(first, SortKey::Name);
        assert_eq!(
            second,
            Some(SortSpec {
                key: SortKey::Name,
                descending: true
            })
        );
        assert_eq!(next_sort(second, SortKey::Name), None);
        assert_eq!(
            next_sort(second, SortKey::Price),
            Some(SortSpec {
                key: SortKey::Price,
                descending: false
            }),
            "switching column starts ascending again"
        );
    }

    #[test]
    fn sort_by_estimate_puts_unknown_lines_last_and_is_stable_by_id() {
        let mut rows = vec![
            (item(3, 30, 2, 0), vec![]),
            (
                item(1, 10, 2, 0),
                vec![fixture_listing(1, 10, 50, 5, false)],
            ),
            (
                item(2, 20, 2, 0),
                vec![fixture_listing(2, 20, 20, 5, false)],
            ),
            (item(4, 40, 2, 0), vec![]),
        ];
        let lines = estimate::lines_by_id(&ultros_calc::list_estimate::estimate_list_items(&rows));
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: false,
            },
            |_| None,
            &lines,
        );
        assert_eq!(ids(&rows), [2, 1, 3, 4]);
        let lines = estimate::lines_by_id(&ultros_calc::list_estimate::estimate_list_items(&rows));
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: true,
            },
            |_| None,
            &lines,
        );
        assert_eq!(
            ids(&rows),
            [1, 2, 3, 4],
            "ties keep ascending row id in either direction"
        );
    }

    #[test]
    fn sort_by_name_and_displayed_requested_quantity() {
        let names = |id: i32| match id {
            10 => Some("Maple Log"),
            20 => Some("Bronze Ingot"),
            _ => None,
        };
        let mut rows = vec![(item(1, 10, 5, 4), vec![]), (item(2, 20, 3, 0), vec![])];
        let lines = estimate::lines_by_id(&ultros_calc::list_estimate::estimate_list_items(&rows));
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Name,
                descending: false,
            },
            names,
            &lines,
        );
        assert_eq!(ids(&rows), [2, 1]);
        let lines = estimate::lines_by_id(&ultros_calc::list_estimate::estimate_list_items(&rows));
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Acquired,
                descending: false,
            },
            names,
            &lines,
        );
        assert_eq!(
            ids(&rows),
            [2, 1],
            "requested three sorts before five, despite ownership"
        );
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Acquired,
                descending: true,
            },
            names,
            &lines,
        );
        assert_eq!(ids(&rows), [1, 2]);
    }

    #[test]
    fn unrequested_costs_sort_after_known_costs_in_both_directions() {
        let original = vec![
            (
                item(1, 10, 2, 0),
                vec![fixture_listing(1, 10, 10, 5, false)],
            ),
            (item(2, 20, 1, 0), vec![fixture_listing(2, 20, 1, 5, false)]),
            (item(3, 30, 1, 0), vec![]),
            (item(4, 40, 1, 1), vec![]),
        ];
        let estimate = ultros_calc::list_estimate::estimate_list_items_with_coverage(
            &original,
            &HashSet::from([10, 30]),
        );
        let lines = estimate::lines_by_id(&estimate);
        for (descending, expected) in [(false, [4, 1, 2, 3]), (true, [1, 4, 2, 3])] {
            let mut rows = original.clone();
            sort_cart_rows(
                &mut rows,
                SortSpec {
                    key: SortKey::Price,
                    descending,
                },
                |_| None,
                &lines,
            );
            assert_eq!(ids(&rows), expected);
        }
    }

    #[test]
    fn quantity_sort_uses_editor_default_and_keeps_ties_stable() {
        let mut default_quantity = item(2, 20, 9, 0);
        default_quantity.quantity = None;
        let rows = vec![
            (item(3, 30, 1, 99), vec![]),
            (item(1, 10, 2, 0), vec![]),
            (default_quantity, vec![]),
        ];
        for (descending, expected) in [(false, [2, 3, 1]), (true, [1, 2, 3])] {
            let mut sorted = rows.clone();
            sort_cart_rows(
                &mut sorted,
                SortSpec {
                    key: SortKey::Acquired,
                    descending,
                },
                |_| None,
                &HashMap::new(),
            );
            assert_eq!(ids(&sorted), expected);
        }
    }

    #[test]
    fn cost_sort_orders_partial_and_acquired_totals_before_missing_estimates() {
        let rows = vec![
            (item(5, 50, 2, 0), vec![]),
            (item(4, 40, 2, 0), vec![]),
            (
                item(3, 30, 2, 0),
                vec![fixture_listing(3, 30, 10, 1, false)],
            ),
            (
                item(2, 20, 1, 0),
                vec![fixture_listing(2, 20, 10, 1, false)],
            ),
            (item(1, 10, 2, 2), vec![]),
        ];
        let mut lines =
            estimate::lines_by_id(&ultros_calc::list_estimate::estimate_list_items(&rows));
        lines.remove(&5); // Missing allocation is unknown, like NoSupply.
        for (descending, expected) in [(false, [1, 2, 3, 4, 5]), (true, [2, 3, 1, 4, 5])] {
            let mut sorted = rows.clone();
            sort_cart_rows(
                &mut sorted,
                SortSpec {
                    key: SortKey::Price,
                    descending,
                },
                |_| None,
                &lines,
            );
            assert_eq!(ids(&sorted), expected);
        }
    }

    #[test]
    fn cost_sort_uses_full_cart_allocation_even_when_a_competing_row_is_hidden() {
        let offers = vec![fixture_listing(1, 10, 10, 2, true)];
        let any = item(1, 10, 2, 0);
        let mut hq = item(2, 10, 2, 0);
        hq.hq = Some(true);
        let mut rows = vec![
            (any, offers.clone()),
            (hq, offers),
            (
                item(3, 20, 2, 0),
                vec![fixture_listing(2, 20, 15, 2, false)],
            ),
        ];
        let estimate = ultros_calc::list_estimate::estimate_list_items(&rows);
        let lines = estimate::lines_by_id(&estimate);
        assert_eq!(estimate.total, 50);
        assert_eq!(lines[&1].status, estimate::LineStatus::NoSupply);
        assert_eq!(lines[&2].allocations[0].units, 2);
        rows.retain(|(item, _)| item.id != 2);
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: false,
            },
            |_| None,
            &lines,
        );
        assert_eq!(
            ids(&rows),
            [3, 1],
            "hiding HQ must not price Any from HQ's units"
        );
    }

    #[test]
    fn mobile_sort_options_round_trip() {
        for value in [
            "name",
            "name-desc",
            "acquired",
            "acquired-desc",
            "price",
            "price-desc",
        ] {
            assert_eq!(sort_option(parse_sort_option(value)), value);
        }
        assert_eq!(parse_sort_option(""), None);
        assert_eq!(parse_sort_option("bogus"), None);
        assert_eq!(sort_option(None), "");
    }
}
