use crate::components::icon::Icon;
use crate::i18n::{t, t_string};
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::{A, Outlet};

use crate::api::{
    add_group_member, create_group, create_list, create_list_invite, delete_list,
    delete_list_invite, edit_list, get_groups, get_list_invites, get_list_shares,
    get_lists_with_permissions, share_list_with_group, share_list_with_user,
    unshare_list_from_group, unshare_list_from_user, use_list_invite,
};
use crate::components::ad::Ad;
use crate::components::modal::Modal;
use crate::components::{loading::*, tooltip::*, world_name::*, world_picker::*};
use crate::global_state::home_world::get_price_zone;
use ultros_api_types::list::{
    CreateInvite, CreateList, List, ListPermission, ListWithPermission, ShareListGroup,
    ShareListUser,
};
use ultros_api_types::user::group::CreateGroup;

fn permission_label(permission: ListPermission) -> &'static str {
    match permission {
        ListPermission::None => "No access",
        ListPermission::Read => "Read",
        ListPermission::Write => "Write",
        ListPermission::Owner => "Owner",
    }
}

fn editable_permission(value: &str) -> ListPermission {
    match value {
        "Write" => ListPermission::Write,
        _ => ListPermission::Read,
    }
}

#[component]
fn ShareListModal(list: List, set_visible: WriteSignal<bool>) -> impl IntoView {
    let list_id = list.id;
    let (user_id, set_user_id) = signal(String::new());
    let (user_permission, set_user_permission) = signal(ListPermission::Read);
    let (group_permission, set_group_permission) = signal(ListPermission::Read);
    let (invite_permission, set_invite_permission) = signal(ListPermission::Read);
    let (invite_max_uses, set_invite_max_uses) = signal(String::new());
    let (new_group_name, set_new_group_name) = signal(String::new());
    let (selected_group, set_selected_group) = signal::<Option<i32>>(None);
    let (member_id, set_member_id) = signal(String::new());

    let share_user = Action::new(move |data: &(i64, ListPermission)| {
        let (target_user_id, permission) = *data;
        share_list_with_user(
            list_id,
            ShareListUser {
                user_id: target_user_id,
                permission,
            },
        )
    });
    let unshare_user =
        Action::new(move |target_user_id: &i64| unshare_list_from_user(list_id, *target_user_id));
    let share_group = Action::new(move |data: &(i32, ListPermission)| {
        let (group_id, permission) = *data;
        share_list_with_group(
            list_id,
            ShareListGroup {
                group_id,
                permission,
            },
        )
    });
    let unshare_group =
        Action::new(move |group_id: &i32| unshare_list_from_group(list_id, *group_id));
    let create_invite =
        Action::new(move |invite: &CreateInvite| create_list_invite(list_id, invite.clone()));
    let delete_invite =
        Action::new(move |invite_id: &String| delete_list_invite(invite_id.clone()));
    let create_group =
        Action::new(move |name: &String| create_group(CreateGroup { name: name.clone() }));
    let add_member = Action::new(move |data: &(i32, i64)| {
        let (group_id, user_id) = *data;
        add_group_member(group_id, user_id)
    });

    let share_data = Resource::new(
        move || {
            (
                share_user.version().get(),
                unshare_user.version().get(),
                share_group.version().get(),
                unshare_group.version().get(),
                create_invite.version().get(),
                delete_invite.version().get(),
                create_group.version().get(),
                add_member.version().get(),
            )
        },
        move |_| async move {
            let (users, groups) = get_list_shares(list_id).await?;
            let invites = get_list_invites(list_id).await?;
            let owned_groups = get_groups().await?;
            Ok::<_, crate::error::AppError>((users, groups, invites, owned_groups))
        },
    );

    view! {
        <Modal set_visible=set_visible max_width="max-w-3xl w-[95%] sm:w-[720px]".to_string()>
            <div class="space-y-5">
                <div>
                    <h2 class="text-2xl font-bold text-[color:var(--brand-fg)]">"Share list"</h2>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{list.name.clone()}</p>
                </div>

                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <section class="space-y-3">
                        <h3 class="font-semibold">"Share with user"</h3>
                        <input
                            class="input w-full"
                            placeholder="Discord user ID"
                            prop:value=user_id
                            on:input=move |ev| set_user_id(event_target_value(&ev))
                        />
                        <select
                            class="input w-full"
                            on:change=move |ev| set_user_permission(editable_permission(&event_target_value(&ev)))
                        >
                            <option value="Read">"Read"</option>
                            <option value="Write">"Write"</option>
                        </select>
                        <button
                            class="btn-primary"
                            prop:disabled=move || { user_id().parse::<i64>().is_err() }
                            on:click=move |_| {
                                if let Ok(parsed) = user_id().parse::<i64>() {
                                    share_user.dispatch((parsed, user_permission()));
                                    set_user_id(String::new());
                                }
                            }
                        >
                            <Icon icon=i::BiUserPlusRegular /> "Share"
                        </button>
                    </section>

                    <section class="space-y-3">
                        <h3 class="font-semibold">"Invites"</h3>
                        <div class="flex flex-col sm:flex-row gap-2">
                            <select
                                class="input flex-1"
                                on:change=move |ev| set_invite_permission(editable_permission(&event_target_value(&ev)))
                            >
                                <option value="Read">"Read"</option>
                                <option value="Write">"Write"</option>
                            </select>
                            <input
                                class="input flex-1"
                                placeholder="Max uses"
                                prop:value=invite_max_uses
                                on:input=move |ev| set_invite_max_uses(event_target_value(&ev))
                            />
                        </div>
                        <button
                            class="btn-primary"
                            on:click=move |_| {
                                let max_uses = invite_max_uses().parse::<i32>().ok();
                                create_invite.dispatch(CreateInvite {
                                    permission: invite_permission(),
                                    max_uses,
                                });
                                set_invite_max_uses(String::new());
                            }
                        >
                            <Icon icon=i::BiLinkRegular /> "Create invite"
                        </button>
                    </section>
                </div>

                <section class="space-y-3">
                    <h3 class="font-semibold">"Groups"</h3>
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <div class="flex gap-2">
                            <input
                                class="input flex-1"
                                placeholder="New group name"
                                prop:value=new_group_name
                                on:input=move |ev| set_new_group_name(event_target_value(&ev))
                            />
                            <button
                                class="btn-secondary"
                                prop:disabled=move || new_group_name().trim().is_empty()
                                on:click=move |_| {
                                    let name = new_group_name().trim().to_string();
                                    if !name.is_empty() {
                                        create_group.dispatch(name);
                                        set_new_group_name(String::new());
                                    }
                                }
                            >
                                <Icon icon=i::BiPlusRegular /> "Create"
                            </button>
                        </div>
                        <div class="flex gap-2">
                            <input
                                class="input flex-1"
                                placeholder="Member Discord user ID"
                                prop:value=member_id
                                on:input=move |ev| set_member_id(event_target_value(&ev))
                            />
                            <button
                                class="btn-secondary"
                            prop:disabled=move || {
                                selected_group().is_none() || member_id().parse::<i64>().is_err()
                            }
                                on:click=move |_| {
                                    if let (Some(group_id), Ok(parsed)) = (selected_group(), member_id().parse::<i64>()) {
                                        add_member.dispatch((group_id, parsed));
                                        set_member_id(String::new());
                                    }
                                }
                            >
                                <Icon icon=i::BiUserPlusRegular /> "Add"
                            </button>
                        </div>
                    </div>
                </section>

                <Suspense fallback=move || view! { <Loading /> }>
                    {move || share_data.get().map(|data| match data {
                        Ok((users, shared_groups, invites, owned_groups)) => {
                            if selected_group().is_none()
                                && let Some(group) = owned_groups.first()
                            {
                                set_selected_group(Some(group.id));
                            }
                            view! {
                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <section class="space-y-2">
                                        <h3 class="font-semibold">"Current user shares"</h3>
                                        <div class="space-y-2">
                                            <For
                                                each=move || users.clone()
                                                key=|share| share.user_id
                                                children=move |share| {
                                                    view! {
                                                        <div class="card p-3 flex items-center justify-between gap-3">
                                                            <div class="min-w-0">
                                                                <div class="font-semibold truncate">{share.username}</div>
                                                                <div class="text-sm text-[color:var(--color-text-muted)]">
                                                                    {share.user_id.to_string()} " · " {permission_label(share.permission)}
                                                                </div>
                                                            </div>
                                                            <button class="btn-danger btn-sm" on:click=move |_| {
                                                                unshare_user.dispatch(share.user_id);
                                                            }>
                                                                <Icon icon=i::BiTrashSolid />
                                                            </button>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    </section>
                                    <section class="space-y-2">
                                        <h3 class="font-semibold">"Current group shares"</h3>
                                        <div class="space-y-2">
                                            <For
                                                each=move || shared_groups.clone()
                                                key=|share| share.group_id
                                                children=move |share| {
                                                    view! {
                                                        <div class="card p-3 flex items-center justify-between gap-3">
                                                            <div class="min-w-0">
                                                                <div class="font-semibold truncate">{share.group_name}</div>
                                                                <div class="text-sm text-[color:var(--color-text-muted)]">
                                                                    {permission_label(share.permission)}
                                                                </div>
                                                            </div>
                                                            <button class="btn-danger btn-sm" on:click=move |_| {
                                                                unshare_group.dispatch(share.group_id);
                                                            }>
                                                                <Icon icon=i::BiTrashSolid />
                                                            </button>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    </section>
                                    <section class="space-y-2">
                                        <h3 class="font-semibold">"Share with group"</h3>
                                        <div class="flex flex-col sm:flex-row gap-2">
                                            <select
                                                class="input flex-1"
                                                on:change=move |ev| {
                                                    set_selected_group(event_target_value(&ev).parse::<i32>().ok());
                                                }
                                            >
                                                <For
                                                    each=move || owned_groups.clone()
                                                    key=|group| group.id
                                                    children=move |group| {
                                                        view! { <option value=group.id.to_string()>{group.name}</option> }
                                                    }
                                                />
                                            </select>
                                            <select
                                                class="input flex-1"
                                                on:change=move |ev| set_group_permission(editable_permission(&event_target_value(&ev)))
                                            >
                                                <option value="Read">"Read"</option>
                                                <option value="Write">"Write"</option>
                                            </select>
                                            <button
                                                class="btn-primary"
                                                prop:disabled=move || selected_group().is_none()
                                                on:click=move |_| {
                                                    if let Some(group_id) = selected_group() {
                                                        share_group.dispatch((group_id, group_permission()));
                                                    }
                                                }
                                            >
                                                "Share"
                                            </button>
                                        </div>
                                    </section>
                                    <section class="space-y-2">
                                        <h3 class="font-semibold">"Active invites"</h3>
                                        <div class="space-y-2">
                                            <For
                                                each=move || invites.clone()
                                                key=|invite| invite.id.clone()
                                                children=move |invite| {
                                                    let invite_id = invite.id.clone();
                                                    view! {
                                                        <div class="card p-3 flex items-center justify-between gap-3">
                                                            <div class="min-w-0">
                                                                <div class="font-mono text-sm truncate">{invite.id}</div>
                                                                <div class="text-sm text-[color:var(--color-text-muted)]">
                                                                    {permission_label(invite.permission)}
                                                                    " · "
                                                                    {invite.uses.to_string()}
                                                                    "/"
                                                                    {invite.max_uses.map(|m| m.to_string()).unwrap_or_else(|| "∞".to_string())}
                                                                </div>
                                                            </div>
                                                            <button class="btn-danger btn-sm" on:click=move |_| {
                                                                delete_invite.dispatch(invite_id.clone());
                                                            }>
                                                                <Icon icon=i::BiTrashSolid />
                                                            </button>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    </section>
                                </div>
                            }.into_any()
                        }
                        Err(e) => view! {
                            <div class="alert alert-error">{e.to_string()}</div>
                        }.into_any(),
                    })}
                </Suspense>
            </div>
        </Modal>
    }
}

#[component]
fn ListCard(
    list: ListWithPermission,
    edit_list: Action<List, Result<(), crate::error::AppError>>,
    delete_list: Action<i32, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    let permission = list.permission;
    let list = list.list;
    let (is_edit, set_is_edit) = signal(false);
    let (share_open, set_share_open) = signal(false);
    // Local state for editing
    let (name, set_name) = signal(list.name.clone());
    let (current_world, set_current_world) = signal(Some(list.wdr_filter));
    let i18n = crate::i18n::use_i18n();

    let list_clone_cancel = list.clone();
    let cancel_edit = move |_| {
        set_name(list_clone_cancel.name.clone());
        set_current_world(Some(list_clone_cancel.wdr_filter));
        set_is_edit(false);
    };
    let list_for_render = list.clone();
    let list_for_share = list.clone();

    view! {
        <div class="panel p-4 rounded-xl flex flex-col gap-2 h-full justify-between transition-shadow hover:shadow-lg dark:hover:shadow-gray-700/30 relative">
            {move || {
                let list = list_for_render.clone();
                if is_edit() {
                    let list_for_save = list.clone();
                    let list_for_delete = list.clone();
                    Either::Left(view! {
                        <div class="flex flex-col gap-3 w-full">
                             <div>
                                <label class="label text-sm font-semibold">{t!(i18n, list_name)}</label>
                                <input
                                    class="input w-full"
                                    prop:value=name
                                    on:input=move |input| set_name(event_target_value(&input))
                                />
                            </div>
                            <div>
                                <label class="label text-sm font-semibold">{t!(i18n, world_region)}</label>
                                <WorldPicker
                                    current_world=current_world.into()
                                    set_current_world=set_current_world.into()
                                />
                            </div>
                            <div class="flex gap-2 justify-end mt-2">
                                <button class="btn-secondary btn-sm" on:click=cancel_edit.clone()>
                                    <Icon icon=i::AiCloseOutlined /> {t!(i18n, cancel)}
                                </button>
                                <button
                                    class="btn-primary btn-sm"
                                    on:click=move |_| {
                                        let mut new_list = list_for_save.clone();
                                        new_list.name = name();
                                        if let Some(world) = current_world() {
                                            new_list.wdr_filter = world;
                                        }
                                        edit_list.dispatch(new_list);
                                        set_is_edit(false);
                                    }
                                >
                                    <Icon icon=i::BiSaveSolid /> {t!(i18n, save)}
                                </button>
                            </div>
                            <Show when=move || { permission >= ListPermission::Owner }>
                                <div class="border-t border-gray-600/50 my-2"></div>
                                <div class="flex justify-between items-center">
                                    <span class="text-red-400 text-sm font-semibold">{t!(i18n, danger_zone)}</span>
                                    <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, delete).to_string())>
                                        <button
                                            class="btn-danger btn-sm"
                                            on:click=move |_| {
                                                let _ = delete_list.dispatch(list_for_delete.id);
                                            }
                                        >
                                            <Icon icon=i::BiTrashSolid /> {t!(i18n, delete)}
                                        </button>
                                    </Tooltip>
                                </div>
                            </Show>
                        </div>
                    })
                } else {
                    Either::Right(view! {
                        <>
                            <div class="flex justify-between items-start gap-2">
                                <div class="flex flex-col gap-1 overflow-hidden">
                                    <a href=format!("/list/{}", list.id) class="text-xl font-bold hover:underline truncate text-[color:var(--link-color)]">
                                        {move || name()}
                                    </a>
                                    <div class="text-sm text-gray-400 flex items-center gap-1">
                                         <Icon icon=i::BiWorldRegular />
                                         <WorldName id=list.wdr_filter />
                                         <span>" · "</span>
                                         <span>{permission_label(permission)}</span>
                                    </div>
                                </div>
                                <div class="flex items-center gap-1">
                                    <Show when=move || { permission >= ListPermission::Owner }>
                                        <Tooltip tooltip_text=Signal::derive(move || "Share list".to_string())>
                                            <button class="btn-ghost btn-sm text-gray-400 hover:text-white" on:click=move |_| set_share_open(true) aria_label="Share list">
                                                <Icon icon=i::BiShareAltRegular />
                                            </button>
                                        </Tooltip>
                                        <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, edit_list).to_string())>
                                            <button class="btn-ghost btn-sm text-gray-400 hover:text-white" on:click=move |_| set_is_edit(true) aria_label=move || t_string!(i18n, edit_list).to_string()>
                                                <Icon icon=i::BsPencilFill />
                                            </button>
                                        </Tooltip>
                                    </Show>
                                </div>
                            </div>
                            <div class="mt-4 flex justify-end">
                                <a href=format!("/list/{}", list.id) class="btn-secondary btn-sm">
                                    {t!(i18n, view_items)} <Icon icon=i::AiArrowRightOutlined attr:class="ml-1"/>
                                </a>
                            </div>
                        </>
                    })
                }
            }}
            <Show when=share_open>
                <ShareListModal list=list_for_share.clone() set_visible=set_share_open />
            </Show>
        </div>
    }
}

#[component]
pub fn EditLists() -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let delete_list = Action::new(move |id: &i32| delete_list(*id));
    let edit_list = Action::new(move |list: &List| edit_list(list.clone()));
    let create_list = Action::new(move |list: &CreateList| create_list(list.clone()));
    let redeem_invite = Action::new(move |invite_id: &String| use_list_invite(invite_id.clone()));
    let lists = Resource::new(
        move || {
            (
                delete_list.version().get(),
                edit_list.version().get(),
                create_list.version().get(),
                redeem_invite.version().get(),
            )
        },
        move |_| get_lists_with_permissions(),
    );
    let (creating, set_creating) = signal(false);
    let (filter, set_filter) = signal(String::new());
    let (invite_id, set_invite_id) = signal(String::new());

    let filtered_lists = Signal::derive(move || {
        let filter_text = filter.get().to_lowercase();
        lists.get().map(|res| {
            res.map(|lists| {
                if filter_text.is_empty() {
                    lists
                } else {
                    lists
                        .into_iter()
                        .filter(|l| l.list.name.to_lowercase().contains(&filter_text))
                        .collect()
                }
            })
        })
    });

    view! {
        <div class="flex flex-col gap-4">
            <div class="flex items-center gap-2 md:gap-3">
                <A exact=true attr:class="nav-link" href="/list">
                    <Icon height="1.25em" width="1.25em" icon=i::AiOrderedListOutlined />
                    <span>{t!(i18n, lists)}</span>
                </A>
            </div>

            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-4">
                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, my_lists)}</h1>
                 <button class="btn-primary" on:click=move |_| set_creating(!creating())>
                    <Icon icon=if creating() { i::AiCloseOutlined } else { i::BiPlusRegular } />
                    {move || if creating() { Either::Left(t!(i18n, cancel_creation)) } else { Either::Right(t!(i18n, create_new_list)) }}
                </button>
            </div>

            {move || {
                creating()
                    .then(|| {
                        let (new_list, set_new_list) = signal("".to_string());
                        let (global, _) = get_price_zone();
                        let selector = global().map(|global| global.into());
                        let (wdr_filter, set_wdr_filter) = signal(selector);
                        view! {
                            <div class="panel p-6 rounded-xl animate-fade-in relative z-10">
                                <h3 class="text-lg font-bold mb-4">{t!(i18n, create_new_list)}</h3>
                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <div class="flex flex-col gap-1">
                                        <label for="new-list-name" class="label font-semibold">{t!(i18n, list_name)}</label>
                                        <input
                                            class="input w-full"
                                            id="new-list-name"
                                            placeholder="My Awesome List"
                                            prop:value=new_list
                                            on:input=move |input| set_new_list(event_target_value(&input))
                                        />
                                    </div>
                                    <div class="flex flex-col gap-1">
                                        <label class="label font-semibold">{t!(i18n, world_region)}</label>
                                        <WorldPicker
                                            current_world=wdr_filter.into()
                                            set_current_world=set_wdr_filter.into()
                                        />
                                    </div>
                                </div>
                                <div class="flex justify-end mt-4">
                                    <button
                                        prop:disabled=move || wdr_filter().is_none() || new_list().is_empty()
                                        class="btn-primary"
                                        on:click=move |_| {
                                            if let Some(wdr_filter) = wdr_filter() {
                                                let list = CreateList {
                                                    name: new_list(),
                                                    wdr_filter,
                                                };
                                                create_list.dispatch(list);
                                                set_new_list("".to_string());
                                                set_creating(false);
                                            }
                                        }
                                    >
                                        <Icon icon=i::BiSaveSolid /> {t!(i18n, create_list)}
                                    </button>
                                </div>
                            </div>
                        }
                    })
            }}

            <div class="relative">
                <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                     <Icon icon=i::AiSearchOutlined attr:class="text-gray-400"/>
                </div>
                <input
                    class="input w-full pl-10"
                    placeholder=move || t_string!(i18n, search_your_lists).to_string()
                    prop:value=filter
                    on:input=move |ev| set_filter(event_target_value(&ev))
                />
            </div>

            <div class="panel p-4 rounded-xl flex flex-col md:flex-row gap-3 md:items-end">
                <div class="flex-1">
                    <label class="label text-sm font-semibold">"Redeem invite"</label>
                    <input
                        class="input w-full"
                        placeholder="Invite code"
                        prop:value=invite_id
                        on:input=move |ev| set_invite_id(event_target_value(&ev))
                    />
                </div>
                <button
                    class="btn-secondary"
                    prop:disabled=move || invite_id().trim().is_empty()
                    on:click=move |_| {
                        let id = invite_id().trim().to_string();
                        if !id.is_empty() {
                            redeem_invite.dispatch(id);
                            set_invite_id(String::new());
                        }
                    }
                >
                    <Icon icon=i::BiLinkRegular /> "Redeem"
                </button>
            </div>

            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    filtered_lists
                        .get()
                        .map(|lists| {
                            match lists {
                                Ok(lists) => {
                                    if lists.is_empty() {
                                        Either::Left(view! {
                                            <div class="flex flex-col items-center justify-center py-12 text-gray-400">
                                                <Icon icon=i::AiOrderedListOutlined width="4em" height="4em" attr:class="mb-4 opacity-50"/>
                                                <h3 class="text-xl font-semibold">{t!(i18n, no_lists_found)}</h3>
                                                <p>{t!(i18n, create_new_list_to_get_started)}</p>
                                            </div>
                                        }.into_any())
                                    } else {
                                        Either::Right(view! {
                                            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                                <For
                                                    each=move || lists.clone()
                                                    key=move |list| list.list.id
                                                    children=move |list| {
                                                        view! {
                                                            <ListCard
                                                                list=list
                                                                edit_list=edit_list
                                                                delete_list=delete_list
                                                            />
                                                        }
                                                    }
                                                />
                                            </div>
                                        }.into_any())
                                    }
                                }
                                Err(e) => {
                                    Either::Right(view! {
                                        <div class="alert alert-error">
                                            {move || t!(i18n, error_loading_lists, error = e.to_string())}
                                        </div>
                                    }.into_any())
                                }
                            }
                        })
                }}
            </Suspense>
        </div>
    }.into_any()
}

#[component]
pub fn Lists() -> impl IntoView {
    view! {
        <div class="mx-auto">
            <div class="main-content">
                <div class="container mx-auto flex flex-col xl:flex-row items-start gap-4">
                    <div class="flex flex-col grow w-full">
                         <div class="w-full mb-4">
                            <Ad class="h-20 w-full" />
                        </div>
                        <Outlet />
                    </div>
                    <div class="shrink-0 xl:w-80">
                         <Ad class="h-96 w-96 xl:h-[600px] xl:w-80" />
                    </div>
                </div>
            </div>
        </div>
    }
    .into_any()
}
