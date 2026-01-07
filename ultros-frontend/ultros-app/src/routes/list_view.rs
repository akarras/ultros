use std::cmp::Reverse;
use std::collections::HashSet;

use crate::components::icon::Icon;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::ListItem;

use crate::api::{
    add_item_to_list, delete_list_item, delete_list_items, edit_list_item,
    get_list_items_with_listings,
};
use crate::components::{
    add_recipe_to_current_list::AddRecipeToCurrentListModal,
    item_icon::*,
    list::{auto_mark_purchases::AutoMarkPurchases, list_item_row::ListItemRow, list_summary::*},
    loading::*,
    make_place_importer::*,
    tooltip::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenuState {
    None,
    Item,
    // Recipe is now handled by a modal
    MakePlace,
}

#[component]
pub fn ListView() -> impl IntoView {
    let params = use_params_map();
    let list_id = Memo::new(move |_| {
        params
            .with(|p| p.get("id").as_ref().and_then(|id| id.parse::<i32>().ok()))
            .unwrap_or_default()
    });
    let add_item = Action::new(move |list_item: &ListItem| {
        let item = list_item.clone();
        add_item_to_list(item.list_id, item)
    });
    let delete_item = Action::new(move |list_item: &i32| delete_list_item(*list_item));

    // This action definition was removed as logic moved to the modal.
    // However, we need to handle the update trigger.
    // We'll rely on the modal's on_success callback to trigger refetch.

    let edit_item = Action::new(move |item: &ListItem| edit_list_item(item.clone()));
    let delete_items = Action::new(move |items: &Vec<i32>| delete_list_items(items.clone()));

    // We need to trigger refetch when items are added via modal.
    // We can use a signal for versioning external updates.
    let (external_update_version, set_external_update_version) = signal(0);

    let list_view = Resource::new(
        move || {
            (
                list_id(),
                (
                    add_item.version().get(),
                    delete_item.version().get(),
                    // removed recipe_add version
                    external_update_version.get(),
                    edit_item.version().get(),
                    delete_items.version().get(),
                ),
            )
        },
        move |(id, _)| get_list_items_with_listings(id),
    );

    let (menu, set_menu) = signal(MenuState::None);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);

    let edit_list_mode = RwSignal::new(false);
    let selected_items = RwSignal::new(HashSet::new());

    // Auto-mark logic moved to AutoMarkPurchases component

    view! {
        <AutoMarkPurchases list_view=list_view />
        <div class="flex-row gap-2">
            <Tooltip tooltip_text="Add an item to the list">
                <button
                    class="btn-primary"
                    class:active=move || menu() == MenuState::Item
                    on:click=move |_| set_menu(
                        match menu() {
                            MenuState::Item => MenuState::None,
                            _ => MenuState::Item,
                        },
                    )
                >

                    <i class="pr-1.5">
                        <Icon icon=i::BiPlusRegular />
                    </i>
                    <span>"Add Item"</span>
                </button>
            </Tooltip>
            <Tooltip tooltip_text="Add a recipe's ingredients to the list">
                <button
                    class="btn-secondary"
                    class:active=move || recipe_modal_open()
                    on:click=move |_| set_recipe_modal_open(true)
                >

                    "Add Recipe"
                </button>
            </Tooltip>
            <Tooltip tooltip_text="Import an item">
                <button
                    class="btn-secondary"
                    class:active=move || menu() == MenuState::MakePlace
                    on:click=move |_| set_menu(
                        match menu() {
                            MenuState::MakePlace => MenuState::None,
                            _ => MenuState::MakePlace,
                        },
                    )
                >

                    "Make Place"
                </button>
            </Tooltip>

        </div>

        <Show when=recipe_modal_open>
            <AddRecipeToCurrentListModal
                list_id=list_id
                set_visible=set_recipe_modal_open
                on_success=move || {
                    set_external_update_version.update(|v| *v += 1);
                    set_recipe_modal_open(false);
                }
            />
        </Show>

        {move || match menu() {
            MenuState::Item => {
                Some(
                    Either::Left({
                        let (search, set_search) = signal("".to_string());
                        let items = &xiv_gen_db::data().items;
                        let item_search = move || {
                            search
                                .with(|s| {
                                    let s_lower = s.to_lowercase();
                                    let mut score = items
                                        .iter()
                                        .filter(|(_, i)| i.item_search_category.0 > 0)
                                        .filter(|_| !s.is_empty())
                                        .filter_map(|(id, i)| {
                                            if i.name.to_lowercase().contains(&s_lower) {
                                                Some((id, i))
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>();
                                    score
                                        .sort_by_key(|(_, i)| (
                                            Reverse(i.level_item.0),
                                        ));
                                    score
                                        .into_iter()
                                        .take(100)
                                        .collect::<Vec<_>>()
                                })
                        };
                        let adding = add_item.pending();
                        let add_result = add_item.value();
                        view! {
                            <div class="panel p-4 rounded-xl space-y-3">
                                <div class="space-y-2">
                                    <label class="text-sm font-semibold text-[color:var(--brand-fg)]">"add item to this list"</label>
                                    <input
                                        class="input w-full"
                                        placeholder="search items..."
                                        prop:value=search
                                        on:input=move |input| set_search(event_target_value(&input))
                                    />
                                    {move || add_result.get().map(|v| {
                                        let text = match v {
                                            Ok(()) => "added to list ✔".to_string(),
                                            Err(e) => format!("failed to add: {e}"),
                                        };
                                        view! { <div class="text-sm">{text}</div> }.into_view()
                                    })}
                                </div>
                                <div class="content-well flex flex-col">
                                    {move || {
                                        item_search()
                                            .into_iter()
                                            .map(move |(id, item)| {
                                                let (quantity, set_quantity) = signal(1);
                                                let read_input_quantity = move |input| {
                                                    if let Ok(quantity) = event_target_value(&input).parse() {
                                                        set_quantity(quantity)
                                                    }
                                                };
                                                view! {
                                                    <div class="card p-2 flex items-center gap-3">
                                                        <ItemIcon item_id=id.0 icon_size=IconSize::Medium />
                                                        <span class="flex-1 min-w-0 truncate">{item.name.as_str()}</span>
                                                        <label class="text-sm text-[color:var(--color-text-muted)]">"qty"</label>
                                                        <input
                                                            type="number"
                                                            min="1"
                                                            class="input w-20"
                                                            on:input=read_input_quantity
                                                            prop:value=quantity
                                                        />
                                                        <button
                                                            class="btn-primary"
                                                            disabled=adding
                                                            on:click=move |_| {
                                                                let item = ListItem {
                                                                    item_id: id.0,
                                                                    list_id: params
                                                                        .with(|p| {
                                                                            p.get("id").as_ref().and_then(|id| id.parse::<i32>().ok())
                                                                        })
                                                                        .unwrap_or_default(),
                                                                    quantity: Some(quantity()),
                                                                    ..Default::default()
                                                                };
                                                                add_item.dispatch(item);
                                                            }
                                                        >
                                                            {move || if adding() {
                                                                Either::Left(view! { <span>"adding..."</span> })
                                                            } else {
                                                                Either::Right(view! {
                                                                    <div class="flex items-center gap-1">
                                                                        <Icon icon=i::BiPlusRegular />
                                                                        <span>"add"</span>
                                                                    </div>
                                                                })
                                                            }}
                                                        </button>
                                                    </div>
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                    }}

                                </div>
                            </div>
                        }
                    }),
                )
            }
            MenuState::None => None,
            // Removed MenuState::Recipe block
            MenuState::MakePlace => {
                Some(
                    Either::Right({
                        view! {
                            <MakePlaceImporter
                                list_id=Signal::derive(move || {
                                    params
                                        .with(|p| {
                                            p.get("id").as_ref().map(|id| id.parse::<i32>().ok())
                                        })
                                        .flatten()
                                        .unwrap_or_default()
                                })

                                refresh=move || { list_view.refetch() }
                            />
                        }
                    }),
                )
            }
        }}

        <Transition fallback=move || {
            view! { <Loading /> }
        }>
            {move || {
                list_view
                    .get()
                    .map(move |list| match list {
                        Ok((list, items)) => {
                            let items = StoredValue::new(items);
                            Either::Left(
                                view! {
                                    <table></table>
                                    <div class="content-well">
                                        <div class="sticky top-0 flex-row justify-between">
                                            <span class="content-title">{list.name}</span>
                                            <div class="flex flex-row">
                                                <button
                                                    class="btn"
                                                    class:bg-brand-950=edit_list_mode
                                                    on:click=move |_| {
                                                        edit_list_mode
                                                            .update(|u| {
                                                                *u = !*u;
                                                            })
                                                    }
                                                >

                                                    "bulk edit"
                                                </button>
                                                <div class:hidden=move || !edit_list_mode()>
                                                    <button
                                                        class="btn"
                                                        on:click=move |_| {
                                                            let items = selected_items
                                                                .with_untracked(|s| s.iter().copied().collect::<Vec<_>>());
                                                            selected_items.update(|i| i.clear());
                                                            delete_items.dispatch(items);
                                                        }
                                                    >

                                                        "DELETE"
                                                    </button>
                                                </div>
                                                <button
                                                    class="btn"
                                                    on:click=move |_| {
                                                        selected_items
                                                            .update(|i| {
                                                                for (item, _) in items.get_value() {
                                                                    i.insert(item.id);
                                                                }
                                                            })
                                                    }
                                                >

                                                    "SELECT ALL"
                                                </button>
                                                <button
                                                    class="btn"
                                                    on:click=move |_| {
                                                        selected_items.update(|i| i.clear());
                                                    }
                                                >

                                                    "DESLECT ALL"
                                                </button>
                                            </div>
                                        </div>
                                        <table class="w-full">
                                            <thead>
                                                <tr>
                                                    <th class="text-left p-2" class:hidden=move || !edit_list_mode()>"✅"</th>
                                                    <th class="text-left p-2">"HQ"</th>
                                                    <th class="text-left p-2">"Item"</th>
                                                    <th class="text-left p-2">"Quantity"</th>
                                                    <th class="text-left p-2">"Price"</th>
                                                    <th class="text-left p-2" class:hidden=edit_list_mode>"Options"</th>
                                                </tr>
                                            </thead>
                                            <tbody>
                                                <For
                                                    each=move || items.get_value()
                                                    key=|(item, _)| item.id
                                                    children=move |(item, listings)| {
                                                        view! {
                                                            <ListItemRow
                                                                item=item
                                                                listings=listings
                                                                edit_list_mode=edit_list_mode.into()
                                                                selected_items=selected_items
                                                                delete_item=delete_item
                                                                edit_item=edit_item
                                                            />
                                                        }
                                                    }
                                                />
                                            </tbody>
                                        </table>
                                        <ListSummary items=items.get_value() />
                                    </div>
                                },
                            )
                        }
                        Err(e) => {
                            Either::Right(
                                view! {
                                    // TODO full table?
                                    // let price_view = items.iter().flat_map(|(list, listings): &(ListItem, Vec<ActiveListing>)| listings.iter().map(|listing| {
                                    // ShoppingListRow { item_id: ItemKey(ItemId(list.item_id)), amount: listing.quantity, lowest_price: listing.price_per_unit, lowest_price_world: listing.world_id.to_string(), lowest_price_datacenter: "TODO".to_string() }
                                    // })).collect::<Vec<_>>();
                                    // <TableContent rows=price_view on_change=move |_| {} />

                                    <div>{format!("Failed to get items\n{e}")}</div>
                                },
                            )
                        }
                    })
            }}

        </Transition>
    }.into_any()
}
