use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::i18n::*;
use icondata as i;
#[cfg(feature = "hydrate")]
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::{
    components::Outlet,
    hooks::{use_navigate, use_params_map},
};

use crate::api::{get_login, use_list_invite};
#[cfg(feature = "hydrate")]
use crate::components::list::share_list_modal::ShareListModal;
use crate::components::meta::{MetaDescription, MetaRobotsNoIndex, MetaTitle};
#[cfg(feature = "hydrate")]
use crate::components::modal::Modal;
use crate::components::skeleton::BoxSkeleton;
#[cfg(feature = "hydrate")]
use crate::components::{tooltip::*, world_name::*, world_picker::*};
#[cfg(feature = "hydrate")]
use ultros_api_types::list::{List, ListCapabilities, ListPermission, ListWithPermission};

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

                <Suspense fallback=move || view! { <BoxSkeleton rows=1 /> }>
                    {move || match login.get() {
                        None => view! { <BoxSkeleton rows=1 /> }.into_any(),
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
                                            {t!(i18n, lists_invite_login_required)}
                                        </div>
                                        <a class="btn-primary" rel="external" href=href>
                                            <Icon icon=i::BsPersonCircle />
                                            <span>{t!(i18n, lists_sign_in_discord_button)}</span>
                                        </a>
                                    </div>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="alert alert-error">{t!(i18n, lists_invite_load_error, error = e.to_string())}</div>
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
                                        <div class="alert alert-error">{t!(i18n, lists_invite_accept_error, error = e.to_string())}</div>
                                        <AppLink href="/list" attr:class="btn-secondary">{t!(i18n, lists_back_to_lists_link)}</AppLink>
                                    </div>
                                }.into_any(),
                                None => view! {
                                    <div class="text-sm text-[color:var(--color-text-muted)]">
                                        {move || if redeem_invite.pending().get() {
                                            t_string!(i18n, lists_invite_accepting).to_string()
                                        } else {
                                            t_string!(i18n, lists_invite_preparing).to_string()
                                        }}
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

#[cfg(feature = "hydrate")]
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

#[cfg(feature = "hydrate")]
pub(crate) const LIST_CARD_CLASS: &str =
    "panel min-w-0 rounded-xl p-4 flex flex-col gap-3 h-full justify-between";
#[cfg(feature = "hydrate")]
pub(crate) const LIST_CARD_TITLE: &str =
    "text-lg font-semibold hover:underline break-words min-w-0";

#[cfg(any(feature = "hydrate", test))]
fn list_card_href(id: i32) -> String {
    format!("/list/{id}")
}

#[cfg(feature = "hydrate")]
#[component]
pub(crate) fn ListCard(
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
    let (confirm_delete, set_confirm_delete) = signal(false);
    let delete_list_id = list.id;
    let delete_list_name = StoredValue::new(list.name.clone());
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
        <div class=LIST_CARD_CLASS>
            {move || {
                let list = list_for_render.clone();
                if is_edit() && (caps.can_admin || caps.can_leave) {
                    let list_id = list.id;
                    if caps.can_admin {
                        let list_for_save = list.clone();
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
                                    <span class="text-negative text-sm font-semibold">{t!(i18n, danger_zone)}</span>
                                    <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, delete).to_string())>
                                        <button
                                            class="btn-danger btn-sm"
                                            on:click=move |_| set_confirm_delete(true)
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
                                <div class="flex min-w-0 flex-col gap-1 overflow-hidden">
                                    <a href=move || list_card_href(list.id) class=LIST_CARD_TITLE>
                                        {move || name()}
                                    </a>
                                    <div class="text-sm text-[color:var(--color-text-muted)] flex items-center gap-2 flex-wrap">
                                        <Icon icon=i::BiWorldRegular />
                                        <WorldName id=list.wdr_filter />
                                        <span aria-hidden="true">"·"</span><span>{t!(i18n,online_connected)}</span>
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
                                <div class="flex shrink-0 items-center gap-1">
                                    <Show when=move || { caps.can_admin }>
                                        <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, online_access).to_string())>
                                            <button
                                                class="btn-ghost min-h-11 min-w-11 p-2"
                                                on:click=move |_| set_share_open(true)
                                                aria-label=move || t_string!(i18n, online_access).to_string()
                                            >
                                                <Icon icon=i::BiShareAltRegular />
                                            </button>
                                        </Tooltip>
                                    </Show>
                                    <Show when=move || { caps.can_admin || caps.can_leave }>
                                        <Tooltip tooltip_text=Signal::derive(move || t_string!(i18n, edit_list).to_string())>
                                            <button
                                                type="button"
                                                class="btn-ghost min-h-11 min-w-11 p-2"
                                                aria-label=move || t_string!(i18n, edit_list).to_string()
                                                on:click=move |_| set_is_edit(true)
                                            >
                                                <Icon icon=i::BsPencilFill />
                                            </button>
                                        </Tooltip>
                                    </Show>
                                </div>
                            </div>
                            <div class="mt-2 flex justify-start">
                                <a href=move || list_card_href(list.id) class="btn-secondary min-h-11">
                                    {t!(i18n, view_items)} <Icon icon=i::AiArrowRightOutlined attr:class="ml-1"/>
                                </a>
                            </div>
                        </>
                    }.into_any()
                }
            }}
            <Show when=confirm_delete>
                <Modal set_visible=set_confirm_delete>
                    <div class="flex flex-col gap-4">
                        <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                            {t!(i18n, list_delete_confirm_title)}
                        </h2>
                        <p class="text-sm text-[color:var(--color-text-muted)]">
                            {move || t!(
                                i18n,
                                list_delete_confirm_body,
                                name = delete_list_name.with_value(|n| n.clone()),
                            )}
                        </p>
                        <div class="flex justify-end gap-2">
                            <button class="btn-secondary" on:click=move |_| set_confirm_delete(false)>
                                <Icon icon=i::AiCloseOutlined /> {t!(i18n, cancel)}
                            </button>
                            <button
                                class="btn-danger"
                                on:click=move |_| {
                                    let _ = delete_list.dispatch(delete_list_id);
                                    set_confirm_delete(false);
                                }
                            >
                                <Icon icon=i::BiTrashSolid /> {t!(i18n, delete)}
                            </button>
                        </div>
                    </div>
                </Modal>
            </Show>
            <Show when=share_open>
                <ShareListModal list=list_for_share.clone() set_visible=set_share_open />
            </Show>
        </div>
    }
}

#[component]
pub fn EditLists() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <MetaTitle title=move || t_string!(i18n, lists_meta_title).to_string() />
        <MetaDescription text=move || t_string!(i18n, lists_meta_desc).to_string() />
        <MetaRobotsNoIndex />
        <crate::routes::guest_lists::DeviceLists />
    }
}

#[component]
pub fn Lists() -> impl IntoView {
    view! {
        <div class="main-content p-2 sm:p-6">
            <div class="flex flex-col w-full">
                <Outlet />
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod design_tests {
    use super::list_card_href;

    #[test]
    fn online_card_navigation_uses_the_default_workspace() {
        assert_eq!(list_card_href(30), "/list/30");
    }
}
