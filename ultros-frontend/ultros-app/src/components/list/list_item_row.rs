use crate::components::icon::Icon;
use crate::components::{clipboard::*, item_icon::*, price_viewer::*, tooltip::*};
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashSet;
use ultros_api_types::ActiveListing;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

#[component]
pub fn ListItemRow(
    item: ListItem,
    listings: Vec<ActiveListing>,
    edit_list_mode: Signal<bool>,
    #[prop(into)] selected_items: RwSignal<HashSet<i32>>,
    // The return type of delete_list_item is impl Future<Output = Result<(), AppError>> so in Action it becomes () for the output if we don't care about the result, but wait. Action<I, O>. The original code used Action::new. Let's check original.
    // original: let delete_item = Action::new(move |list_item: &i32| delete_list_item(*list_item));
    // delete_list_item returns Result<(), AppError>.
    // So Action<i32, Result<(), AppError>>.
    // But since we can't easily import AppError here without deeper imports, and we might not need the result in the view, let's see.
    // Actually, Action types are generic. We can just use Action<i32, Result<(), crate::error::AppError>>.
    // Or just use `Action<i32, impl Send>` if we don't care.
    // Let's look at `edit_item` too. Action<ListItem, Result<(), AppError>>.
    // To be safe and avoid complex types in signature if possible, we can use generic or verify.
    // Let's assume the passed action matches.

    // For simplicity, let's use the exact type if possible or `Action<Input, Output>`.
    // Let's use `Action<i32, Result<(), crate::error::AppError>>`.
    delete_item: Action<i32, Result<(), crate::error::AppError>>,
    edit_item: Action<ListItem, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    let data = xiv_gen_db::data();
    let game_items = &data.items;

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
                                            <div class="flex flex-col gap-1 w-full">
                                                <span>{format!("{a} / {q}")}</span>
                                                <progress
                                                    class="progress progress-primary w-full h-2 rounded"
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
}
