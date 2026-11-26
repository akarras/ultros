//! Bulk "Add to List" modal for the jobset detail page.
//!
//! Generalisation of [`AddRecipeToList`](super::add_recipe_to_list): instead
//! of taking a single `Recipe` and walking its ingredients, this accepts a
//! pre-aggregated list of `(ItemId, quantity)` pairs. The jobset detail
//! page uses it twice — once to add every piece in a gear set at quantity
//! 1, and once to add every craft ingredient for the set summed across
//! recipes.

use crate::api::{add_targets_to_list, bulk_add_item_to_list, get_lists};
use crate::components::icon::Icon;
use crate::components::{
    loading::Loading, modal::Modal, small_item_display::SmallItemDisplay, toggle::Toggle,
    tooltip::Tooltip,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
use icondata::RiPlayListAddMediaLine;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use ultros_api_types::list::{AddCraftTarget, AddCraftTargetsRequest, ListItem};
use ultros_frontend_core::components::account_lists_failure::AccountListsFailure;
use ultros_frontend_core::components::local_list_targets::LocalListTargets;
use xiv_gen::{Item, ItemId};

#[derive(Clone)]
struct EntryState {
    item_id: ItemId,
    item: &'static Item,
    /// Base quantity from the caller. The HQ toggle does not scale this
    /// (HQ is a quality flag, not a quantity multiplier).
    base_amount: i32,
    quantity: RwSignal<i32>,
}

/// Button + modal that bulk-adds a pre-aggregated set of items to a user
/// list. `entries` is a `(ItemId, quantity)` list; the modal lets the
/// user edit each row before committing.
///
/// `subject` is shown under the modal title (the set's stem name, e.g.
/// "Courtly Lover's").
#[component]
pub fn AddSetToList(
    #[prop(into)] button_label: Signal<String>,
    #[prop(into)] tooltip: Signal<String>,
    #[prop(into)] modal_title: Signal<String>,
    #[prop(into)] subject: Signal<String>,
    #[prop(into)] entries: Signal<Vec<(ItemId, i32)>>,
    #[prop(optional, into)] craft_targets: Option<Signal<Vec<(i32, i32)>>>,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let (modal_visible, set_modal_visible) = signal(false);
    // The trigger can retain focus beneath the modal. Hide its floating
    // tooltip while the dialog is open so it cannot cover dialog controls.
    let active_tooltip = Signal::derive(move || {
        if modal_visible.get() {
            String::new()
        } else {
            tooltip.get()
        }
    });
    view! {
        <div class="inline-block">
            <Tooltip tooltip_text=active_tooltip>
                <button
                    type="button"
                    class=if compact { "inline-flex items-center gap-1 text-xs rounded hover:underline focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)]" } else { "btn-primary" }
                    aria-label=move || tooltip.get()
                    on:click=move |_| {
                        set_modal_visible(!modal_visible());
                    }
                >
                    <Icon icon=RiPlayListAddMediaLine />
                    <span>{move || button_label.get()}</span>
                </button>
            </Tooltip>
            <Show when=modal_visible>
                <AddSetToListModal
                    modal_title=modal_title
                    subject=subject
                    entries=entries
                    craft_targets=craft_targets
                    set_visible=set_modal_visible
                />
            </Show>
        </div>
    }
}

#[component]
fn AddSetToListModal(
    modal_title: Signal<String>,
    subject: Signal<String>,
    entries: Signal<Vec<(ItemId, i32)>>,
    #[prop(default = None)] craft_targets: Option<Signal<Vec<(i32, i32)>>>,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let items = &data.items;
    let lists = Resource::new(move || {}, move |_| get_lists());
    let (hq, set_hq) = signal(false);

    let initial_targets = craft_targets
        .map(|s| s.get_untracked())
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect::<Vec<_>>();
    let has_targets = !initial_targets.is_empty();
    let initial_targets = StoredValue::new(initial_targets);
    let (craft_mode, set_craft_mode) = signal(has_targets);

    // Snapshot the entry list once at open time so edits the user makes
    // in the modal don't get clobbered if the parent's `entries` Memo
    // recomputes (e.g. price zone changes). Filter out unknown items so
    // we never render a broken row for a stale id.
    let entry_states = StoredValue::new(
        entries
            .get_untracked()
            .into_iter()
            .filter_map(|(id, amt)| {
                items.get(&id).map(|item| EntryState {
                    item_id: id,
                    item,
                    base_amount: amt,
                    quantity: RwSignal::new(amt),
                })
            })
            .collect::<Vec<_>>(),
    );

    let add_bulk_action = Action::new(move |(list_id, items): &(i32, Vec<ListItem>)| {
        let items = items.clone();
        bulk_add_item_to_list(*list_id, items)
    });
    let add_targets_action = Action::new(move |(list_id, req): &(i32, AddCraftTargetsRequest)| {
        let req = req.clone();
        add_targets_to_list(*list_id, req)
    });

    Effect::new(move |_| {
        if let Some(Ok(_)) = add_bulk_action.value().get() {
            set_visible(false);
        }
        if let Some(Ok(_)) = add_targets_action.value().get() {
            set_visible(false);
        }
    });

    let reset_quantities = move || {
        entry_states.update_value(|states| {
            for s in states {
                s.quantity.set(s.base_amount);
            }
        });
    };

    let target_request = move || AddCraftTargetsRequest {
        targets: initial_targets.with_value(|ts| {
            ts.iter()
                .map(|&(recipe_id, count)| AddCraftTarget { recipe_id, count })
                .collect()
        }),
        exclude_crystals: None,
        include_vendors: None,
        craft_choices: Vec::new(),
    };

    // The rows to add at click time: every entry with a non-zero quantity,
    // HQ only where the item can be. Shared by the account-list buttons and
    // the device-list section (which ignores `list_id`).
    let items_to_add = move |list_id: i32| {
        let hq_only = hq.get_untracked();
        entry_states
            .get_value()
            .iter()
            .filter_map(|s| {
                let quantity = s.quantity.get_untracked();
                if quantity == 0 {
                    return None;
                }
                let can_be_hq = s.item.can_be_hq;
                Some(ListItem {
                    id: 0,
                    item_id: s.item_id.0,
                    list_id,
                    hq: Some(hq_only && can_be_hq),
                    quantity: Some(quantity),
                    acquired: None,
                    target_price: None,
                })
            })
            .collect::<Vec<_>>()
    };
    let local_items = Callback::new(move |()| items_to_add(0));
    let local_targets =
        Callback::new(move |()| (has_targets && craft_mode.get_untracked()).then(target_request));
    let close_on_added = Callback::new(move |()| set_visible(false));

    view! {
        <Modal set_visible>
            <div class="panel p-6 rounded-xl space-y-4 max-w-2xl">
                <div class="flex items-start gap-3">
                    <div class="min-w-0 flex-1">
                        <div data-dialog-title="" class="text-xl font-extrabold text-[color:var(--brand-fg)]">
                            {move || modal_title.get()}
                        </div>
                        <div class="text-[color:var(--color-text-muted)] truncate">
                            {move || subject.get()}
                        </div>
                    </div>
                </div>

                <Show when=move || has_targets>
                    <div class="segmented-control w-fit" role="group" data-testid="add-set-mode-toggle">
                        <button
                            type="button"
                            class="segmented-option"
                            data-testid="add-set-mode-craft"
                            aria-pressed=move || craft_mode.get().to_string()
                            on:click=move |_| set_craft_mode.set(true)
                        >
                            {t!(i18n, add_set_mode_craft_full)}
                        </button>
                        <button
                            type="button"
                            class="segmented-option"
                            data-testid="add-set-mode-buy"
                            aria-pressed=move || (!craft_mode.get()).to_string()
                            on:click=move |_| set_craft_mode.set(false)
                        >
                            {t!(i18n, add_set_mode_buy_finished)}
                        </button>
                    </div>
                </Show>

                <Show
                    when=move || !(has_targets && craft_mode.get())
                    fallback=move || view! {
                        <p class="text-sm text-[color:var(--color-text-muted)]">
                            {move || t_string!(i18n, add_set_craft_summary, count = initial_targets.with_value(|t| t.len())).to_string()}
                        </p>
                    }
                >
                <div class="flex flex-wrap items-center gap-3">
                    <Toggle
                        checked=hq
                        set_checked=set_hq
                        checked_label=t_string!(i18n, add_to_list_hq).to_string()
                        unchecked_label=t_string!(i18n, add_to_list_normal_quality).to_string()
                    />
                    <button
                        type="button"
                        class="btn-secondary text-xs"
                        on:click=move |_| reset_quantities()
                    >
                        {t!(i18n, add_set_to_list_reset_quantities)}
                    </button>
                </div>

                <div class="flex flex-col gap-1.5 max-h-[40vh] overflow-y-auto pr-1">
                    <For
                        each=move || entry_states.get_value()
                        key=|e| e.item_id
                        children=move |entry| {
                            view! {
                                <div class="flex items-center gap-2">
                                    <label
                                        for=format!("set-entry-qty-{}", entry.item_id.0)
                                        class="flex-1 min-w-0"
                                    >
                                        <SmallItemDisplay item=entry.item />
                                    </label>
                                    <input
                                        id=format!("set-entry-qty-{}", entry.item_id.0)
                                        type="number"
                                        min="0"
                                        class="input w-24 ml-auto"
                                        prop:value=move || entry.quantity.get()
                                        on:input=move |e| {
                                            let Ok(q) = event_target_value(&e).parse::<i32>() else { return; };
                                            entry.quantity.set(q.max(0));
                                        }
                                    />
                                </div>
                            }
                        }
                    />
                </div>
                </Show>

                <div class="rounded p-1">
                    <Suspense fallback=Loading>
                        {move || {
                            let lists = match lists.get()? {
                                Ok(lists) => lists,
                                Err(error) => return Some(Either::Right(view! { <AccountListsFailure error /> })),
                            };

                            Some(Either::Left(
                                lists
                                    .into_iter()
                                    .map(|list| {
                                        let (error, set_error) = signal(Option::<String>::None);
                                        Effect::new(move |_| {
                                            if let Some(Err(e)) = add_bulk_action.value().get() {
                                                set_error(Some(e.to_string()));
                                            }
                                            if let Some(Err(e)) = add_targets_action.value().get() {
                                                set_error(Some(e.to_string()));
                                            }
                                        });
                                        let pending = Signal::derive(move || {
                                            add_bulk_action.pending().get() || add_targets_action.pending().get()
                                        });
                                        view! {
                                            <div class="space-y-1">
                                                <div class="flex items-center justify-between card p-2">
                                                    <div class="font-semibold truncate">{list.name}</div>
                                                    <button
                                                        class="btn-primary"
                                                        disabled=pending
                                                        on:click=move |_| {
                                                            let list_id = list.id;
                                                            if has_targets && craft_mode.get_untracked() {
                                                                add_targets_action.dispatch((list_id, target_request()));
                                                            } else {
                                                                let items_to_add = items_to_add(list_id);
                                                                if !items_to_add.is_empty() {
                                                                    add_bulk_action.dispatch((list_id, items_to_add));
                                                                }
                                                            }
                                                        }
                                                    >
                                                        <Show
                                                            when=pending
                                                            fallback=move || view! { <span>{t!(i18n, add_recipe_add_button)}</span> }
                                                        >
                                                            <span>{t!(i18n, add_recipe_adding_button)}</span>
                                                        </Show>
                                                    </button>
                                                </div>
                                                <Show when=Signal::derive(move || error().is_some())>
                                                    <div class="text-xs text-negative px-2">
                                                        {move || error().unwrap_or_default()}
                                                    </div>
                                                </Show>
                                            </div>
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .into_view(),
                            ))
                        }}
                    </Suspense>
                    <LocalListTargets build_items=local_items build_targets=local_targets on_added=close_on_added />
                </div>
            </div>
        </Modal>
    }
}
