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

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use crate::components::list::filter_row::{SortKey, SortSpec};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::list_view::remaining_quantity;
use crate::routes::list_view_sync::{InlineListAdd, InlineRecipeAdd, ListWorkspaceSource};

use feedback::{CartFeedback, Removal, focus_after_removal};
use row::{CartRow, ROW_GRID, focus_element, quantity_input_id, remove_button_id};
use selection::{CartSelectionBar, retain_present};

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
) {
    rows.sort_by(|(a, a_listings), (b, b_listings)| {
        let ordering = match spec.key {
            SortKey::Name => name_of(a.item_id)
                .unwrap_or_default()
                .cmp(name_of(b.item_id).unwrap_or_default()),
            SortKey::Price => {
                let cost = |item: &ListItem, listings: &[ActiveListing]| {
                    let line = estimate::estimate_line(item, listings);
                    // Nothing priced sorts last, after every known cost.
                    if line.status == estimate::LineStatus::NoSupply {
                        i64::MAX
                    } else {
                        line.total
                    }
                };
                cost(a, a_listings).cmp(&cost(b, b_listings))
            }
            SortKey::Acquired => remaining_quantity(a).cmp(&remaining_quantity(b)),
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
                sort_cart_rows(&mut rows, spec, |id| {
                    data.items.get(&ItemId(id)).map(|item| item.name.as_str())
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
    let on_add = Callback::new(move |item| {
        bump();
        source.add.run(item);
    });
    let on_add_many = Callback::new(move |items| {
        bump();
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
    // The whole cart, not the filtered view, priced by the shared estimator
    // (#1431/#1432): a filter narrows what the list shows, never what it
    // costs, and both Labs presentations must agree on the total.
    let estimate = Memo::new(move |_| {
        source
            .rows
            .with(|rows| ultros_calc::list_estimate::estimate_list_items(rows))
    });
    let all_visible_selected = Memo::new(move |_| {
        let ids = visible_ids.get();
        !ids.is_empty() && selected_items.with(|s| ids.iter().all(|id| s.contains(id)))
    });
    let sort_header = move |key: SortKey, label: Signal<String>, align_right: bool| {
        let active = Memo::new(move |_| source.sort.get().filter(|spec| spec.key == key));
        view! {
            <div role="columnheader" class=if align_right { "text-right" } else { "" } aria-sort=move || match active.get() {
                Some(spec) if spec.descending => "descending",
                Some(_) => "ascending",
                None => "none",
            }>
                <button type="button" class="inline-flex items-center gap-1 rounded px-1 uppercase tracking-wide hover:text-[color:var(--color-text)]" class:text-brand-300=move || active.get().is_some() aria-label=move || t_string!(i18n, cart_sort_column, column = label.get()).to_string() on:click=move |_| source.set_sort.run(next_sort(source.sort.get_untracked(), key))>
                    <span>{move || label.get()}</span>
                    <span aria-hidden="true">{move || match active.get() { Some(spec) if spec.descending => "▼", Some(_) => "▲", None => "" }}</span>
                </button>
            </div>
        }
    };
    let item_label = Signal::derive(move || t_string!(i18n, lists_workspace_item).to_string());
    let qty_label = Signal::derive(move || t_string!(i18n, cart_qty).to_string());
    let cost_label = Signal::derive(move || t_string!(i18n, cart_est_cost).to_string());
    view! {
        <section class="space-y-3" data-testid="list-cart">
            <Show when=move || source.can_write.get()>
                <InlineListAdd list_id=source.list_id on_add=on_add pending=source.pending feedback=source.feedback />
                <div class="flex flex-wrap items-center gap-2 text-sm">
                    <button type="button" class="btn-ghost" aria-expanded=move || source.recipe_open.get().to_string() on:click=move |_| source.toggle_recipe.run(())>{t!(i18n, lists_workspace_add_recipe)}</button>
                    <span class="mx-1 h-4 border-l border-[color:var(--color-outline)]" aria-hidden="true"></span>
                    <button type="button" class="btn-ghost disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-undo" disabled=move || !source.can_undo.get() title=move || (!source.can_undo.get()).then(|| t_string!(i18n, lists_workspace_nothing_to_undo).to_string()) on:click=move |_| on_undo.run(())>{t!(i18n, lists_workspace_undo)}</button>
                    <button type="button" class="btn-ghost disabled:opacity-40 disabled:cursor-not-allowed" data-testid="list-redo" disabled=move || !source.can_redo.get() title=move || (!source.can_redo.get()).then(|| t_string!(i18n, lists_workspace_nothing_to_redo).to_string()) on:click=move |_| on_redo.run(())>{t!(i18n, lists_workspace_redo)}</button>
                </div>
                <Show when=move || source.recipe_open.get()><InlineRecipeAdd list_id=source.list_id on_add=on_add_many /></Show>
            </Show>
            <crate::components::list_estimate_summary::ListEstimateSummary estimate=estimate.into() feed=source.market scope=source.scope_name />
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
                <div role="row" class=format!("{ROW_GRID} hidden sm:grid border-b border-[color:var(--color-outline)] text-xs text-[color:var(--color-text-muted)]")>
                    <div role="columnheader" class="justify-self-center">
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
                    <div role="columnheader" class="uppercase tracking-wide">{t!(i18n, lists_workspace_quality)}</div>
                    {sort_header(SortKey::Price, cost_label, true)}
                    <div role="columnheader"></div>
                    <div role="columnheader"></div>
                </div>
                <ul class="text-sm" data-testid="cart-rows">
                    <For each=move || { visible.get().into_iter().map(|(item, _)| item).collect::<Vec<_>>() } key=|item| item.id children=move |initial| {
                        let id = initial.id;
                        let fallback = StoredValue::new(initial);
                        // ⚡ Bolt Optimization: Batch O(N) searches into a single Memo and use Signal::derive for projections
                        let row_data = Memo::new(move |_| {
                            source.rows.with(|rows| {
                                rows.iter()
                                    .find(|(item, _)| item.id == id)
                                    .map(|(item, listings)| {
                                        (item.clone(), listings.clone(), estimate::estimate_line(item, listings))
                                    })
                            })
                        });
                        let item = Signal::derive(move || row_data.with(|data| data.as_ref().map(|(item, _, _)| item.clone()).unwrap_or_else(|| fallback.get_value())));
                        let listings = Signal::derive(move || row_data.with(|data| data.as_ref().map(|(_, listings, _)| listings.clone()).unwrap_or_default()));
                        let line = Signal::derive(move || row_data.with(|data| data.as_ref().map(|(_, _, line)| line.clone())));
                        view! { <CartRow item=item listings=listings line=line selected_items expanded removing=removing.into() on_edit=on_edit on_delete=on_remove can_write=source.can_write highlighted=Signal::derive(move || highlighted.with(|items| items.contains(&id))) /> }
                    } />
                </ul>
                <Show when=move || is_empty.get()>
                    <p class="px-4 py-6 text-center text-sm text-[color:var(--color-text-muted)]">{t!(i18n, cart_empty)}</p>
                </Show>
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
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: false,
            },
            |_| None,
        );
        assert_eq!(ids(&rows), [2, 1, 3, 4]);
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Price,
                descending: true,
            },
            |_| None,
        );
        assert_eq!(
            ids(&rows),
            [3, 4, 1, 2],
            "ties keep ascending row id in either direction"
        );
    }

    #[test]
    fn sort_by_name_and_remaining_quantity() {
        let names = |id: i32| match id {
            10 => Some("Maple Log"),
            20 => Some("Bronze Ingot"),
            _ => None,
        };
        let mut rows = vec![(item(1, 10, 5, 4), vec![]), (item(2, 20, 3, 0), vec![])];
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Name,
                descending: false,
            },
            names,
        );
        assert_eq!(ids(&rows), [2, 1]);
        sort_cart_rows(
            &mut rows,
            SortSpec {
                key: SortKey::Acquired,
                descending: false,
            },
            names,
        );
        assert_eq!(ids(&rows), [1, 2], "one unit left sorts before three");
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
