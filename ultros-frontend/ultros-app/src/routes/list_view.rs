use std::cmp::Reverse;

use leptos::*;
use leptos_router::use_params_map;
use ultros_api_types::list::ListItem;
use xiv_gen::{Item, ItemId, Recipe};

use crate::api::{
    add_item_to_list, bulk_add_item_to_list, delete_list_item, get_list_items_with_listings,
};
use crate::components::related_items::IngredientsIter;
use crate::components::{
    clipboard::*, item_icon::*, loading::*, make_place_importer::*, price_viewer::*,
    small_item_display::*, tooltip::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenuState {
    None,
    Item,
    Recipe,
    MakePlace,
}

#[component]
pub fn ListView(cx: Scope) -> impl IntoView {
    let data = xiv_gen_db::decompress_data();
    let game_items = &data.items;
    let recipes = &data.recipes;

    let params = use_params_map(cx);
    let list_id = create_memo(cx, move |_| {
        params
            .with(|p| {
                p.get("id")
                    .as_ref()
                    .map(|id| id.parse::<i32>().ok())
                    .flatten()
            })
            .unwrap_or_default()
    });
    let add_item = create_action(cx, move |list_item: &ListItem| {
        let item = list_item.clone();
        add_item_to_list(cx, item.list_id, item)
    });
    let delete_item = create_action(cx, move |list_item: &i32| delete_list_item(cx, *list_item));
    let recipe_add = create_action(cx, move |data: &(&Recipe, i32, bool, bool)| {
        let ingredients = IngredientsIter::new(&data.0);
        let craft_count = data.1;
        let items: Vec<_> = ingredients
            .map(|(id, amount)| {
                let item = game_items.get(&id);
                let amount = craft_count * amount as i32;
                let can_be_hq = item.map(|i| i.can_be_hq).unwrap_or(true);
                ListItem {
                    id: 0,
                    item_id: id.0,
                    list_id: list_id(),
                    hq: Some(data.2 && can_be_hq),
                    quantity: Some(amount),
                }
            })
            .collect();

        bulk_add_item_to_list(cx, list_id(), items)
    });
    let list_view = create_resource(
        cx,
        move || {
            (
                list_id(),
                (
                    add_item.version().get(),
                    delete_item.version().get(),
                    recipe_add.version().get(),
                ),
            )
        },
        move |(id, _)| get_list_items_with_listings(cx, id),
    );
    let (menu, set_menu) = create_signal(cx, MenuState::None);
    view! {cx,
        <div class="flex-row">
            <Tooltip tooltip_text="Add an item to the list".to_string()>
                <button class="btn" class:active=move || menu() == MenuState::Item on:click=move |_| set_menu(match menu() { MenuState::Item => MenuState::None, _ => MenuState::Item  })><i class="fa-solid fa-plus" style="padding-right: 5px;"></i><span>"Add Item"</span></button>
            </Tooltip>
            <Tooltip tooltip_text="Add a recipe's ingredients to the list".to_string()>
                <button class="btn" class:active=move || menu() == MenuState::Recipe on:click=move |_| set_menu(match menu() { MenuState::Recipe => MenuState::None, _ => MenuState::Recipe })>"Add Recipe"</button>
            </Tooltip>
            <Tooltip tooltip_text="Import an item".to_string()>
                <button class="btn" class:active=move || menu() == MenuState::MakePlace on:click=move |_| set_menu(match menu() { MenuState::MakePlace => MenuState::None, _ => MenuState::MakePlace})>"Make Place"</button>
            </Tooltip>

        </div>
        {move || match menu() {
            MenuState::Item => {
            let (search, set_search) = create_signal(cx, "".to_string());
            let items = &xiv_gen_db::decompress_data().items;
            let item_search = move || {
                search.with(|s| {
                    let mut score = items
                        .into_iter()
                        .filter(|(_, i)| i.item_search_category.0 > 0)
                        .filter(|_| !s.is_empty())
                        .flat_map(|(id, i)| sublime_fuzzy::best_match(s, &i.name).map(|m| (id, i, m)))
                        .collect::<Vec<_>>();
                    score.sort_by_key(|(_, i, m)| (Reverse(m.score()), Reverse(i.level_item.0)));
                    score
                        .into_iter()
                        .filter(|(_, _, ma)| ma.score() > 0)
                        .map(|(id, item, ma)| (id, item, ma))
                        .take(100)
                        .collect::<Vec<_>>()
                })
            };
            view!{cx, <div>
                    <div class="flex-row"><label>"item search:"</label><br/>
                    <input prop:value=search on:input=move |input| set_search(event_target_value(&input)) /></div>
                    <div class="content-well flex-column">
                        {move || {
                            let search = item_search()
                                .into_iter()
                                .map(move |(id, item, _)| {
                                    let (quantity, set_quantity) = create_signal(cx, 1);
                                    let read_input_quantity = move |input| { if let Ok(quantity) = event_target_value(&input).parse() {
                                        set_quantity(quantity)
                                    } };
                                    view!{cx, <div class="flex-row">
                                        <ItemIcon item_id=id.0 icon_size=IconSize::Medium/>
                                        <span style="width: 400px">{&item.name}</span>
                                        <label for="amount">"quantity:"</label><input on:input=read_input_quantity prop:value=move || quantity()></input>
                                        <button class="btn" on:click=move |_| {
                                            let item = ListItem { item_id: id.0, list_id: params
                                                .with(|p| {
                                                    p.get("id")
                                                        .as_ref()
                                                        .map(|id| id.parse::<i32>().ok())
                                                        .flatten()
                                                })
                                                .unwrap_or_default(), quantity: Some(quantity()), ..Default::default() };
                                            add_item.dispatch(item);
                                        }><i class="fa-solid fa-plus"></i></button>
                                    </div>}
                                }).collect::<Vec<_>>();
                            view!{cx, search.into_view(cx)}
                        }}
                    </div>
                </div>}
                }.into_view(cx),
                MenuState::None => {}.into_view(cx),
                MenuState::Recipe => {
                    let (recipe, set_recipe) = create_signal(cx, "".to_string());
                    let recipe_data : Vec<(&Item, &Recipe)> = recipes.iter().flat_map(|(_, r)| {
                        game_items.get(&r.item_result).map(|i| (i, r))
                     }).collect();
                     let item_search = move || {
                        recipe.with(|r| {
                            let mut score = recipe_data
                                .clone()
                                .into_iter()
                                .filter(|_| !r.is_empty())
                                .flat_map(|(i, recipe)| sublime_fuzzy::best_match(r, &i.name).map(|m| (i.key_id, recipe, i, m)))
                                .collect::<Vec<_>>();
                            score.sort_by_key(|(_, _, i, m)| (Reverse(m.score()), Reverse(i.level_item.0)));
                            score
                                .into_iter()
                                .filter(|(_, _, _, ma)| ma.score() > 0)
                                .map(|(id, ri, item, ma)| (id, ri, item, ma))
                                .take(100)
                                .collect::<Vec<_>>()
                        })
                    };
                    let pending = recipe_add.pending();
                    let result = recipe_add.value();
                    view!{cx,
                        <div class="flex-row"><label>"recipe search:"</label><br/>
                        <input prop:value=recipe on:input=move |input| set_recipe(event_target_value(&input)) /></div>
                        {move || pending().then(|| view!{cx, <Loading/>})}
                        {move || result().map(|v| match v {
                            Ok(()) => view!{cx, "Success"}.into_view(cx),
                            Err(e) => format!("{e:?}").into_view(cx),
                        })}
                        <div class="content-well flex-column">
                            {move || item_search().into_iter().map(|(_id, ri, item, _ma)| {
                                let (quantity, set_quantity) = create_signal(cx, 1);
                                let hq = create_rw_signal(cx, false);
                                let crystals = create_rw_signal(cx, false);
                                view!{cx,
                            <div class="flex-row">
                                <SmallItemDisplay item=item />
                                <label>"Craft count"</label>
                                <input type="number" prop:value=quantity on:input=move |i| {
                                    let input = event_target_value(&i);
                                    if let Ok(i) = input.parse() {
                                        set_quantity(i);
                                    }
                                } />
                                <label>"HQ Only"</label>
                                <input type="checkbox" prop:checked=hq on:click=move |_| hq.update(|h| *h = !*h) />
                                <label>"Ignore Crystals"</label>
                                <input type="checkbox" prop:checked=crystals on:click=move |_| crystals.update(|c| *c = !*c) />
                                <button class="btn" on:click=move |_| {
                                    recipe_add.dispatch((ri, quantity(), hq(), crystals()));
                                }>"Add Recipe"</button>
                            </div>}
                        }).collect::<Vec<_>>()}
                        </div>
                    }
                }.into_view(cx),
                MenuState::MakePlace => {
                    view!{cx, <MakePlaceImporter list_id=Signal::derive(cx, move || params.with(|p| p.get("id").as_ref().map(|id| id.parse::<i32>().ok())).flatten().unwrap_or_default()) />}
                }.into_view(cx),
        }}
        <Transition fallback=move || view!{cx, <Loading />}>
        {move || list_view.read(cx).map(move |list| match list {
            Ok((list, items)) => view!{cx,
                <div class="content-well">
                    <span class="content-title">{list.name}</span>
                    <table>
                        <tr>
                            <th>"HQ"</th>
                            <th>"Item"</th>
                            <th>"Quantity"</th>
                            <th>"Price"</th>
                            <th>"Options"</th>
                        </tr>
                        <For each=move || items.clone() key=|(item, _)| item.id view=move |cx, (item, listings)| {
                            let (edit, set_edit) = create_signal(cx, false);
                            view!{cx, <tr valign="top">
                            <td>{item.hq.and_then(|hq| hq.then(|| "✅"))}</td>
                            <td>
                                <div class="flex-row">
                                    <ItemIcon item_id=item.item_id icon_size=IconSize::Small/>
                                    {game_items.get(&ItemId(item.item_id)).map(|item| &item.name)}
                                    <Clipboard clipboard_text=game_items.get(&ItemId(item.item_id)).map(|item| item.name.to_string()).unwrap_or_default()/>
                                    {game_items.get(&ItemId(item.item_id)).map(|item| item.item_search_category.0 <= 1).unwrap_or_default().then(move || {
                                        view!{cx, <div><Tooltip tooltip_text="This item is not available on the marketboard".to_string()><i class="fa-solid fa-circle-exclamation"></i></Tooltip></div>}
                                    })}
                                </div>
                            </td>
                            <td>
                                {item.quantity}
                            </td>
                            <td>
                                <PriceViewer quantity=item.quantity.unwrap_or(1) hq=item.hq listings=listings/>
                            </td>
                            <td>
                                <button class="btn" on:click=move |_| {delete_item.dispatch(item.id)}>
                                    <i class="fa-solid fa-trash"></i>
                                </button>
                                <button class="btn" on:click=move |_| set_edit(!edit())>
                                    <i class="fa-solid" class:fa-check=edit class:fa-pencil=move || !edit()></i>
                                </button>
                            </td>
                        </tr>}
                            }
                        />
                    </table>
                </div>},
            Err(e) => view!{cx, <div>{format!("Failed to get items\n{e}")}</div>}
        })
        }
        </Transition>
    }
}
