use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::{
    components::{A, Outlet},
    hooks::{use_navigate, use_params_map},
};

use crate::api::{
    create_list, delete_list, edit_list, get_lists_with_permissions, get_login, leave_list,
    use_list_invite,
};
use crate::components::ad::Ad;
use crate::components::list::share_list_modal::ShareListModal;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
use crate::components::{loading::*, tooltip::*, world_name::*, world_picker::*};
use crate::global_state::home_world::get_price_zone;
use ultros_api_types::list::{
    CreateList, List, ListCapabilities, ListPermission, ListWithPermission,
};

#[component]
pub fn ListInviteAccept() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let navigate = use_navigate();
    let invite_id =
        Memo::new(move |_| params.with(|p| p.get("invite_id").clone().unwrap_or_default()));
    let login = Resource::new(|| (), |_| async move { get_login().await });
    let redeem_invite = Action::new(move |invite_id: &String| use_list_invite(invite_id.clone()));
    let redeem_started = RwSignal::new(false);

    Effect::new(move |_| {
        if redeem_started.get() {
            return;
        }
        let Some(Ok(_)) = login.get() else {
            return;
        };
        let invite_id = invite_id();
        if invite_id.is_empty() {
            return;
        }
        redeem_started.set(true);
        redeem_invite.dispatch(invite_id);
    });

    Effect::new(move |_| {
        if let Some(Ok(list_id)) = redeem_invite.value().get() {
            navigate(&format!("/list/{list_id}"), Default::default());
        }
    });

    view! {
        <MetaTitle title=move || t_string!(i18n, list_invite_meta_title).to_string() />
        <MetaRobotsNoIndex />
        <div class="panel mx-auto max-w-xl rounded-xl p-6">
            <div class="space-y-4">
                <div>
                    <h1 class="text-2xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, lists_accept_invite_heading)}</h1>
                    <p class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, lists_accept_invite_body)}</p>
                </div>

                <Suspense fallback=move || view! { <Loading /> }>
                    {move || match login.get() {
                        None => view! { <Loading /> }.into_any(),
                        Some(Err(e)) => {
                            if matches!(
                                e,
                                crate::error::AppError::ApiError(
                                    ultros_api_types::result::ApiError::NotAuthenticated
                                )
                            ) {
                                let href = format!("/login?next=/list/invite/{}", invite_id());
                                view! {
                                    <div class="space-y-4">
                                        <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-4 text-sm text-[color:var(--color-text-muted)]">
                                            "This invite is tied to your Ultros account, so you need to log in before accepting it."
                                        </div>
                                        <a class="btn-primary" rel="external" href=href>
                                            <Icon icon=i::BsPersonCircle />
                                            <span>{t!(i18n, lists_sign_in_discord_button)}</span>
                                        </a>
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="alert alert-error">{format!("Could not load invite: {e}")}</div>
                                }.into_any()
                            }
                        }
                        Some(Ok(_)) => {
                            match redeem_invite.value().get() {
                                Some(Ok(_)) => view! {
                                    <div class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, lists_opening_shared_list)}</div>
                                }.into_any(),
                                Some(Err(e)) => view! {
                                    <div class="space-y-3">
                                        <div class="alert alert-error">{format!("Could not accept invite: {e}")}</div>
                                        <A href="/list" attr:class="btn-secondary">{t!(i18n, lists_back_to_lists_link)}</A>
                                    </div>
                                }.into_any(),
                                None => view! {
                                    <div class="text-sm text-[color:var(--color-text-muted)]">
                                        {move || if redeem_invite.pending().get() { "Accepting invite..." } else { "Preparing invite..." }}
                                    </div>
                                }.into_any(),
                            }
                        }
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
fn PermissionPill(permission: ListPermission) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    match permission {
        ListPermission::Write => Some(Either::Left(view! {
            <span class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium border border-blue-400/40 text-blue-200">
                {t!(i18n, list_shared_editor_badge)}
            </span>
        })),
        ListPermission::Read => Some(Either::Right(view! {
            <span class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium border border-[color:var(--color-outline)] text-gray-300">
                {t!(i18n, list_shared_viewer_badge)}
            </span>
        })),
        _ => None,
    }
}

#[component]
fn ListCard(
    list: ListWithPermission,
    edit_list: Action<List, Result<(), crate::error::AppError>>,
    delete_list: Action<i32, Result<(), crate::error::AppError>>,
    leave_list_action: Action<(i32, u64), Result<(), crate::error::AppError>>,
    user_id: Signal<Option<u64>>,
) -> impl IntoView {
    let permission = list.permission;
    let caps = ListCapabilities::from(permission);
    let list_owner = list.list.owner;
    let owner_name = StoredValue::new(list.owner_name.clone());
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
                if is_edit() && (caps.can_admin || caps.can_leave) {
                    let list_id = list.id;
                    if caps.can_admin {
                        let list_for_save = list.clone();
                        let list_for_delete = list.clone();
                        view! {
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
                            </div>
                        }.into_any()
                    } else {
                        // Non-owner: show leave-list affordance
                        view! {
                            <div class="flex flex-col gap-3 w-full">
                                <p class="text-sm text-gray-300">{t!(i18n, leave_list_confirm)}</p>
                                <div class="flex gap-2 justify-end">
                                    <button class="btn-secondary btn-sm" on:click=cancel_edit.clone()>
                                        <Icon icon=i::AiCloseOutlined /> {t!(i18n, cancel)}
                                    </button>
                                    <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, leave_list_tooltip).to_string())>
                                        <button
                                            class="btn-danger btn-sm"
                                            prop:disabled=move || user_id().is_none()
                                            on:click=move |_| {
                                                let Some(uid) = user_id() else { return; };
                                                leave_list_action.dispatch((list_id, uid));
                                                set_is_edit(false);
                                            }
                                        >
                                            <Icon icon=i::BiExitRegular /> {t!(i18n, leave_list)}
                                        </button>
                                    </Tooltip>
                                </div>
                            </div>
                        }.into_any()
                    }
                } else {
                    view! {
                        <>
                            <div class="flex justify-between items-start gap-2">
                                <div class="flex flex-col gap-1 overflow-hidden">
                                    <a href=format!("/list/{}", list.id) class="text-xl font-bold hover:underline truncate text-[color:var(--link-color)]">
                                        {move || name()}
                                    </a>
                                    <div class="text-sm text-gray-400 flex items-center gap-1 flex-wrap">
                                        <Icon icon=i::BiWorldRegular />
                                        <WorldName id=list.wdr_filter />
                                        <PermissionPill permission />
                                    </div>
                                    <Show when=move || !caps.can_admin>
                                        <div class="text-xs text-gray-500">
                                            {move || {
                                                let name = owner_name.with_value(|n| n.clone()).unwrap_or_else(|| list_owner.to_string());
                                                t!(i18n, list_shared_by, name = name)
                                            }}
                                        </div>
                                    </Show>
                                </div>
                                <div class="flex items-center gap-1">
                                    <Show when=move || { caps.can_admin }>
                                        <Tooltip tooltip_text=Signal::derive(move || "Share list".to_string())>
                                            <button class="btn-ghost btn-sm text-gray-400 hover:text-white" on:click=move |_| set_share_open(true) aria_label="Share list">
                                                <Icon icon=i::BiShareAltRegular />
                                            </button>
                                        </Tooltip>
                                    </Show>
                                    <Show when=move || { caps.can_admin || caps.can_leave }>
                                        <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, edit_list).to_string())>
                                            <button
                                                type="button"
                                                class="btn-ghost btn-sm text-gray-400 hover:text-white"
                                                aria-label=move || t_string!(i18n, edit_list).to_string()
                                                on:click=move |_| set_is_edit(true)
                                            >
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
                    }.into_any()
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
    let leave_list_action =
        Action::new(move |(list_id, user_id): &(i32, u64)| leave_list(*list_id, *user_id));
    let lists = Resource::new(
        move || {
            (
                delete_list.version().get(),
                edit_list.version().get(),
                create_list.version().get(),
                redeem_invite.version().get(),
                leave_list_action.version().get(),
            )
        },
        move |_| get_lists_with_permissions(),
    );
    let user_resource = Resource::new(|| {}, |_| async move { get_login().await.ok() });
    let user_id = Signal::derive(move || user_resource.get().flatten().map(|u| u.id));
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
        <MetaTitle title=move || t_string!(i18n, lists_meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, lists_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        <div class="flex flex-col gap-4">
            <div class="flex items-center gap-2 md:gap-3">
                <A exact=true attr:class="nav-link" href="/list">
                    <Icon height="1.25em" width="1.25em" icon=i::AiOrderedListOutlined />
                    <span>{t!(i18n, lists)}</span>
                </A>
            </div>

            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-4">
                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, lists_page_title)}</h1>
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
                                            placeholder=t_string!(i18n, lists_new_list_placeholder)
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
                    aria-label=move || t_string!(i18n, search_your_lists).to_string()
                    placeholder=move || t_string!(i18n, search_your_lists).to_string()
                    prop:value=filter
                    on:input=move |ev| set_filter(event_target_value(&ev))
                />
            </div>

            <div class="panel p-4 rounded-xl flex flex-col md:flex-row gap-3 md:items-end">
                <div class="flex-1">
                    <label for="invite-code-input" class="label text-sm font-semibold">{t!(i18n, lists_redeem_invite_label)}</label>
                    <input
                        id="invite-code-input"
                        class="input w-full"
                        placeholder=t_string!(i18n, lists_invite_code_placeholder)
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
                                    let (owned, shared): (Vec<_>, Vec<_>) = lists
                                        .into_iter()
                                        .partition(|lwp| ListCapabilities::from(lwp.permission).can_admin);
                                    let shared_count = shared.len();

                                    if owned.is_empty() && shared.is_empty() {
                                        view! {
                                            <div class="flex flex-col items-center justify-center py-12 text-gray-400">
                                                <Icon icon=i::AiOrderedListOutlined width="4em" height="4em" attr:class="mb-4 opacity-50"/>
                                                <h3 class="text-xl font-semibold">{t!(i18n, no_lists_found)}</h3>
                                                <p>{t!(i18n, create_new_list_to_get_started)}</p>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div class="flex flex-col gap-6">
                                                {if owned.is_empty() && shared_count > 0 {
                                                    Some(view! {
                                                        <p class="italic text-gray-400 text-sm">
                                                            {t!(i18n, no_owned_lists_but_shared, count = shared_count)}
                                                        </p>
                                                    })
                                                } else {
                                                    None
                                                }}
                                                {if !owned.is_empty() {
                                                    Some(view! {
                                                        <section class="flex flex-col gap-3">
                                                            <h2 class="text-xl font-semibold text-[color:var(--brand-fg)]">{t!(i18n, my_lists)}</h2>
                                                            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                                                <For
                                                                    each=move || owned.clone()
                                                                    key=move |list| list.list.id
                                                                    children=move |list| {
                                                                        view! {
                                                                            <ListCard
                                                                                list=list
                                                                                edit_list=edit_list
                                                                                delete_list=delete_list
                                                                                leave_list_action=leave_list_action
                                                                                user_id=user_id
                                                                            />
                                                                        }
                                                                    }
                                                                />
                                                            </div>
                                                        </section>
                                                    })
                                                } else {
                                                    None
                                                }}
                                                {if !shared.is_empty() {
                                                    Some(view! {
                                                        <section class="flex flex-col gap-3">
                                                            <h2 class="text-xl font-semibold text-[color:var(--brand-fg)]">{t!(i18n, shared_with_me)}</h2>
                                                            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                                                <For
                                                                    each=move || shared.clone()
                                                                    key=move |list| list.list.id
                                                                    children=move |list| {
                                                                        view! {
                                                                            <ListCard
                                                                                list=list
                                                                                edit_list=edit_list
                                                                                delete_list=delete_list
                                                                                leave_list_action=leave_list_action
                                                                                user_id=user_id
                                                                            />
                                                                        }
                                                                    }
                                                                />
                                                            </div>
                                                        </section>
                                                    })
                                                } else {
                                                    None
                                                }}
                                            </div>
                                        }.into_any()
                                    }
                                }
                                Err(e) => {
                                    view! {
                                        <div class="alert alert-error">
                                            {move || t!(i18n, error_loading_lists, error = e.to_string())}
                                        </div>
                                    }.into_any()
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
