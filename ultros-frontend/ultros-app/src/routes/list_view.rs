use std::cmp::Reverse;

use icondata as i;
use leptos::*;
use leptos_icons::*;
use leptos_router::use_params_map;
use leptos_struct_table::*;
use ultros_api_types::list::ListItem;
use ultros_api_types::ActiveListing;
use xiv_gen::{Item, ItemId, Recipe};

use crate::api::{
    add_item_to_list, bulk_add_item_to_list, delete_list_item, edit_list_item,
    get_list_items_with_listings,
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

#[derive(TableRow, Clone)]
#[table(
    sortable,
    impl_vec_data_provider,
    classes_provider = "TailwindClassesPreset"
)]
struct ShoppingListRow {
    // #[table(renderer = "ItemCellRenderer")]
    item_id: ItemKey,
    amount: i32,
    lowest_price: i32,
    lowest_price_world: String,
    lowest_price_datacenter: String,
}
#[derive(Copy, Clone)]
struct ItemKey(ItemId);

impl PartialOrd for ItemKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let items = &xiv_gen_db::data().items;
        let item_1 = items
            .get(&self.0)
            .map(|i| i.name.as_str())
            .unwrap_or_default();
        let item_2 = items
            .get(&other.0)
            .map(|i| i.name.as_str())
            .unwrap_or_default();
        item_1.partial_cmp(item_2)
    }
}

impl PartialEq for ItemKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 .0 == other.0 .0
    }
}

impl IntoView for ItemKey {
    fn into_view(self) -> View {
        let item = xiv_gen_db::data().items.get(&self.0);
        item.map(|item| {
            view! {
                <SmallItemDisplay item />
            }
        })
        .into_view()
    }
}

#[component]
fn ItemCellRenderer<F>(
    class: String,
    #[prop(into)] value: MaybeSignal<ItemId>,
    on_change: F,
    index: usize,
) -> impl IntoView
where
    F: Fn(String) + 'static,
{
    let items = &xiv_gen_db::data().items;
    view! { <td class=class>items.get(&ItemId(value())).map(|item| view!{ <SmallItemDisplay item />})</td>}
}

#[component]
pub fn ListView() -> impl IntoView {
    let data = xiv_gen_db::data();
    let game_items = &data.items;
    let recipes = &data.recipes;

    let params = use_params_map();
    let list_id = create_memo(move |_| {
        params
            .with(|p| p.get("id").as_ref().and_then(|id| id.parse::<i32>().ok()))
            .unwrap_or_default()
    });
    let add_item = create_action(move |list_item: &ListItem| {
        let item = list_item.clone();
        add_item_to_list(item.list_id, item)
    });
    let delete_item = create_action(move |list_item: &i32| delete_list_item(*list_item));
    let recipe_add = create_action(move |data: &(&Recipe, i32, bool, bool)| {
        let ingredients = IngredientsIter::new(data.0);
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
                    acquired: None,
                }
            })
            .collect();

        bulk_add_item_to_list(list_id(), items)
    });
    let edit_item = create_action(move |item: &ListItem| edit_list_item(item.clone()));
    let list_view = create_resource(
        move || {
            (
                list_id(),
                (
                    add_item.version().get(),
                    delete_item.version().get(),
                    recipe_add.version().get(),
                    edit_item.version().get(),
                ),
            )
        },
        move |(id, _)| get_list_items_with_listings(id),
    );
    let (menu, set_menu) = create_signal(MenuState::None);
    view! {
        <div class="flex-row">
            <Tooltip tooltip_text=Oco::from("Add an item to the list")>
                <button class="btn" class:active=move || menu() == MenuState::Item on:click=move |_| set_menu(match menu() { MenuState::Item => MenuState::None, _ => MenuState::Item  })><i style="padding-right: 5px;"><Icon icon=i::BiPlusRegular /></i><span>"Add Item"</span></button>
            </Tooltip>
            <Tooltip tooltip_text=Oco::from("Add a recipe's ingredients to the list")>
                <button class="btn" class:active=move || menu() == MenuState::Recipe on:click=move |_| set_menu(match menu() { MenuState::Recipe => MenuState::None, _ => MenuState::Recipe })>"Add Recipe"</button>
            </Tooltip>
            <Tooltip tooltip_text=Oco::from("Import an item")>
                <button class="btn" class:active=move || menu() == MenuState::MakePlace on:click=move |_| set_menu(match menu() { MenuState::MakePlace => MenuState::None, _ => MenuState::MakePlace})>"Make Place"</button>
            </Tooltip>

        </div>
        {move || match menu() {
            MenuState::Item => {
            let (search, set_search) = create_signal("".to_string());
            let items = &xiv_gen_db::data().items;
            let item_search = move || {
                search.with(|s| {
                    let mut score = items
                        .iter()
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
            view!{<div>
                    <div class="flex-row"><label>"item search:"</label><br/>
                    <input prop:value=search on:input=move |input| set_search(event_target_value(&input)) /></div>
                    <div class="content-well flex-column">
                        {move || {
                            item_search()
                                .into_iter()
                                .map(move |(id, item, _)| {
                                    let (quantity, set_quantity) = create_signal(1);
                                    let read_input_quantity = move |input| { if let Ok(quantity) = event_target_value(&input).parse() {
                                        set_quantity(quantity)
                                    } };
                                    view!{<div class="flex-row">
                                        <ItemIcon item_id=id.0 icon_size=IconSize::Medium/>
                                        <span style="width: 400px">{&item.name}</span>
                                        <label for="amount">"quantity:"</label><input on:input=read_input_quantity prop:value=quantity></input>
                                        <button class="btn" on:click=move |_| {
                                            let item = ListItem { item_id: id.0, list_id: params
                                                .with(|p| {
                                                    p.get("id")
                                                        .as_ref()
                                                        .and_then(|id| id.parse::<i32>().ok())
                                                })
                                                .unwrap_or_default(), quantity: Some(quantity()), ..Default::default() };
                                            add_item.dispatch(item);
                                        }><Icon icon=i::BiPlusRegular /></button>
                                    </div>}
                                }).collect::<Vec<_>>()
                        }}
                    </div>
                </div>}
                }.into_view(),
                MenuState::None => ().into_view(),
                MenuState::Recipe => {
                    let (recipe, set_recipe) = create_signal("".to_string());
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
                    view!{
                        <div class="flex-row"><label>"recipe search:"</label><br/>
                        <input prop:value=recipe on:input=move |input| set_recipe(event_target_value(&input)) /></div>
                        {move || pending().then(|| view!{<Loading/>})}
                        {move || result().map(|v| match v {
                            Ok(()) => view!{"Success"}.into_view(),
                            Err(e) => format!("{e:?}").into_view(),
                        })}
                        <div class="content-well flex-column">
                            {move || item_search().into_iter().map(|(_id, ri, item, _ma)| {
                                let (quantity, set_quantity) = create_signal(1);
                                let hq = create_rw_signal(false);
                                let crystals = create_rw_signal(false);
                                view!{
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
                }.into_view(),
                MenuState::MakePlace => {
                    view!{<MakePlaceImporter list_id=Signal::derive(move || params.with(|p| p.get("id").as_ref().map(|id| id.parse::<i32>().ok())).flatten().unwrap_or_default()) />}
                }.into_view(),
        }}
        <Transition fallback=move || view!{<Loading />}>
        {move || list_view.get().map(move |list| match list {

            Ok((list, items)) => {
                // TODO full table?
                // let price_view = items.iter().flat_map(|(list, listings): &(ListItem, Vec<ActiveListing>)| listings.iter().map(|listing| {
                //     ShoppingListRow { item_id: ItemKey(ItemId(list.item_id)), amount: listing.quantity, lowest_price: listing.price_per_unit, lowest_price_world: listing.world_id.to_string(), lowest_price_datacenter: "TODO".to_string() }
                // })).collect::<Vec<_>>();
                view!{
                    <table>
                    // <TableContent rows=price_view on_change=move |_| {} />
                </table>
                <div class="content-well">
                    <span class="content-title">{list.name}</span>
                    <table class="w-full">
                        <tr>
                            <th>"HQ"</th>
                            <th>"Item"</th>
                            <th>"Quantity"</th>
                            <th>"Price"</th>
                            <th>"Options"</th>
                        </tr>
                        <For each=move || items.clone() key=|(item, _)| item.id children=move |(item, listings)| {
                            let (edit, set_edit) = create_signal(false);
                            let item = create_rw_signal(item);
                            let temp_item = create_rw_signal(item());
                            let listings = create_rw_signal(listings);
                            view!{<tr valign="top">
                            {move || if !edit() {
                                let item = item();
                                view!{<td>{item.hq.and_then(|hq| hq.then_some("✅"))}</td>
                                <td>
                                    <div class="flex-row">
                                        <ItemIcon item_id=item.item_id icon_size=IconSize::Small/>
                                        {game_items.get(&ItemId(item.item_id)).map(|item| &item.name)}
                                        <Clipboard clipboard_text=game_items.get(&ItemId(item.item_id)).map(|item| item.name.to_string()).unwrap_or_default()/>
                                        {game_items.get(&ItemId(item.item_id)).map(|item| item.item_search_category.0 <= 1).unwrap_or_default().then(move || {
                                            view!{<div><Tooltip tooltip_text=Oco::from("This item is not available on the market board")><Icon icon=i::BiTrashSolid/></Tooltip></div>}
                                        })}
                                    </div>
                                </td>
                                <td>
                                    {item.quantity}
                                </td>
                                <td>
                                    {move || view!{<PriceViewer quantity=item.quantity.unwrap_or(1) hq=item.hq listings=listings()/>}}
                                </td>
                            }
                            } else {
                                let item = item();
                                view!{<td><input type="checkbox" prop:checked=move || temp_item.with(|i| i.hq) on:click=move |_| { temp_item.update(|w| w.hq = Some(!w.hq.unwrap_or_default())) }/></td>
                                <td>
                                    <div class="flex-row">
                                        <ItemIcon item_id=item.item_id icon_size=IconSize::Small/>
                                        {game_items.get(&ItemId(item.item_id)).map(|item| &item.name)}
                                        <Clipboard clipboard_text=game_items.get(&ItemId(item.item_id)).map(|item| item.name.to_string()).unwrap_or_default()/>
                                        {game_items.get(&ItemId(item.item_id)).map(|item| item.item_search_category.0 <= 1).unwrap_or_default().then(move || {
                                            view!{<div><Tooltip tooltip_text=Oco::from("This item is not available on the market board")><Icon icon=i::AiExclamationOutlined/></Tooltip></div>}
                                        })}
                                    </div>
                                </td>
                                <td>
                                    <input prop:value=move || temp_item.with(|i| i.quantity) on:input=move |e| {
                                        if let Ok(value) = event_target_value(&e).parse::<i32>() {
                                            temp_item.update(|i| { i.quantity = Some(value); } ) }
                                        }
                                        />
                                </td>
                                <td>
                                    {move || view!{<PriceViewer quantity=item.quantity.unwrap_or(1) hq=item.hq listings=listings()/>}}
                                </td>}
                            }}
                            <td>
                                <button class="btn" on:click=move |_| {delete_item.dispatch(item().id)}>
                                    <Icon icon=i::BiTrashSolid />
                                </button>
                                <button class="btn" on:click=move |_| {
                                    if temp_item() != item() {
                                        edit_item.dispatch(temp_item())
                                    }
                                    set_edit(!edit())
                                }>
                                    <Icon icon=MaybeSignal::derive(move || if edit() { i::BsCheck } else { i::BsPencilFill }) />
                                </button>
                            </td>
                        </tr>}
                            }
                        />
                    </table>
                </div>}.into_view()
            },
            Err(e) => view!{<div>{format!("Failed to get items\n{e}")}</div>}.into_view()
        })
        }
        </Transition>
    }
}
