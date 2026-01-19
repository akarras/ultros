use crate::api::bulk_add_item_to_list;
use crate::components::icon::Icon;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::components::modal::Modal;
use crate::components::related_items::IngredientsIter;
use icondata as i;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use std::cmp::Reverse;
use ultros_api_types::list::ListItem;
use xiv_gen::{Item, Recipe};

#[component]
pub fn AddRecipeToCurrentListModal(
    #[prop(into)] list_id: Signal<i32>,
    #[prop(into)] set_visible: SignalSetter<bool>,
    #[prop(into, optional)] on_success: Option<Callback<()>>,
) -> impl IntoView {
    let (search, set_search) = signal(String::new());
    let data = xiv_gen_db::data();
    let recipes = &data.recipes;
    let items = &data.items;

    // Pre-calculate recipe data for search (item, recipe)
    let recipe_list = StoredValue::new(
        recipes
            .values()
            .filter_map(|r| items.get(&r.item_result).map(|i| (i, r)))
            .collect::<Vec<_>>(),
    );

    let search_results = Memo::new(move |_| {
        search.with(|s| {
            if s.is_empty() {
                return vec![];
            }
            let s_lower = s.to_lowercase();
            let mut results = recipe_list.with_value(|list| {
                list.iter()
                    .filter(|(i, _)| i.name.to_lowercase().contains(&s_lower))
                    .take(50)
                    .cloned()
                    .collect::<Vec<(&Item, &Recipe)>>()
            });
            // Sort by level descending
            results.sort_by_key(|(i, _)| Reverse(i.level_item.0));
            results
        })
    });

    let add_action = Action::new(
        move |(recipe, quantity, hq, ignore_crystals): &(&Recipe, i32, bool, bool)| {
            let ingredients = IngredientsIter::new(recipe);
            let quantity = *quantity;
            let hq = *hq;
            let ignore_crystals = *ignore_crystals;
            let list_id = list_id();

            let items_to_add: Vec<ListItem> = ingredients
                .filter_map(|(id, amount)| {
                    let item = items.get(&id)?;
                    // Check for crystals if ignore_crystals is true
                    // Crystal category is 59
                    if ignore_crystals && item.item_search_category.0 == 59 {
                        return None;
                    }

                    let total_amount = quantity * amount;
                    let can_be_hq = item.can_be_hq;

                    Some(ListItem {
                        id: 0,
                        item_id: id.0,
                        list_id,
                        hq: Some(hq && can_be_hq),
                        quantity: Some(total_amount),
                        acquired: None,
                    })
                })
                .collect();

            if !items_to_add.is_empty() {
                bulk_add_item_to_list(list_id, items_to_add)
            } else {
                bulk_add_item_to_list(list_id, vec![])
            }
        },
    );

    Effect::new(move |_| {
        if let Some(Ok(_)) = add_action.value().get()
            && let Some(cb) = on_success
        {
            cb.run(());
        }
    });

    view! {
        <Modal set_visible max_width="max-w-[90vw] w-[90vw]">
            <div class="space-y-4 h-[80vh] flex flex-col">
                <div class="flex items-center justify-between shrink-0">
                    <h2 class="text-xl font-bold">"Add Recipe to List"</h2>
                    <button class="btn-ghost p-2" on:click=move |_| set_visible(false)>
                        <Icon icon=i::BsX width="24" height="24" />
                    </button>
                </div>

                <div class="shrink-0">
                    <input
                        class="input w-full"
                        placeholder="Search for a recipe..."
                        autofocus
                        prop:value=search
                        on:input=move |e| set_search(event_target_value(&e))
                    />
                </div>

                <div class="flex-1 overflow-y-auto space-y-2 min-h-0 pr-2">
                    <For
                        each=move || search_results.get()
                        key=|(item, _)| item.key_id
                        children=move |(item, recipe)| {
                            let (quantity, set_quantity) = signal(1);
                            let (hq, set_hq) = signal(false);
                            let (no_crystals, set_no_crystals) = signal(false);

                            view! {
                                <div class="card p-3 flex flex-col sm:flex-row sm:items-center gap-3 bg-[color:var(--color-background-elevated)]">
                                    <div class="flex items-center gap-3 flex-1 min-w-0">
                                        <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Medium item />
                                        <div class="flex flex-col min-w-0">
                                            <span class="font-bold whitespace-normal">{item.name.as_str()}</span>
                                            <span class="text-xs text-[color:var(--color-text-muted)]">
                                                "Lvl " {item.level_item.0}
                                            </span>
                                        </div>
                                    </div>

                                    <div class="flex flex-wrap items-center gap-4 shrink-0">
                                        <div class="flex items-center gap-2">
                                            <label class="text-xs text-[color:var(--color-text-muted)]">"Count"</label>
                                            <input
                                                type="number"
                                                class="input w-16 h-8 text-sm"
                                                min="1"
                                                prop:value=quantity
                                                on:input=move |e| {
                                                    if let Ok(val) = event_target_value(&e).parse::<i32>() {
                                                        set_quantity(val.max(1));
                                                    }
                                                }
                                            />
                                        </div>

                                        <div class="flex items-center gap-2" title="Require HQ ingredients where possible">
                                            <label class="text-sm cursor-pointer select-none flex items-center gap-1">
                                                <input
                                                    type="checkbox"
                                                    class="checkbox checkbox-sm"
                                                    prop:checked=hq
                                                    on:change=move |e| set_hq(event_target_checked(&e))
                                                />
                                                "HQ"
                                            </label>
                                        </div>

                                        <div class="flex items-center gap-2" title="Do not add crystals/shards to the list">
                                            <label class="text-sm cursor-pointer select-none flex items-center gap-1">
                                                <input
                                                    type="checkbox"
                                                    class="checkbox checkbox-sm"
                                                    prop:checked=no_crystals
                                                    on:change=move |e| set_no_crystals(event_target_checked(&e))
                                                />
                                                "No Shards"
                                            </label>
                                        </div>

                                        <button
                                            class="btn-primary h-8 min-h-0 px-3 text-sm"
                                            on:click=move |_| {
                                                add_action.dispatch((recipe, quantity(), hq(), no_crystals()));
                                            }
                                        >
                                            <div class="flex items-center gap-1">
                                                <Icon icon=i::BiPlusRegular />
                                                "Add"
                                            </div>
                                        </button>
                                    </div>
                                </div>
                            }
                        }
                    />
                    <Show when=move || { let s = search.get(); !s.is_empty() && search_results.with(|r| r.is_empty()) }>
                        <div class="text-center text-[color:var(--color-text-muted)] py-8">
                            "No recipes found matching \"" {search} "\""
                        </div>
                    </Show>
                    <Show when=move || search.get().is_empty()>
                        <div class="text-center text-[color:var(--color-text-muted)] py-8">
                            "Start typing to search for a recipe..."
                        </div>
                    </Show>
                </div>
            </div>
        </Modal>
    }
}
