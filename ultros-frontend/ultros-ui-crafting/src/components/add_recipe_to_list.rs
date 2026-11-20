use crate::api::{bulk_add_item_to_list, get_lists};
use crate::components::crafting_cost::IngredientsIter;
use crate::components::icon::Icon;
use crate::components::{
    item_icon::*, loading::Loading, modal::Modal, small_item_display::SmallItemDisplay,
    toggle::Toggle, tooltip::Tooltip,
};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
use icondata::RiPlayListAddMediaLine;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use ultros_api_types::list::ListItem;
use ultros_frontend_core::components::account_lists_failure::AccountListsFailure;
use ultros_frontend_core::components::local_list_targets::LocalListTargets;
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

/// `initial_hq` seeds the modal's HQ-ingredients toggle, e.g. from a page
/// that already asked for HQ ingredients.
#[component]
pub fn AddRecipeToList(
    recipe: &'static Recipe,
    #[prop(optional)] initial_hq: bool,
    #[prop(optional)] show_label: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let (modal_visible, set_modal_visible) = signal(false);
    let items = &tracked_data().items;
    let result_item = items.get(&ItemId(recipe.item_result));
    view! {
        <Tooltip tooltip_text=Signal::derive(move || if modal_visible() { String::new() } else { t_string!(i18n, add_recipe_tooltip).to_string() })>
            <button
                type="button"
                class=if show_label { "inline-flex items-center gap-1 text-xs rounded hover:underline focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)]" } else { "btn-primary" }
                aria-label=move || {
                    result_item
                        .map(|i| format!("{}: {}", t_string!(i18n, add_recipe_tooltip), i.name.as_str()))
                        .unwrap_or_else(|| t_string!(i18n, add_recipe_tooltip).to_string())
                }
                on:click=move |event| {
                    event.prevent_default();
                    event.stop_propagation();
                    set_modal_visible(!modal_visible());
                }
            >
                <Icon icon=RiPlayListAddMediaLine aria_hidden=true />
                <span class=if show_label { "" } else { "sr-only" }>{t!(i18n, analyzer_add_ingredients)}</span>
            </button>
        </Tooltip>
        <Show when=modal_visible>
            <AddRecipeToListModal recipe initial_hq set_visible=set_modal_visible />
        </Show>
    }
}

/// The modal behind [`AddRecipeToList`], for callers that bring their own
/// trigger. `initial_hq` seeds the HQ-ingredients toggle.
#[component]
pub fn AddRecipeToListModal(
    recipe: &'static Recipe,
    #[prop(optional)] initial_hq: bool,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView {
    view! {
        <Modal set_visible>
            <AddRecipeToListForm recipe initial_hq show_header=true on_added=Callback::new(move |()| set_visible(false)) />
        </Modal>
    }
}

/// Shared editor, mounted directly in the cost preview or inside the standalone modal.
#[component]
pub fn AddRecipeToListForm(
    recipe: &'static Recipe,
    #[prop(optional)] initial_hq: bool,
    #[prop(default = true)] initial_include_crystals: bool,
    #[prop(into)] on_added: Callback<()>,
    #[prop(optional)] show_header: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let items = &data.items;
    let result_item = move || items.get(&ItemId(recipe.item_result));
    let lists = Resource::new(move || {}, move |_| get_lists());
    let (hq, set_hq) = signal(initial_hq);
    let (craft_quantity, set_craft_quantity) = signal(1);
    let (include_crystals, set_include_crystals) = signal(initial_include_crystals);
    let selected_list = RwSignal::new(None::<i32>);

    let ingredients = StoredValue::new(
        IngredientsIter::new(recipe)
            .flat_map(|(item_id, amount)| {
                items.get(&item_id).map(|item| {
                    let is_crystal = item.item_search_category
                        == crate::components::crafting_cost::CRYSTAL_SEARCH_CATEGORY;
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

    // The rows to add at click time: every ingredient with a non-zero
    // quantity, HQ only where the item can be. Shared by the account-list
    // buttons and the device-list section (which ignores `list_id`).
    let items_to_add = move |list_id: i32| {
        let hq_only = hq.get_untracked();
        ingredients
            .get_value()
            .iter()
            .filter_map(|i| {
                ingredient_entry(
                    (i.item_id, i.item.can_be_hq, i.is_crystal),
                    list_id,
                    i.quantity.get_untracked(),
                    hq_only,
                    include_crystals.get_untracked(),
                )
            })
            .collect::<Vec<_>>()
    };
    let local_items = Callback::new(move |()| items_to_add(0));
    let close_on_added = on_added;

    Effect::new(move |_| {
        if let Some(Ok(_)) = add_bulk_action.value().get() {
            on_added.run(());
        }
    });

    Effect::new(move |_| {
        let quantity = craft_quantity();
        let include = include_crystals();
        ingredients.update_value(|i| {
            for ingredient in i {
                let next = craft_ingredient_quantity(
                    ingredient.amount,
                    quantity,
                    ingredient.quantity.get_untracked(),
                    ingredient.overridden.get_untracked(),
                    ingredient.is_crystal,
                    include,
                );
                ingredient.quantity.set(next);
            }
        });
    });

    view! {
            <div class="space-y-4" data-recipe-list-form=recipe.key_id.0>
                <Show when=move || show_header>
                <div class="flex items-start gap-3">
                    <div class="shrink-0">
                        <ItemIcon item_id={recipe.item_result} icon_size=IconSize::Medium />
                    </div>
                    <div class="min-w-0 flex-1">
                        <div class="text-xl font-extrabold text-[color:var(--brand-fg)]">
                            {t!(i18n, add_recipe_title)}
                        </div>
                        <div class="text-[color:var(--color-text-muted)] truncate">
                            {move || result_item().map(|i| i.name.to_string()).unwrap_or_else(|| t_string!(i18n, add_recipe_unknown_item).to_string())}
                        </div>
                    </div>
                </div>

                </Show>
                <h3 class="recipe-breakdown-heading">{t!(i18n, analyzer_add_ingredients)}</h3>
                <div class="flex flex-wrap items-center gap-3">
                    <label class="text-sm text-[color:var(--color-text-muted)]" for=format!("craft-qty-{}", recipe.key_id.0)>{t!(i18n, add_recipe_number_of_crafts)}</label>
                    <input
                        id=format!("craft-qty-{}", recipe.key_id.0)
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
                        checked_label=t_string!(i18n, add_recipe_hq_ingredients).to_string()
                        unchecked_label=t_string!(i18n, add_recipe_hq_ingredients).to_string()
                    />
                    <div class="h-6 w-px bg-[color:var(--color-outline)] mx-1"></div>
                    <Toggle
                        checked=include_crystals
                        set_checked=set_include_crystals
                        checked_label=t_string!(i18n, add_recipe_include_crystals).to_string()
                        unchecked_label=t_string!(i18n, add_recipe_include_crystals).to_string()
                    />
                </div>
                <p class="text-sm">
                    {move || format!("{} crafts × {} per craft = {} items", craft_quantity(), recipe.amount_result.max(1), craft_quantity().saturating_mul(recipe.amount_result.max(1)))}
                </p>
                <div class="flex flex-col gap-2">
                    <For
                        each=move || ingredients.get_value()
                        key=|i| i.item_id
                        children=move |ingredient| {
                            view! {
                                <div class="flex items-center gap-2">
                                    <label for=format!("ingredient-qty-{}", ingredient.item_id.0) class="flex-1 min-w-0">
                                        <SmallItemDisplay item=ingredient.item />
                                    </label>
                                    <input
                                        id=format!("ingredient-qty-{}", ingredient.item_id.0)
                                        type="number"
                                        min="0"
                                        class="input w-24 ml-auto"
                                        prop:disabled=move || ingredient.is_crystal && !include_crystals()
                                        prop:value=move || if ingredient.is_crystal && !include_crystals() { 0 } else { ingredient.quantity.get() }
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
                            let lists = match lists.get()? {
                                Ok(lists) => lists,
                                Err(error) => {
                                    let signed_out = matches!(&error, ultros_frontend_core::error::AppError::ApiError(ultros_api_types::result::ApiError::NotAuthenticated));
                                    return Some(Either::Right(view! {
                                        <AccountListsFailure error />
                                        <Show when=move || signed_out>
                                            <a href="/login?next=/recipe-analyzer" rel="external" class="btn-primary">"Sign in to add ingredients to an account list"</a>
                                        </Show>
                                    }));
                                },
                            };

                            let no_lists = lists.is_empty();
                            Some(Either::Left(view! {
                                <Show when=move || no_lists><p class="text-sm">"Create a list to save these ingredients. "<a class="underline" href="/list">"Open lists"</a></p></Show>
                                <form class="space-y-2" on:submit=move |ev| {
                                    ev.prevent_default();
                                    if add_bulk_action.pending().get_untracked() { return; }
                                    if let Some(list_id) = selected_list.get_untracked() {
                                        let items = items_to_add(list_id);
                                        if !items.is_empty() { add_bulk_action.dispatch((list_id, items)); }
                                    }
                                }>
                                    <label class="block text-sm" for=format!("recipe-list-{}", recipe.key_id.0)>"Destination list"</label>
                                    <select id=format!("recipe-list-{}", recipe.key_id.0) class="input w-full"
                                        prop:value=move || selected_list().map(|id| id.to_string()).unwrap_or_default()
                                        on:change=move |ev| selected_list.set(event_target_value(&ev).parse().ok())>
                                        <option value="">"Choose a list"</option>
                                        {lists.into_iter().map(|list| view! { <option value=list.id.to_string()>{list.name}</option> }).collect_view()}
                                    </select>
                                    <button type="submit" class="btn-primary w-full justify-center"
                                        disabled=move || selected_list().is_none() || add_bulk_action.pending().get()
                                            || ingredients.with_value(|items| !items.iter().any(|i| i.quantity.get() > 0 && (!i.is_crystal || include_crystals())))>
                                        {move || if add_bulk_action.pending().get() { t_string!(i18n, add_recipe_adding_button).to_string() } else { t_string!(i18n, analyzer_add_ingredients).to_string() }}
                                    </button>
                                    <div role="alert" class="text-sm text-negative">
                                        {move || add_bulk_action.value().get().and_then(Result::err).map(|e| e.to_string())}
                                    </div>
                                </form>
                            }))
                        }}
                    </Suspense>
                    <LocalListTargets build_items=local_items on_added=close_on_added />
                </div>
            </div>

    }
}

/// Crystal inclusion is a submission rule even for manually overridden quantities.
fn ingredient_entry(
    (item_id, can_be_hq, is_crystal): (ItemId, bool, bool),
    list_id: i32,
    quantity: i32,
    hq: bool,
    include_crystals: bool,
) -> Option<ListItem> {
    (quantity > 0 && (!is_crystal || include_crystals)).then_some(ListItem {
        id: 0,
        item_id: item_id.0,
        list_id,
        hq: Some(hq && can_be_hq),
        quantity: Some(quantity),
        acquired: None,
        target_price: None,
    })
}

fn craft_ingredient_quantity(
    amount: i32,
    crafts: i32,
    current: i32,
    overridden: bool,
    is_crystal: bool,
    include_crystals: bool,
) -> i32 {
    if overridden {
        current
    } else if is_crystal && !include_crystals {
        0
    } else {
        amount.saturating_mul(crafts.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inclusion_applies_even_to_overridden_crystals() {
        assert!(ingredient_entry((ItemId(2), false, true), 9, 17, true, false).is_none());
        let crystal = ingredient_entry((ItemId(2), false, true), 9, 17, true, true).unwrap();
        assert_eq!(
            (crystal.quantity, crystal.hq, crystal.list_id),
            (Some(17), Some(false), 9)
        );
        assert!(ingredient_entry((ItemId(3), true, false), 9, 0, true, true).is_none());
    }

    #[test]
    fn hq_and_destination_are_carried_to_the_payload() {
        for hq in [false, true] {
            for can_be_hq in [false, true] {
                let item =
                    ingredient_entry((ItemId(3), can_be_hq, false), 42, 6, hq, false).unwrap();
                assert_eq!(
                    (item.item_id, item.list_id, item.quantity, item.hq),
                    (3, 42, Some(6), Some(hq && can_be_hq))
                );
            }
        }
    }

    #[test]
    fn craft_changes_scale_defaults_and_preserve_manual_quantities() {
        assert_eq!(craft_ingredient_quantity(3, 4, 3, false, false, false), 12);
        assert_eq!(craft_ingredient_quantity(3, 4, 7, true, false, false), 7);
        assert_eq!(craft_ingredient_quantity(3, 4, 7, true, true, false), 7);
        assert_eq!(craft_ingredient_quantity(3, 4, 3, false, true, false), 0);
        assert_eq!(craft_ingredient_quantity(3, 4, 0, false, true, true), 12);
        assert_eq!(
            craft_ingredient_quantity(3, i32::MAX, 0, false, false, true),
            i32::MAX
        );
    }
}
