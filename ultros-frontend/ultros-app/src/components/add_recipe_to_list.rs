use crate::api::{bulk_add_item_to_list, get_lists};
use crate::components::icon::Icon;
use crate::components::related_items::IngredientsIter;
use crate::components::{
    item_icon::*, loading::Loading, modal::Modal, small_item_display::SmallItemDisplay,
    toggle::Toggle, tooltip::Tooltip,
};
use icondata::RiPlayListAddMediaLine;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use ultros_api_types::list::ListItem;
use xiv_gen::{Item, ItemId, Recipe};

#[derive(Clone)]
struct IngredientState {
    item_id: ItemId,
    item: &'static Item,
    amount: i32,
    is_crystal: bool,
    quantity: RwSignal<i32>,
    overridden: RwSignal<bool>,
}

#[component]
pub fn AddRecipeToList(recipe: &'static Recipe) -> impl IntoView {
    let (modal_visible, set_modal_visible) = signal(false);
    let items = &xiv_gen_db::data().items;
    let result_item = items.get(&recipe.item_result);
    view! {
        <Tooltip tooltip_text="Add recipe ingredients to a list">
            <button
                class="btn-primary"
                attr:aria-label=move || {
                    result_item
                        .map(|i| format!("Add {} recipe to a list", i.name))
                        .unwrap_or_else(|| "Add recipe to a list".to_string())
                }
                on:click=move |_| {
                    set_modal_visible(!modal_visible());
                }
            >
                <Icon icon=RiPlayListAddMediaLine />
                <div class="sr-only">"Add To List"</div>
                <Show when=modal_visible>
                    <AddRecipeToListModal recipe set_visible=set_modal_visible />
                </Show>
            </button>
        </Tooltip>
    }
}

#[component]
fn AddRecipeToListModal(
    recipe: &'static Recipe,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let data = xiv_gen_db::data();
    let items = &data.items;
    let result_item = move || items.get(&recipe.item_result);
    let lists = Resource::new(move || {}, move |_| get_lists());
    let (hq, set_hq) = signal(false);
    let (craft_quantity, set_craft_quantity) = signal(1);
    let (ignore_crystals, set_ignore_crystals) = signal(false);

    let ingredients = StoredValue::new(
        IngredientsIter::new(recipe)
            .flat_map(|(item_id, amount)| {
                items.get(&item_id).map(|item| {
                    let category = item.item_search_category.0;
                    let is_crystal = category == 5 || category == 6;
                    IngredientState {
                        item_id,
                        item,
                        amount,
                        is_crystal,
                        quantity: RwSignal::new(amount),
                        overridden: RwSignal::new(false),
                    }
                })
            })
            .collect::<Vec<_>>(),
    );

    let add_bulk_action = Action::new(move |(list_id, items): &(i32, Vec<ListItem>)| {
        let items = items.clone();
        bulk_add_item_to_list(*list_id, items)
    });

    Effect::new(move |_| {
        if let Some(Ok(_)) = add_bulk_action.value().get() {
            set_visible(false);
        }
    });

    Effect::new(move |_| {
        let quantity = craft_quantity();
        let ignore = ignore_crystals();
        ingredients.update_value(|i| {
            for ingredient in i {
                if !ingredient.overridden.get_untracked() {
                    let amount = if ingredient.is_crystal && ignore {
                        0
                    } else {
                        ingredient.amount * quantity
                    };
                    ingredient.quantity.set(amount);
                }
            }
        });
    });

    view! {
        <Modal set_visible>
            <div class="panel p-6 rounded-xl space-y-4">
                <div class="flex items-start gap-3">
                    <div class="shrink-0">
                        <ItemIcon item_id=recipe.item_result.0 icon_size=IconSize::Medium />
                    </div>
                    <div class="min-w-0 flex-1">
                        <div class="text-xl font-extrabold text-[color:var(--brand-fg)]">
                            "Add Recipe to List"
                        </div>
                        <div class="text-[color:var(--color-text-muted)] truncate">
                            {move || result_item().map(|i| i.name.as_str()).unwrap_or("unknown item")}
                        </div>
                    </div>
                    <button class="btn-secondary" on:click=move |_| set_visible(false)>
                        "close"
                    </button>
                </div>

                <div class="flex flex-wrap items-center gap-3">
                    <label class="text-sm text-[color:var(--color-text-muted)]">"Number of crafts"</label>
                    <input
                        type="number"
                        min="1"
                        class="input w-24"
                        prop:value=craft_quantity
                        on:input=move |e| {
                            let Ok(q) = event_target_value(&e).parse::<i32>() else { return; };
                            set_craft_quantity(q.max(1));
                        }
                    />
                    <div class="h-6 w-px bg-[color:var(--color-outline)] mx-1"></div>
                    <Toggle
                        checked=hq
                        set_checked=set_hq
                        checked_label="HQ Ingredients"
                        unchecked_label="Normal Quality"
                    />
                    <div class="h-6 w-px bg-[color:var(--color-outline)] mx-1"></div>
                    <Toggle
                        checked=ignore_crystals
                        set_checked=set_ignore_crystals
                        checked_label="Ignore Crystals"
                        unchecked_label="Include Crystals"
                    />
                </div>
                <div class="flex flex-col gap-2">
                    <For
                        each=move || ingredients.get_value()
                        key=|i| i.item_id
                        children=move |ingredient| {
                            view! {
                                <div class="flex items-center gap-2">
                                    <SmallItemDisplay item=ingredient.item />
                                    <input
                                        type="number"
                                        min="0"
                                        class="input w-24 ml-auto"
                                        prop:value=move || ingredient.quantity.get()
                                        on:input=move |e| {
                                            let Ok(q) = event_target_value(&e).parse::<i32>() else {
                                                return;
                                            };
                                            ingredient.quantity.set(q.max(0));
                                            ingredient.overridden.set(true);
                                        }
                                    />
                                </div>
                            }
                        }
                    />
                </div>

                <div class="rounded p-1">
                    <Suspense fallback=Loading>
                        {move || {
                            let Ok(lists) = lists.get()? else {
                                return Some(Either::Right(view! {
                                    <div class="text-red-400 text-sm">
                                        "unable to load your lists — are you logged in?"
                                    </div>
                                }));
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
                                        });
                                        view! {
                                            <div class="space-y-1">
                                                <div class="flex items-center justify-between card p-2">
                                                    <div class="font-semibold truncate">{list.name}</div>
                                                    <button
                                                        class="btn-primary"
                                                        disabled=add_bulk_action.pending()
                                                        on:click=move |_| {
                                                            let list_id = list.id;
                                                            let hq_only = hq.get_untracked();
                                                            let items_to_add = ingredients
                                                                .get_value()
                                                                .iter()
                                                                .filter_map(|i| {
                                                                    let quantity = i.quantity.get_untracked();
                                                                    if quantity == 0 {
                                                                        return None;
                                                                    }
                                                                    let can_be_hq = i.item.can_be_hq;
                                                                    Some(ListItem {
                                                                        id: 0,
                                                                        item_id: i.item_id.0,
                                                                        list_id,
                                                                        hq: Some(hq_only && can_be_hq),
                                                                        quantity: Some(quantity),
                                                                        acquired: None,
                                                                    })
                                                                })
                                                                .collect::<Vec<_>>();

                                                            if !items_to_add.is_empty() {
                                                                add_bulk_action.dispatch((list_id, items_to_add));
                                                            }
                                                        }
                                                    >
                                                        <Show
                                                            when=add_bulk_action.pending()
                                                            fallback=|| view! { <span>"Add"</span> }
                                                        >
                                                            <span>"Adding..."</span>
                                                        </Show>
                                                    </button>
                                                </div>
                                                <Show when=Signal::derive(move || error().is_some())>
                                                    <div class="text-xs text-red-400 px-2">
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
                </div>
            </div>
        </Modal>
    }
}
