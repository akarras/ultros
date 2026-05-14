use std::cmp::Reverse;
use std::collections::HashSet;

use crate::global_state::xiv_data::tracked_data;

use crate::components::icon::Icon;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::{ListActivity, ListItem, ListPermission};

use crate::api::{
    add_item_to_list, delete_list_item, delete_list_items, edit_list_item, get_list_activity,
    get_list_items_with_listings,
};
use crate::components::{
    add_recipe_to_current_list::AddRecipeToCurrentListModal,
    item_icon::*,
    list::{
        auto_mark_purchases::AutoMarkPurchases, buying_view::BuyingView,
        list_item_row::ListItemRow, list_summary::*,
    },
    list_subscribe_drawer::ListSubscribeDrawer,
    loading::*,
    make_place_importer::*,
    tooltip::*,
};
use crate::i18n::*;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use ultros_api_types::websocket::{FilterPredicate, ServerClient, SocketMessageType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenuState {
    None,
    Item,
    // Recipe is now handled by a modal
    MakePlace,
}

#[component]
pub fn ListView() -> impl IntoView {
    let i18n = use_i18n();
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
    let (activity_update_version, set_activity_update_version) = signal(0);
    let (realtime_status, set_realtime_status) = signal("Connecting".to_string());

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
    let activity_view = Resource::new(
        move || {
            (
                list_id(),
                activity_update_version.get(),
                add_item.version().get(),
                edit_item.version().get(),
                delete_item.version().get(),
                delete_items.version().get(),
            )
        },
        move |(id, _, _, _, _, _)| get_list_activity(id),
    );

    let realtime = use_realtime();
    let list_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let list_market_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let realtime_for_list = realtime.clone();
    Effect::new(move |_| {
        list_subscription.update_value(|sub| *sub = None);
        let id = list_id.get();
        let Some(realtime) = realtime_for_list.clone() else {
            set_realtime_status.set("Offline".to_string());
            return;
        };
        if id != 0 {
            let sub = realtime.subscribe_list(id, move |message| match message {
                ServerClient::Subscribed { .. } => {
                    set_realtime_status.set("Live".to_string());
                }
                ServerClient::ListUpdate(_) => {
                    set_realtime_status.set("Live".to_string());
                    list_view.refetch();
                    activity_view.refetch();
                }
                ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                    set_realtime_status.set("Reconnecting".to_string());
                    list_view.refetch();
                    activity_view.refetch();
                }
                _ => {}
            });
            list_subscription.set_value(Some(sub));
        }
    });
    let realtime_for_market = realtime.clone();
    Effect::new(move |_| {
        list_market_subscription.update_value(|sub| *sub = None);
        let Some(Ok((list, items))) = list_view.get() else {
            return;
        };
        let item_ids = items
            .iter()
            .map(|(item, _)| item.item_id)
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            return;
        }
        let Some(realtime) = realtime_for_market.clone() else {
            return;
        };
        let filter =
            FilterPredicate::World(list.list.wdr_filter).and(FilterPredicate::Items(item_ids));
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            if matches!(
                message,
                ServerClient::Listings(_) | ServerClient::Stale { .. }
            ) {
                list_view.refetch();
            }
        });
        list_market_subscription.set_value(Some(sub));
    });
    on_cleanup(move || {
        list_subscription.update_value(|sub| *sub = None);
        list_market_subscription.update_value(|sub| *sub = None);
    });

    let (menu, set_menu) = signal(MenuState::None);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);
    let (buying_view, set_buying_view) = signal(false);
    let (subscribe_open, set_subscribe_open) = signal(false);

    let edit_list_mode = RwSignal::new(false);
    let selected_items = RwSignal::new(HashSet::new());

    // Auto-mark logic moved to AutoMarkPurchases component

    view! {
        <div class="flex flex-col gap-4">
            <AutoMarkPurchases list_view=list_view />

            <div class="panel rounded-lg p-3">
                <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                    <div class="flex flex-wrap items-center gap-2">
                        <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_item).to_string()>
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
                                <Icon icon=i::BiPlusRegular />
                                <span>{t!(i18n, list_view_add_item)}</span>
                            </button>
                        </Tooltip>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_recipe).to_string()>
                            <button
                                class="btn-secondary"
                                class:active=move || recipe_modal_open()
                                on:click=move |_| set_recipe_modal_open(true)
                            >
                                <Icon icon=i::BiBookAddRegular />
                                <span>{t!(i18n, list_view_add_recipe)}</span>
                            </button>
                        </Tooltip>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_import_item).to_string()>
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
                                <Icon icon=i::BiImportRegular />
                                <span>{t!(i18n, list_view_make_place)}</span>
                            </button>
                        </Tooltip>
                    </div>

                    <div class="flex flex-wrap gap-2 self-start lg:self-auto">
                        <Tooltip tooltip_text=t_string!(i18n, list_view_subscribe_tooltip).to_string()>
                            <button
                                class="btn-secondary"
                                aria-label=t_string!(i18n, list_view_subscribe_aria)
                                on:click=move |_| set_subscribe_open(true)
                            >
                                <Icon icon=i::BsBell />
                                <span>{t!(i18n, list_view_subscribe_button)}</span>
                            </button>
                        </Tooltip>
                        <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_purchasing_view).to_string()>
                            <button
                                class="btn-secondary"
                                class:bg-brand-900=buying_view
                                class:border-brand-500=buying_view
                                class:active=buying_view
                                on:click=move |_| set_buying_view.update(|v| *v = !*v)
                            >
                                <Icon icon=i::BiCartRegular />
                                <span>{t!(i18n, list_view_purchasing_view)}</span>
                            </button>
                        </Tooltip>
                    </div>
                </div>
            </div>

            <Show when=recipe_modal_open>
                <AddRecipeToCurrentListModal
                    list_id=list_id
                    set_visible=set_recipe_modal_open
                    on_success=move || {
                        set_external_update_version.update(|v| *v += 1);
                        set_activity_update_version.update(|v| *v += 1);
                        set_recipe_modal_open(false);
                    }
                />
            </Show>

            <Show when=subscribe_open>
                {move || {
                    let name = list_view
                        .get()
                        .and_then(|r| r.ok().map(|(l, _)| l.list.name))
                        .unwrap_or_else(|| format!("List {}", list_id()));
                    view! {
                        <ListSubscribeDrawer
                            list_id=list_id()
                            list_name=name
                            set_visible=set_subscribe_open.into()
                        />
                    }
                }}
            </Show>

            {move || match menu() {
                MenuState::Item => {
                    Some(
                        Either::Left({
                            let (search, set_search) = signal("".to_string());
                            let items = &tracked_data().items;
                            let item_search = move || {
                                search
                                    .with(|s| {
                                        let s_lower = s.to_lowercase();
                                        let mut score = items
                                            .iter()
                                            .filter(|(_, i)| i.item_search_category > 0)
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
                                                Reverse(i.level_item),
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
                                <section class="panel rounded-lg p-4 space-y-4">
                                    <div class="flex flex-col gap-2">
                                        <label class="text-sm font-semibold text-[color:var(--brand-fg)]">{t!(i18n, list_view_add_item_to_list)}</label>
                                        <input
                                            class="input w-full"
                                            placeholder=t_string!(i18n, list_view_search_items).to_string()
                                            prop:value=search
                                            on:input=move |input| set_search(event_target_value(&input))
                                        />
                                        {move || add_result.get().map(|v| {
                                            let text = match v {
                                                Ok(()) => t_string!(i18n, list_view_added_to_list_success).to_string(),
                                                Err(e) => format!("{} {e}", t_string!(i18n, list_view_failed_to_add)),
                                            };
                                            view! { <div class="text-sm text-[color:var(--color-text-muted)]">{text}</div> }.into_view()
                                        })}
                                    </div>
                                    <div class="grid gap-2 max-h-96 overflow-y-auto pr-1">
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
                                                        <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-2 flex flex-col gap-3 sm:flex-row sm:items-center">
                                                            <div class="flex min-w-0 flex-1 items-center gap-3">
                                                                <ItemIcon item_id=id.0 icon_size=IconSize::Medium />
                                                                <span class="min-w-0 truncate font-semibold">{item.name.as_str()}</span>
                                                            </div>
                                                            <div class="flex items-center gap-2">
                                                                <label class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, list_view_qty)}</label>
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
                                                                        Either::Left(view! { <span>{t!(i18n, list_view_adding)}</span> })
                                                                    } else {
                                                                        Either::Right(view! {
                                                                            <>
                                                                                <Icon icon=i::BiPlusRegular />
                                                                                <span>{t!(i18n, list_view_add)}</span>
                                                                            </>
                                                                        })
                                                                    }}
                                                                </button>
                                                            </div>
                                                        </div>
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                        }}

                                    </div>
                                </section>
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
                                <section class="panel rounded-lg p-4">
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
                                </section>
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
                                Either::Left(move || {
                                    let item_snapshot = items.get_value();
                                    let total_items = item_snapshot.len();
                                    let remaining_items = item_snapshot
                                        .iter()
                                        .filter(|(item, _)| {
                                            item.quantity.unwrap_or(1)
                                                > item.acquired.unwrap_or(0)
                                        })
                                        .count();
                                    let acquired_items = total_items.saturating_sub(remaining_items);
                                    let can_write = list.permission >= ListPermission::Write;
                                    let list_name = list.list.name.clone();

                                    if buying_view() {
                                        Either::Left(
                                            view! {
                                                <section class="panel rounded-lg overflow-hidden">
                                                    <div class="border-b border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <div class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                                                            <div>
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">"Shopping route"</p>
                                                                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{list_name.clone()}</h1>
                                                            </div>
                                                            <div class="flex flex-wrap gap-2 text-sm">
                                                                <span class="rounded-lg border border-[color:var(--color-outline)] px-3 py-1 text-[color:var(--color-text-muted)]">
                                                                    {realtime_status}
                                                                </span>
                                                                <span class="rounded-lg border border-[color:var(--color-outline)] px-3 py-1 text-[color:var(--color-text-muted)]">
                                                                    {format!("{remaining_items} remaining")}
                                                                </span>
                                                            </div>
                                                        </div>
                                                    </div>
                                                    <div class="p-4 sm:p-5">
                                                        <BuyingView items=items.get_value() edit_item=edit_item />
                                                    </div>
                                                </section>
                                            },
                                        )
                                    } else {
                                        Either::Right(
                                            view! {
                                                <section class="panel rounded-lg overflow-hidden">
                                                    <div class="border-b border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
                                                            <div>
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">"List"</p>
                                                                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{list_name.clone()}</h1>
                                                                <div class="mt-2 inline-flex rounded-lg border border-[color:var(--color-outline)] px-3 py-1 text-xs text-[color:var(--color-text-muted)]">
                                                                    {realtime_status}
                                                                </div>
                                                            </div>
                                                            <div class="grid grid-cols-3 gap-2 text-center text-sm">
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{total_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">"Items"</div>
                                                                </div>
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{remaining_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">"Remaining"</div>
                                                                </div>
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{acquired_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">"Done"</div>
                                                                </div>
                                                            </div>
                                                        </div>
                                                    </div>

                                                    <div class="flex flex-col gap-3 border-b border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)]/60 p-3 lg:flex-row lg:items-center lg:justify-between">
                                                        <div class="flex flex-wrap items-center gap-2">
                                                            <button
                                                                class="btn-secondary"
                                                                class:bg-brand-950=edit_list_mode
                                                                disabled=move || !can_write
                                                                on:click=move |_| {
                                                                    edit_list_mode
                                                                        .update(|u| {
                                                                            *u = !*u;
                                                                        })
                                                                }
                                                            >
                                                                <Icon icon=i::BsPencilFill />
                                                                <span>{t!(i18n, list_view_bulk_edit)}</span>
                                                            </button>
                                                            <div class:hidden=move || !edit_list_mode()>
                                                                <button
                                                                    class="btn-danger"
                                                                    disabled=move || !can_write
                                                                    on:click=move |_| {
                                                                        let items = selected_items
                                                                            .with_untracked(|s| {
                                                                                s.iter().copied().collect::<Vec<_>>()
                                                                            });
                                                                        selected_items.update(|i| i.clear());
                                                                        delete_items.dispatch(items);
                                                                    }
                                                                >
                                                                    <Icon icon=i::BiTrashSolid />
                                                                    <span>{t!(i18n, list_view_delete)}</span>
                                                                </button>
                                                            </div>
                                                        </div>
                                                        <div
                                                            class="flex flex-wrap items-center gap-2"
                                                            class:hidden=move || !edit_list_mode()
                                                        >
                                                            <button
                                                                class="btn-secondary"
                                                                on:click=move |_| {
                                                                    selected_items
                                                                        .update(|i| {
                                                                            for (item, _) in items.get_value() {
                                                                                i.insert(item.id);
                                                                            }
                                                                        })
                                                                }
                                                            >
                                                                {t!(i18n, list_view_select_all)}
                                                            </button>
                                                            <button
                                                                class="btn-secondary"
                                                                on:click=move |_| {
                                                                    selected_items.update(|i| i.clear());
                                                                }
                                                            >
                                                                {t!(i18n, list_view_deselect_all)}
                                                            </button>
                                                        </div>
                                                    </div>

                                                    <div class="overflow-x-auto">
                                                        <table class="w-full min-w-[760px] text-sm">
                                                            <thead>
                                                                <tr class="border-b border-[color:var(--color-outline)] bg-[color:var(--color-background)]/80 text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                                    <th
                                                                        scope="col"
                                                                        class="w-12 px-3 py-3 text-left"
                                                                        class:hidden=move || !edit_list_mode()
                                                                    >
                                                                        "Select"
                                                                    </th>
                                                                    <th scope="col" class="w-16 px-3 py-3 text-left">{t!(i18n, list_view_hq)}</th>
                                                                    <th scope="col" class="px-3 py-3 text-left">{t!(i18n, list_view_item)}</th>
                                                                    <th scope="col" class="w-40 px-3 py-3 text-left">{t!(i18n, list_view_quantity)}</th>
                                                                    <th scope="col" class="px-3 py-3 text-left">{t!(i18n, list_view_price)}</th>
                                                                    <th
                                                                        scope="col"
                                                                        class="w-44 px-3 py-3 text-right"
                                                                        class:hidden=edit_list_mode
                                                                    >
                                                                        {t!(i18n, list_view_options)}
                                                                    </th>
                                                                </tr>
                                                            </thead>
                                                            <tbody class="divide-y divide-[color:var(--color-outline)]">
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
                                                    </div>
                                                    <div class="p-4 sm:p-5">
                                                        <ListSummary items=items.get_value() />
                                                    </div>
                                                    <div class="border-t border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <ActivityFeed activity=activity_view />
                                                    </div>
                                                </section>
                                            },
                                        )
                                    }
                                })
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        // TODO full table?
                                        // let price_view = items.iter().flat_map(|(list, listings): &(ListItem, Vec<ActiveListing>)| listings.iter().map(|listing| {
                                        // ShoppingListRow { item_id: ItemKey(ItemId(list.item_id)), amount: listing.quantity, lowest_price: listing.price_per_unit, lowest_price_world: listing.world_id.to_string(), lowest_price_datacenter: "TODO".to_string() }
                                        // })).collect::<Vec<_>>();
                                        // <TableContent rows=price_view on_change=move |_| {} />

                                        <div class="panel rounded-lg p-4">{format!("{}\n{e}", t_string!(i18n, list_view_failed_to_get_items))}</div>
                                    },
                                )
                            }
                        })
                }}

            </Transition>
        </div>
    }.into_any()
}

#[component]
fn ActivityFeed(
    activity: Resource<Result<Vec<ListActivity>, crate::error::AppError>>,
) -> impl IntoView {
    view! {
        <section class="flex flex-col gap-3">
            <h2 class="text-lg font-bold text-[color:var(--brand-fg)]">"Activity"</h2>
            <Suspense fallback=move || {
                view! { <div class="text-sm text-[color:var(--color-text-muted)]">"Loading activity..."</div> }
            }>
                {move || {
                    activity
                        .get()
                        .map(|result| match result {
                            Ok(rows) if rows.is_empty() => {
                                view! {
                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-4 text-sm text-[color:var(--color-text-muted)]">
                                        "No list activity yet."
                                    </div>
                                }
                                    .into_any()
                            }
                            Ok(rows) => {
                                view! {
                                    <ol class="flex flex-col gap-2">
                                        <For
                                            each=move || rows.clone()
                                            key=|activity| activity.id
                                            children=move |activity| {
                                                view! {
                                                    <li class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                        <div class="text-sm font-semibold text-[color:var(--color-text)]">{activity.message}</div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)]">
                                                            {activity.created_at.format("%Y-%m-%d %H:%M UTC").to_string()}
                                                        </div>
                                                    </li>
                                                }
                                            }
                                        />
                                    </ol>
                                }
                                    .into_any()
                            }
                            Err(e) => {
                                view! {
                                    <div class="rounded-lg border border-red-400/40 p-4 text-sm text-red-200">{format!("{e}")}</div>
                                }
                                    .into_any()
                            }
                        })
                }}
            </Suspense>
        </section>
    }
}
