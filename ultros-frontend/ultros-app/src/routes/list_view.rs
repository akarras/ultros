use std::cmp::Reverse;
use std::collections::HashSet;

use crate::global_state::xiv_data::tracked_data;

use crate::components::icon::Icon;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::hooks::use_params_map;
use ultros_api_types::list::{
    CreateInvite, ListInvite, ListItem, ListPermission, ListSharedGroup, ListSharedUser,
    ShareListGroup, ShareListUser,
};

use crate::api::{
    add_item_to_list, create_list_invite, delete_list_invite, delete_list_item, delete_list_items,
    edit_list_item, get_list_invites, get_list_items_with_listings, get_list_permission,
    get_list_shares, share_list_with_group, share_list_with_user, unshare_list_from_group,
    unshare_list_from_user,
};
use crate::components::{
    add_recipe_to_current_list::AddRecipeToCurrentListModal,
    clipboard::Clipboard,
    item_icon::*,
    list::{
        auto_mark_purchases::AutoMarkPurchases, buying_view::BuyingView,
        list_item_row::ListItemRow, list_summary::*,
    },
    loading::*,
    make_place_importer::*,
    modal::Modal,
    tooltip::*,
};
use crate::global_state::toasts::use_toast;
use crate::i18n::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenuState {
    None,
    Item,
    // Recipe is now handled by a modal
    MakePlace,
}

fn permission_from_input(value: &str) -> ListPermission {
    match value {
        "Write" => ListPermission::Write,
        _ => ListPermission::Read,
    }
}

fn permission_name(permission: ListPermission) -> &'static str {
    match permission {
        ListPermission::Owner => "Owner",
        ListPermission::Write => "Write",
        ListPermission::Read => "Read",
        ListPermission::None => "None",
    }
}

fn invite_url(invite: &ListInvite) -> String {
    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window()
            && let Ok(origin) = window.location().origin()
        {
            return format!("{origin}/invite/{}", invite.id);
        }
    }
    format!("/invite/{}", invite.id)
}

#[component]
fn SharedUsers(
    users: Vec<ListSharedUser>,
    unshare_user: Action<i64, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-2">
            <h3 class="font-semibold text-[color:var(--brand-fg)]">"Shared Users"</h3>
            {if users.is_empty() {
                view! { <div class="text-sm text-[color:var(--color-text-muted)]">"No direct user shares yet."</div> }.into_any()
            } else {
                users
                    .into_iter()
                    .map(|user| {
                        let user_id = user.user_id;
                        view! {
                            <div class="flex items-center justify-between gap-3 rounded border border-[color:var(--brand-border)] p-2">
                                <div class="min-w-0">
                                    <div class="font-semibold truncate">{user.username}</div>
                                    <div class="text-xs text-[color:var(--color-text-muted)]">{format!("{} - {}", user.user_id, permission_name(user.permission))}</div>
                                </div>
                                <button class="btn-danger btn-sm" on:click=move |_| { unshare_user.dispatch(user_id); }>
                                    "Remove"
                                </button>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn SharedGroups(
    groups: Vec<ListSharedGroup>,
    unshare_group: Action<i32, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col gap-2">
            <h3 class="font-semibold text-[color:var(--brand-fg)]">"Shared Groups"</h3>
            {if groups.is_empty() {
                view! { <div class="text-sm text-[color:var(--color-text-muted)]">"No group shares yet."</div> }.into_any()
            } else {
                groups
                    .into_iter()
                    .map(|group| {
                        let group_id = group.group_id;
                        view! {
                            <div class="flex items-center justify-between gap-3 rounded border border-[color:var(--brand-border)] p-2">
                                <div class="min-w-0">
                                    <div class="font-semibold truncate">{group.group_name}</div>
                                    <div class="text-xs text-[color:var(--color-text-muted)]">{format!("{} - {}", group.group_id, permission_name(group.permission))}</div>
                                </div>
                                <button class="btn-danger btn-sm" on:click=move |_| { unshare_group.dispatch(group_id); }>
                                    "Remove"
                                </button>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn ListShareModal(list_id: i32, #[prop(into)] set_visible: SignalSetter<bool>) -> impl IntoView {
    let toasts = use_toast();
    let (user_id, set_user_id) = signal(String::new());
    let (group_id, set_group_id) = signal(String::new());
    let (user_permission, set_user_permission) = signal(ListPermission::Read);
    let (group_permission, set_group_permission) = signal(ListPermission::Read);
    let (invite_permission, set_invite_permission) = signal(ListPermission::Read);
    let (max_uses, set_max_uses) = signal(String::new());

    let share_user =
        Action::new(move |share: &ShareListUser| share_list_with_user(list_id, share.clone()));
    let share_group =
        Action::new(move |share: &ShareListGroup| share_list_with_group(list_id, share.clone()));
    let unshare_user = Action::new(move |user_id: &i64| unshare_list_from_user(list_id, *user_id));
    let unshare_group =
        Action::new(move |group_id: &i32| unshare_list_from_group(list_id, *group_id));
    let create_invite =
        Action::new(move |invite: &CreateInvite| create_list_invite(list_id, invite.clone()));
    let delete_invite =
        Action::new(move |invite_id: &String| delete_list_invite(invite_id.clone()));

    let shares = Resource::new(
        move || {
            (
                list_id,
                share_user.version().get(),
                share_group.version().get(),
                unshare_user.version().get(),
                unshare_group.version().get(),
            )
        },
        move |(list_id, ..)| get_list_shares(list_id),
    );

    let invites = Resource::new(
        move || {
            (
                list_id,
                create_invite.version().get(),
                delete_invite.version().get(),
            )
        },
        move |(list_id, ..)| get_list_invites(list_id),
    );

    Effect::new(move |_| {
        if let Some(result) = share_user.value().get() {
            match result {
                Ok(()) => {
                    set_user_id(String::new());
                    if let Some(toasts) = toasts {
                        toasts.success("User share saved.");
                    }
                }
                Err(error) => {
                    if let Some(toasts) = toasts {
                        toasts.error(format!("Unable to share with user: {error}"));
                    }
                }
            }
        }
    });

    Effect::new(move |_| {
        if let Some(result) = share_group.value().get() {
            match result {
                Ok(()) => {
                    set_group_id(String::new());
                    if let Some(toasts) = toasts {
                        toasts.success("Group share saved.");
                    }
                }
                Err(error) => {
                    if let Some(toasts) = toasts {
                        toasts.error(format!("Unable to share with group: {error}"));
                    }
                }
            }
        }
    });

    Effect::new(move |_| {
        if let Some(result) = create_invite.value().get() {
            match result {
                Ok(_) => {
                    set_max_uses(String::new());
                    if let Some(toasts) = toasts {
                        toasts.success("Invite link created.");
                    }
                }
                Err(error) => {
                    if let Some(toasts) = toasts {
                        toasts.error(format!("Unable to create invite: {error}"));
                    }
                }
            }
        }
    });

    view! {
        <Modal set_visible max_width="max-w-3xl w-[95vw]">
            <div class="flex flex-col gap-6">
                <div>
                    <h2 class="text-2xl font-bold text-[color:var(--brand-fg)]">"Share List"</h2>
                    <p class="text-sm text-[color:var(--color-text-muted)]">"Create invite links or grant access to known user and group IDs."</p>
                </div>

                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div class="panel p-4 rounded-xl flex flex-col gap-3">
                        <h3 class="font-semibold text-[color:var(--brand-fg)]">"Invite Links"</h3>
                        <div class="grid grid-cols-1 sm:grid-cols-[1fr_auto_auto] gap-2 items-end">
                            <label class="flex flex-col gap-1 text-sm">
                                <span class="label">"Max uses"</span>
                                <input
                                    class="input"
                                    type="number"
                                    min="1"
                                    placeholder="Unlimited"
                                    prop:value=max_uses
                                    on:input=move |ev| set_max_uses(event_target_value(&ev))
                                />
                            </label>
                            <label class="flex flex-col gap-1 text-sm">
                                <span class="label">"Access"</span>
                                <select
                                    class="input"
                                    on:change=move |ev| set_invite_permission(permission_from_input(&event_target_value(&ev)))
                                >
                                    <option value="Read" selected=move || invite_permission() == ListPermission::Read>"Read"</option>
                                    <option value="Write" selected=move || invite_permission() == ListPermission::Write>"Write"</option>
                                </select>
                            </label>
                            <button
                                class="btn-primary"
                                prop:disabled=create_invite.pending()
                                on:click=move |_| {
                                    let max_uses = max_uses().trim().parse::<i32>().ok();
                                    create_invite.dispatch(CreateInvite {
                                        permission: invite_permission(),
                                        max_uses,
                                    });
                                }
                            >
                                "Create"
                            </button>
                        </div>
                        {move || match invites.get() {
                            Some(Ok(invites)) => {
                                if invites.is_empty() {
                                    view! { <div class="text-sm text-[color:var(--color-text-muted)]">"No invite links yet."</div> }.into_any()
                                } else {
                                    invites
                                        .into_iter()
                                        .map(|invite| {
                                            let invite_id = invite.id.clone();
                                            let link = invite_url(&invite);
                                            view! {
                                                <div class="rounded border border-[color:var(--brand-border)] p-2 flex flex-col gap-2">
                                                    <div class="flex items-center justify-between gap-2">
                                                        <div class="text-sm min-w-0 truncate">{link.clone()}</div>
                                                        <Clipboard clipboard_text=link />
                                                    </div>
                                                    <div class="flex items-center justify-between gap-2 text-xs text-[color:var(--color-text-muted)]">
                                                        <span>{format!("{} - uses {}{}", permission_name(invite.permission), invite.uses, invite.max_uses.map(|m| format!("/{m}")).unwrap_or_default())}</span>
                                                        <button class="btn-danger btn-sm" on:click=move |_| { delete_invite.dispatch(invite_id.clone()); }>
                                                            "Delete"
                                                        </button>
                                                    </div>
                                                </div>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .into_any()
                                }
                            }
                            Some(Err(error)) => view! { <div class="alert alert-error">{format!("Unable to load invites: {error}")}</div> }.into_any(),
                            None => view! { <Loading /> }.into_any(),
                        }}
                    </div>

                    <div class="panel p-4 rounded-xl flex flex-col gap-3">
                        <h3 class="font-semibold text-[color:var(--brand-fg)]">"Direct Shares"</h3>
                        <div class="grid grid-cols-1 sm:grid-cols-[1fr_auto_auto] gap-2 items-end">
                            <label class="flex flex-col gap-1 text-sm">
                                <span class="label">"User ID"</span>
                                <input class="input" prop:value=user_id on:input=move |ev| set_user_id(event_target_value(&ev)) />
                            </label>
                            <label class="flex flex-col gap-1 text-sm">
                                <span class="label">"Access"</span>
                                <select class="input" on:change=move |ev| set_user_permission(permission_from_input(&event_target_value(&ev)))>
                                    <option value="Read" selected=move || user_permission() == ListPermission::Read>"Read"</option>
                                    <option value="Write" selected=move || user_permission() == ListPermission::Write>"Write"</option>
                                </select>
                            </label>
                            <button
                                class="btn-primary"
                                prop:disabled=share_user.pending()
                                on:click=move |_| {
                                    if let Ok(user_id) = user_id().trim().parse::<i64>() {
                                        share_user.dispatch(ShareListUser {
                                            user_id,
                                            permission: user_permission(),
                                        });
                                    }
                                }
                            >
                                "Share"
                            </button>
                        </div>
                        <div class="grid grid-cols-1 sm:grid-cols-[1fr_auto_auto] gap-2 items-end">
                            <label class="flex flex-col gap-1 text-sm">
                                <span class="label">"Group ID"</span>
                                <input class="input" prop:value=group_id on:input=move |ev| set_group_id(event_target_value(&ev)) />
                            </label>
                            <label class="flex flex-col gap-1 text-sm">
                                <span class="label">"Access"</span>
                                <select class="input" on:change=move |ev| set_group_permission(permission_from_input(&event_target_value(&ev)))>
                                    <option value="Read" selected=move || group_permission() == ListPermission::Read>"Read"</option>
                                    <option value="Write" selected=move || group_permission() == ListPermission::Write>"Write"</option>
                                </select>
                            </label>
                            <button
                                class="btn-primary"
                                prop:disabled=share_group.pending()
                                on:click=move |_| {
                                    if let Ok(group_id) = group_id().trim().parse::<i32>() {
                                        share_group.dispatch(ShareListGroup {
                                            group_id,
                                            permission: group_permission(),
                                        });
                                    }
                                }
                            >
                                "Share"
                            </button>
                        </div>
                    </div>
                </div>

                {move || match shares.get() {
                    Some(Ok((users, groups))) => view! {
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                            <SharedUsers users unshare_user />
                            <SharedGroups groups unshare_group />
                        </div>
                    }.into_any(),
                    Some(Err(error)) => view! { <div class="alert alert-error">{format!("Unable to load shares: {error}")}</div> }.into_any(),
                    None => view! { <Loading /> }.into_any(),
                }}
            </div>
        </Modal>
    }
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
    let permission = Resource::new(list_id, get_list_permission);
    let can_write = Signal::derive(move || {
        permission
            .get()
            .and_then(Result::ok)
            .map(ListPermission::can_write)
            .unwrap_or(false)
    });
    let is_owner = Signal::derive(move || {
        permission
            .get()
            .and_then(Result::ok)
            .map(ListPermission::is_owner)
            .unwrap_or(false)
    });

    #[cfg(not(feature = "ssr"))]
    {
        use crate::ws::live_data::subscribe_to_list;
        Effect::new(move |_| {
            let id = list_id.get();
            if id != 0 {
                leptos::task::spawn_local(async move {
                    let _ = subscribe_to_list(id, move || {
                        list_view.refetch();
                    })
                    .await;
                });
            }
        });
    }

    let (menu, set_menu) = signal(MenuState::None);
    let (recipe_modal_open, set_recipe_modal_open) = signal(false);
    let (share_modal_open, set_share_modal_open) = signal(false);
    let (buying_view, set_buying_view) = signal(false);

    let edit_list_mode = RwSignal::new(false);
    let selected_items = RwSignal::new(HashSet::new());

    // Auto-mark logic moved to AutoMarkPurchases component

    view! {
        <AutoMarkPurchases list_view=list_view />
        <div class="flex-row gap-2">
            <Show when=can_write>
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

                        <i class="pr-1.5">
                            <Icon icon=i::BiPlusRegular />
                        </i>
                        <span>{t!(i18n, list_view_add_item)}</span>
                    </button>
                </Tooltip>
                <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_add_recipe).to_string()>
                    <button
                        class="btn-secondary"
                        class:active=move || recipe_modal_open()
                        on:click=move |_| set_recipe_modal_open(true)
                    >

                        {t!(i18n, list_view_add_recipe)}
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

                        {t!(i18n, list_view_make_place)}
                    </button>
                </Tooltip>
            </Show>
            <Tooltip tooltip_text=t_string!(i18n, list_view_tooltip_purchasing_view).to_string()>
                <button
                    class="btn-secondary"
                    class:active=buying_view
                    on:click=move |_| set_buying_view.update(|v| *v = !*v)
                >
                    <i class="pr-1.5">
                        <Icon icon=i::BiCartRegular />
                    </i>
                    <span>{t!(i18n, list_view_purchasing_view)}</span>
                </button>
            </Tooltip>
            <Show when=is_owner>
                <button class="btn-secondary" on:click=move |_| set_share_modal_open(true)>
                    <Icon icon=i::BsShareFill />
                    <span>"Share"</span>
                </button>
            </Show>

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

        <Show when=share_modal_open>
            <ListShareModal list_id=list_id() set_visible=set_share_modal_open />
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
                            <div class="panel p-4 rounded-xl space-y-3">
                                <div class="space-y-2">
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
                                                                    <div class="flex items-center gap-1">
                                                                        <Icon icon=i::BiPlusRegular />
                                                                        <span>{t!(i18n, list_view_add)}</span>
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
                            Either::Left(move || {
                                if buying_view() {
                                    Either::Left(
                                        view! {
                                            <div class="content-well">
                                                <div class="sticky top-0 flex-row justify-between">
                                                    <span class="content-title">{list.name.clone()}</span>
                                                </div>
                                                <BuyingView items=items.get_value() edit_item=edit_item />
                                            </div>
                                        },
                                    )
                                } else {
                                    Either::Right(
                                        view! {
                                            <div class="content-well">
                                                <div class="sticky top-0 flex-row justify-between">
                                                    <span class="content-title">{list.name.clone()}</span>
                                                    <div class="flex flex-row" class:hidden=move || !can_write()>
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

                                                            {t!(i18n, list_view_bulk_edit)}
                                                        </button>
                                                        <div class:hidden=move || !edit_list_mode()>
                                                            <button
                                                                class="btn"
                                                                on:click=move |_| {
                                                                    let items = selected_items
                                                                        .with_untracked(|s| {
                                                                            s.iter().copied().collect::<Vec<_>>()
                                                                        });
                                                                    selected_items.update(|i| i.clear());
                                                                    delete_items.dispatch(items);
                                                                }
                                                            >

                                                                {t!(i18n, list_view_delete)}
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

                                                            {t!(i18n, list_view_select_all)}
                                                        </button>
                                                        <button
                                                            class="btn"
                                                            on:click=move |_| {
                                                                selected_items.update(|i| i.clear());
                                                            }
                                                        >

                                                            {t!(i18n, list_view_deselect_all)}
                                                        </button>
                                                    </div>
                                                </div>
                                                <table class="w-full">
                                                    <thead>
                                                        <tr>
                                                            <th
                                                                scope="col"
                                                                class="text-left p-2"
                                                                class:hidden=move || !edit_list_mode() || !can_write()
                                                            >
                                                                "✅"
                                                            </th>
                                                            <th scope="col" class="text-left p-2">{t!(i18n, list_view_hq)}</th>
                                                            <th scope="col" class="text-left p-2">{t!(i18n, list_view_item)}</th>
                                                            <th scope="col" class="text-left p-2">{t!(i18n, list_view_quantity)}</th>
                                                            <th scope="col" class="text-left p-2">{t!(i18n, list_view_price)}</th>
                                                            <th
                                                                scope="col"
                                                                class="text-left p-2"
                                                                class:hidden=move || edit_list_mode() || !can_write()
                                                            >
                                                                {t!(i18n, list_view_options)}
                                                            </th>
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
                                                                        can_write=can_write
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

                                    <div>{format!("{}\n{e}", t_string!(i18n, list_view_failed_to_get_items))}</div>
                                },
                            )
                        }
                    })
            }}

        </Transition>
    }.into_any()
}
