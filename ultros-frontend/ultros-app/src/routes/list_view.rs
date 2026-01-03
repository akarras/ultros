use std::cmp::Reverse;
use std::collections::HashSet;

use crate::components::icon::Icon;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use crate::api::{
    add_item_to_list, delete_list_item, delete_list_items, edit_list_item,
    get_list_items_with_listings,
};
use crate::components::{
    add_recipe_to_current_list::AddRecipeToCurrentListModal, clipboard::*, item_icon::*,
    list_summary::*, loading::*, make_place_importer::*, price_viewer::*, tooltip::*,
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
    let data = xiv_gen_db::data();
    let game_items = &data.items;

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

    let (watch_character_name, set_watch_character_name) = signal("".to_string());
    let (is_watching, set_is_watching) = signal(false);

    Effect::new(move |_| {
        use leptos::leptos_dom::helpers::location;
        use leptos::wasm_bindgen::JsCast;
        use ultros_api_types::websocket::{AlertsRx, AlertsTx};
        use web_sys::{MessageEvent, WebSocket};

        if is_watching.get() {
            let name = watch_character_name.get_untracked();
            if name.is_empty() {
                return;
            }

            let protocol = if location().protocol().unwrap() == "https:" {
                "wss"
            } else {
                "ws"
            };
            let host = location().host().unwrap();
            let url = format!("{protocol}://{host}/alerts/websocket");

            if let Ok(ws) = WebSocket::new(&url) {
                let name = name.clone();
                let ws_for_open = ws.clone();
                let onopen_callback =
                    leptos::wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                        let msg = AlertsRx::WatchCharacter { name: name.clone() };
                        match serde_json::to_string(&msg) {
                            Ok(text) => {
                                let _ = ws_for_open.send_with_str(&text);
                            }
                            Err(e) => {
                                leptos::logging::error!(
                                    "Failed to serialize WatchCharacter: {:?}",
                                    e
                                );
                            }
                        }
                    })
                        as Box<dyn FnMut()>);
                ws.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
                onopen_callback.forget();

                let onmessage_callback =
                    leptos::wasm_bindgen::closure::Closure::wrap(Box::new(move |e: MessageEvent| {
                        if let Ok(txt) = e.data().dyn_into::<web_sys::js_sys::JsString>() {
                            let txt: String = txt.into();
                            if let Ok(AlertsTx::ItemPurchased { item_id }) =
                                serde_json::from_str::<AlertsTx>(&txt)
                            {
                                list_view.update(|data| {
                                    if let Some(Ok((_, items))) = data {
                                        for (item, _) in items.iter_mut() {
                                            if item.item_id == item_id {
                                                let q = item.quantity.unwrap_or(1);
                                                let current = item.acquired.unwrap_or(0);
                                                if current < q {
                                                    item.acquired = Some(current + 1);
                                                    let item_clone = item.clone();
                                                    leptos::task::spawn_local(async move {
                                                        let _ = edit_list_item(item_clone).await;
                                                    });
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    })
                        as Box<dyn FnMut(MessageEvent)>);
                ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
                onmessage_callback.forget();

                let ws_clone = send_wrapper::SendWrapper::new(ws.clone());
                on_cleanup(move || {
                    let _ = ws_clone.close();
                });
            }
        }
    });

    view! {
        <div class="flex-row">
            <details class="content-well group w-full mb-4">
                <summary class="flex items-center justify-between p-4 cursor-pointer list-none">
                    <div class="flex items-center gap-2">
                         <Icon icon=i::BiPurchaseTagSolid />
                         <span class="font-bold">"Auto-mark Purchases"</span>
                         <span class="text-xs text-[color:var(--color-text-muted)] ml-2">"Experimental"</span>
                    </div>
                    <Icon icon=i::BiChevronDownRegular attr:class="transition-transform group-open:rotate-180" />
                </summary>
                <div class="p-4 pt-0 border-t border-white/5 mt-2 pt-4 flex flex-col gap-3">
                    <p class="text-sm text-[color:var(--color-text-muted)]">
                        "Enter your character name below. When you purchase an item on the market board, it will automatically be marked as acquired in this list."
                    </p>
                    <div class="join w-full max-w-md">
                        <input
                            class="input input-bordered join-item flex-1"
                            placeholder="Character Name"
                            prop:value=watch_character_name
                            on:input=move |e| set_watch_character_name(event_target_value(&e))
                            disabled=move || is_watching.get()
                        />
                        <button
                            class="btn join-item"
                            class:btn-success=move || is_watching.get()
                            on:click=move |_| set_is_watching.update(|w| *w = !*w)
                        >
                            {move || if is_watching.get() { "Watching..." } else { "Start Watching" }}
                        </button>
                    </div>
                </div>
            </details>
        </div>
        <div class="flex-row">
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
                                                                        Either::Left(
                                                                            view! {
                                                                                <td class:hidden=move || !edit_list_mode()>
                                                                                    <input
                                                                                        type="checkbox"
                                                                                        on:click=move |_| {
                                                                                            selected_items
                                                                                                .update(|u| {
                                                                                                    let id = item.with(|i| i.id);
                                                                                                    if u.contains(&id) {
                                                                                                        u.remove(&id);
                                                                                                    } else {
                                                                                                        u.insert(id);
                                                                                                    }
                                                                                                })
                                                                                        }
                                                                                    />

                                                                                </td>
                                                                                <td>{item.with(|i| i.hq).and_then(|hq| hq.then_some("✅"))}</td>
                                                                                <td>
                                                                                    <div class="flex-row">
                                                                                        <ItemIcon item_id=item.with(|i| i.item_id) icon_size=IconSize::Small />
                                                                                        {game_items
                                                                                            .get(&ItemId(item.with(|i| i.item_id)))
                                                                                            .map(|item| item.name.as_str())}
                                                                                        <Clipboard clipboard_text=game_items
                                                                                            .get(&ItemId(item.with(|i| i.item_id)))
                                                                                            .map(|item| item.name.to_string())
                                                                                            .unwrap_or_default() />
                                                                                        {game_items
                                                                                            .get(&ItemId(item.with(|i| i.item_id)))
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
                                                                                <td>
                                                                                    {move || {
                                                        let item = item.get();
                                                        let q = item.quantity.unwrap_or(1);
                                                        let a = item.acquired.unwrap_or(0);
                                                        if q > 1 {
                                                            view! {
                                                                <div class="flex flex-col gap-1">
                                                                    <span>{format!("{a} / {q}")}</span>
                                                                    <progress
                                                                        class="progress progress-primary w-full h-2"
                                                                        value=a
                                                                        max=q
                                                                    ></progress>
                                                                </div>
                                                            }
                                                                .into_any()
                                                        } else {
                                                            view! { <span>{q}</span> }.into_any()
                                                        }
                                                    }}

                                                </td>
                                                <td>
                                                    {move || {
                                                        let q = item.with(|i| i.quantity.unwrap_or(1));
                                                        let a = item.with(|i| i.acquired.unwrap_or(0));
                                                        let remaining = q.saturating_sub(a);
                                                                                        view! {
                                                                                            <PriceViewer
                                                                quantity=remaining
                                                                hq=item.with(|i| i.hq)
                                                                                                listings=listings()
                                                                                            />
                                                                                        }
                                                                                    }}

                                                </td>
                                                <td class:hidden=edit_list_mode>
                                                    <div class="flex gap-1">
                                                        <button
                                                            class="btn"
                                                            on:click=move |_| {
                                                                let _ = delete_item.dispatch(item.with(|i| i.id));
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
                                                        <Tooltip tooltip_text="Mark as acquired">
                                                            <button
                                                                class="btn"
                                                                on:click=move |_| {
                                                                    item.update(|i| {
                                                                        i.acquired = i.quantity;
                                                                    });
                                                                    let _ = edit_item.dispatch(item());
                                                                }
                                                            >
                                                                <Icon icon=i::BiCheckRegular />
                                                            </button>
                                                        </Tooltip>
                                                    </div>
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
                                                    <div class="flex flex-col gap-1">
                                                        <label class="text-xs">"Qty"</label>
                                                        <input
                                                            class="input w-20"
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

                                                        <label class="text-xs">"Acquired"</label>
                                                        <input
                                                            class="input w-20"
                                                            prop:value=move || temp_item.with(|i| i.acquired.unwrap_or(0))
                                                            on:input=move |e| {
                                                                if let Ok(value) = event_target_value(&e).parse::<i32>() {
                                                                    temp_item
                                                                        .update(|i| {
                                                                            i.acquired = Some(value);
                                                                        })
                                                                }
                                                            }
                                                        />

                                                    </div>
                                                                                </td>
                                                                                <td>
                                                                                    {move || {
                                                        let q = item.quantity.unwrap_or(1);
                                                        let a = item.acquired.unwrap_or(0);
                                                        let remaining = q.saturating_sub(a);
                                                                                        view! {
                                                                                            <PriceViewer
                                                                quantity=remaining
                                                                                                hq=item.hq
                                                                                                listings=listings()
                                                                                            />
                                                                                        }
                                                                                    }}

                                                </td>
                                                <td>
                                                    <button
                                                        class="btn"
                                                        on:click=move |_| {
                                                            let _ = delete_item.dispatch(item.id);
                                                        }
                                                    >
                                                        <Icon icon=i::BiTrashSolid />
                                                    </button>
                                                    <button
                                                        class="btn"
                                                        on:click=move |_| {
                                                            if temp_item() != item {
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
                                                                            },
                                                                        )
                                                                    }
                                }}
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
