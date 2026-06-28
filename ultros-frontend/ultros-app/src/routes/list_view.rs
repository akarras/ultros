use std::cmp::Reverse;
use std::collections::HashSet;

use crate::global_state::xiv_data::tracked_data;

use crate::components::icon::Icon;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::{ListActivity, ListCapabilities, ListItem};

use crate::api::{
    add_item_to_list, delete_list_item, delete_list_items, edit_list, edit_list_item,
    edit_list_items_hq, get_list_activity, get_list_items_with_listings,
};
use crate::components::{
    add_recipe_to_current_list::AddRecipeToCurrentListModal,
    item_icon::*,
    list::{
        auto_mark_purchases::AutoMarkPurchases, buying_view::BuyingView,
        list_item_row::ListItemRow, list_settings_drawer::ListSettingsDrawer, list_summary::*,
    },
    list_subscribe_drawer::ListSubscribeDrawer,
    loading::*,
    make_place_importer::*,
    meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle},
    realtime_status::RealtimeStatus,
    tooltip::*,
};
use crate::i18n::*;
use crate::ws::realtime::{RealtimeSubscription, use_realtime};
use ultros_api_types::websocket::{
    FilterPredicate, ServerClient, SocketMessageType, is_list_market_update_relevant,
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
    let edit_items_hq = Action::new(move |(items, hq): &(Vec<i32>, Option<bool>)| {
        edit_list_items_hq(items.clone(), *hq)
    });
    let edit_list_action =
        Action::new(move |list: &ultros_api_types::list::List| edit_list(list.clone()));

    // We need to trigger refetch when items are added via modal.
    // We can use a signal for versioning external updates.
    let (external_update_version, set_external_update_version) = signal(0);
    let (activity_update_version, set_activity_update_version) = signal(0);
    let (realtime_status, set_realtime_status) = signal("connecting".to_string());
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
    #[allow(unused_variables)]
    let (clock_tick, set_clock_tick) = signal(0_u32);

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
                    edit_items_hq.version().get(),
                    edit_list_action.version().get(),
                ),
            )
        },
        move |(id, _)| get_list_items_with_listings(id),
    );
    let user_resource = Resource::new(|| {}, |_| async move { crate::api::get_login().await.ok() });
    let self_user_id = Signal::derive(move || user_resource.get().flatten().map(|u| u.id));

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
            set_realtime_status.set("offline".to_string());
            return;
        };
        if id != 0 {
            let sub = realtime.subscribe_list(id, move |message| match message {
                ServerClient::Subscribed { .. } => {
                    set_realtime_status.set("live".to_string());
                }
                ServerClient::ListUpdate(_) => {
                    set_realtime_status.set("live".to_string());
                    set_last_update_at.set(Some(chrono::Utc::now()));
                    list_view.refetch();
                    activity_view.refetch();
                }
                ServerClient::Stale { .. } | ServerClient::Error { .. } => {
                    set_realtime_status.set("reconnecting".to_string());
                    set_last_update_at.set(Some(chrono::Utc::now()));
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
        let filter = FilterPredicate::World(list.list.wdr_filter)
            .and(FilterPredicate::Items(item_ids.clone()));
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            if is_list_market_update_relevant(&message, &item_ids) {
                set_last_update_at.set(Some(chrono::Utc::now()));
                list_view.refetch();
            }
        });
        list_market_subscription.set_value(Some(sub));
    });
    on_cleanup(move || {
        list_subscription.update_value(|sub| *sub = None);
        list_market_subscription.update_value(|sub| *sub = None);
    });

    #[cfg(not(feature = "ssr"))]
    {
        use gloo_timers::callback::Interval;
        let interval = Interval::new(1_000, move || {
            set_clock_tick.update(|n| *n = n.wrapping_add(1));
        });
        interval.forget();
    }

    let (menu, set_menu) = signal(MenuState::None);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);
    let (buying_view, set_buying_view) = signal(false);
    let (subscribe_open, set_subscribe_open) = signal(false);
    let (settings_open, set_settings_open) = signal(false);
    let (rename_open, set_rename_open) = signal(false);
    let (rename_value, set_rename_value) = signal(String::new());

    let edit_list_mode = RwSignal::new(false);
    let selected_items = RwSignal::new(HashSet::new());
    let excluded_datacenters = RwSignal::new(HashSet::<String>::new());

    type RowSnapshot = std::collections::HashMap<i32, (Option<i32>, Option<i32>)>;
    let recently_changed: RwSignal<HashSet<i32>> = RwSignal::new(HashSet::new());
    let prev_snapshot: StoredValue<RowSnapshot> = StoredValue::new(RowSnapshot::new());

    Effect::new(move |_| {
        let Some(Ok((_list, items))) = list_view.get() else {
            return;
        };
        let new_snapshot: RowSnapshot = items
            .iter()
            .map(|(i, _)| (i.id, (i.quantity, i.acquired)))
            .collect();
        let mut newly_changed: HashSet<i32> = HashSet::new();
        let prev = prev_snapshot.get_value();
        for (id, current) in &new_snapshot {
            if let Some(prior) = prev.get(id)
                && prior != current
            {
                newly_changed.insert(*id);
            }
        }
        prev_snapshot.set_value(new_snapshot);

        if !newly_changed.is_empty() {
            recently_changed.update(|set| set.extend(newly_changed.iter().copied()));
            #[cfg(not(feature = "ssr"))]
            {
                use gloo_timers::callback::Timeout;
                let ids: Vec<i32> = newly_changed.into_iter().collect();
                Timeout::new(1500, move || {
                    recently_changed.update(|set| {
                        for id in &ids {
                            set.remove(id);
                        }
                    });
                })
                .forget();
            }
        }
    });

    let view_caps = RwSignal::new(ListCapabilities::default());
    Effect::new(move |_| {
        let next = match list_view.get() {
            Some(Ok((list_with_perm, _))) => ListCapabilities::from(list_with_perm.permission),
            _ => ListCapabilities::default(),
        };
        view_caps.set(next);
    });

    let drawer_refresh = Signal::derive(move || {
        last_update_at
            .get()
            .map(|t| t.timestamp_millis() as u32)
            .unwrap_or(0)
    });

    // Auto-mark logic moved to AutoMarkPurchases component

    let list_name_for_meta = Signal::derive(move || {
        list_view
            .get()
            .and_then(|r| r.ok().map(|(l, _)| l.list.name))
            .unwrap_or_default()
    });
    let meta_title = move || {
        let name = list_name_for_meta.get();
        if name.is_empty() {
            t_string!(i18n, list_view_default_meta_title).to_string()
        } else {
            t_string!(i18n, list_view_meta_title)
                .to_string()
                .replace("%name%", &name)
        }
    };

    view! {
        <MetaTitle title=meta_title />
        <MetaDescription text=move || t_string!(i18n, list_view_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        <div class="flex flex-col gap-4">
            <AutoMarkPurchases list_view=list_view />

            <div class="panel rounded-lg p-3">
                <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between list-toolbar">
                    <div class="flex flex-wrap items-center gap-2">
                        <Show when=move || view_caps.with(|c| c.can_write)>
                            <>
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
                            </>
                        </Show>
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
                        <Tooltip tooltip_text=t_string!(i18n, list_view_settings_tooltip).to_string()>
                            <button
                                class="btn-secondary"
                                aria-label=t_string!(i18n, list_view_settings)
                                data-testid="list-settings-btn"
                                on:click=move |_| set_settings_open(true)
                            >
                                <Icon icon=i::BsGear />
                                <span>{t!(i18n, list_view_settings)}</span>
                            </button>
                        </Tooltip>
                    </div>
                </div>
            </div>

            <div class="panel rounded-lg p-3">
                <div class="flex flex-wrap items-center gap-3">
                    <span class="text-xs font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                        {t!(i18n, list_view_exclude_datacenters)}
                    </span>
                    <div class="flex flex-wrap gap-2">
                        {move || {
                            let world_data = use_context::<crate::global_state::LocalWorldData>();
                            let helper = world_data.as_ref().and_then(|d| d.0.as_ref().ok());
                            let list_data = list_view.get();
                            match (helper, list_data) {
                                (Some(helper), Some(Ok((list, _)))) => {
                                    let filter = list.list.wdr_filter;
                                    let datacenters = helper
                                        .lookup_selector(filter)
                                        .map(|r| helper.get_datacenters(&r))
                                        .unwrap_or_default();
                                    datacenters
                                        .into_iter()
                                        .map(|dc| {
                                            let name = dc.name.clone();
                                            let is_excluded = Signal::derive(move || {
                                                excluded_datacenters.with(|set| set.contains(&name))
                                            });
                                            let toggle = {
                                                let name = dc.name.clone();
                                                move |_| {
                                                    excluded_datacenters
                                                        .update(|set| {
                                                            if set.contains(&name) {
                                                                set.remove(&name);
                                                            } else {
                                                                set.insert(name.clone());
                                                            }
                                                        })
                                                }
                                            };
                                            view! {
                                                <button
                                                    class="btn-secondary px-3 py-1 text-xs"
                                                    class:bg-red-950=is_excluded
                                                    class:text-red-200=is_excluded
                                                    class:border-red-400=is_excluded
                                                    on:click=toggle
                                                >
                                                    {dc.name.clone()}
                                                </button>
                                            }
                                        })
                                        .collect_view()
                                        .into_any()
                                }
                                _ => ().into_any(),
                            }
                        }}
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
                                    let total_quantity: i32 = item_snapshot
                                        .iter()
                                        .map(|(i, _)| i.quantity.unwrap_or(1).max(1))
                                        .sum();
                                    let total_acquired: i32 = item_snapshot
                                        .iter()
                                        .map(|(i, _)| {
                                            let q = i.quantity.unwrap_or(1).max(1);
                                            i.acquired.unwrap_or(0).clamp(0, q)
                                        })
                                        .sum();
                                    let pct: i32 = if total_quantity > 0 {
                                        100 * total_acquired / total_quantity
                                    } else {
                                        0
                                    };
                                    let list_name = list.list.name.clone();

                                    if buying_view() {
                                        Either::Left(
                                            view! {
                                                <section class="panel rounded-lg overflow-hidden">
                                                    <div class="border-b border-[color:var(--color-outline)] p-4 sm:p-5">
                                                        <div class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                                                            <div>
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, list_view_shopping_route)}</p>
                                                                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{list_name.clone()}</h1>
                                                            </div>
                                                            <div class="flex flex-wrap gap-2 text-sm">
                                                                <RealtimeStatus
                                                                    status=realtime_status
                                                                    last_update=last_update_at
                                                                />
                                                                <span class="rounded-lg border border-[color:var(--color-outline)] px-3 py-1 text-[color:var(--color-text-muted)]">
                                                                    {format!("{remaining_items} remaining")}
                                                                </span>
                                                            </div>
                                                        </div>
                                                    </div>
                                                    <div class="p-4 sm:p-5">
                                                        <BuyingView
                                                            items=items.get_value()
                                                            edit_item=edit_item
                                                            excluded_datacenters=excluded_datacenters
                                                        />
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
                                                                <p class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, list_view_list_label)}</p>
                                                                <div class="flex items-center gap-2">
                                                                    {
                                                                        let list_for_title = list.list.clone();
                                                                        let display_name = list_for_title.name.clone();
                                                                        move || {
                                                                            if rename_open() && view_caps.with(|c| c.can_admin) {
                                                                                let list_for_save = list_for_title.clone();
                                                                                Either::Left(view! {
                                                                                    <div class="flex flex-wrap items-center gap-2">
                                                                                        <input
                                                                                            class="input text-xl font-bold"
                                                                                            prop:value=rename_value
                                                                                            on:input=move |ev| set_rename_value(event_target_value(&ev))
                                                                                            data-testid="list-rename-input"
                                                                                        />
                                                                                        <button
                                                                                            class="btn-primary"
                                                                                            data-testid="list-rename-save"
                                                                                            on:click={
                                                                                                let list_for_save = list_for_save.clone();
                                                                                                move |_| {
                                                                                                    let mut new_list = list_for_save.clone();
                                                                                                    new_list.name = rename_value().trim().to_string();
                                                                                                    if !new_list.name.is_empty() {
                                                                                                        edit_list_action.dispatch(new_list);
                                                                                                        set_rename_open(false);
                                                                                                    }
                                                                                                }
                                                                                            }
                                                                                        >
                                                                                            <Icon icon=i::BiSaveSolid />
                                                                                            <span>{t!(i18n, list_view_settings_save)}</span>
                                                                                        </button>
                                                                                        <button
                                                                                            class="btn-secondary"
                                                                                            on:click=move |_| set_rename_open(false)
                                                                                        >
                                                                                            {t!(i18n, list_view_settings_cancel)}
                                                                                        </button>
                                                                                    </div>
                                                                                })
                                                                            } else {
                                                                                let display_name = display_name.clone();
                                                                                Either::Right(view! {
                                                                                    <>
                                                                                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{display_name.clone()}</h1>
                                                                                        <Show when=move || view_caps.with(|c| c.can_admin)>
                                                                                            <button
                                                                                                class="btn-ghost p-1"
                                                                                                aria-label=t_string!(i18n, edit_list).to_string()
                                                                                                data-testid="list-rename-btn"
                                                                                                on:click={
                                                                                                    let name = display_name.clone();
                                                                                                    move |_| {
                                                                                                        set_rename_value(name.clone());
                                                                                                        set_rename_open(true);
                                                                                                    }
                                                                                                }
                                                                                            >
                                                                                                <Icon icon=i::BsPencilFill />
                                                                                            </button>
                                                                                        </Show>
                                                                                    </>
                                                                                })
                                                                            }
                                                                        }
                                                                    }
                                                                </div>
                                                                <div class="mt-2">
                                                                    <RealtimeStatus
                                                                        status=realtime_status
                                                                        last_update=last_update_at
                                                                    />
                                                                </div>
                                                                <div class="mt-3 flex items-center gap-3 text-sm">
                                                                    {if total_quantity > 0 {
                                                                        Either::Left(view! {
                                                                            <div
                                                                                class="flex min-w-0 flex-1 flex-col gap-1"
                                                                                aria-label=format!("Overall progress: {total_acquired} of {total_quantity} units acquired")
                                                                            >
                                                                                <span class="text-[color:var(--color-text-muted)]">
                                                                                    {t!(i18n, list_view_units_acquired_progress, acquired = total_acquired, quantity = total_quantity, pct = pct)}
                                                                                </span>
                                                                                <progress
                                                                                    class="progress progress-primary h-2 w-full rounded"
                                                                                    value=total_acquired
                                                                                    max=total_quantity
                                                                                ></progress>
                                                                            </div>
                                                                        })
                                                                    } else {
                                                                        Either::Right(view! {
                                                                            <span class="text-[color:var(--color-text-muted)]">
                                                                                {t!(i18n, list_view_no_items_yet)}
                                                                            </span>
                                                                        })
                                                                    }}
                                                                </div>
                                                            </div>
                                                            <div class="grid grid-cols-3 gap-2 text-center text-sm">
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{total_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, item_explorer_items)}</div>
                                                                </div>
                                                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                    <div class="text-lg font-bold">{remaining_items}</div>
                                                                    <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_remaining)}</div>
                                                                </div>
                                                                <Tooltip tooltip_text=Signal::derive(move || {
                                                                    format!("{acquired_items} of {total_items} items fully acquired")
                                                                })>
                                                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                                        <div class="text-lg font-bold">{acquired_items}</div>
                                                                        <div class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, list_view_acquired)}</div>
                                                                    </div>
                                                                </Tooltip>
                                                            </div>
                                                        </div>
                                                    </div>

                                                    <Show when=move || view_caps.with(|c| c.can_write)>
                                                        <div class="flex flex-col gap-3 border-b border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)]/60 p-3 lg:flex-row lg:items-center lg:justify-between">
                                                            <div class="flex flex-wrap items-center gap-2">
                                                                <button
                                                                    class="btn-secondary"
                                                                    class:bg-brand-950=edit_list_mode
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
                                                                    <button
                                                                        class="btn-secondary"
                                                                        on:click=move |_| {
                                                                            let items = selected_items
                                                                                .with_untracked(|s| {
                                                                                    s.iter().copied().collect::<Vec<_>>()
                                                                                });
                                                                            edit_items_hq.dispatch((items, Some(true)));
                                                                        }
                                                                    >
                                                                        <span>{t!(i18n, list_view_bulk_set_hq)}</span>
                                                                    </button>
                                                                    <button
                                                                        class="btn-secondary"
                                                                        on:click=move |_| {
                                                                            let items = selected_items
                                                                                .with_untracked(|s| {
                                                                                    s.iter().copied().collect::<Vec<_>>()
                                                                                });
                                                                            edit_items_hq.dispatch((items, None));
                                                                        }
                                                                    >
                                                                        <span>{t!(i18n, list_view_bulk_any_quality)}</span>
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
                                                    </Show>

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
                                                                    <th scope="col" class="w-40 px-3 py-3 text-left">{t!(i18n, list_view_acquired_quantity)}</th>
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
                                                                                recently_changed=recently_changed
                                                                                can_write=Signal::derive(move || view_caps.with(|c| c.can_write))
                                                                                excluded_worlds=&[]
                                                                                excluded_datacenters=excluded_datacenters
                                                                            />
                                                                        }
                                                                    }
                                                                />

                                                            </tbody>
                                                        </table>
                                                    </div>
                                                    <div class="p-4 sm:p-5">
                                                        <ListSummary
                                                            items=items.get_value()
                                                            excluded_worlds=&[]
                                                            excluded_datacenters=excluded_datacenters
                                                        />
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

            <Show when=settings_open>
                {move || {
                    let Some(Ok((list_with_perm, _))) = list_view.get() else {
                        return view! { <div></div> }.into_any();
                    };
                    view! {
                        <ListSettingsDrawer
                            list=list_with_perm.list.clone()
                            permission=list_with_perm.permission
                            self_user_id=self_user_id
                            edit_list=edit_list_action
                            refresh_signal=drawer_refresh
                            set_visible=set_settings_open
                        />
                    }
                    .into_any()
                }}
            </Show>
        </div>
    }.into_any()
}

#[component]
fn ActivityFeed(
    activity: Resource<Result<Vec<ListActivity>, crate::error::AppError>>,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <section class="flex flex-col gap-3">
            <h2 class="text-lg font-bold text-[color:var(--brand-fg)]">{t!(i18n, list_view_activity_heading)}</h2>
            <Suspense fallback=move || {
                view! { <div class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, list_view_loading_activity)}</div> }
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
                                        // ⚡ Bolt Optimization: Using collect_view() instead of <For> to prevent unnecessary cloning of rows inside a conditional block that completely recreates the view.
                                        {rows.into_iter().map(|activity| {
                                                view! {
                                                    <li class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2">
                                                        <div class="text-sm font-semibold text-[color:var(--color-text)]">{activity.message}</div>
                                                        <div class="text-xs text-[color:var(--color-text-muted)]">
                                                            {activity.created_at.format("%Y-%m-%d %H:%M UTC").to_string()}
                                                        </div>
                                                    </li>
                                                }
                                        }).collect_view()}
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
