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
    #[prop(into)] listings: Signal<Vec<ActiveListing>>,
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
    recently_changed: RwSignal<HashSet<i32>>,
    can_write: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let game_items = &data.items;

    let (edit, set_edit) = signal(false);
    let (alert_drawer_open, set_alert_drawer_open) = signal(false);
    let item = RwSignal::new(item);
    let temp_item = RwSignal::new(item());

    view! {
        <tr class=move || {
            let item_now = item.get();
            let q = item_now.quantity.unwrap_or(1).max(1);
            let a = item_now.acquired.unwrap_or(0);
            let complete = a >= q;
            let highlighted = recently_changed.with(|set| set.contains(&item_now.id));
            let highlight_class = if highlighted { " ring-2 ring-brand-400/60" } else { "" };
            if complete {
                format!("group transition-all duration-700 bg-green-900/15 hover:bg-green-900/25{highlight_class}")
            } else {
                format!("group transition-all duration-700 hover:bg-[color:var(--color-background-panel)]{highlight_class}")
            }
        }>
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
                                    let item_now = item.get();
                                    let q = item_now.quantity.unwrap_or(1).max(1);
                                    let a = item_now.acquired.unwrap_or(0);
                                    let complete = a >= q;
                                    view! {
                                        <div class="flex flex-col items-start gap-1">
                                            {move || {
                                                if can_write.get() {
                                                    Either::Left(view! {
                                                        <Tooltip tooltip_text=Signal::derive(move || if item.with(|i| i.hq == Some(true)) { t_string!(i18n, list_view_bulk_any_quality).to_string() } else { t_string!(i18n, list_view_bulk_set_hq).to_string() })>
                                                            <button
                                                                class="inline-flex rounded-md border px-2 py-0.5 text-xs font-bold transition-colors"
                                                                class=("border-[color:var(--brand-ring)]/40", move || item.with(|i| i.hq == Some(true)))
                                                                class=("text-[color:var(--brand-fg)]", move || item.with(|i| i.hq == Some(true)))
                                                                class=("border-[color:var(--color-outline)]", move || item.with(|i| i.hq != Some(true)))
                                                                class=("text-[color:var(--color-text-muted)]", move || item.with(|i| i.hq != Some(true)))
                                                                on:click=move |_| {
                                                                    item.update(|i| {
                                                                        i.hq = if i.hq == Some(true) { None } else { Some(true) };
                                                                    });
                                                                    let _ = edit_item.dispatch(item.get());
                                                                }
                                                            >
                                                                "HQ"
                                                            </button>
                                                        </Tooltip>
                                                    })
                                                } else {
                                                    Either::Right(item_now.hq.and_then(|hq| {
                                                        hq.then_some(view! {
                                                            <span class="inline-flex rounded-md border border-[color:var(--brand-ring)]/40 px-2 py-0.5 text-xs font-bold text-[color:var(--brand-fg)]">
                                                                "HQ"
                                                            </span>
                                                        })
                                                    }))
                                                }
                                            }}
                                            {complete.then(|| view! {
                                                <span
                                                    class="inline-flex items-center gap-1 rounded-md border border-green-400/40 px-1.5 py-0.5 text-xs text-green-200"
                                                    aria-label=t_string!(i18n, list_view_completed_row_aria).to_string()
                                                >
                                                    <Icon icon=i::BsCheckCircle />
                                                </span>
                                            })}
                                        </div>
                                    }
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
                                                    <div class="mt-1 inline-flex items-center gap-1 rounded-md border border-red-400/40 px-2 py-0.5 text-xs text-red-200">
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
                                    let q = item.quantity.unwrap_or(1).max(1);
                                    let a = item.acquired.unwrap_or(0).max(0).min(q);
                                    let complete = a >= q;
                                    view! {
                                        <div class="flex flex-col gap-1 w-full">
                                            <span class=move || if complete {
                                                "text-sm font-semibold text-green-300"
                                            } else {
                                                "text-sm"
                                            }>{format!("{a} / {q}")}</span>
                                            <progress
                                                class="progress progress-primary h-2 w-full rounded"
                                                value=a
                                                max=q
                                            ></progress>
                                        </div>
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
                                    <Show when=move || can_write.get()>
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
                                        <Tooltip tooltip_text=Signal::derive(move || {
                                            let q = item.with(|i| i.quantity.unwrap_or(1).max(1));
                                            let a = item.with(|i| i.acquired.unwrap_or(0));
                                            if a >= q {
                                                t_string!(i18n, list_view_mark_unacquired).to_string()
                                            } else {
                                                t_string!(i18n, list_item_row_mark_acquired).to_string()
                                            }
                                        })>
                                            <button
                                                class="btn-secondary h-8 w-8 p-0"
                                                aria-label=move || {
                                                    let q = item.with(|i| i.quantity.unwrap_or(1).max(1));
                                                    let a = item.with(|i| i.acquired.unwrap_or(0));
                                                    if a >= q {
                                                        t_string!(i18n, list_view_mark_unacquired).to_string()
                                                    } else {
                                                        t_string!(i18n, list_item_row_mark_acquired).to_string()
                                                }
                                            }
                                            on:click=move |_| {
                                                item.update(|i| {
                                                    let q = i.quantity.unwrap_or(1).max(1);
                                                    let a = i.acquired.unwrap_or(0);
                                                    if a >= q {
                                                        i.acquired = Some(0);
                                                    } else {
                                                        i.acquired = i.quantity.or(Some(1));
                                                    }
                                                });
                                                let _ = edit_item.dispatch(item.get());
                                            }
                                        >
                                            <Icon icon=i::BiCheckRegular />
                                        </button>
                                        </Tooltip>
                                    </Show>
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
                                                    <div class="mt-1 inline-flex items-center gap-1 rounded-md border border-red-400/40 px-2 py-0.5 text-xs text-red-200">
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
                                <div class="grid min-w-[26rem] grid-cols-3 gap-2">
                                    <label class="flex flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
                                        <span>{t_string!(i18n, list_item_row_qty_label)}</span>
                                        <input
                                            class="input w-full"
                                            type="number"
                                            min="1"
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
                                    </label>

                                    <label class="flex flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
                                        <span>{t_string!(i18n, list_item_row_acquired_label)}</span>
                                        <input
                                            class="input w-full"
                                            type="number"
                                            min="0"
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
                                    </label>

                                    <label class="flex flex-col gap-1 text-xs text-[color:var(--color-text-muted)]">
                                        <span>{t!(i18n, list_item_row_target_price_label)}</span>
                                        <input
                                            class="input w-full"
                                            type="number"
                                            min="0"
                                            placeholder=move || t_string!(i18n, none).to_string()
                                            prop:value=move || {
                                                temp_item
                                                    .with(|i| {
                                                        i.target_price.map(|v| v.to_string()).unwrap_or_default()
                                                    })
                                            }
                                            on:input=move |e| {
                                                let v = event_target_value(&e);
                                                temp_item
                                                    .update(|i| {
                                                        i.target_price = if v.trim().is_empty() {
                                                            None
                                                        } else {
                                                            v.parse::<i64>().ok().filter(|n| *n >= 0)
                                                        };
                                                    })
                                            }
                                        />
                                    </label>
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
