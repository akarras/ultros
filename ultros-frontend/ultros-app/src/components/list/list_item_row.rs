use crate::components::alert_config_drawer::AlertConfigDrawer;
use crate::components::icon::Icon;
use crate::components::{clipboard::*, item_icon::*, price_viewer::*, tooltip::*};
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
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
    let i18n = use_i18n();
    let data = tracked_data();
    let game_items = &data.items;

    let (edit, set_edit) = signal(false);
    let (alert_drawer_open, set_alert_drawer_open) = signal(false);
    let item = RwSignal::new(item);
    let temp_item = RwSignal::new(item());
    let listings = RwSignal::new(listings);

    view! {
        <tr class="group transition-colors hover:bg-[color:var(--color-background-panel)]">
            {move || {
                if !edit() || edit_list_mode() {
                    Either::Left(
                        view! {
                            <td class="px-3 py-3 align-middle" class:hidden=move || !edit_list_mode()>
                                <input
                                    type="checkbox"
                                    prop:checked=move || {
                                        selected_items.with(|u| u.contains(&item.with(|i| i.id)))
                                    }
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
                            <td class="px-3 py-3 align-middle">
                                {move || {
                                    item
                                        .with(|i| i.hq)
                                        .and_then(|hq| {
                                            hq.then_some(
                                                view! {
                                                    <span class="inline-flex rounded-md border border-[color:var(--brand-ring)]/40 px-2 py-0.5 text-xs font-bold text-[color:var(--brand-fg)]">
                                                        "HQ"
                                                    </span>
                                                },
                                            )
                                        })
                                }}
                            </td>
                            <td class="px-3 py-3 align-middle">
                                <div class="flex min-w-0 items-center gap-3">
                                    <ItemIcon item_id=item.with(|i| i.item_id) icon_size=IconSize::Small />
                                    <div class="min-w-0">
                                        <div class="flex min-w-0 items-center gap-2">
                                            <span class="min-w-0 truncate font-semibold">
                                                {game_items
                                                    .get(&ItemId(item.with(|i| i.item_id)))
                                                    .map(|item| item.name.as_str())}
                                            </span>
                                            <Clipboard clipboard_text=game_items
                                                .get(&ItemId(item.with(|i| i.item_id)))
                                                .map(|item| item.name.to_string())
                                                .unwrap_or_default() />
                                        </div>
                                        {game_items
                                            .get(&ItemId(item.with(|i| i.item_id)))
                                            .map(|item| item.item_search_category <= 1)
                                            .unwrap_or_default()
                                            .then(move || {
                                                view! {
                                                    <div class="mt-1 inline-flex items-center gap-1 rounded-md bg-red-500/10 px-2 py-0.5 text-xs text-red-200">
                                                        <Tooltip tooltip_text=t_string!(i18n, list_item_row_not_marketable_tooltip).to_string()>
                                                            <Icon icon=i::AiExclamationOutlined />
                                                        </Tooltip>
                                                        <span>{t!(i18n, list_item_row_unavailable_on_market)}</span>
                                                    </div>
                                                }
                                            })}
                                    </div>

                                </div>
                            </td>
                            <td class="px-3 py-3 align-middle">
                                {move || {
                                    let item = item.get();
                                    let q = item.quantity.unwrap_or(1);
                                    let a = item.acquired.unwrap_or(0);
                                    if q > 1 {
                                        view! {
                                            <div class="flex flex-col gap-1 w-full">
                                                <span>{format!("{a} / {q}")}</span>
                                                <progress
                                                    class="progress progress-primary h-2 w-full rounded"
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
                            <td class="px-3 py-3 align-middle">
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
                            <td class="px-3 py-3 align-middle" class:hidden=edit_list_mode>
                                <div class="flex justify-end gap-1">
                                    <Tooltip tooltip_text=t_string!(i18n, list_item_row_create_alert).to_string()>
                                        <button
                                            class="btn-secondary h-8 w-8 p-0"
                                            aria-label=t_string!(i18n, list_item_row_create_alert)
                                            on:click=move |_| set_alert_drawer_open.set(true)
                                        >
                                            <Icon icon=i::BsBell />
                                        </button>
                                    </Tooltip>
                                    <button
                                        class="btn-secondary h-8 w-8 p-0 hover:text-red-200"
                                        aria-label=t_string!(i18n, list_item_row_delete_aria)
                                        on:click=move |_| {
                                            let _ = delete_item.dispatch(item.with(|i| i.id));
                                        }
                                    >
                                        <Icon icon=i::BiTrashSolid />
                                    </button>
                                    <button
                                        class="btn-secondary h-8 w-8 p-0"
                                        aria-label=move || if edit() { t_string!(i18n, list_item_row_save_edit_aria) } else { t_string!(i18n, list_item_row_edit_item_aria) }
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
                                    <Tooltip tooltip_text=t_string!(i18n, list_item_row_mark_acquired).to_string()>
                                        <button
                                            class="btn-secondary h-8 w-8 p-0"
                                            aria-label=t_string!(i18n, list_item_row_mark_acquired)
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
                            <td class="px-3 py-3 align-middle">
                                <input
                                    type="checkbox"
                                    prop:checked=move || temp_item.with(|i| i.hq)
                                    on:click=move |_| {
                                        temp_item.update(|w| w.hq = Some(!w.hq.unwrap_or_default()))
                                    }
                                />

                            </td>
                            <td class="px-3 py-3 align-middle">
                                <div class="flex min-w-0 items-center gap-3">
                                    <ItemIcon item_id=item.item_id icon_size=IconSize::Small />
                                    <div class="min-w-0">
                                        <div class="flex min-w-0 items-center gap-2">
                                            <span class="min-w-0 truncate font-semibold">
                                                {game_items
                                                    .get(&ItemId(item.item_id))
                                                    .map(|item| item.name.as_str())}
                                            </span>
                                            <Clipboard clipboard_text=game_items
                                                .get(&ItemId(item.item_id))
                                                .map(|item| item.name.to_string())
                                                .unwrap_or_default() />
                                        </div>
                                        {game_items
                                            .get(&ItemId(item.item_id))
                                            .map(|item| item.item_search_category <= 1)
                                            .unwrap_or_default()
                                            .then(move || {
                                                view! {
                                                    <div class="mt-1 inline-flex items-center gap-1 rounded-md bg-red-500/10 px-2 py-0.5 text-xs text-red-200">
                                                        <Tooltip tooltip_text=t_string!(i18n, list_item_row_not_marketable_tooltip).to_string()>
                                                            <Icon icon=i::AiExclamationOutlined />
                                                        </Tooltip>
                                                        <span>{t!(i18n, list_item_row_unavailable_on_market)}</span>
                                                    </div>
                                                }
                                            })}

                                    </div>
                                </div>
                            </td>
                            <td class="px-3 py-3 align-middle">
                                <div class="flex flex-col gap-1">
                                    <label class="text-xs">{t_string!(i18n, list_item_row_qty_label)}</label>
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

                                    <label class="text-xs">{t_string!(i18n, list_item_row_acquired_label)}</label>
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
                            <td class="px-3 py-3 align-middle">
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
                            <td class="px-3 py-3 align-middle">
                                <Tooltip tooltip_text=t_string!(i18n, list_item_row_create_alert).to_string()>
                                    <button
                                        class="btn-secondary h-8 w-8 p-0"
                                        aria-label=t_string!(i18n, list_item_row_create_alert)
                                        on:click=move |_| set_alert_drawer_open.set(true)
                                    >
                                        <Icon icon=i::BsBell />
                                    </button>
                                </Tooltip>
                                <button
                                    class="btn-secondary h-8 w-8 p-0 hover:text-red-200"
                                    aria-label=t_string!(i18n, list_item_row_delete_aria)
                                    on:click=move |_| {
                                        let _ = delete_item.dispatch(item.id);
                                    }
                                >
                                    <Icon icon=i::BiTrashSolid />
                                </button>
                                <button
                                    class="btn-secondary h-8 w-8 p-0"
                                    aria-label=move || if edit() { t_string!(i18n, list_item_row_save_edit_aria) } else { t_string!(i18n, list_item_row_edit_item_aria) }
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
        <Show when=alert_drawer_open>
            <AlertConfigDrawer
                item_id=item.with(|i| i.item_id)
                item_name={
                    let id = item.with(|i| i.item_id);
                    game_items
                        .get(&ItemId(id))
                        .map(|i| i.name.as_str().to_string())
                        .unwrap_or_default()
                }
                default_world=Signal::derive(|| None)
                set_visible=set_alert_drawer_open.into()
            />
        </Show>
    }
}
