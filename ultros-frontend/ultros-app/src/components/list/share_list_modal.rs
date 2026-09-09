use crate::api::{
    create_list_invite, delete_list_invite, get_group_detail, get_groups, get_list_invites,
    get_list_shares, share_list_with_group, share_list_with_role, share_list_with_user,
    unshare_list_from_group, unshare_list_from_role, unshare_list_from_user,
};
use crate::components::icon::Icon;
use crate::components::invite_link;
use crate::components::loading::*;
use crate::components::modal::Modal;
use crate::global_state::clipboard_text::GlobalLastCopiedText;
use crate::global_state::toasts::{Toasts, use_toast};
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::list::{
    CreateInvite, List, ListInvite, ListPermission, ListSharedGroup, ListSharedRole,
    ListSharedUser, ShareListGroup, ShareListRole, ShareListUser,
};

pub(crate) fn permission_label(permission: ListPermission) -> &'static str {
    match permission {
        ListPermission::None => "No access",
        ListPermission::Read => "Read",
        ListPermission::Write => "Write",
        ListPermission::Owner => "Owner",
    }
}

pub(crate) fn editable_permission(value: &str) -> ListPermission {
    match value {
        "Write" => ListPermission::Write,
        _ => ListPermission::Read,
    }
}

/// Route prefix for list invites. Groups use `/group/invite` — see
/// [`crate::components::invite_link`].
const LIST_INVITE_PATH: &str = "/list/invite";

pub(crate) fn invite_url(invite_id: &str) -> String {
    invite_link::invite_url(LIST_INVITE_PATH, invite_id)
}

pub(crate) fn copy_invite_url(
    invite_id: &str,
    last_copied: Option<GlobalLastCopiedText>,
    toasts: Option<Toasts>,
) {
    invite_link::copy_invite_url(
        LIST_INVITE_PATH,
        invite_id,
        last_copied,
        toasts,
        "Invite link copied".to_string(),
    );
}

pub(crate) fn invite_uses_label(invite: &ListInvite) -> String {
    invite_link::uses_label(invite.uses, invite.max_uses)
}

#[component]
pub(crate) fn AccessRow(
    icon: icondata_core::Icon,
    label: String,
    detail: String,
    trailing: String,
    #[prop(into)] on_delete: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="flex items-center gap-3 py-2">
            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-[color:color-mix(in_srgb,var(--color-text)_10%,transparent)] text-[color:var(--color-text-muted)]">
                <Icon icon width="1.25em" height="1.25em" />
            </div>
            <div class="min-w-0 flex-1">
                <div class="flex items-baseline gap-2">
                    <span class="truncate font-bold text-[color:var(--color-text)]">{label}</span>
                    <span class="shrink-0 text-sm text-[color:var(--color-text-muted)]">{detail}</span>
                </div>
            </div>
            <div class="hidden min-w-12 flex-1 border-b border-dotted border-[color:color-mix(in_srgb,var(--color-text-muted)_70%,transparent)] md:block"></div>
            <div class="shrink-0 text-sm text-[color:var(--color-text)]">{trailing}</div>
            <button
                type="button"
                class="btn-ghost p-2 text-[color:var(--color-text-muted)] hover:text-red-200"
                aria-label="Remove access"
                on:click=move |_| on_delete.run(())
            >
                <Icon icon=i::BiTrashSolid />
            </button>
        </div>
    }
}

#[component]
pub(crate) fn ShareListSection(
    list: List,
    #[prop(into, optional)] refresh_signal: Option<Signal<u32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let list_id = list.id;
    let (selected_group_id, set_selected_group_id) = signal(String::new());
    // Empty means "the whole group"; sharing to a role is the narrower case
    // and has to be picked deliberately.
    let (selected_role_id, set_selected_role_id) = signal(String::new());
    let (group_permission, set_group_permission) = signal(ListPermission::Read);
    let (manual_user_id, set_manual_user_id) = signal(String::new());
    let (manual_user_permission, set_manual_user_permission) = signal(ListPermission::Read);
    let (invite_permission, set_invite_permission) = signal(ListPermission::Read);
    let (invite_max_uses, set_invite_max_uses) = signal(String::new());
    let last_copied = use_context::<GlobalLastCopiedText>();
    let toasts = use_toast();
    let last_copied_invite = RwSignal::new(None::<String>);

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
    let share_role = Action::new(move |data: &(i32, ListPermission)| {
        let (role_id, permission) = *data;
        share_list_with_role(
            list_id,
            ShareListRole {
                role_id,
                permission,
            },
        )
    });
    let unshare_role = Action::new(move |role_id: &i32| unshare_list_from_role(list_id, *role_id));
    let create_invite =
        Action::new(move |invite: &CreateInvite| create_list_invite(list_id, invite.clone()));
    let delete_invite =
        Action::new(move |invite_id: &String| delete_list_invite(invite_id.clone()));

    let share_data = Resource::new(
        move || {
            (
                share_user.version().get(),
                unshare_user.version().get(),
                share_group.version().get(),
                unshare_group.version().get(),
                share_role.version().get(),
                unshare_role.version().get(),
                create_invite.version().get(),
                delete_invite.version().get(),
                refresh_signal.map(|s| s.get()).unwrap_or(0),
            )
        },
        move |_| async move {
            let (users, groups, roles) = get_list_shares(list_id).await?;
            let invites = get_list_invites(list_id).await?;
            let owned_groups = get_groups().await?;
            Ok::<_, crate::error::AppError>((users, groups, roles, invites, owned_groups))
        },
    );

    // The roles of whichever group is selected. Resolving it costs one
    // request, so it is keyed on the selection rather than fetched for every
    // group the viewer belongs to.
    let group_roles = Resource::new(
        move || selected_group_id.get(),
        move |group_id| async move {
            let Ok(group_id) = group_id.parse::<i32>() else {
                return Ok(Vec::new());
            };
            get_group_detail(group_id).await.map(|detail| detail.roles)
        },
    );

    Effect::new(move |_| {
        let Some(result) = create_invite.value().get() else {
            return;
        };
        match result {
            Ok(invite) => {
                let already_copied = last_copied_invite
                    .with_untracked(|id| id.as_deref() == Some(invite.id.as_str()));
                if !already_copied {
                    copy_invite_url(&invite.id, last_copied, toasts);
                    last_copied_invite.set(Some(invite.id));
                }
            }
            Err(e) => {
                if let Some(toasts) = toasts {
                    toasts.error(format!("Could not create invite link: {e}"));
                }
            }
        }
    });

    let copy_latest_invite = move |invites: Vec<ListInvite>| {
        if let Some(invite) = invites.last() {
            copy_invite_url(&invite.id, last_copied, toasts);
        }
    };

    view! {
        <Suspense fallback=move || view! { <Loading /> }>
            {move || share_data.get().map(|data| match data {
                Ok((users, shared_groups, shared_roles, invites, owned_groups)) => {
                    let has_owned_groups = !owned_groups.is_empty();
                    let invites_for_copy = invites.clone();
                    let latest_invite_url = invites
                        .last()
                        .map(|invite| invite_url(&invite.id))
                        .unwrap_or_default();
                    view! {
                        <div class="space-y-6">
                            <section class="space-y-3">
                                <h3 class="text-lg font-bold text-[color:var(--color-text)]">{t!(i18n, lists_share_group_heading)}</h3>
                                <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_7rem_9rem]">
                                    <select
                                        class="input w-full text-base"
                                        prop:value=selected_group_id
                                        on:change=move |ev| {
                                            set_selected_group_id(event_target_value(&ev));
                                            // A role belongs to one group, so a
                                            // stale pick would share the wrong
                                            // group's role.
                                            set_selected_role_id(String::new());
                                        }
                                    >
                                        <option value="">
                                            {if has_owned_groups {
                                                t_string!(i18n, lists_share_group_placeholder).to_string()
                                            } else {
                                                t_string!(i18n, lists_share_no_groups_option).to_string()
                                            }}
                                        </option>
                                        <For
                                            each=move || owned_groups.clone()
                                            key=|summary| summary.group.id
                                            children=move |summary| {
                                                let group = summary.group;
                                                view! {
                                                    <option value=group.id.to_string()>{group.name}</option>
                                                }
                                            }
                                        />
                                    </select>
                                    // The Suspense wraps the whole select, not
                                    // its options: `<select>` only admits
                                    // option/optgroup children, and a
                                    // suspense boundary inside one would put
                                    // its streaming markers there. It is also
                                    // nested rather than shared with the
                                    // section's own boundary, so changing the
                                    // group re-suspends this control alone
                                    // instead of blanking the whole modal.
                                    <Suspense fallback=move || {
                                        view! {
                                            <select class="input w-full text-base" disabled>
                                                <option value="">{t!(i18n, lists_share_role_everyone)}</option>
                                            </select>
                                        }
                                    }>
                                        {move || {
                                            let roles = group_roles
                                                .get()
                                                .and_then(|roles| roles.ok())
                                                .unwrap_or_default();
                                            view! {
                                                <select
                                                    class="input w-full text-base"
                                                    aria-label=move || {
                                                        t_string!(i18n, lists_share_role_label).to_string()
                                                    }
                                                    prop:value=selected_role_id
                                                    prop:disabled=move || selected_group_id().is_empty()
                                                    on:change=move |ev| set_selected_role_id(
                                                        event_target_value(&ev),
                                                    )
                                                >
                                                    <option value="">
                                                        {t!(i18n, lists_share_role_everyone)}
                                                    </option>
                                                    {roles
                                                        .into_iter()
                                                        .map(|role| {
                                                            view! {
                                                                <option value=role.id.to_string()>{role.name}</option>
                                                            }
                                                        })
                                                        .collect_view()}
                                                </select>
                                            }
                                        }}
                                    </Suspense>
                                    <select
                                        class="input w-full"
                                        on:change=move |ev| set_group_permission(editable_permission(&event_target_value(&ev)))
                                    >
                                        <option value="Read">{t!(i18n, permission_read)}</option>
                                        <option value="Write">{t!(i18n, permission_write)}</option>
                                    </select>
                                    <button
                                        type="button"
                                        class="btn-primary"
                                        prop:disabled=move || {
                                            selected_group_id().is_empty()
                                                || share_group.pending().get()
                                                || share_role.pending().get()
                                        }
                                        on:click=move |_| {
                                            // A role share is the narrower
                                            // grant, so picking one replaces
                                            // the whole-group share rather
                                            // than adding to it.
                                            if let Ok(role_id) = selected_role_id().parse::<i32>() {
                                                share_role.dispatch((role_id, group_permission()));
                                            } else if let Ok(group_id) = selected_group_id().parse::<i32>() {
                                                share_group.dispatch((group_id, group_permission()));
                                            } else {
                                                return;
                                            }
                                            set_selected_group_id(String::new());
                                            set_selected_role_id(String::new());
                                        }
                                    >
                                        {t!(i18n, lists_share_group_button)}
                                    </button>
                                </div>
                                <Show when=move || !has_owned_groups>
                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                        {t!(i18n, lists_share_no_groups_help)}
                                    </p>
                                </Show>
                            </section>

                            <section class="space-y-3">
                                <h3 class="text-lg font-bold text-[color:var(--color-text)]">{t!(i18n, lists_share_manual_user_heading)}</h3>
                                <p class="text-sm text-[color:var(--color-text-muted)]">
                                    {t!(i18n, lists_share_manual_user_help)}
                                </p>
                                <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_7rem_9rem]">
                                    <input
                                        class="input w-full text-base"
                                        inputmode="numeric"
                                        placeholder=t_string!(i18n, lists_share_manual_user_placeholder)
                                        prop:value=manual_user_id
                                        on:input=move |ev| set_manual_user_id(event_target_value(&ev))
                                    />
                                    <select
                                        class="input w-full"
                                        on:change=move |ev| set_manual_user_permission(editable_permission(&event_target_value(&ev)))
                                    >
                                        <option value="Read">{t!(i18n, permission_read)}</option>
                                        <option value="Write">{t!(i18n, permission_write)}</option>
                                    </select>
                                    <button
                                        type="button"
                                        class="btn-secondary"
                                        prop:disabled=move || manual_user_id().trim().is_empty() || share_user.pending().get()
                                        on:click=move |_| {
                                            let raw = manual_user_id().trim().to_string();
                                            if let Ok(user_id) = raw.parse::<i64>() {
                                                share_user.dispatch((user_id, manual_user_permission()));
                                                set_manual_user_id(String::new());
                                            } else if let Some(toasts) = toasts {
                                                toasts.error("Enter a numeric Discord user ID");
                                            }
                                        }
                                    >
                                        {t!(i18n, lists_share_manual_user_button)}
                                    </button>
                                </div>
                            </section>

                            <section class="space-y-3">
                                <h3 class="text-lg font-bold text-[color:var(--color-text)]">{t!(i18n, lists_share_via_link_heading)}</h3>
                                <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_7rem_9rem_14rem]">
                                    <input
                                        class="input w-full font-mono text-sm"
                                        readonly
                                        prop:value=latest_invite_url
                                        placeholder=t_string!(i18n, lists_invite_create_placeholder)
                                        on:click=move |_| copy_latest_invite(invites_for_copy.clone())
                                    />
                                    <select
                                        class="input w-full"
                                        on:change=move |ev| set_invite_permission(editable_permission(&event_target_value(&ev)))
                                    >
                                        <option value="Read">{t!(i18n, permission_read)}</option>
                                        <option value="Write">{t!(i18n, permission_write)}</option>
                                    </select>
                                    <input
                                        class="input w-full"
                                        inputmode="numeric"
                                        placeholder=t_string!(i18n, lists_invite_max_uses_placeholder)
                                        prop:value=invite_max_uses
                                        on:input=move |ev| set_invite_max_uses(event_target_value(&ev))
                                    />
                                    <button
                                        type="button"
                                        class="btn-primary"
                                        prop:disabled=create_invite.pending()
                                        on:click=move |_| {
                                            let max_uses = invite_max_uses().trim().parse::<i32>().ok();
                                            create_invite.dispatch(CreateInvite {
                                                permission: invite_permission(),
                                                max_uses,
                                            });
                                            set_invite_max_uses(String::new());
                                        }
                                    >
                                        <Icon icon=i::BsClipboard2Fill />
                                        <span>{t!(i18n, lists_invite_copy_button)}</span>
                                    </button>
                                </div>
                            </section>

                            <div class="h-px bg-[color:var(--color-outline)]"></div>

                            <section class="space-y-3">
                                <h3 class="text-lg font-bold text-[color:var(--color-text)]">{t!(i18n, lists_who_has_access_heading)}</h3>
                                <AccessList
                                    users=users
                                    groups=shared_groups
                                    roles=shared_roles
                                    invites=invites
                                    unshare_user
                                    unshare_group
                                    unshare_role
                                    delete_invite
                                />
                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] p-3 text-sm text-[color:var(--color-text-muted)]">
                                    {t!(i18n, lists_share_access_tip)}
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
    }
}

#[component]
pub(crate) fn ShareListModal(list: List, set_visible: WriteSignal<bool>) -> impl IntoView {
    let i18n = use_i18n();
    let list_name = list.name.clone();
    let list_for_section = list.clone();
    view! {
        <Modal set_visible=set_visible max_width="max-w-5xl w-[96%] sm:w-[820px]".to_string()>
            <div class="space-y-6">
                <div class="pr-10">
                    <h2 class="text-3xl font-black text-[color:var(--color-text)]">
                        {t!(i18n, lists_share_heading_prefix)} {list_name.clone()}
                    </h2>
                </div>
                <ShareListSection list=list_for_section.clone() />
            </div>
        </Modal>
    }
}

#[component]
pub(crate) fn AccessList(
    users: Vec<ListSharedUser>,
    groups: Vec<ListSharedGroup>,
    roles: Vec<ListSharedRole>,
    invites: Vec<ListInvite>,
    unshare_user: Action<i64, Result<(), crate::error::AppError>>,
    unshare_group: Action<i32, Result<(), crate::error::AppError>>,
    unshare_role: Action<i32, Result<(), crate::error::AppError>>,
    delete_invite: Action<String, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let is_empty = users.is_empty() && groups.is_empty() && roles.is_empty() && invites.is_empty();
    view! {
        <div class="space-y-1">
            <Show when=move || is_empty>
                <div class="rounded-lg border border-dashed border-[color:var(--color-outline)] p-5 text-center text-[color:var(--color-text-muted)]">
                    "Only the owner has access right now."
                </div>
            </Show>

            <For
                each=move || users.clone()
                key=|share| share.user_id
                children=move |share| {
                    let user_id = share.user_id;
                    view! {
                        <AccessRow
                            icon=i::BsPersonCircle
                            label=share.username
                            detail="(Discord User)".to_string()
                            trailing=permission_label(share.permission).to_string()
                            on_delete=Callback::new(move |_| {
                                unshare_user.dispatch(user_id);
                            })
                        />
                    }
                }
            />

            <For
                each=move || groups.clone()
                key=|share| share.group_id
                children=move |share| {
                    let group_id = share.group_id;
                    view! {
                        <AccessRow
                            icon=i::BiGroupSolid
                            label=share.group_name
                            detail="(Group)".to_string()
                            trailing=permission_label(share.permission).to_string()
                            on_delete=Callback::new(move |_| {
                                unshare_group.dispatch(group_id);
                            })
                        />
                    }
                }
            />

            <For
                each=move || roles.clone()
                key=|share| share.role_id
                children=move |share| {
                    let role_id = share.role_id;
                    view! {
                        <AccessRow
                            icon=i::BiGroupSolid
                            // "Group / Role", so a role share is legible
                            // without opening the group it came from.
                            label=format!("{} / {}", share.group_name, share.role_name)
                            detail=t_string!(i18n, lists_share_role_detail).to_string()
                            trailing=permission_label(share.permission).to_string()
                            on_delete=Callback::new(move |_| {
                                unshare_role.dispatch(role_id);
                            })
                        />
                    }
                }
            />

            <For
                each=move || invites.clone()
                key=|invite| invite.id.clone()
                children=move |invite| {
                    let invite_id = invite.id.clone();
                    let label = format!("Link: {}", invite.id.chars().take(10).collect::<String>());
                    let trailing = invite_uses_label(&invite);
                    view! {
                        <AccessRow
                            icon=i::BiLinkRegular
                            label
                            detail=format!("(Invite · {})", permission_label(invite.permission))
                            trailing
                            on_delete=Callback::new(move |_| {
                                delete_invite.dispatch(invite_id.clone());
                            })
                        />
                    }
                }
            />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::list::{ListInvite, ListPermission};

    #[test]
    fn test_permission_label() {
        assert_eq!(permission_label(ListPermission::None), "No access");
        assert_eq!(permission_label(ListPermission::Read), "Read");
        assert_eq!(permission_label(ListPermission::Write), "Write");
        assert_eq!(permission_label(ListPermission::Owner), "Owner");
    }

    #[test]
    fn test_editable_permission() {
        assert_eq!(editable_permission("Write"), ListPermission::Write);
        assert_eq!(editable_permission("Read"), ListPermission::Read);
        assert_eq!(editable_permission("something else"), ListPermission::Read);
    }

    #[test]
    fn test_invite_url_ssr() {
        // In unit tests, the hydrate feature should not be active, so it should return the relative path
        assert_eq!(invite_url("test-id"), "/list/invite/test-id");
    }

    #[test]
    fn test_invite_uses_label() {
        let invite_unlimited = ListInvite {
            id: "id".to_string(),
            list_id: 1,
            permission: ListPermission::Read,
            max_uses: None,
            uses: 5,
        };
        assert_eq!(invite_uses_label(&invite_unlimited), "5/∞ uses");

        let invite_limited = ListInvite {
            id: "id".to_string(),
            list_id: 1,
            permission: ListPermission::Read,
            max_uses: Some(10),
            uses: 3,
        };
        assert_eq!(invite_uses_label(&invite_limited), "3/10 uses");
    }
}
