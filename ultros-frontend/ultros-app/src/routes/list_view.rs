use std::cmp::Reverse;
use std::collections::HashSet;

use icondata as i;
use leptos::either::{Either, EitherOf3};
use leptos::prelude::*;
use leptos_icons::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::ListItem;
use xiv_gen::{Item, ItemId, Recipe};

use crate::api::{
    add_item_to_list, bulk_add_item_to_list, delete_list_item, delete_list_items, edit_list_item,
    get_list_items_with_listings,
};
use crate::components::related_items::IngredientsIter;
use crate::components::{
    clipboard::*, item_icon::*, list_summary::*, loading::*, make_place_importer::*,
    price_viewer::*, small_item_display::*, tooltip::*,
};
use crate::error::AppError;
use crate::routes::purchasing_view::PurchasingView;
use ultros_api_types::listings::ActiveListing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ViewState {
    List,
    Purchasing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenuState {
    None,
    Item,
    Recipe,
    MakePlace,
}

#[component]
pub fn ListView() -> impl IntoView {
    let data = xiv_gen_db::data();
    let game_items = &data.items;
    let recipes = &data.recipes;

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
    let recipe_add = Action::new(move |data: &(&Recipe, i32, bool, bool)| {
        let ingredients = IngredientsIter::new(data.0);
        let craft_count = data.1;
        let items: Vec<_> = ingredients
            .map(|(id, amount)| {
                let item = game_items.get(&id);
                let amount = craft_count * amount;
                let can_be_hq = item.map(|i| i.can_be_hq).unwrap_or(true);
                ListItem {
                    id: 0,
                    item_id: id.0,
                    list_id: list_id(),
                    hq: Some(data.2 && can_be_hq),
                    quantity: Some(amount),
                    acquired: None,
                }
            })
            .collect();

        bulk_add_item_to_list(list_id(), items)
    });
    let edit_item = Action::new(move |item: &ListItem| edit_list_item(item.clone()));
    let delete_items = Action::new(move |items: &Vec<i32>| delete_list_items(items.clone()));
    let list_view = Resource::new(
        move || {
            (
                list_id(),
                (
                    add_item.version().get(),
                    delete_item.version().get(),
                    recipe_add.version().get(),
                    edit_item.version().get(),
                    delete_items.version().get(),
                ),
            )
        },
        move |(id, _)| get_list_items_with_listings(id),
    );

    let (menu, set_menu) = signal(MenuState::None);
    let (view_state, set_view_state) = signal(ViewState::List);
    let edit_list_mode = RwSignal::new(false);
    let selected_items = RwSignal::new(HashSet::new());

    view! {
        <div class="flex-row">
            <button class="btn" on:click=move |_| set_view_state(ViewState::List)>"List"</button>
            <button class="btn" on:click=move |_| set_view_state(ViewState::Purchasing)>"Purchasing"</button>
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
                    class:active=move || menu() == MenuState::Recipe
                    on:click=move |_| set_menu(
                        match menu() {
                            MenuState::Recipe => MenuState::None,
                            _ => MenuState::Recipe,
                        },
                    )
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
        {move || match menu() {
            MenuState::Item => {
                Some(
                    EitherOf3::A({
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
            MenuState::Recipe => {
                Some(
                    EitherOf3::B({
                        let (recipe, set_recipe) = signal("".to_string());
                        let recipe_data: Vec<(&Item, &Recipe)> = recipes
                            .iter()
                            .flat_map(|(_, r)| { game_items.get(&r.item_result).map(|i| (i, r)) })
                            .collect();
                        let item_search = move || {
                            recipe
                                .with(|r| {
                                    let r_lower = r.to_lowercase();
                                    let mut score = recipe_data
                                        .clone()
                                        .into_iter()
                                        .filter(|_| !r.is_empty())
                                        .filter_map(|(i, recipe)| {
                                            if i.name.to_lowercase().contains(&r_lower) {
                                                Some((i.key_id, recipe, i))
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>();
                                    score
                                        .sort_by_key(|(_, _, i)| (
                                            Reverse(i.level_item.0),
                                        ));
                                    score
                                        .into_iter()
                                        .take(100)
                                        .collect::<Vec<_>>()
                                })
                        };
                        let pending = recipe_add.pending();
                        let result = recipe_add.value();
                        view! {
                            <div class="flex-row">
                                <label>"recipe search:"</label>
                                <br />
                                <input
                                    prop:value=recipe
                                    on:input=move |input| set_recipe(event_target_value(&input))
                                />
                            </div>
                            {move || pending().then(|| view! { <Loading /> })}
                            {move || {
                                result.get()
                                    .map(|v| match v {
                                        Ok(()) => "Success".to_string().into_view(),
                                        Err(e) => format!("{e:?}").into_view(),
                                    })
                            }}

                            <div class="content-well flex-column">
                                {move || {
                                    item_search()
                                        .into_iter()
                                        .map(|(_id, ri, item)| {
                                            let (quantity, set_quantity) = signal(1);
                                            let hq = RwSignal::new(false);
                                            let crystals = RwSignal::new(false);
                                            view! {
                                                <div class="flex-row">
                                                    <SmallItemDisplay item=item />
                                                    <label>"Craft count"</label>
                                                    <input
                                                        type="number"
                                                        prop:value=quantity
                                                        on:input=move |i| {
                                                            let input = event_target_value(&i);
                                                            if let Ok(i) = input.parse() {
                                                                set_quantity(i);
                                                            }
                                                        }
                                                    />

                                                    <label>"HQ Only"</label>
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=hq
                                                        on:click=move |_| hq.update(|h| *h = !*h)
                                                    />
                                                    <label>"Ignore Crystals"</label>
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=crystals
                                                        on:click=move |_| crystals.update(|c| *c = !*c)
                                                    />
                                                    <button
                                                        class="btn"
                                                        on:click=move |_| {
                                                            recipe_add.dispatch((ri, quantity(), hq(), crystals()));
                                                        }
                                                    >

                                                        "Add Recipe"
                                                    </button>
                                                </div>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}

                            </div>
                        }
                    }),
                )
            }
            MenuState::MakePlace => {
                Some(
                    EitherOf3::C({
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
                                    {move || match view_state() {
                                        ViewState::List => {
                                            view! {
                                                <table></table>
                                                <div class="content-well">
                                                    <div class="sticky top-0 flex-row justify-between">
                                                        <span class="content-title">{list.name.clone()}</span>
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
                                                        <tbody>
                                                            <tr>
                                                                <th class:hidden=move || !edit_list_mode()>"✅"</th>
                                                                <th>"HQ"</th>
                                                                <th>"Item"</th>
                                                                <th>"Quantity"</th>
                                                                <th>"Price"</th>
                                                                <th class:hidden=edit_list_mode>"Options"</th>
                                                            </tr>
                                                            <For
                                                                each=move || items.get_value()
                                                                key=|(item, _)| item.id
                                                                children=move |(item, listings): (ListItem, Vec<ActiveListing>)| {
                                                                    let (edit, set_edit) = signal(false);
                                                                    let item = RwSignal::new(item);
                                                                    let temp_item = RwSignal::new(item());
                                                                    let listings = RwSignal::new(listings);
                                                                    view! {
                                                                        <ListRow
                                                                            edit=edit
                                                                            edit_list_mode=edit_list_mode
                                                                            item=item
                                                                            listings=listings
                                                                            temp_item=temp_item
                                                                            set_edit=set_edit
                                                                            selected_items=selected_items
                                                                            delete_item=delete_item
                                                                            edit_item=edit_item
                                                                        />
                                                                    }
                                                                        .into_any()
                                                                }
                                                            />
                                                        </tbody>
                                                    </table>
                                                    <ListSummary items=items.get_value() />
                                                </div>
                                            }
                                                .into_any()
                                        }
                                        ViewState::Purchasing => {
                                            view! { <PurchasingView items=items edit_item=edit_item/> }.into_any()
                                        }
                                    }}
                                },
                            )
                        }
                        Err(e) => {
                            Either::Right(
                                view! {
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
                                            <tbody>
                                                <tr>
                                                    <th class:hidden=move || !edit_list_mode()>"✅"</th>
                                                    <th>"HQ"</th>
                                                    <th>"Item"</th>
                                                    <th>"Quantity"</th>
                                                    <th>"Price"</th>
                                                    <th class:hidden=edit_list_mode>"Options"</th>
                                                </tr>
                                                <For
                                                    each=move || items.get_value()
                                                    key=|(item, _)| item.id
                                                    children=move |(item, listings)| {
                                                        let (edit, set_edit) = signal(false);
                                                        let item = RwSignal::new(item);
                                                        let temp_item = RwSignal::new(item());
                                                        let listings = RwSignal::new(listings);
                                                        view! {
                                                            <tr>
                                                                {move || {
                                                                    if !edit() || edit_list_mode() {
                                                                        let item = item();
                                                                        Either::Left(
                                                                            view! {
                                                                                <td class:hidden=move || !edit_list_mode()>
                                                                                    <input
                                                                                        type="checkbox"
                                                                                        on:click=move |_| {
                                                                                            selected_items
                                                                                                .update(|u| {
                                                                                                    if u.contains(&item.id) {
                                                                                                        u.remove(&item.id);
                                                                                                    } else {
                                                                                                        u.insert(item.id);
                                                                                                    }
                                                                                                })
                                                                                        }
                                                                                    />

                                                                                </td>
                                                                                <td>{item.hq.and_then(|hq| hq.then_some("✅"))}</td>
                                                                                <td>
                                                                                    <div class="flex-row">
                                                                                        <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                                                                        {game_items
                                                                                            .get(&ItemId(item.item_id))
                                                                                            .map(|item| item.name.as_str())}
                                                                                        <Clipboard clipboard_text=game_items
                                                                                            .get(&ItemId(item.item_id))
                                                                                            .map(|item| item.name.to_string())
                                                                                            .unwrap_or_default() />
                                                                                        {game_items
                                                                                            .get(&ItemId(item.item_id))
                                                                                            .map(|item| item.item_search_category.0 <= 1)
                                                                                            .unwrap_or_default()
                                                                                            .then(move || {
                                                                                                view! {
                                                                                                    <div>
                                                                                                        <Tooltip tooltip_text="This item is not available on the market board">
                                                                                                            <Icon icon=i::BiTrashSolid />
                                                                                                        </Tooltip>
                                                                                                    </div>
                                                                                                }
                                                                                            })}

                                                                                    </div>
                                                                                </td>
                                                                                <td>{item.quantity}</td>
                                                                                <td>
                                                                                    {move || {
                                                                                        view! {
                                                                                            <PriceViewer
                                                                                                quantity=item.quantity.unwrap_or(1)
                                                                                                hq=item.hq
                                                                                                listings=listings()
                                                                                            />
                                                                                        }
                                                                                    }}

                                                                                </td>
                                                                            },
                                                                        )
                                                                    } else {
                                                                        let item = item();
                                                                        Either::Right(
                                                                            view! {
                                                                                <td>
                                                                                    <input
                                                                                        type="checkbox"
                                                                                        prop:checked=move || temp_item.with(|i| i.hq)
                                                                                        on:click=move |_| {
                                                                                            temp_item.update(|w| w.hq = Some(!w.hq.unwrap_or_default()))
                                                                                        }
                                                                                    />

                                                                                </td>
                                                                                <td>
                                                                                    <div class="flex-row">
                                                                                        <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                                                                        {game_items
                                                                                            .get(&ItemId(item.item_id))
                                                                                            .map(|item| item.name.as_str())}
                                                                                        <Clipboard clipboard_text=game_items
                                                                                            .get(&ItemId(item.item_id))
                                                                                            .map(|item| item.name.to_string())
                                                                                            .unwrap_or_default() />
                                                                                        {game_items
                                                                                            .get(&ItemId(item.item_id))
                                                                                            .map(|item| item.item_search_category.0 <= 1)
                                                                                            .unwrap_or_default()
                                                                                            .then(move || {
                                                                                                view! {
                                                                                                    <div>
                                                                                                        <Tooltip tooltip_text="This item is not available on the market board">
                                                                                                            <Icon icon=i::AiExclamationOutlined />
                                                                                                        </Tooltip>
                                                                                                    </div>
                                                                                                }
                                                                                            })}

                                                                                    </div>
                                                                                </td>
                                                                                <td>
                                                                                    <input
                                                                                        prop:value=move || temp_item.with(|i| i.quantity)
                                                                                        on:input=move |e| {
                                                                                            if let Ok(value) = event_target_value(&e).parse::<i32>() {
                                                                                                temp_item
                                                                                                    .update(|i| {
                                                                                                        i.quantity = Some(value);
                                                                                                    })
                                                                                            }
                                                                                        }
                                                                                    />

                                                                                </td>
                                                                                <td>
                                                                                    {move || {
                                                                                        view! {
                                                                                            <PriceViewer
                                                                                                quantity=item.quantity.unwrap_or(1)
                                                                                                hq=item.hq
                                                                                                listings=listings()
                                                                                            />
                                                                                        }
                                                                                    }}

                                                                                </td>
                                                                            },
                                                                        )
                                                                    }
                                                                }} <td class:hidden=edit_list_mode>
                                                                    <button
                                                                        class="btn"
                                                                        on:click=move |_| {
                                                                            let _ = delete_item.dispatch(item().id);
                                                                        }
                                                                    >
                                                                        <Icon icon=i::BiTrashSolid />
                                                                    </button>
                                                                    <button
                                                                        class="btn"
                                                                        on:click=move |_| {
                                                                            if temp_item() != item() {
                                                                                let _ = edit_item.dispatch(temp_item());
                                                                            }
                                                                            set_edit(!edit())
                                                                        }
                                                                    >
                                                                        <Icon icon=Signal::derive(move || {
                                                                            if edit() { i::BsCheck } else { i::BsPencilFill }
                                                                        }) />
                                                                    </button>
                                                                </td>
                                                            </tr>
                                                        }
                                                            .into_any()
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

#[component]
fn ListRow(
    edit: ReadSignal<bool>,
    edit_list_mode: RwSignal<bool>,
    item: RwSignal<ListItem>,
    listings: RwSignal<Vec<ActiveListing>>,
    temp_item: RwSignal<ListItem>,
    set_edit: WriteSignal<bool>,
    selected_items: RwSignal<HashSet<i32>>,
    delete_item: Action<i32, Result<(), AppError>>,
    edit_item: Action<ListItem, Result<(), AppError>>,
) -> impl IntoView {
    let game_items = &xiv_gen_db::data().items;
    view! {
        <tr>
            {move || {
                if !edit() || edit_list_mode() {
                    let item = item();
                    Either::Left(
                        view! {
                            <td class:hidden=move || !edit_list_mode()><input type="checkbox" on:click=move |_| {
                                selected_items.update(|u| {
                                    if u.contains(&item.id) {
                                        u.remove(&item.id);
                                    } else {
                                        u.insert(item.id);
                                    }
                                })
                            }/></td>
                            <td>{item.hq.and_then(|hq| hq.then_some("✅"))}</td>
                            <td>
                                <div class="flex-row">
                                    <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                    {game_items.get(&ItemId(item.item_id)).map(|item| item.name.as_str())}
                                    <Clipboard clipboard_text=game_items
                                        .get(&ItemId(item.item_id))
                                        .map(|item| item.name.to_string())
                                        .unwrap_or_default() />
                                    {game_items
                                        .get(&ItemId(item.item_id))
                                        .map(|item| item.item_search_category.0 <= 1)
                                        .unwrap_or_default()
                                        .then(move || {
                                            view! {
                                                <div>
                                                    <Tooltip tooltip_text="This item is not available on the market board">
                                                        <Icon icon=i::BiTrashSolid />
                                                    </Tooltip>
                                                </div>
                                            }
                                        })}
                                </div>
                            </td>
                            <td>{item.quantity}</td>
                            <td>
                                {move || {
                                    view! {
                                        <PriceViewer
                                            quantity=item.quantity.unwrap_or(1)
                                            hq=item.hq
                                            listings=listings()
                                        />
                                    }
                                }}
                            </td>
                        },
                    )
                } else {
                    let item = item();
                    Either::Right(
                        view! {
                            <td><input type="checkbox" prop:checked=move || temp_item.with(|i| i.hq) on:click=move |_| {
                                temp_item.update(|w| w.hq = Some(!w.hq.unwrap_or_default()))
                            }/></td>
                            <td>
                                <div class="flex-row">
                                    <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                    {game_items.get(&ItemId(item.item_id)).map(|item| item.name.as_str())}
                                    <Clipboard clipboard_text=game_items
                                        .get(&ItemId(item.item_id))
                                        .map(|item| item.name.to_string())
                                        .unwrap_or_default() />
                                    {game_items
                                        .get(&ItemId(item.item_id))
                                        .map(|item| item.item_search_category.0 <= 1)
                                        .unwrap_or_default()
                                        .then(move || {
                                            view! {
                                                <div>
                                                    <Tooltip tooltip_text="This item is not available on the market board">
                                                        <Icon icon=i::AiExclamationOutlined />
                                                    </Tooltip>
                                                </div>
                                            }
                                        })}
                                </div>
                            </td>
                            <td><input prop:value=move || temp_item.with(|i| i.quantity) on:input=move |e| {
                                if let Ok(value) = event_target_value(&e).parse::<i32>() {
                                    temp_item.update(|i| {
                                        i.quantity = Some(value);
                                    })
                                }
                            }/></td>
                            <td>
                                {move || {
                                    view! {
                                        <PriceViewer
                                            quantity=item.quantity.unwrap_or(1)
                                            hq=item.hq
                                            listings=listings()
                                        />
                                    }
                                }}
                            </td>
                        },
                    )
                }
            }} <td class:hidden=edit_list_mode><button class="btn" on:click=move |_| {
                let _ = delete_item.dispatch(item().id);
            }><Icon icon=i::BiTrashSolid /></button>
            <button class="btn" on:click=move |_| {
                if temp_item() != item() {
                    let _ = edit_item.dispatch(temp_item());
                }
                set_edit(!edit())
            }><Icon icon=Signal::derive(move || if edit() { i::BsCheck } else { i::BsPencilFill }) /></button></td>
        </tr>
    }
}
