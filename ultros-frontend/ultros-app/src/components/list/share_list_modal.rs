use crate::api::{
    create_list_invite, delete_list_invite, get_groups, get_list_invites, get_list_shares,
    share_list_with_group, share_list_with_user, unshare_list_from_group, unshare_list_from_user,
};
use crate::components::icon::Icon;
use crate::components::loading::*;
use crate::components::modal::Modal;
use crate::global_state::clipboard_text::GlobalLastCopiedText;
use crate::global_state::toasts::{Toasts, use_toast};
use crate::i18n::*;
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::list::{
    CreateInvite, List, ListInvite, ListPermission, ListSharedGroup, ListSharedUser,
    ShareListGroup, ShareListUser,
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

pub(crate) fn invite_url(invite_id: &str) -> String {
    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window()
            && let Ok(origin) = window.location().origin()
        {
            return format!("{origin}/list/invite/{invite_id}");
        }
    }
    format!("/list/invite/{invite_id}")
}

pub(crate) fn copy_invite_url(
    invite_id: &str,
    last_copied: Option<GlobalLastCopiedText>,
    toasts: Option<Toasts>,
) {
    let url = invite_url(invite_id);
    #[cfg(feature = "hydrate")]
    if let Some(window) = web_sys::window() {
        let clipboard = window.navigator().clipboard();
        let _ = clipboard.write_text(&url);
    }
    if let Some(last_copied) = last_copied {
        last_copied.0.set(Some(url));
    }
    if let Some(toasts) = toasts {
        toasts.success("Invite link copied");
    }
}

pub(crate) fn invite_uses_label(invite: &ListInvite) -> String {
    format!(
        "{}/{} uses",
        invite.uses,
        invite
            .max_uses
            .map(|max_uses| max_uses.to_string())
            .unwrap_or_else(|| "∞".to_string())
    )
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
    let (recipient, set_recipient) = signal(String::new());
    let (recipient_permission, set_recipient_permission) = signal(ListPermission::Read);
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
                create_invite.version().get(),
                delete_invite.version().get(),
                refresh_signal.map(|s| s.get()).unwrap_or(0),
            )
        },
        move |_| async move {
            let (users, groups) = get_list_shares(list_id).await?;
            let invites = get_list_invites(list_id).await?;
            let owned_groups = get_groups().await?;
            Ok::<_, crate::error::AppError>((users, groups, invites, owned_groups))
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
                Ok((users, shared_groups, invites, owned_groups)) => {
                    let owned_groups_for_submit = owned_groups.clone();
                    let invites_for_copy = invites.clone();
                    let latest_invite_url = invites
                        .last()
                        .map(|invite| invite_url(&invite.id))
                        .unwrap_or_default();
                    view! {
                        <div class="space-y-6">
                            <section class="space-y-3">
                                <h3 class="text-lg font-bold text-[color:var(--color-text)]">{t!(i18n, lists_share_add_people_heading)}</h3>
                                <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_7rem_7rem]">
                                    <input
                                        class="input w-full text-base"
                                        placeholder=t_string!(i18n, lists_share_search_placeholder)
                                        prop:value=recipient
                                        on:input=move |ev| set_recipient(event_target_value(&ev))
                                    />
                                    <select
                                        class="input w-full"
                                        on:change=move |ev| set_recipient_permission(editable_permission(&event_target_value(&ev)))
                                    >
                                        <option value="Read">{t!(i18n, permission_read)}</option>
                                        <option value="Write">{t!(i18n, permission_write)}</option>
                                    </select>
                                    <button
                                        type="button"
                                        class="btn-primary"
                                        prop:disabled=move || recipient().trim().is_empty()
                                        on:click=move |_| {
                                            let raw = recipient().trim().to_string();
                                            if raw.is_empty() {
                                                return;
                                            }
                                            if let Ok(user_id) = raw.parse::<i64>() {
                                                share_user.dispatch((user_id, recipient_permission()));
                                                set_recipient(String::new());
                                                return;
                                            }
                                            let raw_lower = raw.to_lowercase();
                                            if let Some(group) = owned_groups_for_submit
                                                .iter()
                                                .find(|group| group.name.to_lowercase() == raw_lower)
                                            {
                                                share_group.dispatch((group.id, recipient_permission()));
                                                set_recipient(String::new());
                                            } else if let Some(toasts) = toasts {
                                                toasts.error("Enter a Discord user ID or an exact group name you own");
                                            }
                                        }
                                    >
                                        "Invite"
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
                                    invites=invites
                                    unshare_user
                                    unshare_group
                                    delete_invite
                                />
                                <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--color-text)_4%,transparent)] p-3 text-sm text-[color:var(--color-text-muted)]">
                                    "Tip: group names must match one of your owned groups exactly. Discord users can be added by numeric Discord user ID."
                                    {if owned_groups.is_empty() {
                                        Some(view! { <span> " You do not own any groups yet." </span> })
                                    } else {
                                        None
                                    }}
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
    invites: Vec<ListInvite>,
    unshare_user: Action<i64, Result<(), crate::error::AppError>>,
    unshare_group: Action<i32, Result<(), crate::error::AppError>>,
    delete_invite: Action<String, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    let is_empty = users.is_empty() && groups.is_empty() && invites.is_empty();
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
